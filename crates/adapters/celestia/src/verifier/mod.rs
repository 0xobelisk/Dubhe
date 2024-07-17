use borsh::{BorshDeserialize, BorshSerialize};
use celestia_types::nmt::{Namespace, NS_SIZE};
use celestia_types::{Commitment, DataAvailabilityHeader, NamespacedShares};
use nmt_rs::NamespacedSha2Hasher;
use serde::{Deserialize, Serialize};
use sov_rollup_interface::da::{
    self, BlobReaderTrait, BlockHashTrait as BlockHash, BlockHeaderTrait, DaSpec, RelevantBlobs,
    RelevantProofs,
};
use sov_rollup_interface::digest::Digest;
use sov_rollup_interface::zk::{ValidityCondition, ValidityConditionChecker};
use sov_rollup_interface::Buf;
use thiserror::Error;

pub mod address;
pub mod proofs;

#[cfg(all(target_os = "zkvm", feature = "bench"))]
use risc0_cycle_macros::cycle_tracker;
use tracing::debug;

use self::address::CelestiaAddress;
use self::proofs::*;
use crate::shares::{NamespaceGroup, Share};
use crate::types::{BlobWithSender, ValidationError};
use crate::utils::read_varint;
use crate::{pfb_from_iter, CelestiaHeader};

#[derive(Clone)]
pub struct CelestiaVerifier {
    pub rollup_batch_namespace: Namespace,
    pub rollup_proof_namespace: Namespace,
}

pub const PFB_NAMESPACE: Namespace = Namespace::const_v0([0, 0, 0, 0, 0, 0, 0, 0, 0, 4]);
pub const PARITY_SHARES_NAMESPACE: Namespace = Namespace::PARITY_SHARE;

impl BlobReaderTrait for BlobWithSender {
    type Address = CelestiaAddress;

    fn sender(&self) -> CelestiaAddress {
        self.sender.clone()
    }

    fn hash(&self) -> [u8; 32] {
        self.hash.0
    }

    fn verified_data(&self) -> &[u8] {
        self.blob.accumulator()
    }

    fn total_len(&self) -> usize {
        self.blob.total_len()
    }

    #[cfg(feature = "native")]
    fn advance(&mut self, num_bytes: usize) -> &[u8] {
        self.blob.advance(num_bytes);
        self.verified_data()
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Clone, Eq, Hash, Serialize, Deserialize)]
// Important: #[repr(transparent)] is required for safety as long as we're using
// std::mem::transmute to implement AsRef<TmHash> for tendermint::Hash
#[repr(transparent)]
pub struct TmHash(pub celestia_tendermint::Hash);

impl AsRef<[u8]> for TmHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl core::fmt::Display for TmHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl TmHash {
    pub fn inner(&self) -> &[u8; 32] {
        match self.0 {
            celestia_tendermint::Hash::Sha256(ref h) => h,
            // Hack: when the hash is None, we return a hash of all 255s as a placeholder.
            // TODO: add special casing for the genesis block at a higher level
            celestia_tendermint::Hash::None => unreachable!("Only the genesis block has a None hash, and we use a placeholder in that corner case")
        }
    }
}

impl AsRef<TmHash> for celestia_tendermint::Hash {
    fn as_ref(&self) -> &TmHash {
        // Safety: #[repr(transparent)] guarantees that the memory layout of TmHash is
        // the same as tendermint::Hash, so this `transmute` is sound.
        // See https://doc.rust-lang.org/nomicon/other-reprs.html#reprtransparent
        unsafe { std::mem::transmute(self) }
    }
}

impl BlockHash for TmHash {}

impl From<TmHash> for [u8; 32] {
    fn from(val: TmHash) -> Self {
        *val.inner()
    }
}

#[derive(
    Default,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Eq,
    Clone,
    BorshDeserialize,
    BorshSerialize,
)]
pub struct CelestiaSpec;

impl DaSpec for CelestiaSpec {
    type SlotHash = TmHash;

    type BlockHeader = CelestiaHeader;

    type BlobTransaction = BlobWithSender;

    type Address = CelestiaAddress;

    type ValidityCondition = ChainValidityCondition;

    #[cfg(feature = "native")]
    type Checker = ChainValidityConditionChecker;

    type InclusionMultiProof = Vec<EtxProof>;

    type CompletenessProof = NamespacedShares;

