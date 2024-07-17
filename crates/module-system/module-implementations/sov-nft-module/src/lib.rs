#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod call;
pub use call::*;
mod address;
mod genesis;
pub use address::*;
pub use genesis::*;
mod collection;
use collection::*;
mod nft;
use nft::*;
#[cfg(feature = "native")]
mod query;
#[cfg(feature = "native")]
pub use query::*;
use sov_modules_api::{
    CallResponse, Context, Error, GenesisState, Module, ModuleId, ModuleInfo, Spec, StateMap,
    TxState,
};
mod event;
mod offchain;
#[cfg(feature = "offchain")]
mod sql;
/// Utility functions.
pub mod utils;
use crate::event::Event;

/// Module for non-fungible tokens (NFT).
/// Each token is represented by a unique ID.
#[derive(Clone, ModuleInfo, sov_modules_api::macros::ModuleRestApi)]
pub struct NonFungibleToken<S: Spec> {
    #[id]
    /// The ID of the NonFungibleToken module.
    id: ModuleId,

    #[state]
    /// Mapping of tokens to their owners
    collections: StateMap<CollectionId, Collection<S>>,

    #[state]
    /// Mapping of tokens to their owners
    nfts: StateMap<NftIdentifier, Nft<S>>,
}

impl<S: Spec> Module for NonFungibleToken<S> {
    type Spec = S;

    type Config = NonFungibleTokenConfig;

    type CallMessage = CallMessage<S>;

    type Event = Event;

    fn genesis(
        &self,
        _config: &Self::Config,
        _state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn call(
        &self,
        msg: Self::CallMessage,
        context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, Error> {
        let call_result = match msg {
            CallMessage::CreateCollection {
                name,
                collection_uri,
            } => self.create_collection(&name, &collection_uri, context, state),
            CallMessage::FreezeCollection { collection_name } => {
                self.freeze_collection(&collection_name, context, state)
            }
            CallMessage::MintNft {
                collection_name,
                token_uri,
                token_id,
                owner,
                frozen,
            } => self.mint_nft(
                token_id,
                &collection_name,
                &token_uri,
                &owner,
                frozen,
                context,
                state,
            ),
            CallMessage::UpdateCollection {
                name,
                collection_uri,
            } => self.update_collection(&name, &collection_uri, context, state),
            CallMessage::TransferNft {
                collection_id,
                token_id,
                to,
            } => self.transfer_nft(token_id, &collection_id, &to, context, state),
            CallMessage::UpdateNft {
                collection_name,
                token_id,
                token_uri,
                frozen,
            } => self.update_nft(
                &collection_name,
                token_id,
                token_uri,
                frozen,
                context,
                state,
            ),
        };
        Ok(call_result?)
    }
}
