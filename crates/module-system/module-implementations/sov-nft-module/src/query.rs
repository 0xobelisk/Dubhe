use jsonrpsee::core::RpcResult;
use jsonrpsee::types::ErrorCode;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{ApiStateAccessor, Spec, StateReader};
use sov_state::User;

use crate::utils::get_collection_id;
use crate::{CollectionId, CreatorAddress, NftIdentifier, NonFungibleToken, OwnerAddress, TokenId};

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(bound(
    serialize = "CreatorAddress<S>: serde::Serialize",
    deserialize = "CreatorAddress<S>: serde::Deserialize<'de>"
))]
/// Response for `getCollection` method
pub struct CollectionDetails<S: Spec> {
    /// Collection name
    pub name: String,
    /// Creator Address
    pub creator: CreatorAddress<S>,
    /// frozen or not
    pub frozen: bool,
    /// supply
    pub supply: u64,
    /// Collection metadata uri
    pub collection_uri: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(bound(
    serialize = "OwnerAddress<S>: serde::Serialize",
    deserialize = "OwnerAddress<S>: serde::Deserialize<'de>"
))]
/// Response for `getNft` method
pub struct NftDetails<S: Spec> {
    /// Unique token id scoped to the collection
    pub token_id: TokenId,
    /// URI pointing to offchain metadata
    pub token_uri: String,
    /// frozen status (token_uri mutable or not)
    pub frozen: bool,
    /// Owner of the NFT
    pub owner: OwnerAddress<S>,
    /// Collection id that the NFT belongs to
    pub collection_id: CollectionId,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]

/// Response for `getCollectionId` method
pub struct CollectionIdDetails {
    /// Address of the collection
    pub collection_id: CollectionId,
}

impl<S: Spec> NonFungibleToken<S> {
    /// Get the collection details
    pub fn collection<Reader: StateReader<User>>(
        &self,
        collection_id: CollectionId,
        accessor: &mut Reader,
    ) -> Result<Option<CollectionDetails<S>>, Reader::Error> {
        let c = self.collections.get(&collection_id, accessor)?;

        Ok(c.map(|c| CollectionDetails {
            name: c.get_name().to_string(),
            creator: c.get_creator().clone(),
            frozen: c.is_frozen(),
            supply: c.get_supply(),
            collection_uri: c.get_collection_uri().to_string(),
        }))
    }

    /// Get the collection id
    pub fn collection_id(
        &self,
        creator: CreatorAddress<S>,
        collection_name: &str,
    ) -> CollectionIdDetails {
        let ca = get_collection_id::<S>(collection_name, creator.as_ref());
        CollectionIdDetails { collection_id: ca }
    }

    /// Get the NFT details
    pub fn nft<Reader: StateReader<User>>(
        &self,
        collection_id: CollectionId,
        token_id: TokenId,
        accessor: &mut Reader,
    ) -> Result<Option<NftDetails<S>>, Reader::Error> {
        let nft_id = NftIdentifier(token_id, collection_id);
        let n = self.nfts.get(&nft_id, accessor)?;

        Ok(n.map(|n| NftDetails {
            token_id: n.get_token_id(),
            token_uri: n.get_token_uri().to_string(),
            frozen: n.is_frozen(),
            owner: n.get_owner().clone(),
            collection_id: *n.get_collection_id(),
        }))
    }
}

#[rpc_gen(client, server, namespace = "nft")]
impl<S: Spec> NonFungibleToken<S> {
    #[rpc_method(name = "getCollection")]
    /// Get the collection details
    pub fn get_collection(
        &self,
        collection_id: CollectionId,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<CollectionDetails<S>> {
        self.collection(collection_id, state)
            .unwrap_infallible()
            .ok_or(ErrorCode::InvalidParams.into())
    }
    #[rpc_method(name = "getCollectionId")]
    /// Get the collection id
    pub fn get_collection_id(
        &self,
        creator: CreatorAddress<S>,
        collection_name: &str,
    ) -> RpcResult<CollectionIdDetails> {
        Ok(self.collection_id(creator, collection_name))
    }
    #[rpc_method(name = "getNft")]
    /// Get the NFT details
    pub fn get_nft(
        &self,
        collection_id: CollectionId,
        token_id: TokenId,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<NftDetails<S>> {
        self.nft(collection_id, token_id, state)
            .unwrap_infallible()
            .ok_or(ErrorCode::InvalidParams.into())
    }
}
