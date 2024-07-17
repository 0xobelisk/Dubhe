use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use sov_bank::{Bank, IntoPayable};
use sov_modules_api::capabilities::{
    AuthenticationResult, AuthorizationData, AuthorizeSequencerError, GasEnforcer, HasCapabilities,
    ProofProcessor, RuntimeAuthenticator, RuntimeAuthorization, SequencerAuthorization,
    TryReserveGasError,
};
use sov_modules_api::hooks::{ApplyBatchHooks, FinalizeHook, SlotHooks, TxHooks};
use sov_modules_api::transaction::{AuthenticatedTransactionData, TransactionConsumption};
use sov_modules_api::{
    BatchWithId, Context, DispatchCall, EncodeCall, Gas, GasMeter, Genesis, GenesisState,
    MeteredBorshDeserializeError, Module, ModuleInfo, PreExecWorkingSet, ProofReceipt, RawTx,
    RuntimeEventProcessor, Spec, StateAccessor, StateCheckpoint, TxScratchpad, TypedEvent,
    UnlimitedGasMeter, UnmeteredStateWrapper, WorkingSet,
};
use sov_modules_stf_blueprint::Runtime;
use sov_rollup_interface::da::DaSpec;
use sov_rollup_interface::stf::ProofOutcome;
use sov_rollup_interface::zk::aggregated_proof::SerializedAggregatedProof;
use sov_sequencer_registry::{BatchSequencerOutcome, SequencerRegistry, SequencerStakeMeter};
use sov_state::Storage;

use super::traits::{
    EndSlotHookRegistry, MinimalGenesis, MinimalRuntime, PostTxHookRegistry, StandardRuntime,
    TestRuntimeHookOverrides,
};

pub type WorkingSetClosure<T> =
    Box<dyn FnOnce(UnmeteredStateWrapper<<T as TxHooks>::TxState>) + Send + Sync>;
pub type StateRootClosure<Call, Root, Ws> = dyn FnMut(&mut Call, Root, &mut Ws) + Send + Sync;
pub type EndSlotClosure<T> = Box<dyn FnMut(&mut T) + Send + Sync>;

/// A queue of closures which can be executed in a `Runtime`'s post transaction hook.
///
/// The queue is `None` if no closures have ever been inserted, in which case
/// the runtime will not attempt to execute any closures. If the `closures` field is `Some`,
/// but not enough closures are provided, the runtime will treat this as an error.
pub(crate) struct ClosureQueue<T> {
    closures: Mutex<Option<VecDeque<T>>>,
}

impl<T> Default for ClosureQueue<T> {
    fn default() -> Self {
        Self {
            closures: Mutex::new(None),
        }
    }
}

impl<T> ClosureQueue<T> {
    pub fn insert_all(&self, closures: Vec<T>) {
        // Sleep until the the queue is empty. This ensures that two different tests using the same runtime
        // cannot pollute each other's closure queues. Note that this requires a catch_unwind handler when a test panics
        // to empty the queue so that other tests can run.
        let mut guard = self.closures.lock().unwrap();
        let contents = guard.get_or_insert_with(Default::default);
        contents.extend(closures);
    }

    pub fn try_get_next(&self) -> Option<Option<T>> {
        self.closures
            .lock()
            .unwrap()
            .as_mut()
            .map(|x| x.pop_front())
    }
}

#[derive(Default, Clone)]
pub struct TestRuntimeWrapper<S: Spec, Da: DaSpec, T: TxHooks<Spec = S>> {
    pub inner: T,
    pub(super) post_tx_hook_action_queue: Arc<ClosureQueue<WorkingSetClosure<T>>>,
    pub(super) end_slot_hook_action_queue: Arc<ClosureQueue<EndSlotClosure<StateCheckpoint<S>>>>,
    phantom: PhantomData<(S, Da)>,
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> PostTxHookRegistry<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    fn try_get_next_tx_action(&self) -> Option<Option<WorkingSetClosure<Self>>> {
        self.post_tx_hook_action_queue.try_get_next()
    }

