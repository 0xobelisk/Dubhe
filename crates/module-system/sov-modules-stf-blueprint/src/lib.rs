#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
mod stf_blueprint;
use serde::{Deserialize, Serialize};
use sov_modules_api::TxScratchpad;
mod batch_processing;
#[cfg(feature = "test-utils")]
mod utils;
pub use batch_processing::{process_tx, BatchReceipt, TransactionReceipt};
#[cfg(all(target_os = "zkvm", feature = "bench"))]
use risc0_cycle_macros::cycle_tracker;
use sov_modules_api::capabilities::{AuthenticationError, HasCapabilities, RuntimeAuthenticator};
use sov_modules_api::hooks::{ApplyBatchHooks, FinalizeHook, SlotHooks, TxHooks};
use sov_modules_api::runtime::capabilities::{Kernel, KernelSlotHooks};
use sov_modules_api::transaction::SequencerReward;
pub use sov_modules_api::{BatchWithId, BlobData};
use sov_modules_api::{
    BlobDataWithId, DaSpec, DispatchCall, Error, Gas, GasArray, Genesis, KernelWorkingSet,
    RuntimeEventProcessor, Spec, StateCheckpoint, VersionedStateReadWriter, WorkingSet,
};
use sov_rollup_interface::common::HexHash;
use sov_rollup_interface::da::RelevantBlobIters;
use sov_rollup_interface::stf::{ApplySlotOutput, StateTransitionFunction};
use sov_sequencer_registry::BatchSequencerOutcome;
use sov_state::storage::StateUpdate;
use sov_state::Storage;
pub use stf_blueprint::StfBlueprint;
use thiserror::Error;
use tracing::info;
/// This trait has to be implemented by a runtime in order to be used in `StfBlueprint`.
///
/// The `TxHooks` implementation sets up a transaction context based on the height at which it is
/// to be executed.
pub trait Runtime<S: Spec, Da: DaSpec>:
    DispatchCall<Spec = S>
    + HasCapabilities<S, Da>
    + RuntimeAuthenticator<
        S,
        Decodable = <Self as DispatchCall>::Decodable,
        SequencerStakeMeter = <Self as HasCapabilities<S, Da>>::SequencerStakeMeter,
        AuthorizationData = <Self as HasCapabilities<S, Da>>::AuthorizationData,
    > + Genesis<Spec = S, Config = Self::GenesisConfig>
    + TxHooks<Spec = S, TxState = WorkingSet<S>>
    + SlotHooks<Spec = S>
    + FinalizeHook<Spec = S>
    + ApplyBatchHooks<Da, Spec = S, BatchResult = BatchSequencerOutcome>
    + Default
    + RuntimeEventProcessor
{
    /// GenesisConfig type.
    type GenesisConfig: Send + Sync;

    /// GenesisPaths type.
    #[cfg(feature = "native")]
    type GenesisPaths: Send + Sync;

    /// Default RPC methods and Axum router.
    #[cfg(feature = "native")]
    fn endpoints(storage: tokio::sync::watch::Receiver<S::Storage>) -> RuntimeEndpoints;

    /// Reads genesis configs.
    #[cfg(feature = "native")]
    fn genesis_config(
        genesis_paths: &Self::GenesisPaths,
    ) -> Result<Self::GenesisConfig, anyhow::Error>;
}

/// The reasons for which a transaction can be skipped
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Error)]
pub enum SkippedReason {
    /// The transaction had an invalid nonce.
    #[error("The transaction had an invalid nonce, reason: {0}.")]
    IncorrectNonce(String),
    /// Impossible to reserve gas for the transaction to be executed.
    #[error("Impossible to reserve gas for the transaction to be executed, reason: {0}.")]
    CannotReserveGas(String),
    /// Impossible to resolve the context of the transaction.
    #[error("Impossible to resolve the context of the transaction, reason: {0}.")]
    CannotResolveContext(String),
}

/// The effect of a transaction using the STF blueprint.
pub type TxEffect = sov_rollup_interface::stf::TxEffect<TxReceiptContents>;
/// The effect of a batch using the STF blueprint.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TxReceiptContents;

