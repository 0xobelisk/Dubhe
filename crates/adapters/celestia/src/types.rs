use std::collections::HashMap;
use std::slice::Chunks;

use anyhow::{bail, ensure};
// use borsh::{BorshDeserialize, BorshSerialize};
use celestia_proto::celestia::blob::v1::MsgPayForBlobs;
use celestia_types::consts::appconsts::SHARE_SIZE;
/// Reexport the [`Namespace`] from `celestia-types`
pub use celestia_types::nmt::Namespace;
use celestia_types::nmt::{NamespacedHash, Nmt, NmtExt, NS_SIZE};
use celestia_types::{
    Commitment, DataAvailabilityHeader, ExtendedDataSquare, ExtendedHeader, NamespacedShares,
    ValidateBasic,
};
use nmt_rs::NamespacedSha2Hasher;
use serde::{Deserialize, Serialize};
use sov_rollup_interface::common::HexHash;
#[cfg(feature = "native")]
use sov_rollup_interface::da::BlockHeaderTrait;
use sov_rollup_interface::da::CountedBufReader;
#[cfg(feature = "native")]
use sov_rollup_interface::services::da::SlotData;
use sov_rollup_interface::Bytes;
use tracing::{debug, info};

use crate::shares::{Blob, BlobIterator, NamespaceGroup};
use crate::utils::BoxError;
use crate::verifier::address::CelestiaAddress;
#[cfg(feature = "native")]
use crate::verifier::ChainValidityCondition;
use crate::verifier::{PARITY_SHARES_NAMESPACE, PFB_NAMESPACE};
use crate::{parse_pfb_namespace, CelestiaHeader, TxPosition};

/// Celestia namespace and corresponding shares.
pub struct NamespaceWithShares {
    pub(crate) namespace: Namespace,
    pub(crate) rows: NamespacedShares,
}

impl NamespaceWithShares {
    fn convert_to_namespace_data(
        pfbs: &Vec<(MsgPayForBlobs, TxPosition)>,
        batch_shares: Self,
        proof_shares: Self,
    ) -> (NamespaceData, NamespaceData) {
        // Parse out the pfds and store them for later retrieval.
        debug!("Decoding pfb protobufs...");
        let mut batch_pbf_map = HashMap::new();
        let mut proof_pbf_map = HashMap::new();
        for tx in pfbs {
            for (idx, nid) in tx.0.namespaces.iter().enumerate() {
                if nid == batch_shares.namespace.as_bytes() {
                    // TODO: Retool this map to avoid cloning txs
                    batch_pbf_map.insert(tx.0.share_commitments[idx].clone().into(), tx.clone());
                }

                if nid == proof_shares.namespace.as_bytes() {
                    // TODO: Retool this map to avoid cloning txs
                    proof_pbf_map.insert(tx.0.share_commitments[idx].clone().into(), tx.clone());
                }
            }
        }

        let rollup_batch_data = NamespaceData {
            namespace: batch_shares.namespace,
            group: NamespaceGroup::from(&batch_shares.rows),
            relevant_pfbs: batch_pbf_map,
            rows: batch_shares.rows,
        };

        let rollup_proof_data = NamespaceData {
            namespace: proof_shares.namespace,
            group: NamespaceGroup::from(&proof_shares.rows),
            relevant_pfbs: proof_pbf_map,
            rows: proof_shares.rows,
        };

        (rollup_batch_data, rollup_proof_data)
    }
}

// TODO: derive borsh Serialize, Deserialize <https://github.com/eigerco/celestia-node-rs/issues/155>
#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct BlobWithSender {
    pub blob: CountedBufReader<BlobIterator>,
    pub sender: CelestiaAddress,
    pub hash: HexHash,
}

/// Data that is required for extracting the relevant blobs from the namespace
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct NamespaceData {
    /// Celestia namespace.
    pub(crate) namespace: Namespace,
    pub(crate) group: NamespaceGroup,
    /// A mapping from blob commitment to the PFB containing that commitment
    /// for each blob addressed to the relevant namespace.
    pub(crate) relevant_pfbs: HashMap<Bytes, (MsgPayForBlobs, TxPosition)>,
    /// All relevant rollup shares as they appear in extended data square, with proofs.
    pub(crate) rows: NamespacedShares,
}

