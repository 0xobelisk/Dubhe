use std::marker::PhantomData;

#[cfg(feature = "native")]
use anyhow::ensure;
use sov_state::codec::BorshCodec;
use sov_state::namespaces::{Accessory, CompileTimeNamespace, Kernel, User};
use sov_state::{EncodeKeyLike, Prefix, SlotKey, SlotValue, StateCodec, StateItemCodec};
#[cfg(feature = "native")]
use sov_state::{StateItemDecoder, Storage};
use thiserror::Error;
#[cfg(feature = "arbitrary")]
use unwrap_infallible::UnwrapInfallible;

use crate::state::StateReader;
#[cfg(feature = "native")]
use crate::ProvenStateAccessor;
#[cfg(feature = "arbitrary")]
use crate::{InfallibleStateReaderAndWriter, StateCheckpoint};
use crate::{StateReaderAndWriter, StateWriter};

/// A container that maps keys to values.
///
/// # Type parameters
/// [`StateMap`] is generic over:
/// - a key type `K`;
/// - a value type `V`;
/// - a [`sov_state::StateItemCodec`] `Codec`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct NamespacedStateMap<N, K, V, Codec = BorshCodec> {
    _phantom: PhantomData<(N, K, V)>,
    pub(crate) codec: Codec,
    pub(crate) prefix: Prefix,
}

/// Error type for the get method of state maps.
#[derive(Debug, Error)]
pub enum StateMapError<N> {
    /// Value not found.
    #[error("Value not found for prefix: {0} and storage key: {1} in namespace {}", std::any::type_name::<N>())]
    MissingValue(Prefix, SlotKey, PhantomData<N>),
}

type ValueOrError<V, N> = Result<V, StateMapError<N>>;

/// A container that maps keys to values
///
/// # Type parameters
/// [`StateMap`] is generic over:
/// - a key type `K`;
/// - a value type `V`;
/// - a  [`Codec`](`sov_state::StateItemCodec`).
pub type StateMap<K, V, Codec = BorshCodec> = NamespacedStateMap<User, K, V, Codec>;

/// A container that maps keys to values stored as "accessory" state, outside of
/// the JMT.
///
/// # Type parameters
/// [`AccessoryStateMap`] is generic over:
/// - a key type `K`;
/// - a value type `V`;
/// - a  [`Codec`](`sov_state::StateItemCodec`).
pub type AccessoryStateMap<K, V, Codec = BorshCodec> = NamespacedStateMap<Accessory, K, V, Codec>;

/// A container that maps keys to values which are only accessible the kernel.
///
/// # Type parameters
/// [`KernelStateMap`] is generic over:
/// - a key type `K`;
/// - a value type `V`;
/// - a  [`Codec`](`sov_state::StateItemCodec`).
pub type KernelStateMap<K, V, Codec = BorshCodec> = NamespacedStateMap<Kernel, K, V, Codec>;

impl<N: CompileTimeNamespace, K, V> NamespacedStateMap<N, K, V> {
    /// Creates a new [`StateMap`] with the given prefix and the default
    /// [`sov_state::StateItemCodec`] (i.e. [`BorshCodec`]).
    pub fn new(prefix: Prefix) -> Self {
        Self::with_codec(prefix, BorshCodec)
    }
}

impl<N: CompileTimeNamespace, K, V, Codec> NamespacedStateMap<N, K, V, Codec> {
    /// Creates a new [`StateMap`] with the given prefix and [`sov_modules_core::StateItemCodec`].
    pub fn with_codec(prefix: Prefix, codec: Codec) -> Self {
        Self {
            _phantom: PhantomData,
            codec,
            prefix,
        }
    }

    /// Returns the prefix used when this [`StateMap`] was created.
    pub fn prefix(&self) -> &Prefix {
        &self.prefix
    }

    /// Returns the codec used when this [`StateMap`] was created.
    pub fn codec(&self) -> &Codec {
        &self.codec
    }
}

