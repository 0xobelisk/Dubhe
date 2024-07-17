use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{ApiStateAccessor, Spec};

use crate::{CredentialId, Nonces};

/// This is the response returned from the nonces_getNonce endpoint.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, Clone)]
pub struct NonceResponse {
    /// Nonce
    pub nonce: u64,
}

#[rpc_gen(client, server, namespace = "nonces")]
impl<S: Spec> Nonces<S> {
    #[rpc_method(name = "getNonce")]
    /// Get the nonce corresponding to the given credential id.
    pub fn get_nonce(
        &self,
        credential_id: CredentialId,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<NonceResponse> {
        let nonce = self
            .nonce(&credential_id, state)
            .unwrap_infallible()
            .unwrap_or_default();

        Ok(NonceResponse { nonce })
    }
}
