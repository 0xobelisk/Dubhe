use std::convert::Infallible;
use std::fmt::Debug;

use sov_modules_macros::config_value;
#[cfg(feature = "native")]
use sov_state::StorageProof;
use sov_state::{
    namespaces, Accessory, CompileTimeNamespace, EventContainer, IsValueCached, Kernel,
    ProvableCompileTimeNamespace, ProvableNamespace, SlotKey, SlotValue, StateCodec,
    StateItemCodec, StateItemDecoder, User,
};
use thiserror::Error;

use super::accessors::seal::CachedAccessor;
#[cfg(any(feature = "test-utils", feature = "evm"))]
use crate::UnmeteredStateWrapper;
use crate::{Gas, GasMeter, GasMeteringError, Spec};

/// A type that can both read and write the normal "user-space" state of the rollup.
///
/// ```
/// fn delete_state_string<Accessor: sov_modules_api::StateAccessor>(value: sov_modules_api::StateValue<String>, state: &mut Accessor)
///  -> Result<(), <Accessor as sov_modules_api::StateWriter<sov_state::User>>::Error> {
///     if let Some(original) = value.get(state)? {
///         println!("original: {}", original);
///     }
///     value.delete(state)?;
///     Ok(())
/// }
///
///
/// ```
pub trait StateAccessor: StateReaderAndWriter<User> {
    #[cfg(any(feature = "test-utils", feature = "evm"))]
    fn to_unmetered(&mut self) -> UnmeteredStateWrapper<Self>
    where
        Self: Sized,
    {
        UnmeteredStateWrapper { inner: self }
    }
}

pub trait InfallibleStateAccessor:
    StateReader<User, Error = Infallible> + StateWriter<User, Error = Infallible>
{
}

impl<T> StateAccessor for T where T: StateReaderAndWriter<User> {}

impl<T> InfallibleStateAccessor for T where
    T: StateReader<User, Error = Infallible> + StateWriter<User, Error = Infallible>
{
}

/// The state accessor used during transaction execution. It provides unrestricted
/// access to [`User`]-space state, as well as limited visibility into the `Kernel` state.
pub trait TxState<S: Spec>:
    StateReader<User, Error = StateAccessorError<S::Gas>>
    + StateWriter<User, Error = StateAccessorError<S::Gas>>
    // + StateReader<Kernel> TODO: <https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/596>
    + StateWriter<Accessory>
    + EventContainer
    + GasMeter<S::Gas>
{
}

impl<S: Spec, T> TxState<S> for T where
    T: StateReader<User, Error = StateAccessorError<S::Gas>>
        + StateWriter<User, Error = StateAccessorError<S::Gas>>
        + StateWriter<Accessory>
        + EventContainer
        + GasMeter<S::Gas>
{
}

/// The state accessor used during genesis. It provides unrestricted
/// access to [`User`] and `Kernel` state, as well as limited visibility into [`Accessory`] state.  
pub trait GenesisState<S: Spec>:
    StateReader<User, Error = Infallible>
    + StateWriter<User, Error = Infallible>
    // + StateReader<Kernel> TODO: <https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/596>
    + AccessoryStateWriter
    + EventContainer
    + GasMeter<S::Gas>
{}

impl<S: Spec, T> GenesisState<S> for T where
    T: StateReader<User, Error = Infallible>
        + StateWriter<User, Error = Infallible>
        // + StateReaderAndWriter<sov_state::Kernel>
        + AccessoryStateWriter
        + EventContainer
        + GasMeter<S::Gas>
{
}

