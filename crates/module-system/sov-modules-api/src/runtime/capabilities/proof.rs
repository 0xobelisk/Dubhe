use sov_rollup_interface::da::DaSpec;
use sov_rollup_interface::stf::ProofReceipt;
use sov_state::Storage;

use crate::{Spec, StateCheckpoint};

/// The `ProofProcessor` capability is responsible for processing zk-proofs inside
/// the stf-blueprint.
pub trait ProofProcessor<S: Spec, Da: DaSpec> {
    #[allow(clippy::type_complexity)]
    /// Called by the stf once the proof is received.
    fn process_proof(
        &self,
        proof_batch: Vec<u8>,
        state: StateCheckpoint<S>,
    ) -> (
        ProofReceipt<S::Address, Da, <S::Storage as Storage>::Root, ()>,
        StateCheckpoint<S>,
    );
}