impl NamespaceData {
    pub fn get_blob_with_sender(&self) -> Vec<BlobWithSender> {
        let mut output = Vec::new();
        for blob_ref in self.group.blobs() {
            if blob_ref.is_padding() {
                debug!("Ignoring namespace padding blob. Sequence length 0.");
                continue;
            }

            let commitment = Commitment::from_shares(self.namespace, &blob_ref.celestia_shares())
                .expect("blob must be valid");
            info!(commitment = hex::encode(commitment.0), "Extracting blob");
            let sender = self
                .relevant_pfbs
                .get(&commitment.0[..])
                .expect("blob must be relevant")
                .0
                .signer
                .clone();

            let blob: Blob = blob_ref.into();

            let blob_tx = BlobWithSender {
                blob: CountedBufReader::new(blob.into_iter()),
                sender: sender.parse().expect("Incorrect sender address"),
                hash: HexHash::new(commitment.0),
            };

            output.push(blob_tx);
        }
        output
    }
}

// TODO: derive borsh Serialize, Deserialize <https://github.com/eigerco/celestia-node-rs/issues/155>
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FilteredCelestiaBlock {
    pub(crate) header: CelestiaHeader,
    /// All rows in the extended data square which contain pfb data
    pub(crate) pfb_rows: Vec<Row>,
    /// Batch related data.
    pub(crate) rollup_batch_data: NamespaceData,

    /// Proof related data.
    pub(crate) rollup_proof_data: NamespaceData,
}

#[cfg(feature = "native")]
impl SlotData for FilteredCelestiaBlock {
    type BlockHeader = CelestiaHeader;
    type Cond = ChainValidityCondition;

    fn hash(&self) -> [u8; 32] {
        match self.header.header.hash() {
            celestia_tendermint::Hash::Sha256(h) => h,
            celestia_tendermint::Hash::None => {
                unreachable!("tendermint::Hash::None should not be possible")
            }
        }
    }

    fn header(&self) -> &Self::BlockHeader {
        &self.header
    }

    fn validity_condition(&self) -> ChainValidityCondition {
        ChainValidityCondition {
            prev_hash: *self.header().prev_hash().inner(),
            block_hash: self.hash(),
        }
    }
}

impl FilteredCelestiaBlock {
    pub fn new(
        rollup_batch_shares: NamespaceWithShares,
        rollup_proof_shares: NamespaceWithShares,
        header: ExtendedHeader,
        etx_rows: NamespacedShares,
        data_square: ExtendedDataSquare,
    ) -> Result<Self, BoxError> {
        let tx_data = NamespaceGroup::from(&etx_rows);
        let pfbs = parse_pfb_namespace(tx_data)?;
        // Parse out all of the rows containing etxs
        debug!("Parsing namespaces...");
        let pfb_rows =
            get_rows_containing_namespace(PFB_NAMESPACE, &header.dah, data_square.rows()?)?;

        // validate the extended data square
        data_square.validate()?;

        let (rollup_batch_data, rollup_proof_data) = NamespaceWithShares::convert_to_namespace_data(
            &pfbs,
            rollup_batch_shares,
            rollup_proof_shares,
        );

        Ok(FilteredCelestiaBlock {
            header: CelestiaHeader::new(header.dah, header.header.into()),
            pfb_rows,
            rollup_batch_data,
            rollup_proof_data,
        })
    }

    pub fn square_size(&self) -> usize {
        self.header.square_size()
    }

    pub fn get_row_number(&self, share_idx: usize) -> usize {
        share_idx / self.square_size()
    }
    pub fn get_col_number(&self, share_idx: usize) -> usize {
        share_idx % self.square_size()
    }

    pub fn row_root_for_share(&self, share_idx: usize) -> &NamespacedHash {
        &self.header.dah.row_roots()[self.get_row_number(share_idx)]
    }

