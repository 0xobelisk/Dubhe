use std::sync::Arc;

use backon::{BackoffBuilder, ExponentialBuilder};
use sov_rollup_interface::da::BlockHeaderTrait;
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::stf::ProofSerializer;
use sov_rollup_interface::zk::aggregated_proof::{AggregatedProof, SerializedAggregatedProof};
use sov_rollup_interface::zk::Zkvm;
use tokio::time::{sleep, Duration};
use types::{BlockProofInfo, BlockProofStatus, UnAggregatedProofList};

use self::types::AggregateProofMetadata;
use crate::prover_service::AggregatedProofPublicData;
use crate::{ProverService, StateTransitionInfo};

mod types;

const BACKOFF_POLICY_MIN_DELAY: u64 = 1;
const BACKOFF_POLICY_MAX_DELAY: u64 = 60;
const BACKOFF_POLICY_MAX_NUM_RETRIES: usize = 5;

/// Manages the lifecycle of the `AggregatedProof`.
pub struct ProofManager<Ps: ProverService> {
    da_service: Arc<Ps::DaService>,
    prover_service: Option<Ps>,
    outer_code_commitment: <Ps::Verifier as Zkvm>::CodeCommitment,
    proofs_to_create: UnAggregatedProofList<Ps>,
    aggregated_proof_block_jump: usize,
    proof_serializer: Box<dyn ProofSerializer>,
    backoff_policy: ExponentialBuilder,
}

impl<Ps: ProverService> ProofManager<Ps>
where
    Ps::DaService: DaService<Error = anyhow::Error>,
{
    /// Creates a new proof manager.
    pub fn new(
        da_service: Arc<Ps::DaService>,
        prover_service: Option<Ps>,
        outer_code_commitment: <Ps::Verifier as Zkvm>::CodeCommitment,
        aggregated_proof_block_jump: usize,
        proof_serializer: Box<dyn ProofSerializer>,
    ) -> Self {
        Self {
            da_service,
            prover_service,
            outer_code_commitment,
            proofs_to_create: UnAggregatedProofList::new(),
            aggregated_proof_block_jump,
            proof_serializer,
            backoff_policy: ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(BACKOFF_POLICY_MIN_DELAY))
                .with_max_delay(Duration::from_secs(BACKOFF_POLICY_MAX_DELAY))
                .with_max_times(BACKOFF_POLICY_MAX_NUM_RETRIES),
        }
    }

    /// Verifies raw proofs and returns collection of verified aggregated proofs.
    /// Stops on first invalid proof
    pub(crate) async fn verify_aggregated_proofs(
        &self,
        serialized_proofs: impl Iterator<Item = SerializedAggregatedProof>,
    ) -> anyhow::Result<Vec<AggregatedProof>> {
        let mut aggregated_proofs_data: Vec<AggregatedProof> = Vec::new();
        for serialized_proof in serialized_proofs {
            // Verify aggregated proof before storing it into the database.
            // TODO #815
            let public_data: AggregatedProofPublicData = match <Ps::Verifier as Zkvm>::verify(
                &serialized_proof.raw_aggregated_proof,
                &self.outer_code_commitment,
            ) {
                Ok(public_data) => public_data,
                Err(err) => {
                    tracing::info!(?err, "Received invalid aggregated proof for the DA");
                    return Ok(aggregated_proofs_data);
                }
            };

            aggregated_proofs_data.push(AggregatedProof::new(serialized_proof, public_data));
        }

        Ok(aggregated_proofs_data)
    }

    /// Attempts to generate an `AggregatedProof` and then posts it to DA.
    /// The proof is created only when there are enough of inner proofs in the `ProverService`` queue.
    pub(crate) async fn post_aggregated_proof_to_da_when_ready(
        &mut self,
        transition_data: StateTransitionInfo<
            Ps::StateRoot,
            Ps::Witness,
            <Ps::DaService as DaService>::Spec,
        >,
    ) -> anyhow::Result<()> {
        if let Some(prover_service) = self.prover_service.as_ref() {
            let block_hash = transition_data.da_block_header().hash();
            // Save the transition for later proving. This is temporarily redundant
            // since we always just try to prove blocks right away (because we don't have fee
            // estimates for proving built out yet).
            self.proofs_to_create.append(BlockProofInfo {
                status: BlockProofStatus::Waiting(transition_data),
                hash: block_hash,
                // TODO(@preston-evans98): estimate public data size. This requires a new API on the `prover_service`.
                // <https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/440>
                public_data_size: 0,
            });

            // Start proving the next block right away... for now.
            self.proofs_to_create
                .oldest_mut()
                .prove_any_unproven_blocks(prover_service)
                .await;

            // If we've covered enough blocks for the aggregate proof, generate and submit it to DA
            if self.proofs_to_create.current_proof_jump() >= self.aggregated_proof_block_jump {
                self.proofs_to_create.close_newest_proof();
                let metadata = self.proofs_to_create.take_oldest();
                let agg_proof = self
                    .create_aggregate_proof_with_retries(metadata, prover_service)
                    .await?;

                tracing::debug!(
                    bytes = agg_proof.raw_aggregated_proof.len(),
                    "Sending aggregated proof to DA"
                );

                let serialized_proof = self
                    .proof_serializer
                    .serialize_proof_blob_with_metadata(agg_proof)?;
                let fee = self.da_service.estimate_fee(serialized_proof.len()).await?;

                self.da_service
                    .send_aggregated_zk_proof(&serialized_proof, fee)
                    .await?;
            }
        }
        Ok(())
    }

    async fn create_aggregate_proof_with_retries(
        &self,
        mut metadata: AggregateProofMetadata<Ps>,
        prover_service: &Ps,
    ) -> anyhow::Result<SerializedAggregatedProof> {
        let mut attempt_num = 1u32;
        let mut backoff_iter = self.backoff_policy.build();

        loop {
            let maybe_backoff_duration = backoff_iter.next();
            match metadata.prove(prover_service).await {
                Ok(proof) => return Ok(proof),
                Err((returned_metadata, error)) => {
                    let error_message = format!("Failed to generate aggregate proof: {error}");

                    if error_message.contains("Elf parse error") {
                        // NOTE We exit early on this error since it means the we've failed to find/parse
                        // the zk circuit, and there's no recovering from that.
                        tracing::error!("Fatal error: {error_message}");
                        tracing::error!(
                            "Please check your zk circuit ELF file was built correctly!"
                        );
                        anyhow::bail!(error)
                    };

                    tracing::error!(error_message);
                    match maybe_backoff_duration {
                        None => {
                            tracing::warn!("Maximum number of retries exhausted - exiting");
                            anyhow::bail!(error)
                        }
                        Some(duration) => {
                            tracing::info!("Retrying generation of aggregate proof in {}s, attempt {attempt_num} of {}...", duration.as_secs(), BACKOFF_POLICY_MAX_NUM_RETRIES);
                            attempt_num += 1;
                            sleep(duration).await;
                            metadata = returned_metadata;
                            continue;
                        }
                    }
                }
            }
        }
    }
}
