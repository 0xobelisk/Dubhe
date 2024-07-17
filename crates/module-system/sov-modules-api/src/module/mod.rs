//! Runtime module definitions.

use core::fmt::Debug;

use borsh::{BorshDeserialize, BorshSerialize};
use sov_state::EventContainer;

use crate::common::ModuleError;
use crate::{GenesisState, ModuleId, TxState};

mod dispatch;
mod event;
mod prefix;
mod spec;

pub use dispatch::*;
pub use event::*;
pub use prefix::*;
pub use spec::*;

/// Response type for the `Module::call` method.
#[derive(Default, Debug)]
pub struct CallResponse {}

/// The core trait implemented by all modules. This trait defines how a module is initialized at genesis,
/// and how it handles user transactions (if applicable).
pub trait Module {
    /// Execution context.
    type Spec: Spec;

    /// Configuration for the genesis method.
    type Config;

    /// Module defined argument to the call method.
    type CallMessage: Debug + BorshSerialize + BorshDeserialize;

    /// Module defined event resulting from a call method.
    type Event: Debug + BorshSerialize + BorshDeserialize + 'static + core::marker::Send;

    /// Genesis is called when a rollup is deployed and can be used to set initial state values in the module.
    fn genesis(
        &self,
        _config: &Self::Config,
        _state: &mut impl GenesisState<Self::Spec>,
    ) -> Result<(), ModuleError> {
        Ok(())
    }

    /// Call allows interaction with the module and invokes state changes.
    /// It takes a module defined type and a context as parameters.
    fn call(
        &self,
        _message: Self::CallMessage,
        _context: &Context<Self::Spec>,
        _state: &mut impl TxState<Self::Spec>,
    ) -> Result<CallResponse, ModuleError>;

    /// Attempts to charge the provided amount of gas from the working set.
    ///
    /// The scalar gas value will be computed from the price defined on the working set.
    fn charge_gas(
        &self,
        state: &mut impl TxState<Self::Spec>,
        gas: &<Self::Spec as Spec>::Gas,
    ) -> anyhow::Result<()> {
        Ok(state.charge_gas(gas)?)
    }
}

/// A [`Module`] that has a well-defined and known [JSON
/// Schema](https://json-schema.org/) for its [`Module::CallMessage`].
///
/// This trait is intended to support code generation tools, CLIs, and
/// documentation. This trait is blanket-implemented for all modules with a
/// [`Module::CallMessage`] associated type that implements
/// [`schemars::JsonSchema`].
pub trait ModuleCallJsonSchema: Module {
    /// Returns the JSON schema for [`Module::CallMessage`].
    fn json_schema() -> String;
}

#[cfg(feature = "native")]
impl<T> ModuleCallJsonSchema for T
where
    T: Module,
    T::CallMessage: schemars::JsonSchema,
{
    fn json_schema() -> String {
        let schema = ::schemars::schema_for!(T::CallMessage);

        serde_json::to_string_pretty(&schema)
            .expect("Failed to serialize JSON schema; this is a bug in the module")
    }
}

/// Every module has to implement this trait.
pub trait ModuleInfo {
    /// Execution context.
    type Spec: Spec;

    /// Returns id of the module.
    fn id(&self) -> &ModuleId;

    /// Returns the prefix of the module.
    fn prefix(&self) -> ModulePrefix;

    /// Returns addresses of all the other modules this module is dependent on
    fn dependencies(&self) -> Vec<&ModuleId>;
}

/// Event Emitter trait for a blanket implementation
pub trait EventEmitter: ModuleInfo {
    /// Execution context.
    type Spec: Spec;
    /// Module defined event resulting from a call method.
    type Event: Debug + BorshSerialize + BorshDeserialize + 'static + core::marker::Send;

    /// Emits an event with an auto-generated event key composed by the module
    /// of origin's name and the `enum` variant's name of the event.
    fn emit_event(&self, state: &mut impl EventContainer, event: Self::Event) {
        #[allow(unused_variables)]
        let _ = || (&state, &event);

        if cfg!(feature = "native") {
            let event_debug = format!("{:?}", event);
            // `.unwrap_or_default()` would only happen if `Debug` returns an
            // empty or all-whitespace string, which seems unlikely.
            let event_variant_name = event_debug.split_whitespace().next().unwrap_or_default();
            let event_key = format!("{}/{}", self.prefix().module_name(), event_variant_name);

            state.add_event(&event_key, event);
        }
    }

    /// Emits an event with a custom event key.
    fn emit_event_with_custom_key(
        &self,
        state: &mut impl EventContainer,
        event_key: &str,
        event: Self::Event,
    );
}

impl<T> EventEmitter for T
where
    T: ModuleInfo + Module,
{
    type Spec = <T as ModuleInfo>::Spec;
    type Event = <T as Module>::Event;

    fn emit_event_with_custom_key(
        &self,
        state: &mut impl EventContainer,
        event_key: &str,
        event: Self::Event,
    ) {
        #[allow(unused_variables)]
        let _ = || (&state, &event);

        if cfg!(feature = "native") {
            state.add_event(event_key, event);
        }
    }
}

/// A trait that specifies how a runtime should encode the data for each module
pub trait EncodeCall<M: Module> {
    /// The encoding function
    fn encode_call(data: M::CallMessage) -> Vec<u8>;
}

/// Methods from this trait should be called only once during the rollup deployment.
pub trait Genesis {
    /// Execution context of the module.
    type Spec: Spec;

    /// Initial configuration for the module.
    type Config;

    /// Initializes the state of the rollup.
    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<Self::Spec>,
    ) -> Result<(), ModuleError>;
}

impl<T> Genesis for T
where
    T: Module,
{
    type Spec = <Self as Module>::Spec;

    type Config = <Self as Module>::Config;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<Self::Spec>,
    ) -> Result<(), ModuleError> {
        <Self as Module>::genesis(self, config, state)
    }
}
