use sov_modules_api::digest::Digest;
use sov_modules_api::CryptoSpec;

use crate::{CallMessage, CollectionId, UserAddress};

/// Derives token ID from `collection_name`, `sender`
pub fn get_collection_id<S: sov_modules_api::Spec>(
    collection_name: &str,
    sender: &[u8],
) -> CollectionId {
    let mut hasher = <S::CryptoSpec as CryptoSpec>::Hasher::new();
    hasher.update(sender);
    hasher.update(collection_name.as_bytes());

    let hash: [u8; 32] = hasher.finalize().into();
    hash.into()
}

fn get_collection_metadata_url(base_url: &str, collection_id: &str) -> String {
    format!("{}/collection/{}", base_url, collection_id)
}

fn get_nft_metadata_url(base_url: &str, collection_id: &str, nft_id: u64) -> String {
    format!("{}/nft/{}/{}", base_url, collection_id, nft_id)
}

/// Constructs a CallMessage to create a new NFT collection.
///
/// # Arguments
///
/// * `sender_address`: The address of the sender who will sign the transaction.
/// * `collection_name`: Name of the collection to be created.
/// * `base_uri`: Base URI to be used with the collection name
///
/// # Returns
///
/// Returns a CallMessage Variant which can then be serialized into a transaction
pub fn get_create_collection_message<S: sov_modules_api::Spec>(
    sender_address: &S::Address,
    collection_name: &str,
    base_uri: &str,
) -> CallMessage<S> {
    let collection_id = get_collection_id::<S>(collection_name, sender_address.as_ref());

    let collection_uri = get_collection_metadata_url(base_uri, &collection_id.to_string());
    CallMessage::<S>::CreateCollection {
        name: collection_name.to_string(),
        collection_uri,
    }
}

/// Constructs a CallMessage to mint a new NFT.
///
/// # Arguments
///
/// * `signer`: The private key used for signing the transaction.
/// * `nonce`: The nonce to be used for the transaction.
/// * `collection_name`: The name of the collection to which the NFT belongs.
/// * `token_id`: The unique identifier for the new NFT.
/// * `owner`: The address of the user to whom the NFT will be minted.
///
/// # Returns
///
/// Returns a signed transaction for minting a new NFT to a specified user.
pub fn get_mint_nft_message<S: sov_modules_api::Spec>(
    sender_address: &S::Address,
    collection_name: &str,
    token_id: u64,
    base_uri: &str,
    owner: &S::Address,
) -> CallMessage<S> {
    let collection_id = get_collection_id::<S>(collection_name, sender_address.as_ref());
    let token_uri = get_nft_metadata_url(base_uri, &collection_id.to_string(), token_id);
    CallMessage::<S>::MintNft {
        collection_name: collection_name.to_string(),
        token_uri,
        token_id,
        owner: UserAddress::new(owner),
        frozen: false,
    }
}

/// Constructs a CallMessage to transfer an NFT to another user.
///
/// # Arguments
///
/// * `signer`: The private key used for signing the transaction.
/// * `nonce`: The nonce to be used for the transaction.
/// * `collection_id`: The address of the collection to which the NFT belongs.
/// * `token_id`: The unique identifier for the NFT being transferred.
/// * `to`: The address of the user to whom the NFT will be transferred.
///
/// # Returns
///
/// Returns a signed transaction for transferring an NFT to a specified user.
pub fn get_transfer_nft_message<S: sov_modules_api::Spec>(
    collection_id: &CollectionId,
    token_id: u64,
    to: &S::Address,
) -> CallMessage<S> {
    CallMessage::<S>::TransferNft {
        collection_id: *collection_id,
        token_id,
        to: UserAddress::new(to),
    }
}
