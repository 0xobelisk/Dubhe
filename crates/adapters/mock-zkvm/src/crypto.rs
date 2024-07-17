//! Cryptography for general purpose use
use std::hash::Hash;
#[cfg(feature = "native")]
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use digest::typenum::U32;
use digest::Digest;
use ed25519_dalek::{
    Signature as DalekSignature, VerifyingKey as DalekPublicKey, PUBLIC_KEY_LENGTH,
};
use sov_rollup_interface::crypto::{PublicKeyHex, SigVerificationError};
#[cfg(feature = "native")]
use sov_rollup_interface::schemars;

/// Defines private key types and operations
#[cfg(feature = "native")]
pub mod private_key {
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use sov_rollup_interface::crypto::PrivateKey;

    use super::{Ed25519PublicKey, Ed25519Signature};

    /// A private key for the ed25519 signature scheme.
    /// This struct also stores the corresponding public key.
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    pub struct Ed25519PrivateKey {
        key_pair: SigningKey,
    }

    impl core::fmt::Debug for Ed25519PrivateKey {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Ed25519PrivateKey")
                .field("public_key", &self.key_pair.verifying_key())
                .field("private_key", &"***REDACTED***")
                .finish()
        }
    }

    impl sov_rollup_interface::crypto::PrivateKey for Ed25519PrivateKey {
        type PublicKey = Ed25519PublicKey;

        type Signature = Ed25519Signature;

        fn generate() -> Self {
            let mut csprng = OsRng;
            Self {
                key_pair: SigningKey::generate(&mut csprng),
            }
        }

        fn pub_key(&self) -> Self::PublicKey {
            Ed25519PublicKey {
                pub_key: self.key_pair.verifying_key(),
            }
        }

        fn sign(&self, msg: &[u8]) -> Self::Signature {
            Ed25519Signature {
                msg_sig: self.key_pair.sign(msg),
            }
        }
    }

    impl Ed25519PrivateKey {
        /// Returns the private key as a hex string.
        pub fn as_hex(&self) -> String {
            hex::encode(self.key_pair.to_bytes())
        }

        /// Returns the address associated with the public key derived from this private key.
        pub fn to_address<A: for<'a> From<&'a <Self as PrivateKey>::PublicKey>>(&self) -> A {
            let key = self.pub_key();
            (&key).into()
        }
    }

    #[cfg(feature = "arbitrary")]
    impl<'a> arbitrary::Arbitrary<'a> for Ed25519PrivateKey {
        fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
            use rand::rngs::StdRng;
            use rand::SeedableRng;

            // it is important to generate the secret deterministically from the arbitrary argument
            // so keys and signatures will be reproducible for a given seed.
            // this unlocks fuzzy replay
            let seed = <[u8; 32]>::arbitrary(u)?;
            let rng = &mut StdRng::from_seed(seed);
            let key_pair = SigningKey::generate(rng);

            Ok(Self { key_pair })
        }
    }

    #[cfg(all(feature = "arbitrary", feature = "native"))]
    impl<'a> arbitrary::Arbitrary<'a> for Ed25519PublicKey {
        fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
            Ed25519PrivateKey::arbitrary(u).map(|p| p.pub_key())
        }
    }

    #[cfg(all(feature = "arbitrary", feature = "native"))]
    impl<'a> arbitrary::Arbitrary<'a> for Ed25519Signature {
        fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
            // the secret/public pair is lost; it is impossible to verify this signature
            // to run a verification, generate the keys+payload individually
            let payload_len = u.arbitrary_len::<u8>()?;
            let payload = u.bytes(payload_len)?;
            Ed25519PrivateKey::arbitrary(u).map(|s| s.sign(payload))
        }
    }
}

/// The public key of an ed25519 keypair.
#[cfg_attr(feature = "native", derive(schemars::JsonSchema))]
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Ed25519PublicKey {
    #[cfg_attr(
        feature = "native",
        schemars(with = "&[u8]", length(equal = "ed25519_dalek::PUBLIC_KEY_LENGTH"))
    )]
    pub(crate) pub_key: DalekPublicKey,
}