impl sov_rollup_interface::stf::TxReceiptContents for TxReceiptContents {
    type Reverted = Error;
    type Skipped = SkippedReason;
    type Successful = ();
}

/// The result of applying a transaction to the state.
/// This is the value returned when [`process_tx`] succeeds.
/// It contains the new transaction checkpoint, transaction receipt and the amount of gas tokens that the sequencer should be rewarded.
pub struct ApplyTxResult<S: Spec> {
    /// The transaction scratchpad following the application of the transaction.
    pub tx_scratchpad: TxScratchpad<S>,
    /// The transaction receipt.
    pub receipt: TransactionReceipt,
    /// The amount of gas tokens that the sequencer should be rewarded.
    pub sequencer_reward: SequencerReward,
}

/// The different errors that can be raised after transaction processing
#[derive(Error, Debug)]
pub enum TxProcessingErrorReason {
    /// The sequencer is not authorized to execute the transaction
    #[error("The sequencer is not authorized to execute the transaction, error {0}")]
    SequencerUnauthorized(String),
    /// The transaction was not correctly authenticated
    #[error("The transaction was not correctly authenticated {0}")]
    AuthenticationError(AuthenticationError),
    /// The transaction was not applied because it didn't pass the pre-execution gas checks
    /// (from the `GasEnforcer::try_reserve_gas` capability).
    /// In this case, the sequencer should be charged the amount of gas used for the pre-execution checks.
    #[error("The transaction was not applied because it didn't pass the pre-execution gas checks, reason: {reason}, tx hash: {}.", HexHash::new(*raw_tx_hash))]
    CannotReserveGas {
        /// The reason why this error was raised.
        reason: String,
        /// The raw hash of the transaction that was skipped.
        raw_tx_hash: [u8; 32],
    },
    /// The transaction was not applied because it was a duplicate.
    #[error("The transaction was not applied because it had an invalid nonce, reason: {reason}, tx hash: {}.", HexHash::new(*raw_tx_hash))]
    Nonce {
        /// The reason why this error was raised.
        reason: String,
        /// The raw hash of the transaction that was skipped.
        raw_tx_hash: [u8; 32],
    },

    /// The transaction was not applied because the `Context` could not be resolved.
    #[error("The transaction was not applied because the `Context` could not be resolved, reason: {reason}, tx hash: {}.", HexHash::new(*raw_tx_hash))]
    CannotResolveContext {
        /// The reason why this error was raised.
        reason: String,
        /// The raw hash of the transaction that was skipped.
        raw_tx_hash: [u8; 32],
    },
    /// Transaction from unregistered sequencer was rejected.
    /// These transactions can be processed in the case of direct sequencer registration.
    #[error("The unregistered senders transaction was rejected from processing, reason: {0}")]
    InvalidUnregisteredTx(String),
}

impl TryInto<(SkippedReason, [u8; 32])> for TxProcessingErrorReason {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<(SkippedReason, [u8; 32]), Self::Error> {
        match self {
            TxProcessingErrorReason::Nonce {
                reason,
                raw_tx_hash,
            } => Ok((SkippedReason::IncorrectNonce(reason), raw_tx_hash)),
            TxProcessingErrorReason::CannotResolveContext {
                reason,
                raw_tx_hash,
            } => Ok((SkippedReason::CannotResolveContext(reason), raw_tx_hash)),
            TxProcessingErrorReason::CannotReserveGas {
                reason,
                raw_tx_hash,
            } => Ok((SkippedReason::CannotReserveGas(reason), raw_tx_hash)),
            err => Err(anyhow::anyhow!(
                "The transaction processing error - {err} - cannot be mapped to a SkippedReason"
            )),
        }
    }
}

/// Error type raised when processing a transaction
pub struct TxProcessingError<S: Spec> {
    /// The transaction scratchpad when the error was raised
    pub tx_scratchpad: TxScratchpad<S>,
    /// The reason of the error
    pub reason: TxProcessingErrorReason,
}

/// Genesis parameters for a blueprint
pub struct GenesisParams<RuntimeConfig, KernelConfig> {
    /// The runtime genesis parameters
    pub runtime: RuntimeConfig,
    /// The kernel's genesis parameters
    pub kernel: KernelConfig,
}

