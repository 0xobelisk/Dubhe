use std::sync::{Arc, RwLock, RwLockWriteGuard};

use rockbound::cache::cache_container::CacheContainer;
use rockbound::cache::cache_db::CacheDb;
use rockbound::cache::change_set::ChangeSet;
use rockbound::cache::SnapshotId;

/// Group of cache containers. For consistent usage of all RwLocks.
pub(crate) struct CacheContainerRwLockGroup {
    state_cache_container: Arc<RwLock<CacheContainer>>,
    accessory_cache_container: Arc<RwLock<CacheContainer>>,
    ledger_cache_container: Arc<RwLock<CacheContainer>>,
}

impl CacheContainerRwLockGroup {
    pub(crate) fn new(
        state: CacheContainer,
        accessory: CacheContainer,
        ledger: CacheContainer,
    ) -> Self {
        Self {
            state_cache_container: Arc::new(RwLock::new(state)),
            accessory_cache_container: Arc::new(RwLock::new(accessory)),
            ledger_cache_container: Arc::new(RwLock::new(ledger)),
        }
    }

    pub(crate) fn write(&self) -> CacheContainerGroupWriteGuard {
        CacheContainerGroupWriteGuard {
            state: self
                .state_cache_container
                .write()
                .expect("State cache container lock is poisoned"),
            accessory: self
                .accessory_cache_container
                .write()
                .expect("Accessory cache container lock is poisoned"),
            ledger: self
                .ledger_cache_container
                .write()
                .expect("Ledger cache container lock is poisoned"),
        }
    }

    pub(crate) fn get_cache_db_group(&self, snapshot_id: SnapshotId) -> CacheDbGroup {
        CacheDbGroup {
            state: CacheDb::new(snapshot_id, self.state_cache_container.clone().into()),
            accessory: CacheDb::new(snapshot_id, self.accessory_cache_container.clone().into()),
            ledger: CacheDb::new(snapshot_id, self.ledger_cache_container.clone().into()),
        }
    }

    pub(crate) fn contains_snapshot(&self, snapshot_id: &SnapshotId) -> bool {
        // We know, snapshots added or discarded all together.
        // So it is enough to check that only state contains snapshot id.

        // But because it is read only lock and it is relatively not expensive
        // we can check all containers, to be 100% sure

        // If it happens that this part is the bottle neck, it can be optimized.

        let state_contains = self
            .state_cache_container
            .read()
            .unwrap()
            .contains_snapshot(snapshot_id);
        let accessory_contains = self
            .accessory_cache_container
            .read()
            .unwrap()
            .contains_snapshot(snapshot_id);
        let ledger_contains = self
            .ledger_cache_container
            .read()
            .unwrap()
            .contains_snapshot(snapshot_id);

        assert!(
            state_contains == accessory_contains && accessory_contains == ledger_contains,
            "Discrepancy detected in snapshot containment across containers"
        );

        state_contains
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.state_cache_container.read().unwrap().is_empty()
            && self.accessory_cache_container.read().unwrap().is_empty()
            && self.ledger_cache_container.read().unwrap().is_empty()
    }
}

pub(crate) struct CacheDbGroup {
    pub(crate) state: CacheDb,
    pub(crate) accessory: CacheDb,
    pub(crate) ledger: CacheDb,
}

pub(crate) struct CacheContainerGroupWriteGuard<'a> {
    state: RwLockWriteGuard<'a, CacheContainer>,
    accessory: RwLockWriteGuard<'a, CacheContainer>,
    ledger: RwLockWriteGuard<'a, CacheContainer>,
}

impl<'a> CacheContainerGroupWriteGuard<'a> {
    pub(crate) fn add_snapshot(
        &mut self,
        state_change_set: ChangeSet,
        accessory_change_set: ChangeSet,
        ledger_change_set: ChangeSet,
    ) -> anyhow::Result<()> {
        self.state.add_snapshot(state_change_set)?;
        self.accessory.add_snapshot(accessory_change_set)?;
        self.ledger.add_snapshot(ledger_change_set)?;
        Ok(())
    }

    pub(crate) fn commit_snapshot(&mut self, snapshot_id: &SnapshotId) -> anyhow::Result<()> {
        self.state.commit_snapshot(snapshot_id)?;
        self.accessory.commit_snapshot(snapshot_id)?;
        self.ledger.commit_snapshot(snapshot_id)?;
        Ok(())
    }

    // Returns true if snapshot was present and has been discarded
    // or false if it wasn't there.
    pub(crate) fn discard_snapshot(&mut self, snapshot_id: &SnapshotId) -> bool {
        let state_discarded = self.state.discard_snapshot(snapshot_id).is_some();
        let accessory_discarded = self.accessory.discard_snapshot(snapshot_id).is_some();
        let ledger_discarded = self.ledger.discard_snapshot(snapshot_id).is_some();

        assert!(
            state_discarded == accessory_discarded && accessory_discarded == ledger_discarded,
            "Discrepancy detected in snapshot discarding across containers"
        );
        state_discarded
    }
}
