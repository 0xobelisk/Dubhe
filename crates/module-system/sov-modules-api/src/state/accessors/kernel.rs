/// Provides specialized working set wrappers for dealing with protected state.
use sov_rollup_interface::da::DaSpec;
use sov_state::{namespaces, CompileTimeNamespace, IsValueCached, SlotKey, SlotValue};

use self::checkpoints::StateCheckpoint;
use self::internals::{Delta, RevertableWriter};
use self::scratchpad::{TxScratchpad, WorkingSet};
use super::*;
use crate::capabilities::Kernel;
use crate::state::traits::VersionReader;
use crate::Spec;

impl<'a, S: Spec> VersionReader for VersionedStateReadWriter<'a, StateCheckpoint<S>> {
    fn current_version(&self) -> u64 {
        self.slot_num
    }
}

/// A wrapper over [`WorkingSet`] that allows access to kernel values
/// TODO(@theochap): this struct is deprecated and should be removed in favor of [`KernelWorkingSet`]
pub struct VersionedStateReadWriter<'a, S> {
    pub(super) state: &'a mut S,
    pub(super) slot_num: u64,
}

impl<'a, S: Spec> VersionedStateReadWriter<'a, StateCheckpoint<S>> {
    /// Instantiates a [`VersionedStateReadWriter`] from a kernel working set.
    /// Sets the `slot_num` to the virtual slot number of the kernel.
    pub fn from_kernel_ws_virtual(
        kernel_ws: KernelWorkingSet<'a, S>,
    ) -> VersionedStateReadWriter<'a, StateCheckpoint<S>> {
        VersionedStateReadWriter {
            state: kernel_ws.inner,
            slot_num: kernel_ws.virtual_slot_num,
        }
    }
}

impl<'a, S> VersionedStateReadWriter<'a, S> {
    /// Returns the working slot number
    pub fn slot_num(&self) -> u64 {
        self.slot_num
    }

    /// Returns a reference to the inner working set
    pub fn get_ws(&self) -> &S {
        self.state
    }

    /// Returns a mutable reference to the inner working set
    pub fn get_ws_mut(&mut self) -> &mut S {
        self.state
    }
}

impl<'a, S: Spec> CachedAccessor<namespaces::Kernel>
    for VersionedStateReadWriter<'a, StateCheckpoint<S>>
{
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <Delta<S::Storage> as CachedAccessor<namespaces::Kernel>>::get_cached(
            &mut self.state.delta,
            key,
        )
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <Delta<S::Storage> as CachedAccessor<namespaces::Kernel>>::set_cached(
            &mut self.state.delta,
            key,
            value,
        )
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        <Delta<S::Storage> as CachedAccessor<namespaces::Kernel>>::delete_cached(
            &mut self.state.delta,
            key,
        )
    }
}

impl<'a, S: Spec> CachedAccessor<namespaces::Kernel>
    for VersionedStateReadWriter<'a, WorkingSet<S>>
{
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <RevertableWriter<TxScratchpad<S>> as CachedAccessor<namespaces::Kernel>>::get_cached(
            &mut self.state.delta,
            key,
        )
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <RevertableWriter<TxScratchpad<S>> as CachedAccessor<namespaces::Kernel>>::set_cached(
            &mut self.state.delta,
            key,
            value,
        )
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        <RevertableWriter<TxScratchpad<S>> as CachedAccessor<namespaces::Kernel>>::delete_cached(
            &mut self.state.delta,
            key,
        )
    }
}

/// A special wrapper over [`WorkingSet`] that allows access to kernel values to bootstrap the kernel working set
pub struct BootstrapWorkingSet<'a, S: Spec> {
    /// The inner working set
    pub(crate) inner: &'a mut StateCheckpoint<S>,
}

impl<'a, S: Spec, N: CompileTimeNamespace> CachedAccessor<N> for BootstrapWorkingSet<'a, S> {
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <Delta<S::Storage> as CachedAccessor<N>>::get_cached(&mut self.inner.delta, key)
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <Delta<S::Storage> as CachedAccessor<N>>::set_cached(&mut self.inner.delta, key, value)
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        <Delta<S::Storage> as CachedAccessor<N>>::delete_cached(&mut self.inner.delta, key)
    }
}

/// A wrapper over [`WorkingSet`] that allows access to kernel values
pub struct KernelWorkingSet<'a, S: Spec> {
    /// The inner working set
    pub inner: &'a mut StateCheckpoint<S>,
    /// The actual current slot number
    pub(super) true_slot_num: u64,
    /// The slot number visible to user-space modules
    pub(super) virtual_slot_num: u64,
}

impl<'a, S: Spec> VersionReader for KernelWorkingSet<'a, S> {
    fn current_version(&self) -> u64 {
        self.true_slot_num
    }
}

impl<'a, S: Spec> KernelWorkingSet<'a, S> {
    /// This private method instantiates a bootstrap working set to initialize a kernel
    fn get_bootstrap(inner: &'a mut StateCheckpoint<S>) -> BootstrapWorkingSet<'a, S> {
        BootstrapWorkingSet { inner }
    }

    /// Build a new kernel working set from the associated kernel
    pub fn from_kernel<K: Kernel<S, Da>, Da: DaSpec>(
        kernel: &K,
        state_checkpoint: &'a mut StateCheckpoint<S>,
    ) -> Self {
        let mut bootstrapper = KernelWorkingSet::get_bootstrap(state_checkpoint);
        let true_slot_num = kernel.true_slot_number(&mut bootstrapper);
        let virtual_slot_num = kernel.visible_slot_number(&mut bootstrapper);
        Self {
            inner: state_checkpoint,
            true_slot_num,
            virtual_slot_num,
        }
    }

    /// Returns a kernel working set with its heights intiialized to 0.
    /// This is intended to be used for genesis setup only.
    pub fn uninitialized(state_checkpoint: &'a mut StateCheckpoint<S>) -> Self {
        Self {
            inner: state_checkpoint,
            true_slot_num: 0,
            virtual_slot_num: 0,
        }
    }

    /// Returns the true slot number
    pub fn current_slot(&self) -> u64 {
        self.true_slot_num
    }

    /// Returns the slot number visible from user space
    pub fn virtual_slot(&self) -> u64 {
        self.virtual_slot_num
    }

    /// Updates the kernel working set internals
    pub fn update_true_slot_number(&mut self, true_slot_num: u64) {
        self.true_slot_num = true_slot_num;
    }

    /// Updates the kernel working set internals
    pub fn update_virtual_height(&mut self, virtual_height: u64) {
        self.virtual_slot_num = virtual_height;
    }
}

impl<'a, N: CompileTimeNamespace, S: Spec> CachedAccessor<N> for KernelWorkingSet<'a, S> {
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <Delta<S::Storage> as CachedAccessor<N>>::get_cached(&mut self.inner.delta, key)
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <Delta<S::Storage> as CachedAccessor<N>>::set_cached(&mut self.inner.delta, key, value)
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        <Delta<S::Storage> as CachedAccessor<N>>::delete_cached(&mut self.inner.delta, key)
    }
}
