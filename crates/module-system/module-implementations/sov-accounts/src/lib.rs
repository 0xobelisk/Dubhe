#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
mod call;
#[cfg(all(feature = "arbitrary", feature = "native"))]
mod fuzz;
mod genesis;
mod hooks;
pub use genesis::*;
#[cfg(feature = "native")]
mod query;
#[cfg(feature = "native")]
pub use query::*;
mod event;
#[cfg(test)]
mod tests;
pub use call::CallMessage;
use sov_modules_api::{
    Context, CredentialId, Error, GenesisState, ModuleId, ModuleInfo, Spec, TxState,
};

use crate::event::Event;

/// An account on the rollup.
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Copy,
    Clone,
)]
pub struct Account<S: Spec> {
    /// The address of the account.
    pub addr: S::Address,
}

/// A module responsible for managing accounts on the rollup.
#[derive(Clone, ModuleInfo, sov_modules_api::macros::ModuleRestApi)]
#[cfg_attr(feature = "arbitrary", derive(Debug))]
pub struct Accounts<S: Spec> {
    /// The ID of the sov-accounts module.
    #[id]
    pub id: ModuleId,

    /// Mapping from an account address to a corresponding credential ids.
    #[state]
    pub(crate) credential_ids: sov_modules_api::StateMap<S::Address, Vec<CredentialId>>,

    /// Mapping from a credential to a corresponding account.
    #[state]
    pub(crate) accounts: sov_modules_api::StateMap<CredentialId, Account<S>>,
}

impl<S: Spec> sov_modules_api::Module for Accounts<S> {
    type Spec = S;

    type Config = AccountConfig<S>;

    type CallMessage = call::CallMessage;

    type Event = Event;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        Ok(self.init_module(config, state)?)
    }

    fn call(
        &self,
        msg: Self::CallMessage,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<sov_modules_api::CallResponse, Error> {
        match msg {
            call::CallMessage::InsertCredentialId(new_credential_id) => {
                Ok(self.insert_credential_id(new_credential_id, context, state)?)
            }
        }
    }
}
