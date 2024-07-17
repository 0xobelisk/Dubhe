//! Runtime state machine definitions.
use sov_state::namespaces::User;
#[cfg(feature = "native")]
use sov_state::Storage;
use sov_state::{
    CompileTimeNamespace, EventContainer, IsValueCached, Namespace, SlotKey, SlotValue,
};
#[cfg(feature = "native")]
use sov_state::{NativeStorage, ProvableCompileTimeNamespace, StorageProof};

use super::checkpoints::StateCheckpoint;
use super::internals::{Delta, RevertableWriter};
use super::seal::CachedAccessor;
use super::UniversalStateAccessor;
use crate::module::Spec;
use crate::state::events::TypedEvent;
use crate::transaction::{
    transaction_consumption_helper, AuthenticatedTransactionData, PriorityFeeBips,
    TransactionConsumption, TxGasMeter,
};
#[cfg(feature = "test-utils")]
use crate::UnlimitedGasMeter;
use crate::{Gas, GasMeter, GasMeteringError};
#[cfg(feature = "native")]
use crate::{ProvenStateAccessor, StateReaderAndWriter};

/// A state diff over the storage that contains all the changes related to transaction execution.
/// This structure is built from a [`StateCheckpoint`] and is used in the entire transaction lifecycle (from
/// pre-execution checks to post execution state updates).
///
/// ## Usage note
/// This method tracks the gas consumed outside of the transaction lifecycle without explicitely consuming a finite resource.
/// This should only be used in infailible methods.
pub struct TxScratchpad<S: Spec> {
    delta: RevertableWriter<Delta<S::Storage>>,
}

impl<S: Spec> StateCheckpoint<S> {
    /// Transforms this [`StateCheckpoint`] into a [`PreExecWorkingSet`].
    /// This method takes a [`GasMeter`] as an argument, which is used to charge the gas for the pre-execution checks from the sequencer.
    pub fn to_tx_scratchpad(self) -> TxScratchpad<S> {
        TxScratchpad::<S> {
            delta: RevertableWriter::new(self.delta),
        }
    }
}

impl<S: Spec, Meter: GasMeter<S::Gas>> From<PreExecWorkingSet<S, Meter>> for TxScratchpad<S> {
    fn from(value: PreExecWorkingSet<S, Meter>) -> Self {
        value.inner
    }
}

impl<S: Spec> UniversalStateAccessor for TxScratchpad<S> {
    fn get(&mut self, namespace: Namespace, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <RevertableWriter<Delta<S::Storage>> as UniversalStateAccessor>::get(
            &mut self.delta,
            namespace,
            key,
        )
    }

    fn set(&mut self, namespace: Namespace, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <RevertableWriter<Delta<S::Storage>> as UniversalStateAccessor>::set(
            &mut self.delta,
            namespace,
            key,
            value,
        )
    }

    fn delete(&mut self, namespace: Namespace, key: &SlotKey) -> IsValueCached {
        <RevertableWriter<Delta<S::Storage>> as UniversalStateAccessor>::delete(
            &mut self.delta,
            namespace,
            key,
        )
    }
}

impl<S: Spec> TxScratchpad<S> {
    #[cfg(feature = "native")]
    fn delta(&self) -> &Delta<S::Storage> {
        &self.delta.inner
    }

    pub fn commit(self) -> StateCheckpoint<S> {
        StateCheckpoint {
            delta: self.delta.commit(),
        }
    }

    pub fn revert(self) -> StateCheckpoint<S> {
        StateCheckpoint {
            delta: self.delta.revert(),
        }
    }

    pub fn to_pre_exec_working_set<Meter: GasMeter<S::Gas>>(
        self,
        gas_meter: Meter,
    ) -> PreExecWorkingSet<S, Meter> {
        PreExecWorkingSet {
            inner: self,
            gas_meter,
        }
    }
}

#[cfg(feature = "test-utils")]
impl<S: Spec> TxScratchpad<S> {
    /// Produces an unmetered [`PreExecWorkingSet`] from this [`StateCheckpoint`].
    /// This is useful for tests that don't need to track gas consumption.
    pub fn pre_exec_ws_unmetered(self) -> PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>> {
        PreExecWorkingSet {
            inner: self,
            gas_meter: UnlimitedGasMeter::new(),
        }
    }

