use std::fmt;

use sov_modules_api::impl_hash32_type;
use sov_modules_api::macros::address_type;

/// Address representing a simple user capable of owning an NFT.
#[address_type]
pub struct UserAddress;

impl_hash32_type!(CollectionId, CellectionIdBech32, "collection");

/// Address representing the owner of an NFT.
#[address_type]
pub struct OwnerAddress;

/// Address representing a creator of an NFT collection.
#[address_type]
pub struct CreatorAddress;
