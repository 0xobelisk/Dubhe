//! Defines rpc queries exposed by the accounts module, along with the relevant types
use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{ApiStateAccessor, CredentialId, Spec};

use crate::{Account, Accounts};

/// This is the response returned from the accounts_getAccount endpoint.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, Clone)]
#[serde(bound = "Addr: serde::Serialize + serde::de::DeserializeOwned")]
pub enum Response<Addr> {
    /// The account corresponding to the given credential id exists.
    AccountExists {
        /// The address of the account,
        addr: Addr,
    },
    /// The account corresponding to the credential id does not exist.
    AccountEmpty,
}

#[rpc_gen(client, server, namespace = "accounts")]
impl<S: Spec> Accounts<S> {
    #[rpc_method(name = "getAccount")]
    /// Get the account corresponding to the given credential id.
    pub fn get_account(
        &self,
        credential_id: CredentialId,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<Response<S::Address>> {
        let response = match self.accounts.get(&credential_id, state).unwrap_infallible() {
            Some(Account { addr }) => Response::AccountExists { addr },
            None => Response::AccountEmpty,
        };

        Ok(response)
    }
}
