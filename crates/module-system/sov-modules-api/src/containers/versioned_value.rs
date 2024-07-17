use std::marker::PhantomData;

use borsh::{BorshDeserialize, BorshSerialize};
use sov_state::{
    BorshCodec, Kernel, Namespace, Prefix, SlotKey, SlotValue, StateCodec, StateItemCodec,
};
use unwrap_infallible::UnwrapInfallible;

use crate::{KernelWorkingSet, Spec, StateReader, StateWriter, VersionReader};

/// A `versioned` value stored in kernel state. The semantics of this type are different
/// depending on the priveleges of the accessor. For a standard ("user space") interaction
/// via a `VersionedStateReadWriter`, only one version of this value is accessible. Inside the kernel,
/// (where access is mediated by a [`KernelWorkingSet`]), all versions of this value are accessible.
///
/// Under the hood, a versioned value is implemented as a map from a slot number to a value. From the kernel, any
/// value can be accessed
// TODO: Automatically clear out old versions from state https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/383
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    BorshDeserialize,
    BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct VersionedStateValue<V, Codec = BorshCodec> {
    _phantom: PhantomData<V>,
    codec: Codec,
    prefix: Prefix,
}

impl<V> VersionedStateValue<V> {
    /// Crates a new [`VersionedStateValue`] with the given prefix and the default
    /// [`StateItemCodec`] (i.e. [`BorshCodec`]).
    pub fn new(prefix: Prefix) -> Self {
        Self::with_codec(prefix, BorshCodec)
    }
}

impl<V, Codec> VersionedStateValue<V, Codec> {
    pub const NAMESPACE: Namespace = Namespace::Kernel;

    /// Creates a new [`VersionedStateValue`] with the given prefix and codec.
    pub fn with_codec(prefix: Prefix, codec: Codec) -> Self {
        Self {
            _phantom: PhantomData,
            codec,
            prefix,
        }
    }

    /// Returns the prefix used when this [`VersionedStateValue`] was created.
    pub fn prefix(&self) -> &Prefix {
        &self.prefix
    }
}

impl<V, Codec> VersionedStateValue<V, Codec> {
    fn encode_key(&self, slot: &u64) -> SlotKey
    where
        Codec: StateCodec,
        Codec::KeyCodec: StateItemCodec<u64>,
    {
        SlotKey::new(self.prefix(), slot, self.codec.key_codec())
    }

    /// Any version_aware working set can read the current contents of a versioned value.
    pub fn get_current<Reader: VersionReader>(
        &self,
        state: &mut Reader,
    ) -> Result<Option<V>, <Reader as StateReader<Kernel>>::Error>
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>,
        Codec::KeyCodec: StateItemCodec<u64>,
    {
        state.get_decoded(&self.encode_key(&state.current_version()), &self.codec)
    }