impl<N, K, V, Codec> NamespacedStateMap<N, K, V, Codec>
where
    N: CompileTimeNamespace,
    Codec: StateCodec,
    Codec::KeyCodec: StateItemCodec<K>,
    Codec::ValueCodec: StateItemCodec<V>,
{
    fn slot_key<Q>(&self, key: &Q) -> SlotKey
    where
        Q: ?Sized,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
    {
        SlotKey::new(self.prefix(), key, self.codec().key_codec())
    }

    fn slot_value(&self, value: &V) -> SlotValue {
        SlotValue::new(value, self.codec().value_codec())
    }

    /// Inserts a key-value pair into the map.
    ///
    /// The key may be any borrowed form of the
    /// map’s key type.
    pub fn set<Q, Writer: StateWriter<N>>(
        &self,
        key: &Q,
        value: &V,
        state: &mut Writer,
    ) -> Result<(), Writer::Error>
    where
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        Q: ?Sized,
    {
        state.set(&self.slot_key(key), self.slot_value(value))
    }

    /// Returns the value corresponding to the key, or [`None`] if the map
    /// doesn't contain the key.
    ///
    /// # Examples
    ///
    /// The key may be any item that implements [`EncodeKeyLike`] the map's key type
    /// using your chosen codec.
    ///
    /// ```
    /// use sov_modules_api::{Spec, Context, StateMap, WorkingSet, StateAccessorError};
    ///
    /// fn foo<S: Spec>(map: StateMap<Vec<u8>, u64>, key: &[u8], state: &mut WorkingSet<S>) -> Result<Option<u64>, StateAccessorError<S::Gas>>
    /// {
    ///     // We perform the `get` with a slice, and not the `Vec`. it is so because `Vec` borrows
    ///     // `[T]`.
    ///     map.get(key, state)
    /// }
    /// ```
    ///
    /// If the map's key type does not implement [`EncodeKeyLike`] for your desired
    /// target type, you'll have to convert the key to something else. An
    /// example of this would be "slicing" an array to use in [`Vec`]-keyed
    /// maps:
    ///
    /// ```
    /// use sov_modules_api::{Spec, Context, StateMap, WorkingSet, StateAccessorError};
    ///
    /// fn foo<S: Spec>(map: StateMap<Vec<u8>, u64>, key: [u8; 32], state: &mut WorkingSet<S>) -> Result<Option<u64>, StateAccessorError<S::Gas>>
    /// {
    ///     map.get(&key[..], state)
    /// }
    /// ```
    pub fn get<Q, Reader: StateReader<N>>(
        &self,
        key: &Q,
        state: &mut Reader,
    ) -> Result<Option<V>, Reader::Error>
    where
        Codec: StateCodec,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        Codec::ValueCodec: StateItemCodec<V>,
        Q: ?Sized,
    {
        state.get_decoded(&self.slot_key(key), self.codec())
    }

    /// Returns the value corresponding to the key or [`StateMapError`] if key is absent from
    /// the map.
    pub fn get_or_err<Q, Reader: StateReader<N>>(
        &self,
        key: &Q,
        state: &mut Reader,
    ) -> Result<ValueOrError<V, N>, Reader::Error>
    where
        Codec: StateCodec,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        Codec::ValueCodec: StateItemCodec<V>,
        Q: ?Sized,
    {
        Ok(self.get(key, state)?.ok_or_else(|| {
            StateMapError::MissingValue(
                self.prefix().clone(),
                SlotKey::new(self.prefix(), key, self.codec().key_codec()),
                PhantomData,
            )
        }))
    }

    /// Removes a key from the map, returning the corresponding value (or
    /// [`None`] if the key is absent).
    pub fn remove<Q, ReaderAndWriter: StateReaderAndWriter<N>>(
        &self,
        key: &Q,
        state: &mut ReaderAndWriter,
    ) -> Result<Option<V>, <ReaderAndWriter as StateWriter<N>>::Error>
    where
        Codec: StateCodec,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        Codec::ValueCodec: StateItemCodec<V>,
        Q: ?Sized,
    {
        state.remove_decoded(&self.slot_key(key), self.codec())
    }

    /// Removes a key from the map, returning the corresponding value (or
    /// [`StateMapError`] if the key is absent).
    ///
    /// Use [`NamespacedStateMap::remove`] if you want an [`Option`] instead of a [`Result`].
    pub fn remove_or_err<Q, ReaderAndWriter: StateReaderAndWriter<N>>(
        &self,
        key: &Q,
        state: &mut ReaderAndWriter,
    ) -> Result<ValueOrError<V, N>, <ReaderAndWriter as StateWriter<N>>::Error>
    where
        Codec: StateCodec,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        Codec::ValueCodec: StateItemCodec<V>,
        Q: ?Sized,
    {
        Ok(self.remove(key, state)?.ok_or_else(|| {
            StateMapError::MissingValue(
                self.prefix().clone(),
                SlotKey::new(self.prefix(), key, self.codec().key_codec()),
                PhantomData,
            )
        }))
    }

    /// Deletes a key-value pair from the map.
    ///
    /// This is equivalent to [`NamespacedStateMap::remove`], but doesn't deserialize and
    /// return the value before deletion.
    pub fn delete<Q, Writer: StateWriter<N>>(
        &self,
        key: &Q,
        state: &mut Writer,
    ) -> Result<(), Writer::Error>
    where
        Codec: StateCodec,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        Q: ?Sized,
    {
        state.delete(&self.slot_key(key))
    }
}

