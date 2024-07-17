mod manager;
mod parallel;

mod stf_info;
use async_trait::async_trait;
pub use manager::ProofManager;
pub use parallel::ParallelProverService;
use serde::Serialize;
use sov_rollup_interface::da::DaSpec;
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::zk::aggregated_proof::{
    AggregatedProofPublicData, SerializedAggregatedProof,
};
use sov_rollup_interface::zk::Zkvm;
pub use stf_info::StateTransitionInfo;
use thiserror::Error;

/// The possible configurations of the prover.
#[derive(PartialEq, Eq, strum::EnumString, strum::Display)]
// Note: it's best if all string conversions to and from this type (even
// `Debug`) use the same casing, to avoid bad UX or confusion around env. vars
// expected behavior.
#[strum(serialize_all = "snake_case")]
pub enum RollupProverConfig {
    /// Skip proving.
    Skip,
    /// Run the rollup verification logic inside the current process.
    Simulate,
    /// Run the rollup verifier in a zkVM executor.
    Execute,
    /// Run the rollup verifier and create a SNARK of execution.
    Prove,
}

impl std::fmt::Debug for RollupProverConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

/// Represents the status of a witness submission.
#[derive(Debug, Eq, PartialEq)]
pub enum WitnessSubmissionStatus {
    /// The witness has been submitted to the prover.
    SubmittedForProving,
    /// The witness is already present in the prover.
    WitnessExist,
}

/// Represents the status of a DA proof submission.
#[derive(Debug, Eq, PartialEq)]
pub enum ProofAggregationStatus {
    /// Indicates successful proof generation.
    Success(SerializedAggregatedProof),
    /// Indicates that proof generation is currently in progress.
    ProofGenerationInProgress,
}

/// Represents the current status of proof generation.
#[derive(derivative::Derivative)]
#[derivative(Debug(bound = ""))]
pub enum ProofProcessingStatus<StateRoot, Witness, Da: DaSpec> {
    /// Indicates that proof generation is currently in progress.
    ProvingInProgress,
    /// Indicates that the prover is busy and will not initiate a new proving process.
    /// Returns the witness data that was provided by the caller.
    Busy(#[derivative(Debug = "ignore")] StateTransitionInfo<StateRoot, Witness, Da>),
}

/// An error that occurred during ZKP proving.
#[derive(Error, Debug)]
pub enum ProverServiceError {
    /// The prover is too busy to take on any additional jobs at the moment.
    #[error("Prover is too busy")]
    ProverBusy,
    /// Some internal prover error.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// This service is responsible for ZK proof generation.
/// The proof generation process involves the following stages:
///     1. Submitting a witness using the `submit_witness` method to a prover service.
///     2. Initiating proof generation with the `prove` method.
/// Once the proof is ready, it can be sent to the DA with `send_proof_to_da` method.
/// Currently, the cancellation of proving jobs for submitted witnesses is not supported,
/// but this functionality will be added in the future (#1185).
#[async_trait]
pub trait ProverService {
    /// Ths root hash of state merkle tree.
    type StateRoot: Serialize + Clone + AsRef<[u8]>;
    /// Data that is produced during batch execution.
    type Witness: Serialize;
    /// Data Availability service.
    type DaService: DaService;

    /// Verifier for the aggregated proof.
    type Verifier: Zkvm;

    /// Creates ZK proof for a block corresponding to `block_header_hash`.
    async fn prove(
        &self,
        state_transition_info: StateTransitionInfo<
            Self::StateRoot,
            Self::Witness,
            <Self::DaService as DaService>::Spec,
        >,
    ) -> Result<
        ProofProcessingStatus<Self::StateRoot, Self::Witness, <Self::DaService as DaService>::Spec>,
        ProverServiceError,
    >;

    /// Sends the ZK proof to the DA.
    /// This method is noy yet fully implemented: see #1185
    async fn create_aggregated_proof(
        &self,
        block_header_hashes: &[<<Self::DaService as DaService>::Spec as DaSpec>::SlotHash],
    ) -> Result<ProofAggregationStatus, anyhow::Error>;
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn prover_config_debug_and_display_are_the_same() {
        let config = RollupProverConfig::Skip;
        assert_eq!(format!("{:?}", config), format!("{}", config));
    }

    #[test]
    fn prover_config_display_from_str() {
        let config = RollupProverConfig::Skip;
        assert_eq!(
            RollupProverConfig::from_str(&config.to_string()).unwrap(),
            config
        );
    }
}