    /// Only the kernel working set can write to versioned values
    pub fn set_true_current<S: Spec>(&self, value: &V, state: &mut KernelWorkingSet<'_, S>)
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>,
        Codec::KeyCodec: StateItemCodec<u64>,
    {
        StateWriter::<Kernel>::set(
            state,
            &self.encode_key(&state.current_version()),
            SlotValue::new(value, self.codec.value_codec()),
        )
        .unwrap_infallible();
    }

    /// Only the kernel working set can write to versioned values
    pub fn set<S: Spec>(&self, key: &u64, value: &V, state: &mut KernelWorkingSet<'_, S>)
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>,
        Codec::KeyCodec: StateItemCodec<u64>,
    {
        StateWriter::<Kernel>::set(
            state,
            &self.encode_key(key),
            SlotValue::new(value, self.codec.value_codec()),
        )
        .unwrap_infallible();
    }

    /// Any version_aware working set can read the current contents of a versioned value.
    pub fn get<S: Spec>(&self, key: &u64, state: &mut KernelWorkingSet<'_, S>) -> Option<V>
    where
        Codec: StateCodec,
        Codec::ValueCodec: StateItemCodec<V>,
        Codec::KeyCodec: StateItemCodec<u64>,
    {
        StateReader::<Kernel>::get_decoded(state, &self.encode_key(key), &self.codec)
            .unwrap_infallible()
    }
}

#[cfg(test)]
mod tests {
    use sov_mock_da::MockDaSpec;
    use sov_mock_zkvm::MockZkVerifier;
    use sov_prover_storage_manager::new_orphan_storage;
    use sov_rollup_interface::execution_mode::Native;
    use sov_state::Prefix;
    use unwrap_infallible::UnwrapInfallible;

    use crate::capabilities::mocks::MockKernel;
    use crate::{Address, Context, KernelWorkingSet, StateCheckpoint, VersionedStateValue};

    type TestSpec = crate::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;

    #[test]
    fn test_kernel_state_value_as_value() {
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = new_orphan_storage(tmpdir.path()).unwrap();
        let mut working_set = StateCheckpoint::new(storage);

        let prefix = Prefix::new(b"test".to_vec());
        let value = VersionedStateValue::<u64>::new(prefix.clone());

        // Initialize a value in the kernel state during slot 4
        {
            let kernel = MockKernel::<TestSpec, MockDaSpec>::new(4, 1);
            let mut kernel_state = KernelWorkingSet::from_kernel(&kernel, &mut working_set);
            value.set_true_current(&100, &mut kernel_state);
            assert_eq!(
                value.get_current(&mut kernel_state).unwrap_infallible(),
                Some(100)
            );
        }

        let signer = Address::from([1; 32]);
        let sequencer = Address::from([2; 32]);

        {
            {
                let mut versioned_state = working_set.versioned_state(&Context::<TestSpec>::new(
                    signer,
                    Default::default(),
                    sequencer,
                    1,
                ));
                // Try to read the value from user space with the slot number set to 1. Should fail.
                assert_eq!(
                    value.get_current(&mut versioned_state).unwrap_infallible(),
                    None
                );
            }
            let mut versioned_state = working_set.versioned_state(&Context::<TestSpec>::new(
                signer,
                Default::default(),
                sequencer,
                4,
            ));
            // Try to read the value from user space with the slot number set to 4. Should succeed.
            assert_eq!(
                value.get_current(&mut versioned_state).unwrap_infallible(),
                Some(100)
            );
        }
    }

    #[test]
    fn test_kernel_state_value_as_map() {
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = new_orphan_storage(tmpdir.path()).unwrap();
        let mut working_set = StateCheckpoint::new(storage);

        let prefix = Prefix::new(b"test".to_vec());
        let value = VersionedStateValue::<u64>::new(prefix.clone());
        let kernel = MockKernel::<TestSpec, MockDaSpec>::new(4, 1);

        // Initialize a versioned value in the kernel state to be available starting at slot 2
        {
            let mut kernel_state = KernelWorkingSet::from_kernel(&kernel, &mut working_set);
            value.set(&2, &100, &mut kernel_state);
            assert_eq!(value.get(&2, &mut kernel_state), Some(100));
            value.set_true_current(&17, &mut kernel_state);
        }

        let signer = Address::from([1; 32]);
        let sequencer = Address::from([2; 32]);

        {
            {
                let mut versioned_state = working_set.versioned_state(&Context::<TestSpec>::new(
                    signer,
                    Default::default(),
                    sequencer,
                    1,
                ));
                // Try to read the value from user space with the slot number set to 1. Should fail.
                assert_eq!(
                    value.get_current(&mut versioned_state).unwrap_infallible(),
                    None
                );
            }
            {
                // Try to read the value from user space with the slot number set to 2. Should succeed.
                let mut versioned_state = working_set.versioned_state(&Context::<TestSpec>::new(
                    signer,
                    Default::default(),
                    sequencer,
                    2,
                ));

                assert_eq!(
                    value.get_current(&mut versioned_state).unwrap_infallible(),
                    Some(100)
                );
            }

            // Try to read the value from user space with the slot number set to 4. Should succeed.
            let mut versioned_state = working_set.versioned_state(&Context::<TestSpec>::new(
                signer,
                Default::default(),
                sequencer,
                4,
            ));
            assert_eq!(
                value.get_current(&mut versioned_state).unwrap_infallible(),
                Some(17)
            );
        }
    }
}
