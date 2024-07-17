use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

use rockbound::cache::cache_container::CacheContainer;
use rockbound::cache::cache_db::CacheDb;
use rockbound::cache::change_set::ChangeSet;
use rockbound::{ReadOnlyLock, SchemaBatch};
use sov_db::accessory_db::AccessoryDb;
use sov_db::ledger_db::LedgerDb;
use sov_db::state_db::StateDb;
use sov_state::{MerkleProofSpec, ProverChangeSet, ProverStorage};

use crate::cache_container_group::{CacheContainerRwLockGroup, CacheDbGroup};
use crate::jmt_init;

/// Creates a read-only [`ProverStorage`] which just points directly to the underlying database.
/// Should be used only in tests.
pub fn new_orphan_storage<S: MerkleProofSpec>(
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<ProverStorage<S>> {
    let state_db_raw = StateDb::get_rockbound_options().default_setup_db_in_path(path.as_ref())?;
    let state_db_sm = Arc::new(RwLock::new(CacheContainer::orphan(state_db_raw)));
    let state_db_snapshot = CacheDb::new(0, state_db_sm.clone().into());
    let state_db = StateDb::with_cache_db(state_db_snapshot)?;
    let accessory_db_raw =
        AccessoryDb::get_rockbound_options().default_setup_db_in_path(path.as_ref())?;
    let accessory_db_sm = Arc::new(RwLock::new(CacheContainer::orphan(accessory_db_raw)));
    let accessory_db_snapshot = CacheDb::new(0, accessory_db_sm.into());
    let accessory_db = AccessoryDb::with_cache_db(accessory_db_snapshot)?;
    if let Some(ProverChangeSet {
        state_change_set, ..
    }) = ProverStorage::<S>::should_init_db(&state_db)
    {
        let mut state_sm_rw = state_db_sm.write().unwrap();
        let state_change_set = ChangeSet::new_with_operations(0, state_change_set);
        state_sm_rw.add_snapshot(state_change_set).unwrap();
        state_sm_rw.commit_snapshot(&0).unwrap();
    }
    Ok(ProverStorage::with_db_handles(state_db, accessory_db))
}

/// Implementation of storage manager that provides prover storage
/// and commits changes directly to the underlying database
pub struct SimpleStorageManager<S: MerkleProofSpec> {
    cache_containers: CacheContainerRwLockGroup,
    phantom_mp_spec: PhantomData<S>,
}

impl<S: MerkleProofSpec> SimpleStorageManager<S> {
    pub fn new(path: impl AsRef<std::path::Path>) -> Self {
        let state_rocksdb = StateDb::get_rockbound_options()
            .default_setup_db_in_path(path.as_ref())
            .unwrap();
        let accessory_rocksdb = AccessoryDb::get_rockbound_options()
            .default_setup_db_in_path(path.as_ref())
            .unwrap();
        let ledger_rocksdb = LedgerDb::get_rockbound_options()
            .default_setup_db_in_path(path.as_ref())
            .unwrap();

        let snapshot_id_to_parent = Arc::new(RwLock::new(HashMap::new()));

        let read_only_snapshot_id_to_parent = ReadOnlyLock::new(snapshot_id_to_parent.clone());

        let state_cache_container =
            CacheContainer::new(state_rocksdb, read_only_snapshot_id_to_parent.clone());
        let accessory_cache_container =
            CacheContainer::new(accessory_rocksdb, read_only_snapshot_id_to_parent.clone());
        let ledger_cache_container =
            CacheContainer::new(ledger_rocksdb, read_only_snapshot_id_to_parent.clone());

        let cache_containers = CacheContainerRwLockGroup::new(
            state_cache_container,
            accessory_cache_container,
            ledger_cache_container,
        );

        jmt_init::<S>(&cache_containers);

        Self {
            cache_containers,
            phantom_mp_spec: Default::default(),
        }
    }

    pub fn create_storage(&mut self) -> ProverStorage<S> {
        let CacheDbGroup {
            state: state_cache_db,
            accessory: accessory_cache_db,
            ..
        } = self.cache_containers.get_cache_db_group(0);
        let state_db = StateDb::with_cache_db(state_cache_db).unwrap();
        let accessory_db = AccessoryDb::with_cache_db(accessory_cache_db).unwrap();
        ProverStorage::with_db_handles(state_db, accessory_db)
    }

    // If we want it faster, can keep in memory
    pub fn commit(&mut self, prover_change_set: ProverChangeSet) {
        let ProverChangeSet {
            state_change_set,
            accessory_change_set,
        } = prover_change_set;
        let state_change_set = ChangeSet::new_with_operations(0, state_change_set);
        let accessory_change_set = ChangeSet::new_with_operations(0, accessory_change_set);
        let mut cache_containers = self.cache_containers.write();
        let CacheDbGroup { ledger, .. } = self.cache_containers.get_cache_db_group(0);
        cache_containers
            .add_snapshot(state_change_set, accessory_change_set, ledger.into())
            .unwrap();
        cache_containers.commit_snapshot(&0).unwrap();
    }
}

/// Simplified storage manager only for [`LedgerDb`]
pub struct SimpleLedgerStorageManager {
    ledger_cache_container: Arc<RwLock<CacheContainer>>,
}

impl SimpleLedgerStorageManager {
    pub fn new(path: impl AsRef<std::path::Path>) -> Self {
        let ledger_rocksdb = LedgerDb::get_rockbound_options()
            .default_setup_db_in_path(path.as_ref())
            .unwrap();
        let snapshot_id_to_parent = Arc::new(RwLock::new(HashMap::new()));

        let read_only_snapshot_id_to_parent = ReadOnlyLock::new(snapshot_id_to_parent.clone());
        let ledger_cache_container =
            CacheContainer::new(ledger_rocksdb, read_only_snapshot_id_to_parent.clone());
        Self {
            ledger_cache_container: Arc::new(RwLock::new(ledger_cache_container)),
        }
    }

    pub fn create_ledger_storage(&mut self) -> CacheDb {
        CacheDb::new(0, self.ledger_cache_container.clone().into())
    }

    pub fn commit(&mut self, ledger_change_set: SchemaBatch) {
        let ledger_change_set = ChangeSet::new_with_operations(0, ledger_change_set);
        let mut ledger_cache_container = self.ledger_cache_container.write().unwrap();
        ledger_cache_container
            .add_snapshot(ledger_change_set)
            .unwrap();
        ledger_cache_container.commit_snapshot(&0).unwrap();
    }
}