    type ChainParams = RollupParams;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RollupParams {
    pub rollup_batch_namespace: Namespace,
    pub rollup_proof_namespace: Namespace,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Hash,
    BorshDeserialize,
    BorshSerialize,
)]
/// A validity condition expressing that a chain of DA layer blocks is contiguous and canonical
pub struct ChainValidityCondition {
    pub prev_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

#[derive(Error, Debug)]
pub enum ValidityConditionError {
    #[error("conditions for validity can only be combined if the blocks are consecutive")]
    BlocksNotConsecutive,
}

impl ValidityCondition for ChainValidityCondition {
    type Error = ValidityConditionError;
    fn combine<H: Digest>(&self, rhs: Self) -> Result<Self, Self::Error> {
        if self.block_hash != rhs.prev_hash {
            return Err(ValidityConditionError::BlocksNotConsecutive);
        }
        Ok(rhs)
    }
}

/// The [`ValidityConditionChecker`] used to validate Celestia's [`ChainValidityCondition`]
/// This validity condition checker is trivial because the validity condition consistency
/// constraints are enforced in the `combine` method.
#[derive(Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
pub struct ChainValidityConditionChecker;

impl ValidityConditionChecker<ChainValidityCondition> for ChainValidityConditionChecker {
    type Error = anyhow::Error;
    fn check(&mut self, _condition: &ChainValidityCondition) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl da::DaVerifier for CelestiaVerifier {
    type Spec = CelestiaSpec;

    type Error = ValidationError;

    fn new(params: <Self::Spec as DaSpec>::ChainParams) -> Self {
        Self {
            rollup_proof_namespace: params.rollup_proof_namespace,
            rollup_batch_namespace: params.rollup_batch_namespace,
        }
    }

    #[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
    fn verify_relevant_tx_list(
        &self,
        block_header: &<Self::Spec as DaSpec>::BlockHeader,
        relevant_blobs: &RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>,
        relevant_proofs: RelevantProofs<
            <Self::Spec as DaSpec>::InclusionMultiProof,
            <Self::Spec as DaSpec>::CompletenessProof,
        >,
    ) -> Result<<Self::Spec as DaSpec>::ValidityCondition, Self::Error> {
        // Validate that the provided DAH is well-formed
        block_header.validate_dah()?;

        Self::verify_txs(
            block_header,
            &relevant_blobs.proof_blobs,
            self.rollup_proof_namespace,
            relevant_proofs.proof.inclusion_proof,
            relevant_proofs.proof.completeness_proof,
        )?;

        Self::verify_txs(
            block_header,
            &relevant_blobs.batch_blobs,
            self.rollup_batch_namespace,
            relevant_proofs.batch.inclusion_proof,
            relevant_proofs.batch.completeness_proof,
        )?;

        let validity_condition = ChainValidityCondition {
            prev_hash: *block_header.prev_hash().inner(),
            block_hash: *block_header.hash().inner(),
        };

        Ok(validity_condition)
    }
}

impl CelestiaVerifier {
    fn verify_txs(
        block_header: &CelestiaHeader,
        txs: &[BlobWithSender],
        namespace: Namespace,
        inclusion_proof: Vec<EtxProof>,
        completeness_proof: NamespacedShares,
    ) -> Result<(), ValidationError> {
        // Check the validity and completeness of the rollup row proofs, against the DAH.
        // Extract the data from the row proofs and build a namespace_group from it
        let verified_shares =
            Self::verify_row_proofs(namespace, completeness_proof, &block_header.dah)?;

        if verified_shares.is_empty() {
            if !txs.is_empty() {
                return Err(ValidationError::MissingTx);
            }
            Ok(())
        } else {
            Self::verify_inclusion_proof(
                namespace,
                block_header,
                verified_shares,
                txs,
                inclusion_proof,
            )
        }
    }