    // Add assertions to the post dispatch hook. Callers should provide exactly one assertion per transaction.
    fn add_post_dispatch_tx_hook_actions(&self, closures: Vec<WorkingSetClosure<Self>>) {
        self.post_tx_hook_action_queue.insert_all(closures);
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> EndSlotHookRegistry<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    fn add_end_slot_hook_actions(&self, closures: Vec<EndSlotClosure<StateCheckpoint<S>>>) {
        self.end_slot_hook_action_queue.insert_all(closures);
    }

    fn try_get_next_slot_action(&self) -> Option<Option<EndSlotClosure<StateCheckpoint<S>>>> {
        self.end_slot_hook_action_queue.try_get_next()
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> TestRuntimeHookOverrides<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    // Override the post dispatch hook to run the assertions which
    // were set up using `add_post_dispatch_tx_hook_actions`
    fn post_dispatch_tx_hook_override(
        &self,
        _tx: &AuthenticatedTransactionData<S>,
        _ctx: &Context<S>,
        working_set: &mut <Self as TxHooks>::TxState,
    ) -> anyhow::Result<()> {
        if let Some(queue) = self.try_get_next_tx_action() {
            let closure = queue
                .into_iter()
                .next()
                .expect("Must provide one closure per transaction");

            closure(working_set.to_unmetered());
        }
        Ok(())
    }

    fn end_slot_hook_override(&self, working_set: &mut StateCheckpoint<S>) {
        if let Some(queue) = self.try_get_next_slot_action() {
            let mut closure = queue
                .into_iter()
                .next()
                .expect("Must provide one closure per transaction");

            closure(working_set);
        }
    }
}
impl<S: Spec, Da: DaSpec, T: MinimalGenesis<S, Da = Da> + TxHooks<Spec = S>> MinimalGenesis<S>
    for TestRuntimeWrapper<S, Da, T>
{
    type Da = Da;
    fn sequencer_registry_config(
        config: &<T as Genesis>::Config,
    ) -> &<SequencerRegistry<S, Da> as Genesis>::Config {
        T::sequencer_registry_config(config)
    }

    fn bank_config(config: &<T as Genesis>::Config) -> &<Bank<S> as Genesis>::Config {
        T::bank_config(config)
    }
}

impl<S, Da, T> TxHooks for TestRuntimeWrapper<S, Da, T>
where
    Self: TestRuntimeHookOverrides<S, Da>,
    T: StandardRuntime<S, Da>,
    S: Spec,
    Da: DaSpec,
{
    type Spec = S;
    type TxState = WorkingSet<S>;

    fn pre_dispatch_tx_hook(
        &self,
        tx: &AuthenticatedTransactionData<S>,
        state: &mut Self::TxState,
    ) -> anyhow::Result<()> {
        self.pre_dispatch_tx_hook_override(tx, state)
    }

    fn post_dispatch_tx_hook(
        &self,
        tx: &AuthenticatedTransactionData<S>,
        ctx: &Context<S>,
        state: &mut Self::TxState,
    ) -> anyhow::Result<()> {
        self.post_dispatch_tx_hook_override(tx, ctx, state)
    }
}

impl<S, Da: DaSpec, T> DispatchCall for TestRuntimeWrapper<S, Da, T>
where
    T: StandardRuntime<S, Da>,
    S: Spec,
    Da: DaSpec,
{
    type Spec = S;

    type Decodable = T::Decodable;

    fn decode_call(
        serialized_message: &[u8],
        meter: &mut impl GasMeter<<Self::Spec as Spec>::Gas>,
    ) -> Result<Self::Decodable, MeteredBorshDeserializeError<<Self::Spec as Spec>::Gas>> {
        T::decode_call(serialized_message, meter)
    }

    fn dispatch_call(
        &self,
        message: Self::Decodable,
        state: &mut WorkingSet<S>,
        context: &Context<S>,
    ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
        self.inner.dispatch_call(message, state, context)
    }

    fn module_id(&self, message: &Self::Decodable) -> &sov_modules_api::ModuleId {
        self.inner.module_id(message)
    }
}

impl<S, Da: DaSpec, T> ApplyBatchHooks<Da> for TestRuntimeWrapper<S, Da, T>
where
    Self: TestRuntimeHookOverrides<S, Da>,
    T: StandardRuntime<S, Da>,
    S: Spec,
    T: MinimalRuntime<S, Da>,
{
    type Spec = S;
    type BatchResult = BatchSequencerOutcome;

    fn begin_batch_hook(
        &self,
        batch: &BatchWithId,
        sender: &Da::Address,
        state_checkpoint: &mut StateCheckpoint<S>,
    ) -> anyhow::Result<()> {
        self.begin_batch_hook_override(batch, sender, state_checkpoint)
    }

    fn end_batch_hook(
        &self,
        result: Self::BatchResult,
        sender: &Da::Address,
        state_checkpoint: &mut StateCheckpoint<S>,
    ) {
        self.end_batch_hook_override(result, sender, state_checkpoint);
    }
}

impl<S, Da, T> SlotHooks for TestRuntimeWrapper<S, Da, T>
where
    Self: TestRuntimeHookOverrides<S, Da>,
    T: StandardRuntime<S, Da>,
    S: Spec,
    Da: DaSpec,
{
    type Spec = S;

    fn begin_slot_hook(
        &self,
        pre_state_root: S::VisibleHash,
        state: &mut sov_modules_api::VersionedStateReadWriter<StateCheckpoint<S>>,
    ) {
        self.begin_slot_hook_override(pre_state_root, state);
    }

    fn end_slot_hook(&self, state: &mut StateCheckpoint<S>) {
        self.end_slot_hook_override(state);
    }
}

impl<S, Da, T> FinalizeHook for TestRuntimeWrapper<S, Da, T>
where
    Self: TestRuntimeHookOverrides<S, Da>,
    T: StandardRuntime<S, Da>,
    S: Spec,
    Da: DaSpec,
{
    type Spec = S;
    fn finalize_hook(
        &self,
        root_hash: S::VisibleHash,
        state: &mut impl sov_modules_api::prelude::StateReaderAndWriter<
            sov_state::namespaces::Accessory,
        >,
    ) {
        self.finalize_hook_override(root_hash, state);
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> RuntimeAuthenticator<S>
    for TestRuntimeWrapper<S, Da, T>
{
    type Decodable = <T as DispatchCall>::Decodable;

    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;

    type AuthorizationData = AuthorizationData<S>;

    fn authenticate(
        &self,
        raw_tx: &RawTx,
        pre_exec_ws: &mut PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> AuthenticationResult<S, Self::Decodable, Self::AuthorizationData> {
        sov_modules_api::capabilities::authenticate::<S, Self, Self::SequencerStakeMeter>(
            &raw_tx.data,
            pre_exec_ws,
        )
    }

    fn authenticate_unregistered(
        &self,
        raw_tx: &RawTx,
        pre_exec_ws: &mut PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>>,
    ) -> AuthenticationResult<
        S,
        Self::Decodable,
        Self::AuthorizationData,
        sov_modules_api::capabilities::UnregisteredAuthenticationError,
    > {
        Ok(sov_modules_api::capabilities::authenticate::<
            S,
            Self,
            UnlimitedGasMeter<S::Gas>,
        >(&raw_tx.data, pre_exec_ws)?)
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> MinimalRuntime<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    fn bank(&self) -> &Bank<S> {
        self.inner.bank()
    }

    fn sequencer_registry(&self) -> &SequencerRegistry<S, Da> {
        self.inner.sequencer_registry()
    }

    fn base_fee_recipient(&self) -> impl sov_bank::Payable<S> {
        self.inner.base_fee_recipient()
    }
}

impl<S, Da, T, M> EncodeCall<M> for TestRuntimeWrapper<S, Da, T>
where
    T: EncodeCall<M>,
    T: StandardRuntime<S, Da>,
    S: Spec,
    Da: DaSpec,
    M: Module,
{
    fn encode_call(message: M::CallMessage) -> Vec<u8> {
        T::encode_call(message)
    }
}

impl<S, Da, T> Runtime<S, Da> for TestRuntimeWrapper<S, Da, T>
where
    Self: TestRuntimeHookOverrides<S, Da>,
    T: StandardRuntime<S, Da>,
    Self: DispatchCall<
        Decodable = <Self as RuntimeAuthenticator<S>>::Decodable,
        Spec = <T as DispatchCall>::Spec,
    >,
    <Self as Genesis>::Config: Send + Sync,
    S: Spec,
    Da: DaSpec,
    Self: TxHooks<TxState = WorkingSet<S>>,
{
    type GenesisConfig = <Self as Genesis>::Config;

    type GenesisPaths = ();

    fn endpoints(
        _storage: tokio::sync::watch::Receiver<S::Storage>,
    ) -> sov_modules_stf_blueprint::RuntimeEndpoints {
        todo!()
    }

    fn genesis_config(
        _genesis_paths: &Self::GenesisPaths,
    ) -> Result<Self::GenesisConfig, anyhow::Error> {
        todo!()
    }
}

// This test runtime has custom implementations of the capabilities
impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> HasCapabilities<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    type Capabilities<'a> = Self
    where
    T: 'a,;
    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;

    type AuthorizationData = AuthorizationData<S>;

    fn capabilities(&self) -> Self::Capabilities<'_> {
        Self::default()
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> RuntimeEventProcessor
    for TestRuntimeWrapper<S, Da, T>
{
    type RuntimeEvent = T::RuntimeEvent;
    fn convert_to_runtime_event(event: TypedEvent) -> Option<Self::RuntimeEvent> {
        T::convert_to_runtime_event(event)
    }
}

impl<S: Spec, Da: DaSpec, T: Genesis<Spec = S> + TxHooks<Spec = S>> Genesis
    for TestRuntimeWrapper<S, Da, T>
{
    type Spec = S;
    type Config = T::Config;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<(), sov_modules_api::Error> {
        self.inner.genesis(config, state)
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> GasEnforcer<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    /// Reserves enough gas for the transaction to be processed, if possible.
    fn try_reserve_gas<Meter: GasMeter<S::Gas>>(
        &self,
        tx: &AuthenticatedTransactionData<S>,
        context: &Context<S>,
        pre_exec_working_set: PreExecWorkingSet<S, Meter>,
    ) -> Result<WorkingSet<S>, TryReserveGasError<S, Meter>> {
        self.bank()
            .reserve_gas(tx, context.sender(), pre_exec_working_set)
            .map_err(Into::into)
    }

    fn allocate_consumed_gas(
        &self,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        self.bank().allocate_consumed_gas(
            &self.inner.base_fee_recipient(),
            &self.sequencer_registry().id().to_payable(),
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
        self.bank()
            .refund_remaining_gas(context.sender(), tx_consumption, tx_scratchpad);
    }
}

impl<S: Spec, Da: DaSpec, T: StandardRuntime<S, Da>> SequencerAuthorization<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;

    fn authorize_sequencer(
        &self,
        sequencer: &<Da as DaSpec>::Address,
        base_fee_per_gas: &<S::Gas as Gas>::Price,
        tx_scratchpad: TxScratchpad<S>,
    ) -> Result<PreExecWorkingSet<S, Self::SequencerStakeMeter>, AuthorizeSequencerError<S>> {
        self.sequencer_registry()
            .authorize_sequencer(sequencer, base_fee_per_gas, tx_scratchpad)
    }

    fn penalize_sequencer(
        &self,
        sequencer: &Da::Address,
        reason: impl std::fmt::Display,
        pre_exec_working_set: PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> TxScratchpad<S> {
        self.sequencer_registry()
            .penalize_sequencer(sequencer, reason, pre_exec_working_set)
    }
}

impl<T: StandardRuntime<S, Da>, S: Spec, Da: DaSpec> RuntimeAuthorization<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;

    type AuthorizationData = AuthorizationData<S>;
    /// Prevents duplicate transactions from running.
    // TODO(@preston-evans98): Use type system to prevent writing to the `StateCheckpoint` during this check
    fn check_uniqueness<Meter: GasMeter<S::Gas>>(
        &self,
        _auth_tx: &Self::AuthorizationData,
        _context: &Context<S>,
        _state: &mut PreExecWorkingSet<S, Meter>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// Resolves the context for a transaction.
    fn resolve_context(
        &self,
        auth_tx: &Self::AuthorizationData,
        sequencer: &Da::Address,
        height: u64,
        state: &mut PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> Result<Context<S>, anyhow::Error> {
        let sender = auth_tx.default_address.clone().unwrap();
        let sequencer = self
            .sequencer_registry()
            .resolve_da_address(sequencer, state)?
            .expect("Sequencer is no longer registered by the time of context resolution. This is a bug");
        Ok(Context::new(
            sender,
            auth_tx.credentials.clone(),
            sequencer,
            height,
        ))
    }

    fn resolve_unregistered_context(
        &self,
        auth_tx: &Self::AuthorizationData,
        height: u64,
        _state: &mut PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>>,
    ) -> Result<Context<S>, anyhow::Error> {
        let sender = auth_tx.default_address.clone().unwrap();
        // The tx sender & sequencer are the same entity
        Ok(Context::new(
            sender.clone(),
            auth_tx.credentials.clone(),
            sender,
            height,
        ))
    }

    /// Marks a transaction as having been executed, preventing it from executing again.
    fn mark_tx_attempted(
        &self,
        _auth_tx: &Self::AuthorizationData,
        _sequencer: &Da::Address,
        _tx_scratchpad: &mut TxScratchpad<S>,
    ) {
    }
}

impl<T: StandardRuntime<S, Da>, S: Spec, Da: DaSpec> ProofProcessor<S, Da>
    for TestRuntimeWrapper<S, Da, T>
{
    fn process_proof(
        &self,
        _proof_batch: Vec<u8>,
        state: StateCheckpoint<S>,
    ) -> (
        ProofReceipt<S::Address, Da, <S::Storage as Storage>::Root, ()>,
        StateCheckpoint<S>,
    ) {
        (
            ProofReceipt {
                raw_proof: SerializedAggregatedProof {
                    raw_aggregated_proof: vec![],
                },
                blob_hash: [0; 32],
                outcome: ProofOutcome::Ignored,
                extra_data: (),
            },
            state,
        )
    }
}
