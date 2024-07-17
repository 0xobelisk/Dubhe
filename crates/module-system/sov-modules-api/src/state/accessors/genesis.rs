use sov_state::{CompileTimeNamespace, EventContainer, IsValueCached, SlotKey, SlotValue};

use super::checkpoints::StateCheckpoint;
use super::internals::Delta;
use super::seal::CachedAccessor;
use crate::state::events::TypedEvent;
use crate::{Gas, GasMeter, GasMeteringError, Genesis, Spec, UnlimitedGasMeter};

pub struct GenesisStateAccessor<S: Spec> {
    delta: Delta<S::Storage>,
    pub(super) events: Vec<TypedEvent>,
    gas_meter: UnlimitedGasMeter<S::Gas>,
}

impl<S: Spec> StateCheckpoint<S> {
    /// Produces an unmetered [`GenesisStateAccessor`] from a [`StateCheckpoint`] for genesis.
    pub fn to_genesis_state_accessor<G: Genesis>(
        self,
        // This argument prevents this method from being called outside of genesis.
        _config: &G::Config,
    ) -> GenesisStateAccessor<S> {
        GenesisStateAccessor {
            delta: self.delta,
            gas_meter: UnlimitedGasMeter::new(),
            events: Default::default(),
        }
    }
}

impl<S: Spec, N: CompileTimeNamespace> CachedAccessor<N> for GenesisStateAccessor<S> {
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

impl<S: Spec> GasMeter<S::Gas> for GenesisStateAccessor<S> {
    fn charge_gas(&mut self, amount: &S::Gas) -> Result<(), GasMeteringError<S::Gas>> {
        self.gas_meter.charge_gas(amount)
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

impl<S: Spec> GenesisStateAccessor<S> {
    pub fn checkpoint(self) -> StateCheckpoint<S> {
        StateCheckpoint { delta: self.delta }
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
}

impl<S: Spec> EventContainer for GenesisStateAccessor<S> {
    fn add_event<E: 'static + core::marker::Send>(&mut self, event_key: &str, event: E) {
        self.events.push(TypedEvent::new(event_key, event));
    }
}
