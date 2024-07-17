use anyhow::{anyhow, bail, Context as _};
use sov_modules_api::{Context, Spec, StateAccessor, StateMap};

use crate::collection::Collection;
use crate::{CollectionId, OwnerAddress, UserAddress};

/// tokenId for the NFT that's unique within the scope of the collection
pub type TokenId = u64;

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Clone, Debug, PartialEq, Eq, Hash)]
/// A simple wrapper struct to mark an NFT identifier as a combination of
/// a token id (u64) and a collection id
pub struct NftIdentifier(pub TokenId, pub CollectionId);

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
/// Defines an nft
pub struct Nft<S: Spec> {
    /// A token id that uniquely identifies an NFT within the scope of a (collection name, creator)
    token_id: TokenId,
    /// A collection id that uniquely identifies a collection - derived from (collection name, creator)
    collection_id: CollectionId,
    /// Owner address of a specific token_id within a collection
    owner: OwnerAddress<S>,
    /// A frozen NFT cannot have its data altered and is immutable
    /// Cannot be unfrozen. token_uri cannot be modified
    frozen: bool,
    /// A URI pointing to the offchain metadata
    token_uri: String,
}

/// NewType representing an owned NFT
/// An owned NFT is owned by the context sender and is transferable
pub struct OwnedNft<S: Spec>(Nft<S>);

/// NewType representing a Mutable NFT
/// A mutable NFT is modifiable by the creator, but only certain fields (frozen, token_uri)
pub struct MutableNft<S: Spec>(Nft<S>);

impl<S: Spec> OwnedNft<S> {
    pub fn new(nft: Nft<S>, context: &Context<S>) -> anyhow::Result<Self> {
        let sender = OwnerAddress::new(context.sender());
        if nft.owner == sender {
            Ok(OwnedNft(nft))
        } else {
            Err(anyhow!("NFT not owned by sender")).with_context(|| {
                format!(
                    "user: {} does not own nft: {} from collection id: {} , owner is: {}",
                    sender, nft.token_id, nft.collection_id, nft.owner
                )
            })
        }
    }

    pub fn inner(&self) -> &Nft<S> {
        &self.0
    }
    pub fn set_owner(&mut self, to: &UserAddress<S>) {
        self.0.owner = OwnerAddress::new(to.get_address());
    }
}

impl<S: Spec> MutableNft<S> {
    pub fn inner(&self) -> &Nft<S> {
        &self.0
    }

    pub fn freeze(&mut self) {
        self.0.frozen = true;
    }
    pub fn update_token_uri(&mut self, token_uri: &str) {
        self.0.token_uri = token_uri.to_string();
    }
}

impl<S: Spec> Nft<S> {
    pub fn new(
        token_id: TokenId,
        token_uri: &str,
        mint_to_address: &UserAddress<S>,
        frozen: bool,
        collection_id: &CollectionId,
        nfts: &StateMap<NftIdentifier, Nft<S>>,
        state: &mut impl StateAccessor,
    ) -> anyhow::Result<Self> {
        if nfts
            .get(&NftIdentifier(token_id, *collection_id), state)?
            .is_some()
        {
            bail!(
                "NFT with id {} already exists for collection id {}",
                token_id,
                collection_id
            )
        }
        Ok(Nft {
            token_id,
            collection_id: *collection_id,
            owner: OwnerAddress::new(mint_to_address.get_address()),
            frozen,
            token_uri: token_uri.to_string(),
        })
    }

    pub fn get_owned_nft(
        token_id: TokenId,
        collection_id: &CollectionId,
        nfts: &StateMap<NftIdentifier, Nft<S>>,
        context: &Context<S>,
        state: &mut impl StateAccessor,
    ) -> anyhow::Result<OwnedNft<S>> {
        let nft_identifier = NftIdentifier(token_id, *collection_id);
        let nft = nfts
            .get(&nft_identifier, state)?
            .ok_or_else(|| anyhow!("NFT not found"))
            .with_context(|| {
                format!(
                    "Nft with token_id: {} in collection_id: {} does not exist",
                    token_id, collection_id
                )
            })?;
        OwnedNft::new(nft, context)
    }

    pub fn get_mutable_nft(
        token_id: TokenId,
        collection_name: &str,
        nfts: &StateMap<NftIdentifier, Nft<S>>,
        collections: &StateMap<CollectionId, Collection<S>>,
        context: &Context<S>,
        state: &mut impl StateAccessor,
    ) -> anyhow::Result<(CollectionId, MutableNft<S>)> {
        let (collection_id, _) =
            Collection::get_owned_collection(collection_name, collections, context, state)?;
        let token_identifier = NftIdentifier(token_id, collection_id);
        let n = nfts.get(&token_identifier, state)?;
        if let Some(nft) = n {
            if !nft.frozen {
                Ok((collection_id, MutableNft(nft.clone())))
            } else {
                bail!(
                    "NFT with token id {} in collection id {} is frozen",
                    token_id,
                    token_identifier.1
                )
            }
        } else {
            bail!(
                "Nft with token_id: {} in collection_id: {} does not exist",
                token_id,
                token_identifier.1
            )
        }
    }

    // Allow dead code used to suppress warnings when native feature flag is not used
    // 1. The getters are primarily used by rpc which is not native
    // 2. The getters can still be used by other modules in the future

    #[allow(dead_code)]
    pub fn get_token_id(&self) -> TokenId {
        self.token_id
    }
    #[allow(dead_code)]
    pub fn get_collection_id(&self) -> &CollectionId {
        &self.collection_id
    }
    #[allow(dead_code)]
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    #[allow(dead_code)]
    pub fn get_token_uri(&self) -> &str {
        &self.token_uri
    }
    #[allow(dead_code)]
    pub fn get_owner(&self) -> &OwnerAddress<S> {
        &self.owner
    }
}
