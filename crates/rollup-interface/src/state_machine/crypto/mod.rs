//! Defines useful cryptographic primitives that are needed by all sovereign-sdk rollups.
mod simple_hasher;

pub use simple_hasher::NoOpHasher;
mod signatures;
pub use signatures::*;

/// Type that represents an identifier for an authorizer of the transaction.
/// The credential is a [u8; 32] array.
/// For example, this can be a padded EVM address or a hash of a rollup public key.
#[cfg_attr(feature = "native", derive(schemars::JsonSchema))]
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
)]
pub struct CredentialId(pub [u8; 32]);

impl core::str::FromStr for CredentialId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_start_matches("0x");
        let decoded = <[u8; 32] as hex::FromHex>::from_hex(s)?;
        Ok(Self(decoded))
    }
}

impl core::fmt::Display for CredentialId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use proptest::prelude::*;

    use super::*;

    fn check_str_and_back(credential_id: CredentialId) {
        let s1 = credential_id.to_string();
        let s2 = s1.replace("0x", "");
        let credential_id_2 = CredentialId::from_str(&s1).unwrap();
        let credential_id_3 = CredentialId::from_str(&s2).unwrap();
        assert_eq!(credential_id, credential_id_2);
        assert_eq!(credential_id, credential_id_3);
    }

    #[test]
    fn test_hash_str_and_back_simple() {
        for i in 0..1 {
            let credential_id = CredentialId([i as u8; 32]);
            check_str_and_back(credential_id);
        }
    }

    proptest! {
        #[test]
        fn test_arbitrary_hash_str_and_back(input in prop::array::uniform32(any::<u8>())) {
            let credential_id = CredentialId(input);
            check_str_and_back(credential_id);
        }
    }
}
