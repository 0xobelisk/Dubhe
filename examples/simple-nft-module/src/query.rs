use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{ApiStateAccessor, Spec, StateReader};
use sov_state::User;

use crate::NonFungibleToken;

/// Response for `getOwner` method.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct OwnerResponse<S: Spec> {
    /// Optional owner address
    pub owner: Option<S::Address>,
}

/// Response for `getNftsCount` method.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, Clone)]
pub struct NftsCountResponse {
    /// The amount of NFTs owned by a certain address.
    pub count: u64,
}

impl<S: Spec> NonFungibleToken<S> {
    /// Get the owner of a token
    pub fn get_owner<Reader: StateReader<User>>(
        &self,
        token_id: u64,
        state: &mut Reader,
    ) -> Result<OwnerResponse<S>, Reader::Error> {
        Ok(OwnerResponse {
            owner: self.owners.get(&token_id, state)?,
        })
    }
}

#[rpc_gen(client, server, namespace = "nft")]
impl<S: Spec> NonFungibleToken<S> {
    #[rpc_method(name = "getOwner")]
    /// Get the owner of a token
    pub fn get_owner_rpc(
        &self,
        token_id: u64,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<OwnerResponse<S>> {
        Ok(self.get_owner(token_id, state).unwrap_infallible())
    }
}