#[cfg(feature = "native")]
impl<N: sov_state::namespaces::ProvableCompileTimeNamespace, K, V, Codec>
    NamespacedStateMap<N, K, V, Codec>
where
    Codec: StateCodec,
    Codec::ValueCodec: StateItemCodec<V>,
    Codec::KeyCodec: StateItemCodec<K>,
{
    pub fn get_with_proof<Q, W>(&self, key: &Q, state: &mut W) -> sov_state::StorageProof<W::Proof>
    where
        Q: ?Sized,
        Codec::KeyCodec: EncodeKeyLike<Q, K>,
        W: ProvenStateAccessor<N>,
    {
        state.get_with_proof(self.slot_key(key))
    }

    pub fn verify_proof<S: crate::Spec>(
        &self,
        state_root: <S::Storage as Storage>::Root,
        proof: sov_state::StorageProof<<<S as crate::Spec>::Storage as Storage>::Proof>,
    ) -> Result<(K, Option<V>), anyhow::Error>
where {
        ensure!(
            proof.namespace == N::PROVABLE_NAMESPACE,
            "The provided proof comes from a different namespace. Expected {:?} but found {:?}",
            N::PROVABLE_NAMESPACE,
            proof.namespace
        );

        let (complete_key, value) = <<S as crate::Spec>::Storage>::open_proof(state_root, proof)?;
        let complete_key_bytes = complete_key.key();
        let item_key = complete_key_bytes
            .strip_prefix(self.prefix().as_ref())
            .ok_or_else(|| {
                anyhow::anyhow!("The key in the proof did not match the expected key. Expected key with prefix: {:?}, found key: {:?}", self.prefix(), complete_key.key())
            })?;

        let item_key = self
            .codec()
            .key_codec()
            .try_decode(item_key)
            .map_err(|e| anyhow::anyhow!("Failed to decode key from proof: {:?}", e))?;

        let value = value
            .map(|v| {
                self.codec()
                    .value_codec()
                    .try_decode(v.value())
                    .map_err(|e| anyhow::anyhow!("Failed to decode value from proof: {:?}", e))
            })
            .transpose()?;
        Ok((item_key, value))
    }
}

#[cfg(feature = "arbitrary")]
impl<'a, N, K, V, Codec> NamespacedStateMap<N, K, V, Codec>
where
    K: arbitrary::Arbitrary<'a>,
    V: arbitrary::Arbitrary<'a>,
    Codec: sov_state::StateCodec,
    Codec::KeyCodec: sov_state::StateItemCodec<K>,
    Codec::ValueCodec: sov_state::StateItemCodec<V>,
    N: CompileTimeNamespace,
{
    /// Returns an arbitrary [`StateMap`] instance.
    ///
    /// See the [`arbitrary`] crate for more information.
    pub fn arbitrary_state_map<S>(
        u: &mut arbitrary::Unstructured<'a>,
        working_set: &mut crate::StateCheckpoint<S>,
    ) -> arbitrary::Result<Self>
    where
        S: crate::Spec,
        StateCheckpoint<S>: InfallibleStateReaderAndWriter<N>,
    {
        use arbitrary::Arbitrary;

        let prefix = Prefix::arbitrary(u)?;
        let len = u.arbitrary_len::<(K, V)>()?;
        let codec = Codec::default();
        let map = Self::with_codec(prefix, codec);

        (0..len).try_fold(map, |map, _| {
            let key = K::arbitrary(u)?;
            let value = V::arbitrary(u)?;

            map.set(&key, &value, working_set).unwrap_infallible();

            Ok(map)
        })
    }
}
