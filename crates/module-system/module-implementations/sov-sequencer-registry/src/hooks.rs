#[cfg(all(target_os = "zkvm", feature = "bench"))]
use risc0_cycle_macros::cycle_tracker;
use sov_modules_api::hooks::ApplyBatchHooks;
use sov_modules_api::{BatchWithId, Spec, StateCheckpoint};

use crate::{AllowedSequencerError, BatchSequencerOutcome, SequencerRegistry};

impl<S: Spec, Da: sov_modules_api::DaSpec> ApplyBatchHooks<Da> for SequencerRegistry<S, Da> {
    type Spec = S;
    type BatchResult = BatchSequencerOutcome;

    #[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
    fn begin_batch_hook(
        &self,
        _batch: &BatchWithId,
        sender: &Da::Address,
        state: &mut StateCheckpoint<S>,
    ) -> anyhow::Result<()> {
        match self.is_sender_allowed(sender, state) {
            Ok(_) | Err(AllowedSequencerError::NotRegistered) => Ok(()),
            Err(AllowedSequencerError::InsufficientStakeAmount { .. }) => {
                anyhow::bail!(
                    "sender {} is not allowed to submit blobs, they are not sufficiently staked",
                    sender
                )
            }
        }
    }

    fn end_batch_hook(
        &self,
        result: Self::BatchResult,
        sender: &Da::Address,
        state_checkpoint: &mut StateCheckpoint<S>,
    ) {
        match result {
            BatchSequencerOutcome::Rewarded(amount) => {
                self.reward_sequencer(sender, amount.into(), state_checkpoint);
            }
            BatchSequencerOutcome::Slashed(_) => {
                self.slash_sequencer(sender, state_checkpoint);
            }
            BatchSequencerOutcome::Ignored(_) | BatchSequencerOutcome::NotRewardable => {}
        };
    }
}