/// The set of errors that can be raised during state accesses. For now all these errors are
/// caused by gas metering issues, hence this error type is a wrapper around the [`GasMeteringError`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StateAccessorError<GU: Gas> {
    /// An error occurred when trying to get a value from the state.
    #[error(
        "An error occured while trying to get the value (key {key:?}) from the state: {inner}, namespace: {namespace:?}"
    )]
    Get {
        key: SlotKey,
        inner: GasMeteringError<GU>,
        namespace: ProvableNamespace,
    },
    /// An error occurred when trying to set a value in the state.
    #[error(
        "An error occured while trying to set the value (key {key:?}) in the state: {inner}, namespace: {namespace:?}"
    )]
    Set {
        key: SlotKey,
        inner: GasMeteringError<GU>,
        namespace: ProvableNamespace,
    },
    /// An error occurred when trying to decode a value retrieved from the state.
    #[error(
        "An error occured while trying to decode the value (key {key:?}) in the state: {inner}, namespace: {namespace:?}"
    )]
    Decode {
        key: SlotKey,
        inner: GasMeteringError<GU>,
        namespace: ProvableNamespace,
    },
    /// An error occurred when trying to delete a value from the state.
    #[error(
        "An error occured while trying to delete the value (key {key:?}) in the state: {inner}, namespace: {namespace:?}"
    )]
    Delete {
        key: SlotKey,
        inner: GasMeteringError<GU>,
        namespace: ProvableNamespace,
    },
}

/// Returns the gas to charge for a decoding operation.
///
/// ## NOTE
/// The constants' value should be updated based on benchmarks to ensure that the gas cost of the read operation is
/// optimal
fn decode_gas_cost<GU: Gas>(input: &SlotValue) -> GU {
    const GAS_TO_CHARGE_FOR_DECODING: [u64; 2] = config_value!("GAS_TO_CHARGE_FOR_DECODING");
    let mut gas_cost = GU::from_slice(&GAS_TO_CHARGE_FOR_DECODING);
    let input_len = input.value().len();
    gas_cost.scalar_product(input_len as u64);

    gas_cost
}

/// Returns the gas to charge for a read operation. This value is the maximum amount of gas that can be charged
/// for a read operation. Some of this amount may be refunded to the gas meter if the read operation access a warm value.
///
/// ## NOTE
/// The constants' value should be updated based on benchmarks to ensure that the gas cost of the read operation is
/// optimal
fn gas_to_charge_for_read<GU: Gas>() -> GU {
    const GAS_TO_CHARGE_FOR_READ: [u64; 2] = config_value!("GAS_TO_CHARGE_FOR_ACCESS");
    GU::from_slice(&GAS_TO_CHARGE_FOR_READ)
}

/// Gas to refund for a read operation. Now this is the value to refund for a read operation that accesses a warm value.
/// In the future we may want to support more access patterns and improve the granularity of the refund.
fn gas_to_refund_for_hot_read<GU: Gas>() -> GU {
    const GAS_TO_REFUND_FOR_HOT_READ: [u64; 2] = config_value!("GAS_TO_REFUND_FOR_HOT_ACCESS");
    GU::from_slice(&GAS_TO_REFUND_FOR_HOT_READ)
}

/// Returns the gas to charge for a write operation. This value is the maximum amount of gas that can be charged
/// for a write operation. Some of this amount may be refunded to the gas meter if the write operation access a warm value.
///
/// ## NOTE
/// The constants' value should be updated based on benchmarks to ensure that the gas cost of the write operation is
/// optimal
///  
/// For now, charges the same amount of gas for delete as for write.
/// In the future, we may want to charge a different amount and improve the granularity of the refund.
fn gas_to_charge_for_write<GU: Gas>() -> GU {
    const GAS_TO_CHARGE_FOR_WRITE: [u64; 2] = config_value!("GAS_TO_CHARGE_FOR_WRITE");
    GU::from_slice(&GAS_TO_CHARGE_FOR_WRITE)
}

/// Gas to refund for a write operation. Now this is the value to refund for a write operation that accesses a warm value.
/// In the future we may want to support more access patterns and improve the granularity of the refund.
fn gas_to_refund_for_hot_write<GU: Gas>() -> GU {
    const GAS_TO_REFUND_FOR_HOT_WRITE: [u64; 2] = config_value!("GAS_TO_REFUND_FOR_HOT_WRITE");
    GU::from_slice(&GAS_TO_REFUND_FOR_HOT_WRITE)
}