impl Ed25519PublicKey {
    /// Returns the address associated with the public key derived from this private key.
    pub fn to_address<'a, A>(&'a self) -> A
    where
        A: From<&'a Self>,
    {
        self.into()
    }
}

impl sov_rollup_interface::crypto::PublicKey for Ed25519PublicKey {
    fn credential_id<Hasher: Digest<OutputSize = U32>>(
        &self,
    ) -> sov_rollup_interface::crypto::CredentialId {
        let hash = {
            let mut hasher = Hasher::new();
            hasher.update(self.pub_key);
            hasher.finalize().into()
        };

        sov_rollup_interface::crypto::CredentialId(hash)
    }
}

impl Hash for Ed25519PublicKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pub_key.as_bytes().hash(state);
    }
}

impl BorshDeserialize for Ed25519PublicKey {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut buffer = [0; PUBLIC_KEY_LENGTH];
        reader.read_exact(&mut buffer)?;

        let pub_key = DalekPublicKey::from_bytes(&buffer).map_err(map_error)?;

        Ok(Self { pub_key })
    }
}

impl BorshSerialize for Ed25519PublicKey {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(self.pub_key.as_bytes())
    }
}

/// An ed25519 signature. Wraps the optimized Risc0 fork of the ed25519-dalek crate.
#[cfg_attr(feature = "native", derive(schemars::JsonSchema))]
#[derive(PartialEq, Eq, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ed25519Signature {
    /// The inner signature.
    #[cfg_attr(
        feature = "native",
        schemars(with = "&[u8]", length(equal = "ed25519_dalek::Signature::BYTE_SIZE"))
    )]
    pub msg_sig: DalekSignature,
}

impl BorshDeserialize for Ed25519Signature {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut buffer = [0; DalekSignature::BYTE_SIZE];
        reader.read_exact(&mut buffer)?;

        Ok(Self {
            msg_sig: DalekSignature::from_bytes(&buffer),
        })
    }
}

impl BorshSerialize for Ed25519Signature {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.msg_sig.to_bytes())
    }
}

impl TryFrom<&[u8]> for Ed25519Signature {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self {
            msg_sig: DalekSignature::from_slice(value).map_err(anyhow::Error::msg)?,
        })
    }
}

impl sov_rollup_interface::crypto::Signature for Ed25519Signature {
    type PublicKey = Ed25519PublicKey;

    fn verify(&self, pub_key: &Self::PublicKey, msg: &[u8]) -> Result<(), SigVerificationError> {
        pub_key
            .pub_key
            .verify_strict(msg, &self.msg_sig)
            .map_err(|e| SigVerificationError::BadSignature(e.to_string()))
    }
}

#[cfg(feature = "native")]
fn map_error(e: ed25519_dalek::SignatureError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}
#[cfg(not(feature = "native"))]
fn map_error(_e: ed25519_dalek::SignatureError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, "Signature error")
}

#[cfg(feature = "native")]
impl FromStr for Ed25519PublicKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pk_hex = PublicKeyHex::try_from(s)?;
        Ed25519PublicKey::try_from(&pk_hex)
    }
}

#[cfg(feature = "native")]
impl FromStr for Ed25519Signature {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;

        let bytes: ed25519_dalek::ed25519::SignatureBytes = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid signature"))?;

        Ok(Ed25519Signature {
            msg_sig: DalekSignature::from_bytes(&bytes),
        })
    }
}

#[cfg(test)]
#[cfg(feature = "native")]
mod tests {
    use sov_rollup_interface::crypto::PrivateKey;

    use super::*;

    #[test]
    fn test_privatekey_serde_bincode() {
        use self::private_key::Ed25519PrivateKey;

        let key_pair = Ed25519PrivateKey::generate();
        let serialized = bincode::serialize(&key_pair).expect("Serialization to vec is infallible");
        let output = bincode::deserialize::<Ed25519PrivateKey>(&serialized)
            .expect("SigningKey is serialized correctly");

        assert_eq!(key_pair.as_hex(), output.as_hex());
    }