    pub fn col_root_for_share(&self, share_idx: usize) -> &NamespacedHash {
        &self.header.dah.column_roots()[self.get_col_number(share_idx)]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Missing data hash in header")]
    MissingDataHash,

    #[error("Data root hash doesn't match computed one")]
    InvalidDataRoot,

    #[error("Invalid etx proof: {0}")]
    InvalidEtxProof(&'static str),

    #[error("Transaction missing")]
    MissingTx,

    #[error("Invalid row proof")]
    InvalidRowProof,

    #[error("Invalid signer")]
    InvalidSigner,

    #[error("Incomplete data")]
    IncompleteData,

    #[error(transparent)]
    DahValidation(#[from] celestia_types::ValidationError),
}

impl CelestiaHeader {
    pub fn validate_dah(&self) -> Result<(), ValidationError> {
        self.dah.validate_basic()?;
        let data_hash = self
            .header
            .data_hash
            .as_ref()
            .ok_or(ValidationError::MissingDataHash)?;
        if self.dah.hash().as_ref() != data_hash.0 {
            return Err(ValidationError::InvalidDataRoot);
        }
        Ok(())
    }
}

pub trait ExtendedDataSquareExt {
    fn square_size(&self) -> Result<usize, BoxError>;

    fn rows(&self) -> Result<Chunks<'_, Vec<u8>>, BoxError>;

    fn validate(&self) -> Result<(), BoxError>;
}

impl ExtendedDataSquareExt for ExtendedDataSquare {
    fn square_size(&self) -> Result<usize, BoxError> {
        let len = self.data_square().len();
        let square_size = (len as f64).sqrt() as usize;
        ensure!(
            square_size * square_size == len,
            "eds size {} is not a perfect square",
            len
        );
        Ok(square_size)
    }

    fn rows(&self) -> Result<Chunks<'_, Vec<u8>>, BoxError> {
        let square_size = self.square_size()?;
        Ok(self.data_square().chunks(square_size))
    }

    fn validate(&self) -> Result<(), BoxError> {
        let len = self.square_size()?;
        ensure!(len * len == self.data_square().len(), "Invalid square size");

        if let Some(share) = self
            .rows()
            .expect("after first check this must succeed")
            .flatten()
            .find(|shares| shares.len() != SHARE_SIZE)
        {
            bail!("Invalid share size: {}", share.len())
        }
        Ok(())
    }
}

// TODO: derive borsh Serialize, Deserialize <https://github.com/eigerco/celestia-node-rs/issues/155>
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct Row {
    pub shares: Vec<Vec<u8>>,
    pub root: NamespacedHash,
}

impl Row {
    pub fn merklized(&self) -> Nmt {
        let mut nmt = Nmt::default();
        for (idx, share) in self.shares.iter().enumerate() {
            // Shares in the two left-hand quadrants are prefixed with their namespace, while parity
            // shares (in the right-hand) quadrants should always be treated as PARITY_SHARES_NAMESPACE
            let namespace = if idx < self.shares.len() / 2 {
                share_namespace_unchecked(share)
            } else {
                PARITY_SHARES_NAMESPACE
            };
            nmt.push_leaf(share.as_ref(), *namespace)
                .expect("shares are pushed in order");
        }
        nmt
    }
}

/// get namespace from a share without verifying if it's a correct namespace
/// (version 0 or parity ns).
fn share_namespace_unchecked(share: &[u8]) -> Namespace {
    nmt_rs::NamespaceId(
        share[..NS_SIZE]
            .try_into()
            .expect("must succeed for correct size"),
    )
    .into()
}

fn get_rows_containing_namespace<'a>(
    nid: Namespace,
    dah: &'a DataAvailabilityHeader,
    data_square_rows: impl Iterator<Item = &'a [Vec<u8>]>,
) -> Result<Vec<Row>, BoxError> {
    let mut output = vec![];

    for (row, root) in data_square_rows.zip(dah.row_roots().iter()) {
        if root.contains::<NamespacedSha2Hasher<NS_SIZE>>(*nid) {
            output.push(Row {
                shares: row.to_vec(),
                root: root.clone(),
            });
        }
    }
    Ok(output)
}

#[cfg(test)]
pub mod tests {
    use crate::parse_pfb_namespace;
    use crate::test_helper::files::*;
    use crate::types::{NamespaceGroup, NamespacedShares};
    use crate::verifier::PFB_NAMESPACE;

