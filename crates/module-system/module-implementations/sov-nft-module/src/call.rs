use anyhow::Result;
use sov_modules_api::{CallResponse, Context, Spec, TxState};

use crate::address::UserAddress;
use crate::offchain::{update_collection, update_nft};
use crate::{Collection, CollectionId, Nft, NftIdentifier, NonFungibleToken, TokenId};

/// A transaction handled by the NFT module. Mints, Transfers, or Burns an NFT by id
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(bound = "S::Address: ::schemars::JsonSchema", rename = "CallMessage")
)]
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub enum CallMessage<S: Spec> {
    /// Create a new collection
    CreateCollection {
        /// Name of the collection
        name: String,
        /// meta data url for collection
        collection_uri: String,
    },
    /// update collection metadata
    UpdateCollection {
        /// Name of the collection
        name: String,
        /// meta data url for collection
        collection_uri: String,
    },
    /// Freeze a collection that is unfrozen.
    /// This prevents new NFTs from being minted.
    FreezeCollection {
        /// collection name
        collection_name: String,
    },
    /// mint a new nft
    MintNft {
        /// Name of the collection
        collection_name: String,
        /// Meta data url for collection
        token_uri: String,
        /// nft id. a unique identifier for each NFT
        token_id: TokenId,
        /// Address that the NFT should be minted to
        owner: UserAddress<S>,
        /// A frozen nft cannot have its metadata_url modified or be unfrozen
        /// Setting this to true makes the nft immutable
        frozen: bool,
    },
    /// Update nft metadata url or frozen status
    UpdateNft {
        /// Name of the collection
        collection_name: String,
        /// nft id
        token_id: TokenId,
        /// Meta data url for collection
        token_uri: Option<String>,
        /// Frozen status
        frozen: Option<bool>,
    },
    /// Transfer an NFT from an owned address to another address
    TransferNft {
        /// Collection id
        collection_id: CollectionId,
        /// NFT id of the owned token to be transferred
        token_id: u64,
        /// Target address of the user to transfer the NFT to
        to: UserAddress<S>,
    },
}

impl<S: Spec> NonFungibleToken<S> {
    pub(crate) fn create_collection(
        &self,
        collection_name: &str,
        collection_uri: &str,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let (collection_id, collection) = Collection::new(
            collection_name,
            collection_uri,
            &self.collections,
            context,
            state,
        )?;
        self.collections.set(&collection_id, &collection, state)?;
        update_collection(&collection);
        Ok(CallResponse::default())
    }

    pub(crate) fn update_collection(
        &self,
        collection_name: &str,
        collection_uri: &str,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let (collection_id, collection_state) =
            Collection::get_owned_collection(collection_name, &self.collections, context, state)?;
        let mut collection = collection_state.get_mutable_or_bail()?;
        collection.set_collection_uri(collection_uri);
        self.collections
            .set(&collection_id, collection.inner(), state)?;
        update_collection(collection.inner());
        Ok(CallResponse::default())
    }

    pub(crate) fn freeze_collection(
        &self,
        collection_name: &str,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let (collection_id, collection_state) =
            Collection::get_owned_collection(collection_name, &self.collections, context, state)?;
        let mut collection = collection_state.get_mutable_or_bail()?;
        collection.freeze();
        self.collections
            .set(&collection_id, collection.inner(), state)?;
        update_collection(collection.inner());
        Ok(CallResponse::default())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mint_nft(
        &self,
        token_id: u64,
        collection_name: &str,
        token_uri: &str,
        mint_to_address: &UserAddress<S>,
        frozen: bool,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let (collection_id, collection_state) =
            Collection::get_owned_collection(collection_name, &self.collections, context, state)?;
        let mut collection = collection_state.get_mutable_or_bail()?;
        let new_nft = Nft::new(
            token_id,
            token_uri,
            mint_to_address,
            frozen,
            &collection_id,
            &self.nfts,
            state,
        )?;
        self.nfts
            .set(&NftIdentifier(token_id, collection_id), &new_nft, state)?;
        collection.increment_supply();
        self.collections
            .set(&collection_id, collection.inner(), state)?;

        update_collection(collection.inner());
        update_nft(&new_nft, None);

        Ok(CallResponse::default())
    }

    pub(crate) fn transfer_nft(
        &self,
        nft_id: u64,
        collection_id: &CollectionId,
        to: &UserAddress<S>,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let mut owned_nft = Nft::get_owned_nft(nft_id, collection_id, &self.nfts, context, state)?;
        let original_owner = owned_nft.inner().get_owner().clone();
        owned_nft.set_owner(to);
        self.nfts.set(
            &NftIdentifier(nft_id, *collection_id),
            owned_nft.inner(),
            state,
        )?;
        update_nft(owned_nft.inner(), Some(original_owner.clone()));
        Ok(CallResponse::default())
    }

    pub(crate) fn update_nft(
        &self,
        collection_name: &str,
        token_id: u64,
        token_uri: Option<String>,
        frozen: Option<bool>,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        let (collection_id, mut mutable_nft) = Nft::get_mutable_nft(
            token_id,
            collection_name,
            &self.nfts,
            &self.collections,
            context,
            state,
        )?;
        if let Some(true) = frozen {
            mutable_nft.freeze();
        }
        if let Some(uri) = token_uri {
            mutable_nft.update_token_uri(&uri);
        }
        self.nfts.set(
            &NftIdentifier(token_id, collection_id),
            mutable_nft.inner(),
            state,
        )?;
        update_nft(mutable_nft.inner(), None);
        Ok(CallResponse::default())
    }
}
