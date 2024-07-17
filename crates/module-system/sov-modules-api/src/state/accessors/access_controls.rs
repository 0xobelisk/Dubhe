//! This file defines all the possible ways to access the state of the rollup for the
//! accessors defined in this module.

use std::convert::Infallible;

use sov_state::{
    Accessory, SlotKey, SlotValue, StateCodec, StateItemCodec, StateItemDecoder, Storage, User,
};

use super::genesis::GenesisStateAccessor;
use super::internals::Delta;
use super::seal::CachedAccessor;
use crate::state::traits::{AccessoryStateWriter, ProvableStateReader, ProvableStateWriter};
use crate::{
    AccessoryDelta, AccessoryStateCheckpoint, AccessoryStateReader, GasMeter, PreExecWorkingSet,
    Spec, StateCheckpoint, StateReader, StateWriter, TxScratchpad, WorkingSet,
};

macro_rules! inner_impl_unmetered_state_reader {
    ($namespace:ty) => {
        type Error = Infallible;

        fn get(&mut self, key: &SlotKey) -> Result<Option<SlotValue>, Self::Error> {
            Ok(<Self as CachedAccessor<$namespace>>::get_cached(self, key).0)
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

            Ok(storage_value
                .map(|storage_value| codec.value_codec().decode_unwrap(storage_value.value())))
        }
    };
}

macro_rules! inner_impl_unmetered_state_writer {
    ($namespace:ty) => {
        type Error = Infallible;

        fn set(&mut self, key: &SlotKey, value: SlotValue) -> Result<(), Self::Error> {
            <Self as CachedAccessor<$namespace>>::set_cached(self, key, value);
            Ok(())
        }

        fn delete(&mut self, key: &SlotKey) -> Result<(), Self::Error> {
            <Self as CachedAccessor<$namespace>>::delete_cached(self, key);
            Ok(())
        }
    };
}

#[cfg(feature = "native")]
mod http_api {
    use std::convert::Infallible;

    use sov_state::{
        Kernel, SlotKey, SlotValue, StateCodec, StateItemCodec, StateItemDecoder, User,
    };

    use super::CachedAccessor;
    use crate::state::accessors::http_api::ApiStateAccessor;
    use crate::{AccessoryStateReader, AccessoryStateWriter, Spec, StateReader, StateWriter};

    impl<S: Spec> AccessoryStateReader for ApiStateAccessor<S> {}
    impl<S: Spec> AccessoryStateWriter for ApiStateAccessor<S> {}

    impl<S: Spec> StateReader<User> for ApiStateAccessor<S> {
        inner_impl_unmetered_state_reader!(User);
    }
    impl<S: Spec> StateWriter<User> for ApiStateAccessor<S> {
        inner_impl_unmetered_state_writer!(User);
    }

    impl<S: Spec> StateReader<Kernel> for ApiStateAccessor<S> {
        inner_impl_unmetered_state_reader!(Kernel);
    }
    impl<S: Spec> StateWriter<Kernel> for ApiStateAccessor<S> {
        inner_impl_unmetered_state_writer!(Kernel);
    }
}

impl<S: Storage> AccessoryStateReader for AccessoryDelta<S> {}
impl<S: Storage> AccessoryStateWriter for AccessoryDelta<S> {}

impl<S: Spec> StateReader<User> for GenesisStateAccessor<S> {
    inner_impl_unmetered_state_reader!(User);
}
impl<S: Spec> StateWriter<User> for GenesisStateAccessor<S> {
    inner_impl_unmetered_state_writer!(User);
}
impl<S: Spec> AccessoryStateWriter for GenesisStateAccessor<S> {}

impl<S: Spec> StateReader<User> for StateCheckpoint<S> {
    inner_impl_unmetered_state_reader!(User);
}
impl<S: Spec> StateWriter<User> for StateCheckpoint<S> {
    inner_impl_unmetered_state_writer!(User);
}

impl<S: Spec> AccessoryStateWriter for StateCheckpoint<S> {}

impl<S: Spec> StateReader<User> for TxScratchpad<S> {
    inner_impl_unmetered_state_reader!(User);
}
impl<S: Spec> StateWriter<User> for TxScratchpad<S> {
    inner_impl_unmetered_state_writer!(User);
}

