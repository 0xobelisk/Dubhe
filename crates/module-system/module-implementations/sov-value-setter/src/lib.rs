#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
mod call;
mod genesis;

#[cfg(test)]
mod tests;

mod event;
pub use call::*;
pub use genesis::*;
use sov_modules_api::{Context, Error, GenesisState, ModuleId, ModuleInfo, TxState};

use crate::event::Event;

/// A new module:
/// - Must derive `ModuleInfo`
/// - Must contain `[address]` field
/// - Can contain any number of ` #[state]` or `[module]` fields
#[derive(Clone, ModuleInfo, sov_modules_api::macros::ModuleRestApi)]
pub struct ValueSetter<S: sov_modules_api::Spec> {
    /// The ID of the module.
    #[id]
    pub id: ModuleId,

    /// Some value kept in the state.
    #[state]
    pub value: sov_modules_api::StateValue<u32>,

    /// Some more values kept in state.
    #[state]
    many_values: sov_modules_api::StateVec<u8>,

    /// Holds the address of the admin user who is allowed to update the value.
    #[state]
    pub admin: sov_modules_api::StateValue<S::Address>,
}

impl<S: sov_modules_api::Spec> sov_modules_api::Module for ValueSetter<S> {
    type Spec = S;

    type Config = ValueSetterConfig<S>;

    type CallMessage = call::CallMessage;

    type Event = Event;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        // The initialization logic
        Ok(self.init_module(config, state)?)
    }

    fn call(
        &self,
        msg: Self::CallMessage,
        context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,
    ) -> Result<sov_modules_api::CallResponse, Error> {
        match msg {
            call::CallMessage::SetValue(new_value) => {
                Ok(self.set_value(new_value, context, state)?)
            }
            CallMessage::SetManyValues(many) => Ok(self.set_values(many, context, state)?),
        }
    }
}
