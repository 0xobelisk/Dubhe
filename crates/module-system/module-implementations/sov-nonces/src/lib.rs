#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
mod call;
mod capabilities;
#[cfg(feature = "native")]
mod query;
use call::NotInstantiable;
#[cfg(feature = "native")]
pub use query::*;
use sov_modules_api::{
    Context, CredentialId, Error, GenesisState, ModuleId, ModuleInfo, Spec, StateReader, TxState,
};
use sov_state::User;

/// A module responsible for managing nonces on the rollup.
#[derive(Clone, ModuleInfo, sov_modules_api::macros::ModuleRestApi)]
pub struct Nonces<S: Spec> {
    /// The ID of the sov-nonces module.
    #[id]
    pub id: ModuleId,

    /// Mapping from a credential id to a nonce.
    #[state]
    pub(crate) nonces: sov_modules_api::StateMap<CredentialId, u64>,

    /// PhantomData
    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

impl<S: Spec> Nonces<S> {
    /// Retrieves the nonce for a given credential id.
    pub fn nonce<Reader: StateReader<User>>(
        &self,
        credential_id: &CredentialId,
        state: &mut Reader,
    ) -> Result<Option<u64>, Reader::Error> {
        self.nonces.get(credential_id, state)
    }
}

impl<S: Spec> sov_modules_api::Module for Nonces<S> {
    type Spec = S;

    type Config = ();

    type CallMessage = NotInstantiable;

    type Event = ();

    fn genesis(
        &self,
        _config: &Self::Config,
        _state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn call(
        &self,
        _msg: Self::CallMessage,
        _context: &Context<S>,
        _state: &mut impl TxState<S>,
    ) -> Result<sov_modules_api::CallResponse, Error> {
        unreachable!()
    }
}