/// Returns the gas to charge for a delete operation. This value is the maximum amount of gas that can be charged
/// for a delete operation. Some of this amount may be refunded to the gas meter if the delete operation access a warm value.
///
/// ## NOTE
/// The constants' value should be updated based on benchmarks to ensure that the gas cost of the delete operation is
/// optimal
///  
/// For now, charges the same amount of gas for delete as for delete.
/// In the future, we may want to charge a different amount and improve the granularity of the refund.
fn gas_to_charge_for_delete<GU: Gas>() -> GU {
    const GAS_TO_CHARGE_FOR_WRITE: [u64; 2] = config_value!("GAS_TO_CHARGE_FOR_WRITE");
    GU::from_slice(&GAS_TO_CHARGE_FOR_WRITE)
}

/// Gas to refund for a delete operation. Now this is the value to refund for a delete operation that accesses a warm value.
/// In the future we may want to support more access patterns and improve the granularity of the refund.
fn gas_to_refund_for_hot_delete<GU: Gas>() -> GU {
    const GAS_TO_REFUND_FOR_HOT_WRITE: [u64; 2] = config_value!("GAS_TO_REFUND_FOR_HOT_WRITE");
    GU::from_slice(&GAS_TO_REFUND_FOR_HOT_WRITE)
}

pub trait InfallibleStateReaderAndWriter<N: CompileTimeNamespace>:
    StateReader<N, Error = Infallible> + StateWriter<N, Error = Infallible>
{
}

impl<
        T: StateReader<N, Error = Infallible> + StateWriter<N, Error = Infallible>,
        N: CompileTimeNamespace,
    > InfallibleStateReaderAndWriter<N> for T
{
}

pub trait AccessoryStateReaderAndWriter: InfallibleStateReaderAndWriter<Accessory> {}
impl<T: InfallibleStateReaderAndWriter<Accessory>> AccessoryStateReaderAndWriter for T {}

/// A wrapper trait for storage reader and writer that can be used to charge gas
/// for the read/write operations.
pub trait StateReaderAndWriter<N: CompileTimeNamespace>:
    StateReader<N> + StateWriter<N, Error = <Self as StateReader<N>>::Error>
{
    /// Deletes a value from the storage. Basically a wrapper around [`StateWriter::delete`].
    ///
    /// ## Note
    /// For now, charges the same amount of gas for delete as for write. In the future, we may want to charge a different amount and improve the granularity of the refund.
    fn remove(&mut self, key: &SlotKey) -> Result<(), <Self as StateWriter<N>>::Error> {
        <Self as StateReader<N>>::get(self, key)?;

        <Self as StateWriter<N>>::delete(self, key)?;

        Ok(())
    }

    /// Removes a value from storage and decode the result
    fn remove_decoded<V, Codec>(
        &mut self,
        key: &SlotKey,
        codec: &Codec,
    ) -> Result<Option<V>, <Self as StateWriter<N>>::Error>
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>,
    {
        let value = self.get_decoded(key, codec)?;
        <Self as StateWriter<N>>::delete(self, key)?;
        Ok(value)
    }
}

impl<T, N> StateReaderAndWriter<N> for T
where
    T: StateReader<N> + StateWriter<N, Error = <Self as StateReader<N>>::Error>,
    N: CompileTimeNamespace,
{
}

/// A storage reader which can access a particular namespace.
pub trait StateReader<N: CompileTimeNamespace>: CachedAccessor<N> {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get a value from the storage. Basically a wrapper around [`StateReader::get`].
    ///
    /// ## Error
    /// This method can fail if the gas meter doesn't have enough funds to pay for the read operation.
    fn get(&mut self, key: &SlotKey) -> Result<Option<SlotValue>, Self::Error>;

