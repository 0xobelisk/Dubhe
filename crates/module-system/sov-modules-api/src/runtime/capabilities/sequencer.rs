use std::fmt;
use std::fmt::Debug;

use sov_rollup_interface::da::DaSpec;

use crate::{Gas, GasMeter, PreExecWorkingSet, Spec, TxScratchpad};

/// An error that can be returned within the [`SequencerAuthorization::authorize_sequencer`] capability.
pub struct AuthorizeSequencerError<S: Spec> {
    /// The reason why the sequencer was not authorized.
    pub reason: anyhow::Error,
    /// A [`TxScratchpad`] that contains all the changes made during the transaction processing
    pub tx_scratchpad: TxScratchpad<S>,
}

impl<S: Spec> Debug for AuthorizeSequencerError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AuthorizeSequencerError")
            .field(&self.reason)
            .finish()
    }
}

/// Authorizes the sequencer to submit and process batches.
pub trait SequencerAuthorization<S: Spec, Da: DaSpec> {
    /// A type-safe struct that should track the staked amount of the sequencer and the eventual execution penalities.
    type SequencerStakeMeter: GasMeter<S::Gas>;

    /// Checks if the sequencer has staked the minimum bond to attest transactions.
    ///
    /// ## Returns
    /// Returns a [`AuthorizeSequencerError`] error if the sequencer is not registered or does not have enough staked amount.
    /// Returns a [`PreExecWorkingSet`] if the sequencer is registered and has enough staked amount.
    fn authorize_sequencer(
        &self,
        sequencer: &Da::Address,
        base_fee_per_gas: &<S::Gas as Gas>::Price,
        tx_scratchpad: TxScratchpad<S>,
    ) -> Result<PreExecWorkingSet<S, Self::SequencerStakeMeter>, AuthorizeSequencerError<S>>;

    /// Penalizes the sequencer without slashing his account.
    /// If the sequencer is penalized, the stake amount of the sequencer is reduced, potentially preventing future transactions from being executed.
    ///
    /// ## Note
    /// This method consumes the [`PreExecWorkingSet`].
    /// It should only be called once the sequencer cannot be penalized anymore.
    /// The penalty should be accumulated in the [`SequencerAuthorization::SequencerStakeMeter`] during the execution of the transaction.
    fn penalize_sequencer(
        &self,
        sequencer: &Da::Address,
        reason: impl std::fmt::Display,
        pre_exec_ws: PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> TxScratchpad<S>;
}