    /// Produces an unmetered [`PreExecWorkingSet`] from this [`StateCheckpoint`] for a given price.
    /// This is useful for tests that don't need to test failure over gas exhaustion.
    pub fn pre_exec_ws_unmetered_with_price(
        self,
        gas_price: &<S::Gas as Gas>::Price,
    ) -> PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>> {
        PreExecWorkingSet {
            inner: self,
            gas_meter: UnlimitedGasMeter::new_with_price(gas_price.clone()),
        }
    }
}

/// A working set that can be used to charge gas for pre transaction execution checks.
pub struct PreExecWorkingSet<S: Spec, PreExecChecksMeter: GasMeter<S::Gas>> {
    inner: TxScratchpad<S>,
    gas_meter: PreExecChecksMeter,
}

pub struct AuthorizeTransactionError<S: Spec, PreExecChecksMeter: GasMeter<S::Gas>> {
    pub reason: anyhow::Error,
    pub pre_exec_working_set: PreExecWorkingSet<S, PreExecChecksMeter>,
}

impl<S: Spec, PreExecChecksMeter: GasMeter<S::Gas>> PreExecWorkingSet<S, PreExecChecksMeter> {
    /// Builds a [`WorkingSet`] from the this [`PreExecWorkingSet`].
    /// This method can fail if the transaction has not locked enough gas for the pre-execution checks.
    pub fn transfer_gas_to_working_set(
        self,
        tx: &AuthenticatedTransactionData<S>,
    ) -> Result<WorkingSet<S>, AuthorizeTransactionError<S, PreExecChecksMeter>> {
        let max_fee = tx.max_fee;

        let mut gas_meter = tx.gas_meter(self.gas_meter.gas_price());

        if let Err(e) = gas_meter.charge_gas(self.gas_meter.gas_used()) {
            return Err(AuthorizeTransactionError {
                reason: e.into(),
                pre_exec_working_set: self,
            });
        }

        Ok(WorkingSet {
            delta: RevertableWriter::new(self.inner),
            events: Default::default(),
            gas_meter,
            max_fee,
            max_priority_fee_bips: tx.max_priority_fee_bips,
        })
    }
}

impl<S: Spec, Meter: GasMeter<S::Gas>> GasMeter<S::Gas> for PreExecWorkingSet<S, Meter> {
    fn gas_used(&self) -> &S::Gas {
        self.gas_meter.gas_used()
    }

    fn refund_gas(&mut self, gas: &S::Gas) -> Result<(), GasMeteringError<S::Gas>> {
        self.gas_meter.refund_gas(gas)
    }

    fn gas_price(&self) -> &<S::Gas as Gas>::Price {
        self.gas_meter.gas_price()
    }

    fn charge_gas(&mut self, amount: &S::Gas) -> anyhow::Result<(), GasMeteringError<S::Gas>> {
        self.gas_meter.charge_gas(amount)
    }

    fn remaining_funds(&self) -> u64 {
        self.gas_meter.remaining_funds()
    }
}

impl<S: Spec, Meter: GasMeter<S::Gas>> CachedAccessor<User> for PreExecWorkingSet<S, Meter> {
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <TxScratchpad<S> as CachedAccessor<User>>::get_cached(&mut self.inner, key)
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <TxScratchpad<S> as CachedAccessor<User>>::set_cached(&mut self.inner, key, value)
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        <TxScratchpad<S> as CachedAccessor<User>>::delete_cached(&mut self.inner, key)
    }
}

#[cfg(feature = "test-utils")]
impl<S: Spec> StateCheckpoint<S> {
    /// Produces an unmetered [`WorkingSet`] from this [`StateCheckpoint`].
    /// This is useful for tests that don't need to track gas consumption.
    pub fn to_working_set_unmetered(self) -> WorkingSet<S> {
        let stashed_working_set = TxScratchpad {
            delta: RevertableWriter::new(self.delta),
        };

        WorkingSet {
            delta: RevertableWriter::new(stashed_working_set),
            events: Default::default(),
            gas_meter: TxGasMeter::unmetered(),
            max_fee: 0,
            max_priority_fee_bips: PriorityFeeBips::ZERO,
        }
    }

