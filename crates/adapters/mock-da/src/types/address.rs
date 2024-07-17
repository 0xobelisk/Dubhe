use std::str::FromStr;

use sov_rollup_interface::{BasicAddress, RollupAddress};

/// Sequencer DA address used in tests.
pub const MOCK_SEQUENCER_DA_ADDRESS: [u8; 32] = [0u8; 32];

/// A mock address type used for testing. Internally, this type is standard 32 byte array.
#[derive(
    Debug, PartialEq, Clone, Eq, Copy, Hash, Default, borsh::BorshDeserialize, borsh::BorshSerialize,
)]
pub struct MockAddress {
    /// Underlying mock address.
    addr: [u8; 32],
}

impl MockAddress {
    /// Creates a new mock address containing the given bytes.
    pub const fn new(addr: [u8; 32]) -> Self {
        Self { addr }
    }
}

#[cfg(feature = "native")]
impl schemars::JsonSchema for MockAddress {
    fn schema_name() -> String {
        "MockAddress".to_string()
    }

    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "string",
            "pattern": "^[a-fA-F0-9]{64}$",
            // This description assumes that `serializer` uses a human-readable format.
            "description": "Mock address; 32 bytes in hex-encoded format",
        }))
        .unwrap()
    }
}

impl serde::Serialize for MockAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            hex::serialize(self.addr, serializer)
        } else {
            self.addr.serialize(serializer)
        }
    }
}

impl<'de> serde::Deserialize<'de> for MockAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            hex::deserialize(deserializer).map(MockAddress::new)
        } else {
            serde::Deserialize::deserialize(deserializer).map(MockAddress::new)
        }
    }
}

impl FromStr for MockAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let addr = hex::decode(s).map_err(anyhow::Error::msg)?;
        Self::try_from(addr.as_slice())
    }
}

impl<'a> TryFrom<&'a [u8]> for MockAddress {
    type Error = anyhow::Error;

    fn try_from(addr: &'a [u8]) -> Result<Self, Self::Error> {
        let addr = addr
            .try_into()
            .map_err(|_| anyhow::anyhow!("address must be 32 bytes long"))?;
        Ok(Self { addr })
    }
}

impl AsRef<[u8]> for MockAddress {
    fn as_ref(&self) -> &[u8] {
        &self.addr
    }
}

impl From<[u8; 32]> for MockAddress {
    fn from(addr: [u8; 32]) -> Self {
        MockAddress { addr }
    }
}

impl std::fmt::Display for MockAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", hex::encode(self.addr))
    }
}

impl BasicAddress for MockAddress {}
impl RollupAddress for MockAddress {}

#[cfg(test)]
mod tests {
    use std::string::ToString;

    use super::*;

    #[test]
    fn human_readable_serde_roundtrip() {
        let addr = MockAddress::new([3u8; 32]);
        let json = serde_json::to_string(&addr).unwrap();
        let recovered_addr = serde_json::from_str::<MockAddress>(&json).unwrap();
        assert_eq!(addr, recovered_addr);
    }

    #[test]
    fn binary_serde_roundtrip() {
        let addr = MockAddress::new([3u8; 32]);
        let bytes = bincode::serialize(&addr).unwrap();
        let recovered_addr = bincode::deserialize::<MockAddress>(&bytes).unwrap();
        assert_eq!(addr, recovered_addr);
    }

    #[test]
    fn try_from_bytes() {
        let addr = MockAddress::new([100u8; 32]);
        let addr_bytes = addr.as_ref();
        let recovered_addr = MockAddress::try_from(addr_bytes).unwrap();
        assert_eq!(addr, recovered_addr);
    }

    #[test]
    fn parse_from_string() {
        let addr = MockAddress::new([1u8; 32]);
        let s = addr.to_string();
        let recovered_addr = s.parse::<MockAddress>().unwrap();
        assert_eq!(addr, recovered_addr);
    }
}
