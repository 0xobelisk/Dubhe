use sov_modules_api::hooks::{ApplyBatchHooks, FinalizeHook, SlotHooks, TxHooks};
use sov_modules_api::{
    AccessoryStateReaderAndWriter, BatchWithId, Spec, StateCheckpoint, WorkingSet,
};
use sov_rollup_interface::da::DaSpec;
use sov_sequencer_registry::{BatchSequencerOutcome, SequencerRegistry};
use tracing::info;

use crate::runtime::Runtime;

impl<S: Spec, Da: DaSpec> TxHooks for Runtime<S, Da> {
    type Spec = S;
    type TxState = WorkingSet<S>;
}

impl<S: Spec, Da: DaSpec> ApplyBatchHooks<Da> for Runtime<S, Da> {
    type Spec = S;
    type BatchResult = BatchSequencerOutcome;

    fn begin_batch_hook(
        &self,
        batch: &BatchWithId,
        sender: &Da::Address,
        state: &mut StateCheckpoint<S>,
    ) -> anyhow::Result<()> {
        // Before executing each batch, check that the sender is registered as a sequencer
        self.sequencer_registry
            .begin_batch_hook(batch, sender, state)
    }

    fn end_batch_hook(
        &self,
        result: Self::BatchResult,
        sender: &Da::Address,
        state: &mut StateCheckpoint<S>,
    ) {
        // Since we need to make sure the `StfBlueprint` doesn't depend on the module system, we need to
        // convert the `SequencerOutcome` structures manually.
        match &result {
            BatchSequencerOutcome::Rewarded(amount) => {
                info!(%sender, ?amount, "Rewarding sequencer");
                <SequencerRegistry<S, Da> as ApplyBatchHooks<Da>>::end_batch_hook(
                    &self.sequencer_registry,
                    result,
                    sender,
                    state,
                );
            }
            BatchSequencerOutcome::Ignored(_) | BatchSequencerOutcome::NotRewardable => {}
            BatchSequencerOutcome::Slashed(reason) => {
                info!(%sender, ?reason, "Slashing sequencer");
                <SequencerRegistry<S, Da> as ApplyBatchHooks<Da>>::end_batch_hook(
                    &self.sequencer_registry,
                    result,
                    sender,
                    state,
                );
            }
        }
    }
}

impl<S: Spec, Da: DaSpec> SlotHooks for Runtime<S, Da> {
    type Spec = S;

    fn begin_slot_hook(
        &self,
        pre_state_root: <S as Spec>::VisibleHash,
        versioned_working_set: &mut sov_modules_api::VersionedStateReadWriter<StateCheckpoint<S>>,
    ) {
        self.evm
            .begin_slot_hook(pre_state_root, versioned_working_set);
    }

    fn end_slot_hook(&self, state: &mut sov_modules_api::StateCheckpoint<S>) {
        self.evm.end_slot_hook(state);
    }
}

impl<S: Spec, Da: sov_modules_api::DaSpec> FinalizeHook for Runtime<S, Da> {
    type Spec = S;

    fn finalize_hook(
        &self,
        #[allow(unused_variables)] root_hash: S::VisibleHash,
        #[allow(unused_variables)] state: &mut impl AccessoryStateReaderAndWriter,
    ) {
        #[cfg(feature = "native")]
        self.evm.finalize_hook(root_hash, state);
    }
}
