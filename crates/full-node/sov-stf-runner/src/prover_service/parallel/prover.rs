use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, RwLock};

use borsh::BorshSerialize;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sov_rollup_interface::da::{BlockHeaderTrait, DaSpec, DaVerifier};
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::zk::aggregated_proof::{
    AggregatedProofPublicData, CodeCommitment, SerializedAggregatedProof,
};
use sov_rollup_interface::zk::{
    StateTransitionPublicData, StateTransitionWitness, StateTransitionWitnessWithAddress,
    ZkvmGuest, ZkvmHost,
};
use tracing::{debug, error, info};

use super::state::{ProverState, ProverStatus};
use super::{ProverServiceError, Verifier};
use crate::prover_service::stf_info::BlockProof;
use crate::verifier::StateTransitionVerifier;
use crate::{
    ProofAggregationStatus, ProofProcessingStatus, RollupProverConfig, StateTransitionInfo,
};

// A prover that generates proofs in parallel using a thread pool. If the pool is saturated,
// the prover will reject new jobs.
pub(crate) struct Prover<Address, StateRoot, Witness, Da: DaService> {
    prover_address: Address,
    prover_state: Arc<RwLock<ProverState<Address, StateRoot, Da::Spec>>>,
    num_threads: usize,
    pool: rayon::ThreadPool,
    code_commitment: CodeCommitment,
    phantom: std::marker::PhantomData<(StateRoot, Witness, Da)>,
}

impl<Address, StateRoot, Witness, Da> Prover<Address, StateRoot, Witness, Da>
where
    Da: DaService,
    Address:
        BorshSerialize + Serialize + DeserializeOwned + AsRef<[u8]> + Clone + Send + Sync + 'static,
    StateRoot: Serialize + DeserializeOwned + Clone + AsRef<[u8]> + Send + Sync + 'static,
    Witness: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub(crate) fn new(
        prover_address: Address,
        num_threads: usize,
        code_commitment: CodeCommitment,
    ) -> Self {
        Self {
            code_commitment,
            num_threads,
            pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap(),

            prover_state: Arc::new(RwLock::new(ProverState {
                prover_status: Default::default(),
                pending_tasks_count: Default::default(),
            })),
            prover_address,
            phantom: PhantomData,
        }
    }

    pub(crate) fn start_proving<InnerVm, OuterVm, V>(
        &self,
        state_transition_info: StateTransitionInfo<StateRoot, Witness, <Da as DaService>::Spec>,
        config: Arc<RollupProverConfig>,
        mut inner_vm: InnerVm,
        zk_storage: V::PreState,
        verifier: Arc<Verifier<Da, InnerVm, OuterVm, V>>,
    ) -> Result<
        ProofProcessingStatus<StateRoot, Witness, <Da as DaService>::Spec>,
        ProverServiceError,
    >
    where
        InnerVm: ZkvmHost + 'static,
        OuterVm: ZkvmHost + 'static,
        V: StateTransitionFunction<
                <InnerVm::Guest as ZkvmGuest>::Verifier,
                <OuterVm::Guest as ZkvmGuest>::Verifier,
                Da::Spec,
            > + Send
            + Sync
            + 'static,
        V::PreState: Send + Sync + 'static,
    {
        let block_header_hash = state_transition_info.da_block_header().hash();

        let mut prover_state = self.prover_state.write().expect("Lock was poisoned");
        if let Some(duplicate_proof) = prover_state.get_prover_status(&block_header_hash) {
            return match duplicate_proof {
                ProverStatus::ProvingInProgress => Err(anyhow::anyhow!(
                    "Proof generation for {:?} still in progress",
                    block_header_hash
                )
                .into()),
                ProverStatus::Proved(_) => Err(anyhow::anyhow!(
                    "Witness for block_header_hash {:?}, submitted multiple times.",
                    block_header_hash,
                )
                .into()),
                ProverStatus::Err(e) => Err(anyhow::format_err!("{}", e).into()), // "Clone" the anyhow error without cloning, because anyhow doesn't support that
            };
        }

        let start_prover = prover_state.inc_task_count_if_not_busy(self.num_threads);

        let prover_state_clone = self.prover_state.clone();
        // Initiate a new proving job only if the prover is not busy.
        if start_prover {
            prover_state.set_to_proving(block_header_hash.clone());

            let data = StateTransitionWitnessWithAddress {
                stf_witness: state_transition_info.data,
                prover_address: self.prover_address.clone(),
            };

            let prover_address = self.prover_address.clone();

            inner_vm.add_hint(&data);

            self.pool.spawn(move || {
                tracing::info_span!("guest_execution").in_scope(|| {
                    let proof = make_inner_proof::<_, InnerVm, OuterVm, Da>(
                        inner_vm,
                        config,
                        zk_storage,
                        &verifier.stf_verifier,
                    );

                    let mut prover_state = prover_state_clone.write().expect("Lock was poisoned");

                    let StateTransitionWitness {
                        initial_state_root,
                        final_state_root,
                        da_block_header,
                        relevant_proofs,
                        relevant_blobs: blobs,
                        ..
                    } = data.stf_witness;

                    let validity_condition = verifier
                        .da_verifier
                        .verify_relevant_tx_list(&da_block_header, &blobs, relevant_proofs)
                        .expect("Invalid validity condition");

                    let block_proof = proof.map(|p| BlockProof {
                        _proof: p,
                        st: StateTransitionPublicData::<Address, Da::Spec, StateRoot> {
                            initial_state_root,
                            final_state_root,
                            slot_hash: block_header_hash.clone(),
                            validity_condition,
                            prover_address,
                        },
                        slot_number: state_transition_info.slot_number,
                    });

                    prover_state.set_to_proved(block_header_hash, block_proof);
                    prover_state.dec_task_count();
                });
            });

            Ok(ProofProcessingStatus::ProvingInProgress)
        } else {
            Ok(ProofProcessingStatus::Busy(state_transition_info))
        }
    }

    pub(crate) fn create_aggregated_proof<OuterVm: ZkvmHost + 'static>(
        &self,
        mut outer_vm: OuterVm,
        block_header_hashes: &[<Da::Spec as DaSpec>::SlotHash],
    ) -> Result<ProofAggregationStatus, anyhow::Error> {
        assert!(!block_header_hashes.is_empty());
        let mut prover_state = self.prover_state.write().expect("Lock was poisoned");

        let mut block_proofs_data = Vec::default();

        for slot_hash in block_header_hashes {
            let state = prover_state.get_prover_status(slot_hash);

            match state {
                Some(ProverStatus::ProvingInProgress) => {
                    return Ok(ProofAggregationStatus::ProofGenerationInProgress);
                }
                Some(ProverStatus::Proved(block_proof)) => {
                    assert_eq!(slot_hash, &block_proof.st.slot_hash);
                    block_proofs_data.push(block_proof);
                }
                Some(ProverStatus::Err(e)) => return Err(anyhow::anyhow!(e.to_string())),
                None => return Err(anyhow::anyhow!("Missing required proof of {:?}. Use the `prove` method to generate a proof of that block and try again.", slot_hash)),
            }
        }

        // It is ok to unwrap here as we asserted that block_proofs_data.len() >= 1.
        let initial_block_proof = block_proofs_data.first().unwrap();
        let final_block_proof = block_proofs_data.last().unwrap();

        let mut rewarded_addresses = Vec::default();
        let mut validity_conditions = Vec::default();
        for bp in block_proofs_data.iter() {
            rewarded_addresses.push(
                borsh::to_vec(&bp.st.prover_address).expect("Serializing to vec is infallible"),
            );

            validity_conditions.push(
                borsh::to_vec(&bp.st.validity_condition).expect("Serializing to vec is infallible"),
            );
        }

        let public_data = AggregatedProofPublicData {
            validity_conditions,
            rewarded_addresses,
            initial_slot_number: initial_block_proof.slot_number,
            final_slot_number: final_block_proof.slot_number,
            genesis_state_root: Default::default(),
            initial_state_root: initial_block_proof.st.initial_state_root.as_ref().to_vec(),
            final_state_root: final_block_proof.st.final_state_root.as_ref().to_vec(),
            initial_slot_hash: initial_block_proof.st.slot_hash.clone().into().to_vec(),
            final_slot_hash: final_block_proof.st.slot_hash.clone().into().to_vec(),
            code_commitment: self.code_commitment.clone(),
        };

        debug!(%public_data, "generating aggregate proof");
        // TODO: https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/316
        // `add_hint`  should take witness instead of the public input.
        outer_vm.add_hint(public_data);
        let serialized_aggregated_proof = SerializedAggregatedProof {
            raw_aggregated_proof: outer_vm.run(false)?,
        };

        for slot_hash in block_header_hashes {
            prover_state.remove(slot_hash);
        }
        Ok(ProofAggregationStatus::Success(serialized_aggregated_proof))
    }
}

