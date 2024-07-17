use borsh::BorshDeserialize;
use sov_bank::IntoPayable;
use sov_modules_api::capabilities::{
    AuthorizationData, AuthorizeSequencerError, GasEnforcer, ProofProcessor, RuntimeAuthorization,
    SequencerAuthorization, TryReserveGasError,
};
use sov_modules_api::prelude::tracing;
use sov_modules_api::proof_metadata::SerializeProofWithDetails;
use sov_modules_api::transaction::{AuthenticatedTransactionData, TransactionConsumption};
use sov_modules_api::{
    Context, DaSpec, Gas, GasMeter, ModuleInfo, PreExecWorkingSet, ProofOutcome, ProofReceipt,
    Spec, StateCheckpoint, Storage, TxScratchpad, UnlimitedGasMeter, WorkingSet,
};
use sov_rollup_interface::zk::aggregated_proof::SerializedAggregatedProof;
use sov_sequencer_registry::{SequencerRegistry, SequencerStakeMeter};

/// Implements the basic capabilities required for a zk-rollup runtime.
pub struct StandardProvenRollupCapabilities<'a, S: Spec, Da: DaSpec> {
    pub bank: &'a sov_bank::Bank<S>,
    pub sequencer_registry: &'a SequencerRegistry<S, Da>,
    pub accounts: &'a sov_accounts::Accounts<S>,
    pub nonces: &'a sov_nonces::Nonces<S>,
    pub prover_incentives: &'a sov_prover_incentives::ProverIncentives<S, Da>,
}

impl<'a, S: Spec, Da: DaSpec> GasEnforcer<S, Da> for StandardProvenRollupCapabilities<'a, S, Da> {
    /// Reserves enough gas for the transaction to be processed, if possible.
    fn try_reserve_gas<Meter: GasMeter<S::Gas>>(
        &self,
        tx: &AuthenticatedTransactionData<S>,
        context: &Context<S>,
        pre_exec_working_set: PreExecWorkingSet<S, Meter>,
    ) -> Result<WorkingSet<S>, TryReserveGasError<S, Meter>> {
        self.bank
            .reserve_gas(tx, context.sender(), pre_exec_working_set)
            .map_err(Into::into)
    }

    fn allocate_consumed_gas(
        &self,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        // TODO(@theochap): In the next PR this method will become failible
        self.bank.allocate_consumed_gas(
            &self.prover_incentives.id().to_payable(),
            &self.sequencer_registry.id().to_payable(),
            tx_consumption,
            tx_scratchpad,
        );
    }

    fn refund_remaining_gas(
        &self,
        context: &Context<S>,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        self.bank
            .refund_remaining_gas(context.sender(), tx_consumption, tx_scratchpad);
    }
}

impl<'a, S: Spec, Da: DaSpec> SequencerAuthorization<S, Da>
    for StandardProvenRollupCapabilities<'a, S, Da>
{
    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;

    fn authorize_sequencer(
        &self,
        sequencer: &<Da as DaSpec>::Address,
        base_fee_per_gas: &<S::Gas as Gas>::Price,
        tx_scratchpad: TxScratchpad<S>,
    ) -> Result<PreExecWorkingSet<S, Self::SequencerStakeMeter>, AuthorizeSequencerError<S>> {
        self.sequencer_registry
            .authorize_sequencer(sequencer, base_fee_per_gas, tx_scratchpad)
    }

    fn penalize_sequencer(
        &self,
        sequencer: &Da::Address,
        reason: impl std::fmt::Display,
        pre_exec_working_set: PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> TxScratchpad<S> {
        self.sequencer_registry
            .penalize_sequencer(sequencer, reason, pre_exec_working_set)
    }
}

impl<'a, S: Spec, Da: DaSpec> RuntimeAuthorization<S, Da>
    for StandardProvenRollupCapabilities<'a, S, Da>
{
    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;
    type AuthorizationData = AuthorizationData<S>;

    /// Prevents duplicate transactions from running.
    // TODO(@preston-evans98): Use type system to prevent writing to the `StateCheckpoint` during this check
    fn check_uniqueness<Meter: GasMeter<S::Gas>>(
        &self,
        auth_data: &Self::AuthorizationData,
        _context: &Context<S>,
        pre_exec_working_set: &mut PreExecWorkingSet<S, Meter>,
    ) -> Result<(), anyhow::Error> {
        self.nonces.check_nonce(
            &auth_data.credential_id,
            auth_data.nonce,
            pre_exec_working_set,
        )
    }

    /// Marks a transaction as having been executed, preventing it from executing again.
    fn mark_tx_attempted(
        &self,
        auth_data: &Self::AuthorizationData,
        _sequencer: &Da::Address,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        self.nonces
            .mark_tx_attempted(&auth_data.credential_id, tx_scratchpad);
    }

    /// Resolves the context for a transaction.
    fn resolve_context(
        &self,
        auth_data: &Self::AuthorizationData,
        sequencer: &Da::Address,
        height: u64,
        state: &mut PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> Result<Context<S>, anyhow::Error> {
        // TODO(@preston-evans98): This is a temporary hack to get the sequencer address
        // This should be resolved by the sequencer registry during blob selection
        let sequencer = self
            .sequencer_registry
            .resolve_da_address(sequencer, state)?
            .ok_or(anyhow::anyhow!("Sequencer was no longer registered by the time of context resolution. This is a bug")).unwrap();
        let sender = self.accounts.resolve_sender_address(
            &auth_data.default_address,
            &auth_data.credential_id,
            state,
        )?;
        Ok(Context::new(
            sender,
            auth_data.credentials.clone(),
            sequencer,
            height,
        ))
    }

    fn resolve_unregistered_context(
        &self,
        auth_data: &Self::AuthorizationData,
        height: u64,
        state: &mut PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>>,
    ) -> Result<Context<S>, anyhow::Error> {
        let sender = self.accounts.resolve_sender_address(
            &auth_data.default_address,
            &auth_data.credential_id,
            state,
        )?;
        // The tx sender & sequencer are the same entity
        Ok(Context::new(
            sender.clone(),
            auth_data.credentials.clone(),
            sender,
            height,
        ))
    }
}

impl<'a, S: Spec, Da: DaSpec> ProofProcessor<S, Da>
    for StandardProvenRollupCapabilities<'a, S, Da>
{
    fn process_proof(
        &self,
        raw_proof: Vec<u8>,
        state: StateCheckpoint<S>,
    ) -> (
        ProofReceipt<S::Address, Da, <S::Storage as Storage>::Root, ()>,
        StateCheckpoint<S>,
    ) {
        // TODO #815
        match SerializeProofWithDetails::<S>::try_from_slice(&raw_proof) {
            Ok(proof_with_details) => (
                ProofReceipt {
                    raw_proof: proof_with_details.proof,
                    blob_hash: [0; 32],
                    outcome: ProofOutcome::Ignored,
                    extra_data: (),
                },
                state,
            ),
            Err(e) => {
                tracing::warn!("Unable to deserialize raw proof from DA {}", e);
                (
                    ProofReceipt {
                        // TODO #815: We will return the serialized proof only for verified proofs.
                        raw_proof: SerializedAggregatedProof {
                            raw_aggregated_proof: Default::default(),
                        },
                        blob_hash: [0; 32],
                        outcome: ProofOutcome::Invalid,
                        extra_data: (),
                    },
                    state,
                )
            }
        }
    }
}
