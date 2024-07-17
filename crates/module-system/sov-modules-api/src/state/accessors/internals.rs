use core::fmt;
use std::collections::HashMap;

use sov_state::{
    namespaces, Accessory, IsValueCached, Namespace, ProvableStorageCache, SlotKey, SlotValue,
    StateAccesses, Storage,
};

use super::seal::CachedAccessor;
use super::UniversalStateAccessor;

/// A [`Delta`] is a diff over an underlying [`Storage`] instance. When queried, it first checks
/// whether the value is in its local cache and, if so, returns it. Otherwise, it queries the
/// underlying storage for the requested key, adds it to the Witness, and populates the value Into
/// its own local cache before returning.
///
/// Writes are always performed on the local cache, and are only committed to the underlying storage
/// when the `Delta` is frozen.
pub(super) struct Delta<S: Storage> {
    pub(super) inner: S,
    witness: S::Witness,
    kernel_cache: ProvableStorageCache<namespaces::Kernel>,
    user_cache: ProvableStorageCache<namespaces::User>,
    accessory_writes: HashMap<SlotKey, Option<SlotValue>>,
    pub(super) version: Option<u64>,
}

impl<S: Storage> Delta<S> {
    pub(super) fn new(inner: S, version: Option<u64>) -> Self {
        Self::with_witness(inner, Default::default(), version)
    }

    pub(super) fn with_witness(inner: S, witness: S::Witness, version: Option<u64>) -> Self {
        Self {
            inner,
            witness,
            user_cache: Default::default(),
            kernel_cache: Default::default(),
            accessory_writes: Default::default(),
            version,
        }
    }

    pub(super) fn freeze(self) -> (StateAccesses, AccessoryDelta<S>, S::Witness) {
        let Self {
            inner,
            user_cache,
            kernel_cache,
            accessory_writes,
            witness,
            version,
        } = self;

        (
            StateAccesses {
                user: user_cache.into(),
                kernel: kernel_cache.into(),
            },
            AccessoryDelta {
                version,
                writes: accessory_writes,
                storage: inner,
            },
            witness,
        )
    }
}

impl<S: Storage> UniversalStateAccessor for Delta<S> {
    fn get(&mut self, namespace: Namespace, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        match namespace {
            Namespace::User => {
                self.user_cache
                    .get_or_fetch(key, &self.inner, &self.witness, self.version)
            }
            Namespace::Kernel => {
                self.kernel_cache
                    .get_or_fetch(key, &self.inner, &self.witness, self.version)
            }
            Namespace::Accessory => match self.accessory_writes.get(key).cloned() {
                Some(Some(value)) => (Some(value), IsValueCached::Yes),
                Some(None) => (None, IsValueCached::Yes),
                None => (
                    self.inner.get_accessory(key, self.version),
                    IsValueCached::No,
                ),
            },
        }
    }

    fn set(&mut self, namespace: Namespace, key: &SlotKey, value: SlotValue) -> IsValueCached {
        match namespace {
            Namespace::User => self.user_cache.set(key, value),
            Namespace::Kernel => self.kernel_cache.set(key, value),
            Namespace::Accessory => {
                if self
                    .accessory_writes
                    .insert(key.clone(), Some(value))
                    .is_none()
                {
                    IsValueCached::No
                } else {
                    IsValueCached::Yes
                }
            }
        }
    }

    fn delete(&mut self, namespace: Namespace, key: &SlotKey) -> IsValueCached {
        match namespace {
            Namespace::User => self.user_cache.delete(key),
            Namespace::Kernel => self.kernel_cache.delete(key),
            Namespace::Accessory => {
                if self.accessory_writes.remove(key).is_none() {
                    IsValueCached::No
                } else {
                    IsValueCached::Yes
                }
            }
        }
    }
}

impl<S: Storage> fmt::Debug for Delta<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delta").finish()
    }
}

/// A delta containing *only* the accessory state.
pub struct AccessoryDelta<S: Storage> {
    // This inner storage is never accessed inside the zkVM because reads are
    // not allowed, so it can result as dead code.
    #[allow(dead_code)]
    version: Option<u64>,
    writes: HashMap<SlotKey, Option<SlotValue>>,
    storage: S,
}

impl<S: Storage> AccessoryDelta<S> {
    /// Freeze the accessory delta, preventing further accesses.
    pub fn freeze(self) -> Vec<(SlotKey, Option<SlotValue>)> {
        self.writes.into_iter().collect()
    }
}

impl<S: Storage> CachedAccessor<Accessory> for AccessoryDelta<S> {
    fn get_cached(&mut self, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        if let Some(value) = self.writes.get(key) {
            return (value.clone().map(Into::into), IsValueCached::Yes);
        }

        (
            self.storage.get_accessory(key, self.version),
            IsValueCached::No,
        )
    }

    fn set_cached(&mut self, key: &SlotKey, value: SlotValue) -> IsValueCached {
        if self.writes.insert(key.clone(), Some(value)).is_none() {
            IsValueCached::No
        } else {
            IsValueCached::Yes
        }
    }

    fn delete_cached(&mut self, key: &SlotKey) -> IsValueCached {
        if self.writes.insert(key.clone(), None).is_none() {
            IsValueCached::No
        } else {
            IsValueCached::Yes
        }
    }
}

pub(super) struct RevertableWriter<T> {
    pub(super) inner: T,
    writes: HashMap<(SlotKey, Namespace), Option<SlotValue>>,
}

impl<T: fmt::Debug> fmt::Debug for RevertableWriter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevertableWriter")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<T> RevertableWriter<T> {
    pub(super) fn new(inner: T) -> Self {
        Self {
            inner,
            writes: Default::default(),
        }
    }

    /// Commit all items from [`RevertableWriter`] returning the inner storage.
    pub(super) fn commit(mut self) -> T
    where
        T: UniversalStateAccessor,
    {
        for ((key, namespace), value) in self.writes.into_iter() {
            Self::commit_entry(&mut self.inner, namespace, key, value);
        }

        self.inner
    }

    pub(super) fn revert(self) -> T {
        self.inner
    }

    fn commit_entry(inner: &mut T, namespace: Namespace, key: SlotKey, value: Option<SlotValue>)
    where
        T: UniversalStateAccessor,
    {
        match value {
            Some(value) => inner.set(namespace, &key, value),
            None => inner.delete(namespace, &key),
        };
    }
}

impl<T> UniversalStateAccessor for RevertableWriter<T>
where
    T: UniversalStateAccessor,
{
    fn get(&mut self, namespace: Namespace, key: &SlotKey) -> (Option<SlotValue>, IsValueCached) {
        if let Some(value) = self.writes.get(&(key.clone(), namespace)) {
            (value.as_ref().cloned().map(Into::into), IsValueCached::Yes)
        } else {
            <T as UniversalStateAccessor>::get(&mut self.inner, namespace, key)
        }
    }

    fn set(&mut self, namespace: Namespace, key: &SlotKey, value: SlotValue) -> IsValueCached {
        if self
            .writes
            .insert((key.clone(), namespace), Some(value))
            .is_none()
        {
            IsValueCached::No
        } else {
            IsValueCached::Yes
        }
    }

    fn delete(&mut self, namespace: Namespace, key: &SlotKey) -> IsValueCached {
        if self.writes.insert((key.clone(), namespace), None).is_none() {
            IsValueCached::No
        } else {
            IsValueCached::Yes
        }
    }
}
