use super::{StateCodec, StateItemDecoder, StateItemEncoder};

/// A [`StateCodec`] that uses [`bcs`] for all keys and values.
#[derive(Debug, Default, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct BcsCodec;

impl<V> StateItemEncoder<V> for BcsCodec
where
    V: serde::Serialize,
{
    fn encode(&self, value: &V) -> Vec<u8> {
        bcs::to_bytes(value).expect("Failed to serialize value")
    }
}

impl<V> StateItemDecoder<V> for BcsCodec
where
    V: for<'a> serde::Deserialize<'a>,
{
    type Error = bcs::Error;

    fn try_decode(&self, bytes: &[u8]) -> Result<V, Self::Error> {
        bcs::from_bytes(bytes)
    }
}

impl StateCodec for BcsCodec {
    type KeyCodec = Self;
    type ValueCodec = Self;

    fn key_codec(&self) -> &Self::KeyCodec {
        self
    }

    fn value_codec(&self) -> &Self::ValueCodec {
        self
    }
}