    /// Get a decoded value from the storage.
    ///
    /// ## Error
    /// This method can fail if the gas meter doesn't have enough funds to pay for the read operation.
    ///
    /// ## Note
    /// For now this method doesn't charge an extra amount of gas for the decoding operation. This may change in the future.
    fn get_decoded<V, Codec>(
        &mut self,
        storage_key: &SlotKey,
        codec: &Codec,
    ) -> Result<Option<V>, Self::Error>
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>;
}

/// A storage reader which can access the accessory state.
/// Does not charge gas for read operations.
pub trait AccessoryStateReader: CachedAccessor<Accessory> {}

/// A trait wrapper that replicates the functionality of [`StateReader`] but with a gas metering interface.
/// This allows a storage reader to charge gas for read operations.
pub trait ProvableStateReader<N: ProvableCompileTimeNamespace>:
    CachedAccessor<N> + GasMeter<Self::GU>
{
    type GU: Gas;
}

macro_rules! blanket_impl_metered_state_reader {
    ($namespace:ty) => {
        impl<T: ProvableStateReader<$namespace>> StateReader<$namespace> for T {
            type Error = StateAccessorError<T::GU>;

            fn get(&mut self, key: &SlotKey) -> Result<Option<SlotValue>, Self::Error> {
                self.charge_gas(&gas_to_charge_for_read())
                    .map_err(|e| StateAccessorError::Get{
                        key: key.clone(),
                        inner: e,
                        namespace: <$namespace>::PROVABLE_NAMESPACE,
                    })?;

                let (val, is_value_cached) = CachedAccessor::<$namespace>::get_cached(self, key);

                if is_value_cached == IsValueCached::Yes {
                    self.refund_gas(&gas_to_refund_for_hot_read()).expect("Failed to refund gas for read operation. This is a bug. The gas refund constant should always be lower than the gas to charge.");
                }

                Ok(val)
            }

            fn get_decoded<V, Codec>(
                &mut self,
                storage_key: &SlotKey,
                codec: &Codec,
            ) -> Result<Option<V>, Self::Error>
            where
                Codec: StateCodec,
                Codec::ValueCodec: StateItemCodec<V>,
            {
                let storage_value = <Self as StateReader<$namespace>>::get(self, storage_key)?;

                if let Some(storage_value) = &storage_value {
                    self.charge_gas(&decode_gas_cost(storage_value)).map_err(|e| StateAccessorError::Decode{
                        key: storage_key.clone(),
                        inner: e,
                        namespace: <$namespace>::PROVABLE_NAMESPACE,
                    })?
                }

                Ok(storage_value
                    .map(|storage_value| codec.value_codec().decode_unwrap(storage_value.value())))
            }
        }
    };
}

blanket_impl_metered_state_reader!(Kernel);
blanket_impl_metered_state_reader!(User);

impl<T: AccessoryStateReader> StateReader<Accessory> for T {
    type Error = Infallible;

    /// Get a value from the storage.
    fn get(&mut self, key: &SlotKey) -> Result<Option<SlotValue>, Self::Error> {
        Ok(<Self as CachedAccessor<Accessory>>::get_cached(self, key).0)
    }

    /// Get a decoded value from the storage.
    fn get_decoded<V, Codec>(
        &mut self,
        storage_key: &SlotKey,
        codec: &Codec,
    ) -> Result<Option<V>, Self::Error>
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>,
    {
        let storage_value = <Self as StateReader<Accessory>>::get(self, storage_key)?;

        Ok(storage_value
            .map(|storage_value| codec.value_codec().decode_unwrap(storage_value.value())))
    }
}

/// Provides write-only access to a particular namespace
pub trait StateWriter<N: CompileTimeNamespace>: CachedAccessor<N> {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Sets a value in the storage. Basically a wrapper around [`StateWriter::set`].
    ///
    /// ## Error
    /// This method can fail if the gas meter doesn't have enough funds to pay for the write operation.
    fn set(&mut self, key: &SlotKey, value: SlotValue) -> Result<(), Self::Error>;

