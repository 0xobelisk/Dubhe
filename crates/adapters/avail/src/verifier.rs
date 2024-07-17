use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sov_rollup_interface::da::{DaSpec, DaVerifier};
use sov_rollup_interface::services::da::{RelevantBlobs, RelevantProofs};
use sov_rollup_interface::zk::{ValidityCondition, ValidityConditionChecker};
use thiserror::Error;

use crate::spec::DaLayerSpec;

#[derive(Error, Debug)]
pub enum ValidityConditionError {
    #[error("conditions for validity can only be combined if the blocks are consecutive")]
    BlocksNotConsecutive,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy, BorshDeserialize, BorshSerialize,
)]
/// A validity condition expressing that a chain of DA layer blocks is contiguous and canonical
pub struct ChainValidityCondition {
    pub prev_hash: [u8; 32],
    pub block_hash: [u8; 32],
    //Chained or batch txs commitment.
    pub txs_commitment: [u8; 32],
}

impl ValidityCondition for ChainValidityCondition {
    type Error = ValidityConditionError;

    fn combine<SimpleHasher>(&self, rhs: Self) -> Result<Self, Self::Error> {
        let mut combined_hashes: Vec<u8> = Vec::with_capacity(64);
        combined_hashes.extend_from_slice(self.txs_commitment.as_ref());
        combined_hashes.extend_from_slice(rhs.txs_commitment.as_ref());

        let combined_root = sp_core_hashing::blake2_256(&combined_hashes);

        if self.block_hash != rhs.prev_hash {
            return Err(ValidityConditionError::BlocksNotConsecutive);
        }

        Ok(Self {
            prev_hash: rhs.prev_hash,
            block_hash: rhs.block_hash,
            txs_commitment: combined_root,
        })
    }
}

/// The [`ValidityConditionChecker`] used to validate Avail's [`ChainValidityCondition`]
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

#[derive(Clone)]
pub struct Verifier;

impl DaVerifier for Verifier {
    type Spec = DaLayerSpec;

    type Error = ValidityConditionError;

    // Verify that the given list of blob transactions is complete and correct.
    // NOTE: Function return unit since application client already verifies application data.
    fn verify_relevant_tx_list(
        &self,
        _block_header: &<Self::Spec as DaSpec>::BlockHeader,
        _relevant_blobs: &RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>,
        _relevant_proofs: RelevantProofs<
            <Self::Spec as DaSpec>::InclusionMultiProof,
            <Self::Spec as DaSpec>::CompletenessProof,
        >,
    ) -> Result<<Self::Spec as DaSpec>::ValidityCondition, Self::Error> {
        todo!()
    }

    fn new(_params: <Self::Spec as DaSpec>::ChainParams) -> Self {
        Verifier {}
    }
}