    #[test]
    fn filtered_block_with_prof_data() {
        let block = with_rollup_proof_data::filtered_block();

        // valid dah
        block.header.validate_dah().unwrap();

        let rollup_proof_data = block.rollup_proof_data;

        assert_eq!(rollup_proof_data.group.shares().len(), 1);
        assert_eq!(rollup_proof_data.rows.rows.len(), 1);
        assert_eq!(rollup_proof_data.rows.rows[0].shares.len(), 1);
        assert!(rollup_proof_data.rows.rows[0].proof.is_of_presence());

        assert_eq!(block.pfb_rows.len(), 1);
        let pfbs_count = block.pfb_rows[0]
            .shares
            .iter()
            .filter(|share| share.starts_with(PFB_NAMESPACE.as_ref()))
            .count();
        assert_eq!(pfbs_count, 1);
        assert_eq!(rollup_proof_data.relevant_pfbs.len(), 1);
    }

    #[test]
    fn filtered_block_with_batch_data() {
        let block = with_rollup_batch_data::filtered_block();
        // valid dah
        block.header.validate_dah().unwrap();

        let rollup_batch_data = block.rollup_batch_data;

        // single rollup share
        assert_eq!(rollup_batch_data.group.shares().len(), 1);
        assert_eq!(rollup_batch_data.rows.rows.len(), 1);
        assert_eq!(rollup_batch_data.rows.rows[0].shares.len(), 1);
        assert!(rollup_batch_data.rows.rows[0].proof.is_of_presence());

        // 3 pfbs at all but only one belongs to rollup
        assert_eq!(block.pfb_rows.len(), 1);
        let pfbs_count = block.pfb_rows[0]
            .shares
            .iter()
            .filter(|share| share.starts_with(PFB_NAMESPACE.as_ref()))
            .count();
        assert_eq!(pfbs_count, 1);
        assert_eq!(rollup_batch_data.relevant_pfbs.len(), 1);
    }

    #[test]
    fn filtered_block_without_batch_data() {
        let block = without_rollup_batch_data::filtered_block();

        // valid dah
        block.header.validate_dah().unwrap();

        let rollup_batch_data = block.rollup_batch_data;

        // no rollup shares
        assert_eq!(rollup_batch_data.group.shares().len(), 0);
        // we still get single row, but with absence proof and no shares
        assert_eq!(rollup_batch_data.rows.rows.len(), 1);
        assert_eq!(rollup_batch_data.rows.rows[0].shares.len(), 0);
        assert!(rollup_batch_data.rows.rows[0].proof.is_of_absence());

        // 2 pfbs at all and no relevant
        assert_eq!(block.pfb_rows.len(), 1);
        let pfbs_count = block.pfb_rows[0]
            .shares
            .iter()
            .filter(|share| share.starts_with(PFB_NAMESPACE.as_ref()))
            .count();
        assert_eq!(pfbs_count, 2);
        assert_eq!(rollup_batch_data.relevant_pfbs.len(), 0);
    }

    #[test]
    fn test_get_pfbs() {
        let path = make_test_path(with_rollup_batch_data::DATA_PATH);
        let rows: NamespacedShares = load_from_file(&path, ETX_ROWS_JSON).unwrap();

        let pfb_ns = NamespaceGroup::from(&rows);
        let pfbs = parse_pfb_namespace(pfb_ns).expect("failed to parse pfb shares");
        assert_eq!(pfbs.len(), 1);
    }

    #[test]
    fn test_get_rollup_data() {
        let path = make_test_path(with_rollup_batch_data::DATA_PATH);
        let rows: NamespacedShares = load_from_file(&path, ROLLUP_BATCH_ROWS_JSON).unwrap();

        let rollup_ns_group = NamespaceGroup::from(&rows);
        let mut blobs = rollup_ns_group.blobs();
        let first_blob = blobs
            .next()
            .expect("iterator should contain exactly one blob");

        // this is a batch submitted by sequencer, consisting of a single
        // "CreateToken" transaction, but we verify only length there to
        // not make this test depend on deserialization logic
        assert_eq!(first_blob.data().count(), 277);

        assert!(blobs.next().is_none());
    }
}