    /// Deletes a value from the storage. Basically a wrapper around [`StateWriter::delete`].
    ///
    /// ## Error
    /// This method can fail if the gas meter doesn't have enough funds to pay for the delete operation.
    fn delete(&mut self, key: &SlotKey) -> Result<(), Self::Error>;
}

pub trait ProvableStateWriter<N: ProvableCompileTimeNamespace>:
    CachedAccessor<N> + GasMeter<Self::GU>
{
    type GU: Gas;
}

macro_rules! blanket_impl_metered_state_writer {
    ($namespace:ty) => {
        impl<T: ProvableStateWriter<$namespace>> StateWriter<$namespace> for T {
            type Error = StateAccessorError<T::GU>;

            fn set(&mut self, key: &SlotKey, value: SlotValue) -> Result<(), Self::Error> {
                self.charge_gas(&gas_to_charge_for_write())
                    .map_err(|e| StateAccessorError::Set{
                        key: key.clone(),
                        inner: e,
                        namespace: <$namespace>::PROVABLE_NAMESPACE,
                    })?;
                let is_value_cached = CachedAccessor::<$namespace>::set_cached(self, key, value);

                if is_value_cached == IsValueCached::Yes {
                    self.refund_gas(&gas_to_refund_for_hot_write()).expect("Failed to refund gas for write operation. This is a bug. The gas refund constant should always be lower than the gas to charge.");
                }

                Ok(())
            }

            fn delete(&mut self, key: &SlotKey) -> Result<(), Self::Error> {
                self.charge_gas(&gas_to_charge_for_delete()).
                    map_err(|e| StateAccessorError::Delete{
                        key: key.clone(),
                        inner: e,
                        namespace: <$namespace>::PROVABLE_NAMESPACE,
                    })?;
                let is_value_cached = CachedAccessor::<$namespace>::delete_cached(self, key);

                if is_value_cached == IsValueCached::Yes {
                    self.refund_gas(&gas_to_refund_for_hot_delete()).expect("Failed to refund gas for delete operation. This is a bug. The gas refund constant should always be lower than the gas to charge.");
                }

                Ok(())
            }
        }
    };
}

blanket_impl_metered_state_writer!(User);
blanket_impl_metered_state_writer!(Kernel);

/// Provides write-only access to the accessory state
/// Does not charge gas for write/delete operations.
pub trait AccessoryStateWriter: CachedAccessor<Accessory> {}

impl<T: AccessoryStateWriter> StateWriter<Accessory> for T {
    type Error = Infallible;

    /// Replaces a storage value.
    fn set(&mut self, key: &SlotKey, value: SlotValue) -> Result<(), Self::Error> {
        <Self as CachedAccessor<Accessory>>::set_cached(self, key, value);
        Ok(())
    }

    /// Deletes a storage value.
    fn delete(&mut self, key: &SlotKey) -> Result<(), Self::Error> {
        <Self as CachedAccessor<Accessory>>::delete_cached(self, key);
        Ok(())
    }
}

#[cfg(feature = "native")]
/// Allows a type to retrieve state values with a proof of their presence/absence.
pub trait ProvenStateAccessor<N: ProvableCompileTimeNamespace>: StateReaderAndWriter<N> {
    /// The underlying storage whose proof is returned
    type Proof;
    /// Fetch the value with the requested key and provide a proof of its presence/absence.
    fn get_with_proof(&mut self, key: SlotKey) -> StorageProof<Self::Proof>
    where
        Self: StateReaderAndWriter<N>,
        N: ProvableCompileTimeNamespace;
}

/// A trait indicating that this working set is version aware
pub trait VersionReader: StateReaderAndWriter<namespaces::Kernel> {
    /// Returns the current version of the working set
    fn current_version(&self) -> u64;
}