fn make_inner_proof<V, InnerVm, OuterVm, Da>(
    mut vm: InnerVm,
    config: Arc<RollupProverConfig>,
    zk_storage: V::PreState,
    stf_verifier: &StateTransitionVerifier<V, Da::Verifier, InnerVm::Guest, OuterVm::Guest>,
) -> Result<Vec<u8>, anyhow::Error>
where
    Da: DaService,
    InnerVm: ZkvmHost + 'static,
    OuterVm: ZkvmHost + 'static,
    V: StateTransitionFunction<
            <InnerVm::Guest as ZkvmGuest>::Verifier,
            <OuterVm::Guest as ZkvmGuest>::Verifier,
            Da::Spec,
        > + Send
        + Sync
        + 'static,
    V::PreState: Send + Sync + 'static,
{
    let result = match config.deref() {
        RollupProverConfig::Skip => Ok(Vec::default()),
        RollupProverConfig::Simulate => stf_verifier
            .run_block(vm.simulate_with_hints(), zk_storage)
            .map(|_| Vec::default())
            .map_err(|e| anyhow::anyhow!("Guest execution must succeed but failed with {:?}", e)),
        RollupProverConfig::Execute => {
            info!(
                "Executing in VM without constructing proof using {}",
                std::any::type_name::<InnerVm>()
            );
            vm.run(false)
        }
        RollupProverConfig::Prove => {
            info!("Generating proof with {}", std::any::type_name::<InnerVm>());
            vm.run(true)
        }
    };
    match result {
        Ok(ref proof) => {
            info!(
                bytes = proof.len(),
                "Proof generation completed successfully"
            );
        }
        Err(ref e) => {
            error!("Proof generation failed: {:?}", e);
        }
    }
    result
}