    fn verify_inclusion_proof(
        namespace: Namespace,
        block_header: &CelestiaHeader,
        verified_shares: NamespaceGroup,
        txs: &[BlobWithSender],
        inclusion_proof: Vec<EtxProof>,
    ) -> Result<(), ValidationError> {
        // Check the e-tx proofs...
        // TODO(@preston-evans98): Remove this logic if Celestia adds blob.sender metadata directly into blob
        let mut tx_iter = txs.iter();
        let mut tx_proofs = inclusion_proof.into_iter();
        let square_size = block_header.dah.row_roots().len();
        for blob in verified_shares.blobs() {
            if blob.is_padding() {
                debug!("Ignoring namespace padding blob. Sequence length 0.");
                continue;
            }
            // Get the etx proof for this blob
            let Some(tx_proof) = tx_proofs.next() else {
                return Err(ValidationError::InvalidEtxProof("not all blobs proven"));
            };

            // Force the row number to be monotonically increasing
            let start_offset = tx_proof.proof[0].start_offset;

            // Verify each sub-proof and flatten the shares back into a sequential array
            // First, enforce that the sub-proofs cover a contiguous range of shares
            for i in 1..tx_proof.proof.len() {
                let l = &tx_proof.proof[i.checked_sub(1).expect("invalid tx proof len")];
                let r = &tx_proof.proof[i];
                assert_eq!(
                    l.start_share_idx.saturating_add(l.shares.len()),
                    r.start_share_idx
                );
            }
            let mut tx_shares = Vec::new();
            // Then, verify the sub proofs
            for sub_proof in tx_proof.proof.into_iter() {
                let row_num = sub_proof
                    .start_share_idx
                    .checked_div(square_size)
                    .expect("the square size is invalid");
                let root = &block_header.dah.row_roots()[row_num];
                sub_proof
                    .proof
                    .verify_range(root, &sub_proof.shares, PFB_NAMESPACE.into())
                    .map_err(|_| ValidationError::InvalidEtxProof("invalid sub proof"))?;
                tx_shares.extend(
                    sub_proof
                        .shares
                        .into_iter()
                        .map(|share_vec| Share::new(share_vec.into())),
                );
            }

            // Next, ensure that the start_index is valid
            if !tx_shares[0].is_valid_tx_start(start_offset) {
                return Err(ValidationError::InvalidEtxProof("invalid start index"));
            }

            // Collect all of the shares data into a single array
            let trailing_shares = tx_shares[1..]
                .iter()
                .flat_map(|share| share.data_ref().iter());
            let tx_data: Vec<u8> = tx_shares[0].data_ref()[start_offset..]
                .iter()
                .chain(trailing_shares)
                .copied()
                .collect();

            // Deserialize the pfb transaction
            let (len, len_of_len) = {
                let cursor = std::io::Cursor::new(&tx_data);
                read_varint(cursor).expect("tx must be length prefixed")
            };
            let mut cursor =
                std::io::Cursor::new(&tx_data[len_of_len..len_of_len.saturating_add(len as usize)]);

            let pfb = pfb_from_iter(&mut cursor, len as usize)
                .map_err(|_| ValidationError::InvalidEtxProof("invalid pfb"))?;

            // Verify the sender and data of each blob which was sent into this namespace
            for (blob_idx, nid) in pfb.namespaces.iter().enumerate() {
                if nid != namespace.as_bytes() {
                    continue;
                }
                let tx: &BlobWithSender = tx_iter.next().ok_or(ValidationError::MissingTx)?;
                if tx.sender.to_string() != pfb.signer {
                    return Err(ValidationError::InvalidSigner);
                }

                let blob_ref = blob.clone();

                let mut blob_iter = blob_ref.data();
                let mut blob_data = vec![0; blob_iter.remaining()];
                blob_iter.copy_to_slice(blob_data.as_mut_slice());

                let tx_data = tx.verified_data();

                assert!(
                    tx_data.len() <= blob_data.len(),
                    "claimed data must not be larger smaller than blob data"
                );
                for (l, r) in tx_data.iter().zip(blob_data.iter()) {
                    assert_eq!(l, r, "claimed data must match observed data");
                }

                // Link blob commitment to e-tx commitment
                let expected_commitment =
                    Commitment::from_shares(namespace, &blob_ref.celestia_shares()).map_err(
                        |_| ValidationError::InvalidEtxProof("failed to recreate commitment"),
                    )?;

                assert_eq!(&pfb.share_commitments[blob_idx][..], &expected_commitment.0);
            }
        }

        if tx_proofs.next().is_some() {
            return Err(ValidationError::InvalidEtxProof("more proofs than blobs"));
        }

        Ok(())
    }

    fn verify_row_proofs(
        namespace: Namespace,
        row_proofs: NamespacedShares,
        dah: &DataAvailabilityHeader,
    ) -> Result<NamespaceGroup, ValidationError> {
        let mut row_proofs = row_proofs.rows.into_iter();
        // Check the validity and completeness of the rollup share proofs
        let mut verified_shares = Vec::new();
        for row_root in dah.row_roots().iter() {
            // TODO: short circuit this loop at the first row after the rollup namespace
            if row_root.contains::<NamespacedSha2Hasher<NS_SIZE>>(namespace.into()) {
                let row_proof = row_proofs.next().ok_or(ValidationError::InvalidRowProof)?;
                row_proof
                    .proof
                    .verify_complete_namespace(row_root, &row_proof.shares, namespace.into())
                    .expect("Proofs must be valid");

                for leaf in row_proof.shares {
                    verified_shares.push(leaf);
                }
            }
        }
        Ok(NamespaceGroup::from_shares(verified_shares))
    }
}
