//! Defines traits for storage access
use sov_state::{CompileTimeNamespace, IsValueCached, Namespace, SlotKey, SlotValue};

mod access_controls;
mod checkpoints;
mod genesis;
mod internals;
mod kernel;
#[cfg(any(feature = "test-utils", feature = "evm"))]
mod unmetered_state_wrapper;

#[cfg(any(feature = "test-utils", feature = "evm"))]
pub use unmetered_state_wrapper::UnmeteredStateWrapper;

#[cfg(feature = "native")]
mod http_api;

#[cfg(feature = "native")]
pub use http_api::ApiStateAccessor;

mod scratchpad;

pub use checkpoints::{AccessoryStateCheckpoint, StateCheckpoint};
pub use genesis::GenesisStateAccessor;
pub use internals::AccessoryDelta;
pub use kernel::{BootstrapWorkingSet, KernelWorkingSet, VersionedStateReadWriter};
pub use scratchpad::{AuthorizeTransactionError, PreExecWorkingSet, TxScratchpad, WorkingSet};

use self::seal::CachedAccessor;

pub(super) mod seal {
    use sov_state::{CompileTimeNamespace, IsValueCached, SlotKey, SlotValue};

    /// A helper trait that is used to derive [`crate::StateReader`]/[`crate::StateWriter`] and the corresponding unmetered versions.
    pub trait CachedAccessor<N: CompileTimeNamespace> {
        fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached);
        fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached;
        fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached;
    }
}

/// A helper trait allowing a type to access any namespace by their *runtime* enum variant.
/// Structs that implements this trait also implement [`CachedAccessor`] for any namespace by default.
/// Useful to represent structs with caches containing different state value namespaces that can be committed to the storage.
trait UniversalStateAccessor {
    fn get(&mut self, namespace: Namespace, key: &SlotKey) -> (Option<SlotValue>, IsValueCached);
    fn set(&mut self, namespace: Namespace, key: &SlotKey, value: SlotValue) -> IsValueCached;
    fn delete(&mut self, namespace: Namespace, key: &SlotKey) -> IsValueCached;
}

impl<N: CompileTimeNamespace, T: UniversalStateAccessor> CachedAccessor<N> for T {
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        <Self as UniversalStateAccessor>::get(self, N::NAMESPACE, key)
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        <Self as UniversalStateAccessor>::set(self, N::NAMESPACE, key, value)
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        <Self as UniversalStateAccessor>::delete(self, N::NAMESPACE, key)
    }
}