impl<S, RT, Da, K> StfBlueprint<S, Da, RT, K>
where
    S: Spec,
    Da: DaSpec,
    RT: Runtime<S, Da>,
    K: KernelSlotHooks<S, Da>,
{
    #[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
    fn begin_slot(
        &self,
        state_checkpoint: &mut StateCheckpoint<S>,
        slot_header: &Da::BlockHeader,
        validity_condition: &Da::ValidityCondition,
        pre_state_root: &<S::Storage as Storage>::Root,
    ) -> <S::Gas as Gas>::Price {
        // WARNING: The kernel slot hooks should always be called before the runtime slot hooks.
        // That way the state of the runtime modules is always in sync with the transaction `being executed`.
        let gas_price = self.kernel.begin_slot_hook(
            slot_header,
            validity_condition,
            pre_state_root,
            state_checkpoint,
        );

        // We build and pass down the VersionedStateReadWriter to the [`begin_slot_hook`] method to have access to context
        // aware information.
        let kernel_working_set = KernelWorkingSet::from_kernel(&self.kernel, state_checkpoint);
        let mut versioned_working_set =
            VersionedStateReadWriter::from_kernel_ws_virtual(kernel_working_set);

        let visible_hash = <S as Spec>::VisibleHash::from(pre_state_root.clone());

        self.runtime
            .begin_slot_hook(visible_hash, &mut versioned_working_set);

        gas_price
    }

    #[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
    fn end_slot(
        &self,
        storage: S::Storage,
        gas_used: &S::Gas,
        mut checkpoint: StateCheckpoint<S>,
    ) -> (
        <S::Storage as Storage>::Root,
        <S::Storage as Storage>::Witness,
        <S::Storage as Storage>::ChangeSet,
    ) {
        // Run end_slot_hook
        self.runtime.end_slot_hook(&mut checkpoint);
        self.kernel.end_slot_hook(gas_used, &mut checkpoint);

        let (cache_log, mut accessory_delta, witness) = checkpoint.freeze();

        let (root_hash, mut state_update) = storage
            .compute_state_update(cache_log, &witness)
            .expect("jellyfish merkle tree update must succeed");

        let visible_root_hash = <S as Spec>::VisibleHash::from(root_hash.clone());

        self.runtime
            .finalize_hook(visible_root_hash, &mut accessory_delta);

        state_update.add_accessory_items(accessory_delta.freeze());
        let change_set = storage.materialize_changes(&state_update);

        (root_hash, witness, change_set)
    }
}

impl<S, RT, Da, K> StateTransitionFunction<S::InnerZkvm, S::OuterZkvm, Da>
    for StfBlueprint<S, Da, RT, K>