    /// Produces a metered [`WorkingSet`] from this [`StateCheckpoint`].
    /// This is useful for tests that need to bypass pre-execution checks.
    ///
    /// ## Deprecated(@theochap)
    /// This method is deprecated and will be removed in the future. Please refrain from writing tests that use this method.
    pub fn to_working_set_deprecated(
        self,
        tx: &AuthenticatedTransactionData<S>,
        gas_price: &<S::Gas as Gas>::Price,
    ) -> WorkingSet<S> {
        let stashed_working_set = TxScratchpad {
            delta: RevertableWriter::new(self.delta),
        };

        WorkingSet {
            delta: RevertableWriter::new(stashed_working_set),
            events: Default::default(),
            gas_meter: tx.gas_meter(gas_price),
            max_fee: tx.max_fee,
            max_priority_fee_bips: tx.max_priority_fee_bips,
        }
    }
}

/// This structure contains the read-write set and the events collected during the execution of a transaction.
/// There are two ways to convert it into a StateCheckpoint:
/// 1. By using the [`WorkingSet::finalize`] method, where all the changes are added to the underlying
/// [`TxScratchpad`].
/// 2. By using the [`WorkingSet::revert`] method, where the most recent changes are reverted and the previous [`TxScratchpad`] is returned.
pub struct WorkingSet<S: Spec> {
    pub(super) delta: RevertableWriter<TxScratchpad<S>>,
    events: Vec<TypedEvent>,
    gas_meter: TxGasMeter<S::Gas>,

    // Gas parameters of the transaction associated with the working set
    max_fee: u64,
    max_priority_fee_bips: PriorityFeeBips,
}

impl<S: Spec> WorkingSet<S> {
    /// Builds a [`crate::TransactionConsumption`] from the [`WorkingSet`].
    pub(crate) fn transaction_consumption(&self) -> TransactionConsumption<S::Gas> {
        // The base fee is the amount of gas consumed by the transaction execution.
        let base_fee = self.gas_meter.gas_used();
        let gas_price = self.gas_meter.gas_price();

        transaction_consumption_helper::<S>(
            base_fee,
            gas_price,
            self.max_fee,
            self.max_priority_fee_bips,
        )
    }

    /// Turns this [`WorkingSet`] into a [`TxScratchpad`], commits the changes to the [`WorkingSet`] to the
    /// inner scratchpad.
    pub fn finalize(
        self,
    ) -> (
        TxScratchpad<S>,
        TransactionConsumption<S::Gas>,
        Vec<TypedEvent>,
    ) {
        let tx_reward = self.transaction_consumption();
        (self.delta.commit(), tx_reward, self.events)
    }

    /// Reverts the most recent changes to this [`WorkingSet`], returning a pristine
    /// [`TxScratchpad`] instance.
    pub fn revert(self) -> (TxScratchpad<S>, TransactionConsumption<S::Gas>) {
        let tx_consumption = self.transaction_consumption();
        (self.delta.revert(), tx_consumption)
    }

    /// Extracts all typed events from this working set.
    pub fn take_events(&mut self) -> Vec<TypedEvent> {
        core::mem::take(&mut self.events)
    }

    /// Extracts a typed event at index `index`
    pub fn take_event(&mut self, index: usize) -> Option<TypedEvent> {
        if index < self.events.len() {
            Some(self.events.remove(index))
        } else {
            None
        }
    }

    /// Returns an immutable map of all typed events that have been previously
    /// written to this working set.
    pub fn events(&self) -> &[TypedEvent] {
        &self.events
    }

    /// Returns the remaining gas funds.
    pub fn gas_remaining_funds(&self) -> u64 {
        self.gas_meter.remaining_funds()
    }

    /// Returns the maximum fee that can be paid for this transaction expressed in gas token amount.
    pub fn max_fee(&self) -> u64 {
        self.max_fee
    }

    /// A helper function to create a new [`WorkingSet`] with a given gas price and remaining funds.
    #[cfg(test)]
    pub fn new_with_gas_meter(
        inner: S::Storage,
        remaining_funds: u64,
        price: &<S::Gas as Gas>::Price,
    ) -> Self {
        let state_checkpoint: StateCheckpoint<S> = StateCheckpoint::new(inner);
        let tx_scratchpad = TxScratchpad {
            delta: RevertableWriter::new(state_checkpoint.delta),
        };

        WorkingSet {
            delta: RevertableWriter::new(tx_scratchpad),
            events: Default::default(),
            gas_meter: TxGasMeter::new(remaining_funds, price.clone()),
            max_fee: 0,
            max_priority_fee_bips: PriorityFeeBips::ZERO,
        }
    }
}

