use std::fmt::Debug;

use anyhow::Result;
#[cfg(feature = "native")]
use sov_modules_api::macros::CliWalletArg;
use sov_modules_api::{CallResponse, Context, EventEmitter, TxState};
use thiserror::Error;

use super::ValueSetter;
use crate::event::Event;

/// This enumeration represents the available call messages for interacting with the `sov-value-setter` module.
#[cfg_attr(feature = "native", derive(CliWalletArg), derive(schemars::JsonSchema))]
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub enum CallMessage {
    /// Single value to set.
    SetValue(
        /// Singe new value.
        u32,
    ),
    /// Many values to set.
    SetManyValues(
        /// Many new values.
        Vec<u8>,
    ),
}

/// Example of a custom error.
#[derive(Debug, Error)]
enum SetValueError {
    #[error("Only admin can change the value")]
    WrongSender,
}

impl<S: sov_modules_api::Spec> ValueSetter<S> {
    /// Sets `value` field to the `new_value`, only admin is authorized to call this method.
    pub(crate) fn set_value(
        &self,
        new_value: u32,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        // If admin is not then early return:
        let admin = self.admin.get_or_err(state)??;

        if &admin != context.sender() {
            // Here we use a custom error type.
            Err(SetValueError::WrongSender)?;
        }

        // This is how we set a new value:
        self.value.set(&new_value, state)?;

        self.emit_event(state, Event::NewValue(new_value));

        Ok(CallResponse::default())
    }

    pub(crate) fn set_values(
        &self,
        new_value: Vec<u8>,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let admin = self.admin.get_or_err(state)??;

        if &admin != context.sender() {
            // Here we use a custom error type.
            Err(SetValueError::WrongSender)?;
        }

        // This is how we set a new value:
        self.many_values.set_all(new_value, state)?;
        Ok(CallResponse::default())
    }
}