    #[test]
    fn test_privatekey_serde_json() {
        use self::private_key::Ed25519PrivateKey;

        let key_pair = Ed25519PrivateKey::generate();
        let serialized = serde_json::to_vec(&key_pair).expect("Serialization to vec is infallible");
        let output = serde_json::from_slice::<Ed25519PrivateKey>(&serialized)
            .expect("Keypair is serialized correctly");

        assert_eq!(key_pair.as_hex(), output.as_hex());
    }
}

impl serde::Serialize for Ed25519PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            serde::Serialize::serialize(&PublicKeyHex::from(self), serializer)
        } else {
            serde::Serialize::serialize(&self.pub_key, serializer)
        }
    }
}

impl<'de> serde::Deserialize<'de> for Ed25519PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let pub_key_hex: PublicKeyHex = serde::Deserialize::deserialize(deserializer)?;
            Ok(Ed25519PublicKey::try_from(&pub_key_hex).map_err(serde::de::Error::custom)?)
        } else {
            let pub_key: DalekPublicKey = serde::Deserialize::deserialize(deserializer)?;
            Ok(Ed25519PublicKey { pub_key })
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_pub_key_json() {
        let pub_key_hex: PublicKeyHex =
            "022e229198d957bf0c0a504e7d7bcec99a1d62cccc7861ed2452676ad0323ad8"
                .try_into()
                .unwrap();

        let pub_key = Ed25519PublicKey::try_from(&pub_key_hex).unwrap();
        let pub_key_str: String = serde_json::to_string(&pub_key).unwrap();

        assert_eq!(
            pub_key_str,
            r#""022e229198d957bf0c0a504e7d7bcec99a1d62cccc7861ed2452676ad0323ad8""#
        );

        let deserialized: Ed25519PublicKey = serde_json::from_str(&pub_key_str).unwrap();
        assert_eq!(deserialized, pub_key);
    }
}

impl From<&Ed25519PublicKey> for PublicKeyHex {
    fn from(pub_key: &Ed25519PublicKey) -> Self {
        let hex = hex::encode(pub_key.pub_key.as_bytes());
        Self { hex }
    }
}

impl TryFrom<&PublicKeyHex> for Ed25519PublicKey {
    type Error = anyhow::Error;

    fn try_from(pub_key: &PublicKeyHex) -> Result<Self, Self::Error> {
        let bytes = hex::decode(&pub_key.hex)?;

        let bytes: [u8; PUBLIC_KEY_LENGTH] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid public key size"))?;

        let pub_key = DalekPublicKey::from_bytes(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid public key"))?;

        Ok(Ed25519PublicKey { pub_key })
    }
}

#[cfg(test)]
#[cfg(feature = "native")]
mod hex_tests {
    use sov_rollup_interface::crypto::PrivateKey;

    use super::*;
    use crate::crypto::private_key::Ed25519PrivateKey;

    #[test]
    fn test_pub_key_hex() {
        let pub_key = Ed25519PrivateKey::generate().pub_key();
        let pub_key_hex = PublicKeyHex::from(&pub_key);
        let converted_pub_key = Ed25519PublicKey::try_from(&pub_key_hex).unwrap();
        assert_eq!(pub_key, converted_pub_key);
    }

    #[test]
    fn test_pub_key_hex_str() {
        let key = "022e229198d957bf0c0a504e7d7bcec99a1d62cccc7861ed2452676ad0323ad8";
        let pub_key_hex_lower: PublicKeyHex = key.try_into().unwrap();
        let pub_key_hex_upper: PublicKeyHex = key.to_uppercase().try_into().unwrap();

        let pub_key_lower = Ed25519PublicKey::try_from(&pub_key_hex_lower).unwrap();
        let pub_key_upper = Ed25519PublicKey::try_from(&pub_key_hex_upper).unwrap();

        assert_eq!(pub_key_lower, pub_key_upper);
    }
}