#[cfg(feature = "test-utils")]
impl<S: Spec> WorkingSet<S> {
    /// Creates a new [`WorkingSet`] instance backed by the given [`Storage`].
    ///
    /// ## Deprecated(@theochap)
    /// This method is deprecated and will be removed in the future. Please refrain from writing
    /// tests that use this method.
    /// Instead, one could use (in decreasing order of preference):
    /// - the testing framework,
    /// - or [`crate::ApiStateAccessor::new`]
    /// - or [`StateCheckpoint::new`]
    pub fn new_deprecated(inner: S::Storage) -> Self {
        let state_checkpoint: StateCheckpoint<S> = StateCheckpoint::new(inner);
        let tx_scratchpad = TxScratchpad {
            delta: RevertableWriter::new(state_checkpoint.delta),
        };

        WorkingSet {
            delta: RevertableWriter::new(tx_scratchpad),
            events: Default::default(),
            gas_meter: TxGasMeter::unmetered(),
            max_fee: 0,
            max_priority_fee_bips: PriorityFeeBips::ZERO,
        }
    }

    /// Turns this [`WorkingSet`] into a [`StateCheckpoint`], in preparation for
    /// committing the changes to the underlying [`Storage`] via
    /// [`StateCheckpoint::freeze`].
    ///
    /// ## Safety note
    /// This function calls [`WorkingSet::finalize`] under the hood, please be sure that we can skip this
    /// intermediary committing step. This function is only accessible in tests
    pub fn checkpoint(
        self,
    ) -> (
        StateCheckpoint<S>,
        TransactionConsumption<S::Gas>,
        Vec<TypedEvent>,
    ) {
        let (tx_scratchpad, transaction_consumption, events) = self.finalize();
        let checkpoint = tx_scratchpad.commit();

        (checkpoint, transaction_consumption, events)
    }
}

impl<S: Spec> GasMeter<S::Gas> for WorkingSet<S> {
    fn charge_gas(&mut self, gas: &S::Gas) -> Result<(), GasMeteringError<S::Gas>> {
        self.gas_meter.charge_gas(gas)
    }

    fn refund_gas(&mut self, gas: &S::Gas) -> Result<(), GasMeteringError<S::Gas>> {
        self.gas_meter.refund_gas(gas)
    }

    fn gas_price(&self) -> &<S::Gas as Gas>::Price {
        self.gas_meter.gas_price()
    }

    fn gas_used(&self) -> &S::Gas {
        self.gas_meter.gas_used()
    }

    fn remaining_funds(&self) -> u64 {
        self.gas_meter.remaining_funds()
    }
}

impl<S: Spec, N: CompileTimeNamespace> CachedAccessor<N> for WorkingSet<S> {
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        CachedAccessor::<N>::get_cached(&mut self.delta, key)
    }
    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        CachedAccessor::<N>::set_cached(&mut self.delta, key, value)
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        CachedAccessor::<N>::delete_cached(&mut self.delta, key)
    }
}

impl<S: Spec> EventContainer for WorkingSet<S> {
    fn add_event<E: 'static + core::marker::Send>(&mut self, event_key: &str, event: E) {
        self.events.push(TypedEvent::new(event_key, event));
    }
}

#[cfg(feature = "native")]
impl<S: Spec> WorkingSet<S> {
    fn version(&self) -> Option<u64> {
        self.inner().delta().version
    }

    fn inner(&self) -> &TxScratchpad<S> {
        &self.delta.inner
    }

    pub(crate) fn storage(&self) -> &S::Storage {
        &self.inner().delta().inner
    }
}

#[cfg(feature = "native")]
impl<N: ProvableCompileTimeNamespace, S: Spec> ProvenStateAccessor<N> for WorkingSet<S>
where
    WorkingSet<S>: StateReaderAndWriter<N>,
{
    type Proof = <S::Storage as Storage>::Proof;

    fn get_with_proof(&mut self, key: SlotKey) -> StorageProof<Self::Proof> {
        self.storage().get_with_proof::<N>(key, self.version())
    }
}