where
    S: Spec,
    Da: DaSpec,
    RT: Runtime<S, Da>,
    K: KernelSlotHooks<S, Da, BlobType = BlobDataWithId>,
{
    type StateRoot = <S::Storage as Storage>::Root;

    type Address = S::Address;

    type GenesisParams =
        GenesisParams<<RT as Genesis>::Config, <K as Kernel<S, Da>>::GenesisConfig>;
    type PreState = S::Storage;
    type ChangeSet = <S::Storage as Storage>::ChangeSet;

    type TxReceiptContents = TxReceiptContents;

    type BatchReceiptContents = BatchSequencerOutcome;

    type ProofReceiptContents = ();

    type Witness = <S::Storage as Storage>::Witness;

    type Condition = Da::ValidityCondition;

    fn init_chain(
        &self,
        pre_state: Self::PreState,
        params: Self::GenesisParams,
    ) -> (Self::StateRoot, Self::ChangeSet) {
        // TODO(@preston-evans98): Get rid of the Clone here by making pre-state read only.
        let mut state_checkpoint = StateCheckpoint::new(pre_state.clone());
        let mut startup_ws = KernelWorkingSet::uninitialized(&mut state_checkpoint);

        // Important! The kernel *must* be initialized before the runtime, since runtime
        // module authors are allowed to depend on the kernel.
        self.kernel
            .genesis(&params.kernel, &mut startup_ws)
            .expect("Kernel initialization must succeed");

        // TODO(@theochap): for now we are using the unmetered gas meter here, but we should add type safety to be able to remove that method.
        let mut working_set = state_checkpoint.to_genesis_state_accessor::<RT>(&params.runtime);
        if let Err(e) = self.runtime.genesis(&params.runtime, &mut working_set) {
            tracing::error!(error = %e, "Runtime initialization must succeed");
            panic!("Runtime initialization must succeed {}", e);
        }

        let checkpoint = working_set.checkpoint();

        let (log, mut accessory_delta, witness) = checkpoint.freeze();

        let (genesis_hash, mut state_update) = pre_state
            .compute_state_update(log, &witness)
            .expect("Storage update must succeed");

        let visible_genesis_hash = <S as Spec>::VisibleHash::from(genesis_hash.clone());

        self.runtime
            .finalize_hook(visible_genesis_hash, &mut accessory_delta);

        state_update.add_accessory_items(accessory_delta.freeze());

        let change_set = pre_state.materialize_changes(&state_update);

        (genesis_hash, change_set)
    }

    fn apply_slot<'a, I>(
        &self,
        pre_state_root: &Self::StateRoot,
        pre_state: Self::PreState,
        witness: Self::Witness,
        slot_header: &Da::BlockHeader,
        validity_condition: &Da::ValidityCondition,
        relevant_blobs: RelevantBlobIters<I>,
    ) -> ApplySlotOutput<S::InnerZkvm, S::OuterZkvm, Da, Self>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        let mut checkpoint = StateCheckpoint::with_witness(pre_state.clone(), witness);
        let gas_price = self.begin_slot(
            &mut checkpoint,
            slot_header,
            validity_condition,
            pre_state_root,
        );

        let mut kernel_working_set = KernelWorkingSet::from_kernel(&self.kernel, &mut checkpoint);
        let visible_height = kernel_working_set.virtual_slot();

        let all_blobs = relevant_blobs
            .batch_blobs
            .into_iter()
            .chain(relevant_blobs.proof_blobs);

        let selected_blobs = self
            .kernel
            .get_blobs_for_this_slot(all_blobs, &mut kernel_working_set)
            .expect("blob selection must succeed, probably serialization failed");

        info!(
            blob_count = selected_blobs.len(),
            virtual_slot = visible_height,
            true_slot = kernel_working_set.current_slot(),
            "Selected batch(es) for execution in current slot"
        );

        let mut proof_receipts = Vec::new();
        let mut batch_receipts = vec![];

        let mut total_gas = S::Gas::zero();
        for (blob_idx, (blob, sender)) in selected_blobs.into_iter().enumerate() {
            match blob.data {
                BlobData::Batch(batch) => {
                    let batch_with_id = BatchWithId { batch, id: blob.id };

                    let (next_checkpoint, batch_receipt, gas_used) = self.process_batch(
                        batch_with_id,
                        checkpoint,
                        blob_idx,
                        &sender,
                        &gas_price,
                        visible_height,
                        blob.from_registered_sequencer,
                    );

                    checkpoint = next_checkpoint;
                    batch_receipts.push(batch_receipt);
                    total_gas.combine(&gas_used);
                }
                BlobData::Proof(proof) => {
                    let (receipt, next_checkpoint) = self.process_proof(proof, checkpoint);

                    checkpoint = next_checkpoint;
                    proof_receipts.push(receipt);
                }
            }
        }

        let (state_root, witness, change_set) = self.end_slot(pre_state, &total_gas, checkpoint);
        ApplySlotOutput {
            state_root,
            change_set,
            proof_receipts,
            batch_receipts,
            witness,
        }
    }
}

/// The return type of [`Runtime::endpoints`].
#[cfg(feature = "native")]
pub struct RuntimeEndpoints {
    /// The [`axum::Router`] for the runtime's HTTP server.
    pub axum_router: axum::Router<()>,
    /// A [`jsonrpsee::RpcModule`] for the runtime's JSON-RPC server.
    pub jsonrpsee_module: jsonrpsee::RpcModule<()>,
}

#[cfg(feature = "native")]
impl Default for RuntimeEndpoints {
    fn default() -> Self {
        Self {
            axum_router: Default::default(),
            jsonrpsee_module: jsonrpsee::RpcModule::new(()),
        }
    }
}
