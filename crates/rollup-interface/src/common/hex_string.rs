use std::fmt::Display;

/// A [`hex`]-encoded 32-byte hash. Note, this is not necessarily a transaction
/// hash, rather a generic hash.
pub type HexHash = HexString<[u8; 32]>;

/// A [`serde`]-compatible newtype wrapper around [`Vec<u8>`] or other
/// bytes-like types, which is serialized as a 0x-prefixed hex string.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::AsRef)]
#[cfg_attr(feature = "arbitrary", derive(proptest_derive::Arbitrary))]
pub struct HexString<T = Vec<u8>>(pub T);

impl<T> HexString<T> {
    /// Creates a new [`HexString`] from its inner contents.
    pub const fn new(bytes: T) -> Self {
        Self(bytes)
    }
}

impl<T> From<T> for HexString<T> {
    fn from(bytes: T) -> Self {
        Self(bytes)
    }
}

impl<T> Display for HexString<T>
where
    T: AsRef<[u8]>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}

impl<T> serde::Serialize for HexString<T>
where
    T: AsRef<[u8]>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            serializer.serialize_bytes(self.0.as_ref())
        }
    }
}

impl<'de, T> serde::Deserialize<'de> for HexString<T>
where
    T: TryFrom<Vec<u8>>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes = if deserializer.is_human_readable() {
            let string = String::deserialize(deserializer)?;
            let s = string
                .strip_prefix("0x")
                .ok_or_else(|| serde::de::Error::custom("Missing 0x prefix"))?;

            hex::decode(s)
                .map_err(|e| anyhow::anyhow!("failed to decode hex: {}", e))
                .map_err(serde::de::Error::custom)?
        } else {
            Vec::<u8>::deserialize(deserializer)?
        };

        Ok(HexString(bytes.try_into().map_err(|_| {
            serde::de::Error::custom("Invalid hex string length")
        })?))
    }
}

/// [`serde`] (de)serialization functions for [`HexString`], to be used with
/// `#[serde(with = "...")]`.
pub mod hex_string_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::HexString;

    /// Serializes `data` as hex string using lowercase characters and prefixing with '0x'.
    ///
    /// Lowercase characters are used (e.g. `f9b4ca`). The resulting string's length
    /// is always even, each byte in data is always encoded using two hex digits.
    /// Thus, the resulting string contains exactly twice as many bytes as the input
    /// data.
    pub fn serialize<S, T>(data: T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: AsRef<[u8]>,
    {
        HexString::<T>::new(data).serialize(serializer)
    }

    /// Deserializes a hex string into raw bytes.
    ///
    /// Both upper and lower case characters are valid in the input string and can
    /// even be mixed.
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
    {
        HexString::<T>::deserialize(deserializer).map(|s| s.0)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use proptest::proptest;

    use super::*;

    /// Serializes, then deserializes a value with [`serde_json`], then asserts
    /// equality.
    pub fn test_serialization_roundtrip_equality_json<T>(item: T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + Debug,
    {
        let serialized = serde_json::to_string(&item).unwrap();
        let deserialized: T = serde_json::from_str(&serialized).unwrap();
        assert_eq!(item, deserialized);
    }

    proptest! {
        #[test]
        fn hex_string_serialization_roundtrip(item: HexString) {
            test_serialization_roundtrip_equality_json(item);
        }
    }
}
