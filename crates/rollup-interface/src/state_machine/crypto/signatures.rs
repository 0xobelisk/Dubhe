//! Asymmetric cryptography primitive definitions

use alloc::string::String;
use core::fmt::Debug;
use core::hash;

use digest::typenum::U32;
use digest::Digest;
use serde::{Deserialize, Serialize};

use super::CredentialId;
use crate::alloc::borrow::ToOwned;

/// Representation of a signature verification error.
#[derive(Debug, thiserror::Error)]
pub enum SigVerificationError {
    /// The signature is invalid for the provided public key.
    #[error("Bad signature error: {0}")]
    BadSignature(String),
}

/// A digital signature.
pub trait Signature:
    for<'a> TryFrom<&'a [u8], Error = anyhow::Error>
    + Eq
    + Clone
    + Debug
    + Send
    + Sync
    + Serialize
    + for<'a> Deserialize<'a>
{
    /// The public key associated with the signature.
    type PublicKey;

    /// Verifies the signature.
    fn verify(&self, pub_key: &Self::PublicKey, msg: &[u8]) -> Result<(), SigVerificationError>;
}

/// A public key for verifying digital signatures.
pub trait PublicKey:
    Eq + hash::Hash + Clone + Debug + Send + Sync + Serialize + for<'a> Deserialize<'a>
{
    /// Returns hashed public key.
    fn credential_id<Hasher: Digest<OutputSize = U32>>(&self) -> CredentialId;
}

/// A private key for generating digital signatures.
#[cfg(feature = "native")]
pub trait PrivateKey:
    Debug + Send + Sync + Serialize + Clone + serde::de::DeserializeOwned
{
    /// The public key type associated with this signature scheme.
    type PublicKey: PublicKey;

    /// The signature associated with the key pair.
    type Signature: Signature<PublicKey = Self::PublicKey>;

    /// Generates a new key pair.
    fn generate() -> Self;

    /// Returns the public key derived from this private key.
    fn pub_key(&self) -> Self::PublicKey;

    /// Signs the provided message using the private key.
    fn sign(&self, msg: &[u8]) -> Self::Signature;
}

/// A hex-encoded public key.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    Debug,
    PartialEq,
    Clone,
    Eq,
)]
#[serde(try_from = "String", into = "String")]
pub struct PublicKeyHex {
    /// The public key in hexadecimal format.
    pub hex: String,
}

impl TryFrom<&str> for PublicKeyHex {
    type Error = anyhow::Error;

    fn try_from(hex: &str) -> Result<Self, Self::Error> {
        Self::try_from(hex.to_owned())
    }
}

impl TryFrom<String> for PublicKeyHex {
    type Error = anyhow::Error;

    fn try_from(hex: String) -> Result<Self, Self::Error> {
        if hex.len() & 1 != 0 {
            anyhow::bail!("Bad hex conversion: odd input length")
        }

        if let Some((index, c)) = hex
            .chars()
            .enumerate()
            .find(|(_, c)| !c.is_ascii_hexdigit())
        {
            anyhow::bail!(
                "Bad hex conversion: wrong character `{}` at index {}",
                c,
                index
            )
        }

        Ok(Self { hex })
    }
}

impl From<PublicKeyHex> for String {
    fn from(pub_key: PublicKeyHex) -> Self {
        pub_key.hex
    }
}

impl core::fmt::Display for PublicKeyHex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.hex)
    }
}

#[cfg(test)]
mod tests {
    use crate::alloc::string::ToString;
    use crate::crypto::PublicKeyHex;

    #[test]
    fn test_bad_pub_key_hex_str() {
        let key = "022e229198d957Zf0c0a504e7d7bcec99a1d62cccc7861ed2452676ad0323ad8";
        let err = PublicKeyHex::try_from(key).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Bad hex conversion: wrong character `Z` at index 14"
        );

        let key = "022";
        let err = PublicKeyHex::try_from(key).unwrap_err();

        assert_eq!(err.to_string(), "Bad hex conversion: odd input length");
    }
}
