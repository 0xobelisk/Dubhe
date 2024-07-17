use borsh::{BorshDeserialize, BorshSerialize};

use super::{EncodeKeyLike, StateCodec, StateItemDecoder, StateItemEncoder};

/// A [`StateCodec`] that uses [`borsh`] for all keys and values.
#[derive(Debug, Default, PartialEq, Eq, Clone, BorshDeserialize, borsh::BorshSerialize)]
pub struct BorshCodec;

impl<V> StateItemEncoder<V> for BorshCodec
where
    V: BorshSerialize,
{
    fn encode(&self, value: &V) -> Vec<u8> {
        borsh::to_vec(value).expect("Failed to serialize value")
    }
}

impl<V> StateItemDecoder<V> for BorshCodec
where
    V: BorshDeserialize,
{
    type Error = std::io::Error;

    fn try_decode(&self, bytes: &[u8]) -> Result<V, Self::Error> {
        V::try_from_slice(bytes)
    }
}

impl StateCodec for BorshCodec {
    type KeyCodec = Self;
    type ValueCodec = Self;

    fn key_codec(&self) -> &Self::KeyCodec {
        self
    }

    fn value_codec(&self) -> &Self::ValueCodec {
        self
    }
}

// In borsh, a slice is encoded the same way as a vector except in edge case where
// T is zero-sized, in which case Vec<T> is not borsh encodable.
impl<T> EncodeKeyLike<[T], Vec<T>> for BorshCodec
where
    T: BorshSerialize,
{
    fn encode_key_like(&self, borrowed: &[T]) -> Vec<u8> {
        borsh::to_vec(borrowed).expect("Borsh serialization to vec is infallible")
    }
}
