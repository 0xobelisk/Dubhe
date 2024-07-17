//! Asymmetric cryptography primitive definitions

use borsh::{BorshDeserialize, BorshSerialize};

#[cfg(not(feature = "native"))]
pub trait SignatureExt:
    sov_rollup_interface::crypto::Signature + BorshDeserialize + BorshSerialize
{
}

#[cfg(not(feature = "native"))]
impl<S: sov_rollup_interface::crypto::Signature + BorshDeserialize + BorshSerialize> SignatureExt
    for S
{
}

/// A digital signature.
#[cfg(feature = "native")]
pub trait SignatureExt:
    sov_rollup_interface::crypto::Signature
    + BorshDeserialize
    + BorshSerialize
    + schemars::JsonSchema
    + std::str::FromStr<Err = anyhow::Error>
{
}

/// A digital signature.
#[cfg(feature = "native")]
impl<
        S: sov_rollup_interface::crypto::Signature
            + BorshDeserialize
            + BorshSerialize
            + std::str::FromStr<Err = anyhow::Error>
            + schemars::JsonSchema,
    > SignatureExt for S
{
}

/// PublicKey used in the Module System.
#[cfg(feature = "native")]
pub trait PublicKeyExt:
    sov_rollup_interface::crypto::PublicKey
    + BorshDeserialize
    + BorshSerialize
    + ::schemars::JsonSchema
    + std::str::FromStr<Err = anyhow::Error>
{
}

#[cfg(feature = "native")]
impl<
        P: sov_rollup_interface::crypto::PublicKey
            + BorshDeserialize
            + BorshSerialize
            + ::schemars::JsonSchema
            + std::str::FromStr<Err = anyhow::Error>,
    > PublicKeyExt for P
{
}

/// PublicKey used in the Module System.
#[cfg(not(feature = "native"))]
pub trait PublicKeyExt:
    sov_rollup_interface::crypto::PublicKey + BorshDeserialize + BorshSerialize
{
}

#[cfg(not(feature = "native"))]
impl<P: sov_rollup_interface::crypto::PublicKey + BorshDeserialize + BorshSerialize> PublicKeyExt
    for P
{
}

// /// A PrivateKey used in the Module System.
// #[cfg(feature = "native")]
// pub trait PrivateKey: sov_rollup_interface::crypto::PrivateKey {
//     /// The public key type associated with this signature scheme.
//     type PublicKey: PublicKey;

//     type Signature: Signature<PublicKey = Self::PublicKey>;
// }

// #[cfg(feature = "native")]
// impl<
//         P: sov_rollup_interface::crypto::PrivateKey
//         S: Signature<PublicKey = P::PublicKey>,
//     > PrivateKey for P
// {
//     type PublicKey = P::PublicKey;
//     type Signature = S;
// }
#[cfg(feature = "native")]
pub use sov_rollup_interface::crypto::PrivateKey as PrivateKeyExt;