impl<S: Spec, PreExecChecksMeter: GasMeter<S::Gas>> ProvableStateReader<User>
    for PreExecWorkingSet<S, PreExecChecksMeter>
{
    type GU = S::Gas;
}
impl<S: Spec, PreExecChecksMeter: GasMeter<S::Gas>> ProvableStateWriter<User>
    for PreExecWorkingSet<S, PreExecChecksMeter>
{
    type GU = S::Gas;
}

impl<S: Spec> ProvableStateReader<User> for WorkingSet<S> {
    type GU = S::Gas;
}
impl<S: Spec> ProvableStateWriter<User> for WorkingSet<S> {
    type GU = S::Gas;
}

impl<S: Spec> AccessoryStateWriter for WorkingSet<S> {}

#[cfg(feature = "test-utils")]
impl<S: Spec> AccessoryStateReader for WorkingSet<S> {}

impl<'a, S: Spec> StateReader<Accessory> for AccessoryStateCheckpoint<'a, S> {
    type Error = Infallible;
    fn get(&mut self, key: &SlotKey) -> Result<Option<SlotValue>, Self::Error> {
        if !cfg!(feature = "native") {
            // Note: We might want to have a special case for that
            panic!("Trying to access a native-protected value {key:?}, from the accessory state, outside of native mode");
        } else {
            Ok(
                <Delta<S::Storage> as CachedAccessor<Accessory>>::get_cached(
                    &mut self.checkpoint.delta,
                    key,
                )
                .0,
            )
        }
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
impl<'a, S: Spec> AccessoryStateWriter for AccessoryStateCheckpoint<'a, S> {}

pub mod kernel_state {
    use std::convert::Infallible;

    use sov_state::{
        Kernel, SlotKey, SlotValue, StateCodec, StateItemCodec, StateItemDecoder, User,
    };

    use crate::state::accessors::seal::CachedAccessor;
    use crate::{
        BootstrapWorkingSet, KernelWorkingSet, Spec, StateCheckpoint, StateReader, StateWriter,
        VersionedStateReadWriter, WorkingSet,
    };

    impl<'a, S: Spec> StateReader<Kernel> for VersionedStateReadWriter<'a, StateCheckpoint<S>> {
        inner_impl_unmetered_state_reader!(Kernel);
    }

    impl<'a, S: Spec> StateWriter<Kernel> for VersionedStateReadWriter<'a, StateCheckpoint<S>> {
        inner_impl_unmetered_state_writer!(Kernel);
    }

    impl<'a, S: Spec> StateReader<Kernel> for VersionedStateReadWriter<'a, WorkingSet<S>> {
        inner_impl_unmetered_state_reader!(Kernel);
    }

    impl<'a, S: Spec> StateWriter<Kernel> for VersionedStateReadWriter<'a, WorkingSet<S>> {
        inner_impl_unmetered_state_writer!(Kernel);
    }

    impl<'a, S: Spec> StateReader<Kernel> for BootstrapWorkingSet<'a, S> {
        inner_impl_unmetered_state_reader!(Kernel);
    }
    impl<'a, S: Spec> StateReader<User> for BootstrapWorkingSet<'a, S> {
        inner_impl_unmetered_state_reader!(User);
    }

    impl<'a, S: Spec> StateWriter<Kernel> for BootstrapWorkingSet<'a, S> {
        inner_impl_unmetered_state_writer!(Kernel);
    }
    impl<'a, S: Spec> StateWriter<User> for BootstrapWorkingSet<'a, S> {
        inner_impl_unmetered_state_writer!(User);
    }

    impl<'a, S: Spec> StateReader<Kernel> for KernelWorkingSet<'a, S> {
        inner_impl_unmetered_state_reader!(Kernel);
    }
    impl<'a, S: Spec> StateReader<User> for KernelWorkingSet<'a, S> {
        inner_impl_unmetered_state_reader!(User);
    }

    impl<'a, S: Spec> StateWriter<Kernel> for KernelWorkingSet<'a, S> {
        inner_impl_unmetered_state_writer!(Kernel);
    }
    impl<'a, S: Spec> StateWriter<User> for KernelWorkingSet<'a, S> {
        inner_impl_unmetered_state_writer!(User);
    }
}
