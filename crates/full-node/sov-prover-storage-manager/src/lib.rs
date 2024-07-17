use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

use rockbound::cache::cache_container::CacheContainer;
use rockbound::cache::cache_db::CacheDb;
use rockbound::cache::change_set::ChangeSet;
use rockbound::cache::SnapshotId;
use rockbound::{ReadOnlyLock, SchemaBatch};
use sov_db::accessory_db::AccessoryDb;
use sov_db::ledger_db::LedgerDb;
use sov_db::state_db::StateDb;
use sov_rollup_interface::da::{BlockHeaderTrait, DaSpec};
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_state::{MerkleProofSpec, ProverChangeSet, ProverStorage};

use crate::cache_container_group::{CacheContainerRwLockGroup, CacheDbGroup};

mod cache_container_group;
#[cfg(feature = "test-utils")]
mod test_utils;

#[cfg(feature = "test-utils")]
pub use test_utils::*;

/// Implementation of [`HierarchicalStorageManager`] that handles relation between snapshots
/// And reorgs on Data Availability layer.
pub struct ProverStorageManager<Da: DaSpec, S: MerkleProofSpec> {
    // L1 forks representation
    // Chain: prev_block -> child_blocks
    chain_forks: HashMap<Da::SlotHash, Vec<Da::SlotHash>>,
    // Reverse: child_block -> parent
    blocks_to_parent: HashMap<Da::SlotHash, Da::SlotHash>,

    latest_snapshot_id: SnapshotId,
    block_hash_to_snapshot_id: HashMap<Da::SlotHash, SnapshotId>,

    // This is for tracking snapshots which are used for view of the head state
    // So they are not meant to be saved
    dangled_snapshots: HashSet<SnapshotId>,

    // Same reference for individual managers
    snapshot_id_to_parent: Arc<RwLock<HashMap<SnapshotId, SnapshotId>>>,

    cache_containers: CacheContainerRwLockGroup,

    phantom_mp_spec: PhantomData<S>,
}

impl<Da: DaSpec, S: MerkleProofSpec> ProverStorageManager<Da, S>
where
    Da::SlotHash: Hash,
{
    fn with_db_handles(
        state_rocksdb: rockbound::DB,
        accessory_rocksdb: rockbound::DB,
        ledger_rocksdb: rockbound::DB,
    ) -> Self {
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
            chain_forks: Default::default(),
            blocks_to_parent: Default::default(),
            latest_snapshot_id: 0,
            block_hash_to_snapshot_id: Default::default(),
            dangled_snapshots: Default::default(),
            snapshot_id_to_parent,
            cache_containers,
            phantom_mp_spec: Default::default(),
        }
    }

    /// Create new [`ProverStorageManager`] from state config.
    pub fn new(config: sov_state::config::Config) -> anyhow::Result<Self> {
        let path = config.path;

        let state_rocksdb = StateDb::get_rockbound_options().default_setup_db_in_path(&path)?;
        let accessory_rocksdb =
            AccessoryDb::get_rockbound_options().default_setup_db_in_path(&path)?;
        let ledger_rocksdb = LedgerDb::get_rockbound_options().default_setup_db_in_path(&path)?;

        Ok(Self::with_db_handles(
            state_rocksdb,
            accessory_rocksdb,
            ledger_rocksdb,
        ))
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.chain_forks.is_empty()
            && self.blocks_to_parent.is_empty()
            && self.block_hash_to_snapshot_id.is_empty()
            && self.snapshot_id_to_parent.read().unwrap().is_empty()
            && self.cache_containers.is_empty()
    }

    fn get_storage_with_snapshot_id(
        &self,
        snapshot_id: SnapshotId,
    ) -> anyhow::Result<(ProverStorage<S>, CacheDb)> {
        let CacheDbGroup {
            state: state_cache_db,
            accessory: accessory_cache_db,
            ledger: ledger_cache_db,
        } = self.cache_containers.get_cache_db_group(snapshot_id);

        let state_db = StateDb::with_cache_db(state_cache_db)?;
        let accessory_db = AccessoryDb::with_cache_db(accessory_cache_db)?;
        Ok((
            ProverStorage::with_db_handles(state_db, accessory_db),
            ledger_cache_db,
        ))
    }

    fn finalize_by_hash_pair(
        &mut self,
        prev_block_hash: Da::SlotHash,
        current_block_hash: Da::SlotHash,
    ) -> anyhow::Result<()> {
        tracing::debug!(
            ?prev_block_hash,
            ?current_block_hash,
            "Finalizing block by pair"
        );
        // Check if snapshot has been saved, but not removing id
        let snapshot_id = &self
            .block_hash_to_snapshot_id
            .get(&current_block_hash)
            .ok_or(anyhow::anyhow!("Attempt to finalize non existing snapshot"))?;
        if !self.cache_containers.contains_snapshot(snapshot_id) {
            anyhow::bail!("Attempt to finalize snapshot which hasn't been saved yet");
        }

        // Check if this is the oldest block
        if self
            .block_hash_to_snapshot_id
            .contains_key(&prev_block_hash)
        {
            if let Some(grand_parent) = self.blocks_to_parent.remove(&prev_block_hash) {
                self.finalize_by_hash_pair(grand_parent, prev_block_hash.clone())?;
            }
        }
        self.blocks_to_parent.remove(&current_block_hash);

        // Removing previous
        self.block_hash_to_snapshot_id.remove(&prev_block_hash);
        let snapshot_id = &self
            .block_hash_to_snapshot_id
            .remove(&current_block_hash)
            .ok_or(anyhow::anyhow!("Attempt to finalize non existing snapshot"))?;

        let mut cache_containers = self.cache_containers.write();

        let mut snapshot_id_to_parent = self.snapshot_id_to_parent.write().unwrap();
        snapshot_id_to_parent.remove(snapshot_id);

        // Panic, because what else can we do? We don't know what data
        cache_containers
            .commit_snapshot(snapshot_id)
            .expect("Unable to commit snapshot");

        for orphan_id in self.dangled_snapshots.iter() {
            if snapshot_id_to_parent.get(orphan_id) == Some(snapshot_id) {
                snapshot_id_to_parent.remove(orphan_id);
            }
        }

        // All siblings of current snapshot
        let mut to_discard: Vec<_> = self
            .chain_forks
            .remove(&prev_block_hash)
            .expect("Inconsistent chain_forks")
            .into_iter()
            .filter(|bh| bh != &current_block_hash)
            .collect();

        while let Some(block_hash) = to_discard.pop() {
            let child_block_hashes = self.chain_forks.remove(&block_hash).unwrap_or_default();
            self.blocks_to_parent.remove(&block_hash).unwrap();

            let snapshot_id = self.block_hash_to_snapshot_id.remove(&block_hash).unwrap();

            for orphan_id in self.dangled_snapshots.iter() {
                if snapshot_id_to_parent.get(orphan_id) == Some(&snapshot_id) {
                    snapshot_id_to_parent.remove(orphan_id);
                }
            }

            snapshot_id_to_parent.remove(&snapshot_id);

            // TODO: This should be addressed in the future.
            // Ideally non saved back snapshots should be discarded
            let has_been_discarded = cache_containers.discard_snapshot(&snapshot_id);
            tracing::debug!(snapshot_id, ?has_been_discarded, "Discarding the snapshot");
            to_discard.extend(child_block_hashes);
        }

        // Removing snapshot id pointers for children of this one
        for child_block_hash in self.chain_forks.get(&current_block_hash).unwrap_or(&vec![]) {
            let child_snapshot_id = self
                .block_hash_to_snapshot_id
                .get(child_block_hash)
                .unwrap();
            snapshot_id_to_parent.remove(child_snapshot_id);
        }

        Ok(())
    }
}

impl<Da: DaSpec, S: MerkleProofSpec> HierarchicalStorageManager<Da> for ProverStorageManager<Da, S>
where
    Da::SlotHash: Hash,
{
    type StfState = ProverStorage<S>;
    type StfChangeSet = ProverChangeSet;
    type LedgerState = CacheDb;
    type LedgerChangeSet = SchemaBatch;

    fn create_bootstrap_state(&mut self) -> anyhow::Result<(Self::StfState, Self::LedgerState)> {
        self.latest_snapshot_id += 1;
        let new_snapshot_id = self.latest_snapshot_id;
        self.dangled_snapshots.insert(new_snapshot_id);
        let CacheDbGroup {
            state: state_cache_db,
            accessory: accessory_cache_db,
            ledger: ledger_cache_db,
        } = self.cache_containers.get_cache_db_group(new_snapshot_id);

        let state_db = StateDb::with_cache_db(state_cache_db)?;
        let accessory_db = AccessoryDb::with_cache_db(accessory_cache_db)?;

        Ok((
            ProverStorage::with_db_handles(state_db, accessory_db),
            ledger_cache_db,
        ))
    }

    fn create_state_for(
        &mut self,
        block_header: &Da::BlockHeader,
    ) -> anyhow::Result<(Self::StfState, Self::LedgerState)> {
        tracing::trace!(?block_header, "Requested native storage for block");
        let current_block_hash = block_header.hash();
        let prev_block_hash = block_header.prev_hash();
        assert_ne!(
            current_block_hash, prev_block_hash,
            "Cannot provide storage for corrupt block: prev_hash == current_hash"
        );
        if let Some(prev_snapshot_id) = self.block_hash_to_snapshot_id.get(&prev_block_hash) {
            if !self.cache_containers.contains_snapshot(prev_snapshot_id) {
                anyhow::bail!("Snapshot for previous block has not been saved yet");
            }
        }

        let new_snapshot_id = match self.block_hash_to_snapshot_id.get(&current_block_hash) {
            // Storage for this block has been requested before
            Some(snapshot_id) => *snapshot_id,
            // Storage requested first time
            None => {
                let new_snapshot_id = self.latest_snapshot_id.wrapping_add(1);
                if let Some(parent_snapshot_id) =
                    self.block_hash_to_snapshot_id.get(&prev_block_hash)
                {
                    let mut snapshot_id_to_parent = self.snapshot_id_to_parent.write().unwrap();
                    snapshot_id_to_parent.insert(new_snapshot_id, *parent_snapshot_id);
                }

                self.block_hash_to_snapshot_id
                    .insert(current_block_hash.clone(), new_snapshot_id);

                self.chain_forks
                    .entry(prev_block_hash.clone())
                    .or_default()
                    .push(current_block_hash.clone());

                self.blocks_to_parent
                    .insert(current_block_hash, prev_block_hash);

                // Update latest snapshot id
                self.latest_snapshot_id = new_snapshot_id;
                new_snapshot_id
            }
        };
        tracing::debug!(
            block_header = %block_header.display(),
            new_snapshot_id,
            "Requested the native storage given block and snapshot ID"
        );

        self.get_storage_with_snapshot_id(new_snapshot_id)
    }

    fn create_state_after(
        &mut self,
        block_header: &Da::BlockHeader,
    ) -> anyhow::Result<(Self::StfState, Self::LedgerState)> {
        let current_block_hash = block_header.hash();
        let prev_block_hash = block_header.prev_hash();
        assert_ne!(
            current_block_hash, prev_block_hash,
            "Cannot provide storage for corrupt block: prev_hash == current_hash"
        );

        let parent_snapshot_id = match self.block_hash_to_snapshot_id.get(&current_block_hash) {
            None => anyhow::bail!("Snapshot for current block has been saved yet"),
            Some(prev_snapshot_id) => {
                if !self.cache_containers.contains_snapshot(prev_snapshot_id) {
                    anyhow::bail!("Snapshot for current block has been saved yet");
                }
                prev_snapshot_id
            }
        };

        self.latest_snapshot_id = self.latest_snapshot_id.wrapping_add(1);
        let new_snapshot_id = self.latest_snapshot_id;
        tracing::debug!(
            block_header = %block_header.display(),
            new_snapshot_id,
            parent_snapshot_id,
            "Creating a new storage snapshot"
        );
        {
            let mut snapshot_id_to_parent = self.snapshot_id_to_parent.write().unwrap();
            snapshot_id_to_parent.insert(new_snapshot_id, *parent_snapshot_id);
        }
        self.dangled_snapshots.insert(new_snapshot_id);

        let CacheDbGroup {
            state: state_cache_db,
            accessory: accessory_cache_db,
            ledger: ledger_cache_db,
        } = self.cache_containers.get_cache_db_group(new_snapshot_id);

        let state_db = StateDb::with_cache_db(state_cache_db)?;
        let accessory_db = AccessoryDb::with_cache_db(accessory_cache_db)?;

        Ok((
            ProverStorage::with_db_handles(state_db, accessory_db),
            ledger_cache_db,
        ))
    }

    fn save_change_set(
        &mut self,
        block_header: &Da::BlockHeader,
        stf_change_set: Self::StfChangeSet,
        ledger_change_set: Self::LedgerChangeSet,
    ) -> anyhow::Result<()> {
        if !self.chain_forks.contains_key(&block_header.prev_hash()) {
            anyhow::bail!(
                "Attempt to save changeset for unknown block header {:?}",
                block_header
            );
        }

        tracing::debug!(
            block_header = %block_header.display(),
            "Saving the ProverChangeSet"
        );

        let ProverChangeSet {
            state_change_set,
            accessory_change_set,
        } = stf_change_set;

        let snapshot_id = *self
            .block_hash_to_snapshot_id
            .get(&block_header.hash())
            .ok_or(anyhow::format_err!(
                "Attempt to save change set for unknown block {}",
                block_header.display(),
            ))?;

        // Just wrapping in a ChangeSet with id for given block.
        // This is done for compatibility with existing ProverStorageManager.
        // It should be addressed in the future.
        let state_change_set = ChangeSet::new_with_operations(snapshot_id, state_change_set);
        let accessory_change_set =
            ChangeSet::new_with_operations(snapshot_id, accessory_change_set);
        let ledger_change_set = ChangeSet::new_with_operations(snapshot_id, ledger_change_set);

        {
            let mut cache_containers = self.cache_containers.write();
            cache_containers
                .add_snapshot(state_change_set, accessory_change_set, ledger_change_set)
                .expect("Adding duplicate change sets, bug detected");
        }
        tracing::debug!(
            block_header = %block_header.display(),
            snapshot_id,
            "Snapshot for block has been saved to StorageManager",
        );
        Ok(())
    }

    fn finalize(&mut self, block_header: &Da::BlockHeader) -> anyhow::Result<()> {
        tracing::debug!(block_header = %block_header.display(), "Finalizing block");
        let current_block_hash = block_header.hash();
        let prev_block_hash = block_header.prev_hash();
        self.finalize_by_hash_pair(prev_block_hash, current_block_hash)
    }
}

pub(crate) fn jmt_init<S: MerkleProofSpec>(cache_containers: &CacheContainerRwLockGroup) {
    let CacheDbGroup {
        state: state_cache_db,
        ledger,
        ..
    } = cache_containers.get_cache_db_group(0);

    if let Some(ProverChangeSet {
        state_change_set,
        accessory_change_set,
    }) = ProverStorage::<S>::should_init_db(&StateDb::with_cache_db(state_cache_db).unwrap())
    {
        let state_change_set = ChangeSet::new_with_operations(0, state_change_set);
        let accessory_change_set = ChangeSet::new_with_operations(0, accessory_change_set);
        let mut containers_write = cache_containers.write();
        containers_write
            .add_snapshot(state_change_set, accessory_change_set, ledger.into())
            .unwrap();
        containers_write.commit_snapshot(&0).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use sov_mock_da::{MockBlockHeader, MockHash};
    use sov_rollup_interface::da::Time;
    use sov_state::namespaces::User;
    use sov_state::{ArrayWitness, OrderedReadsAndWrites, StateAccesses, StateUpdate, Storage};

    use super::*;

    type Da = sov_mock_da::MockDaSpec;
    type S = sov_state::DefaultStorageSpec<sha2::Sha256>;

    fn validate_internal_consistency(storage_manager: &ProverStorageManager<Da, S>) {
        let snapshot_id_to_parent = storage_manager.snapshot_id_to_parent.read().unwrap();

        for (block_hash, parent_block_hash) in storage_manager.blocks_to_parent.iter() {
            // For each block hash, there should be snapshot id
            let snapshot_id = storage_manager
                .block_hash_to_snapshot_id
                .get(block_hash)
                .expect("Missing snapshot_id");

            let contains = storage_manager
                .cache_containers
                .contains_snapshot(snapshot_id);
            // Dangled snapshots must not be saved
            if storage_manager.dangled_snapshots.contains(snapshot_id) {
                assert!(
                    !contains,
                    "dangled snapshot id={} somehow got saved into cache container",
                    snapshot_id
                );
            }

            // If there's a reference to parent snapshot id, it should be consistent with block hash i
            match snapshot_id_to_parent.get(snapshot_id) {
                None => {
                    assert!(!storage_manager
                        .block_hash_to_snapshot_id
                        .contains_key(parent_block_hash));
                }
                Some(parent_snapshot_id) => {
                    let parent_snapshot_id_from_block_hash = storage_manager
                        .block_hash_to_snapshot_id
                        .get(parent_block_hash)
                        .unwrap_or_else(|| panic!(
                            "Missing parent snapshot_id for block_hash={:?}, parent_block_hash={:?}, snapshot_id={}, expected_parent_snapshot_id={}",
                            block_hash, parent_block_hash, snapshot_id, parent_snapshot_id,
                        ));
                    assert_eq!(parent_snapshot_id, parent_snapshot_id_from_block_hash);
                }
            }
        }
    }

    fn build_dbs(path: &std::path::Path) -> (rockbound::DB, rockbound::DB, rockbound::DB) {
        let state_rocksdb = StateDb::get_rockbound_options()
            .default_setup_db_in_path(path)
            .unwrap();
        let accessory_rocksdb = AccessoryDb::get_rockbound_options()
            .default_setup_db_in_path(path)
            .unwrap();
        let ledger_rocksdb = LedgerDb::get_rockbound_options()
            .default_setup_db_in_path(path)
            .unwrap();

        (state_rocksdb, accessory_rocksdb, ledger_rocksdb)
    }

    fn key_from(value: u64) -> SlotKey {
        let x = value.to_be_bytes().to_vec();
        SlotKey::from_bytes(x)
    }

    fn value_from(value: u64) -> SlotValue {
        let x = value.to_be_bytes().to_vec();
        SlotValue::from(x)
    }

    fn write_op(key: u64, value: Option<u64>) -> (SlotKey, Option<SlotValue>) {
        (key_from(key), value.map(value_from))
    }

    fn to_state_accesses(user_state_accesses: OrderedReadsAndWrites) -> StateAccesses {
        StateAccesses {
            user: user_state_accesses,
            kernel: OrderedReadsAndWrites::default(),
        }
    }

    fn materialize_change_set(
        storage: &ProverStorage<S>,
        witness: &ArrayWitness,
        state_writes: &[(u64, Option<u64>)],
        accessory_writes: &[(u64, Option<u64>)],
    ) -> ProverChangeSet {
        let mut state_operations = OrderedReadsAndWrites::default();
        for (key, val) in state_writes {
            state_operations.ordered_writes.push(write_op(*key, *val));
        }
        let (_, mut state_update) = storage
            .compute_state_update(to_state_accesses(state_operations), witness)
            .unwrap();
        for (key, val) in accessory_writes {
            let (key, value) = write_op(*key, *val);
            state_update.add_accessory_item(key, value);
        }
        storage.materialize_changes(&state_update)
    }

    #[test]
    fn initiate_new() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());
        validate_internal_consistency(&storage_manager);
    }

    #[test]
    fn get_new_storage() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_header = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 1,
            time: Time::now(),
        };

        let _storage = storage_manager.create_state_for(&block_header).unwrap();

        assert!(!storage_manager.is_empty());
        // main `.is_empty()` check covers everything, but since it is a test, we want to double-check ourselves.
        assert!(!storage_manager.chain_forks.is_empty());
        assert!(!storage_manager.block_hash_to_snapshot_id.is_empty());
        assert!(storage_manager
            .snapshot_id_to_parent
            .read()
            .unwrap()
            .is_empty());
        assert!(storage_manager.cache_containers.is_empty());
    }

    #[test]
    fn try_get_new_storage_same_block() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_header = MockBlockHeader {
            prev_hash: MockHash::from([0; 32]),
            hash: MockHash::from([1; 32]),
            height: 1,
            time: Time::now(),
        };

        let (_storage_1, ledger_state_1) = storage_manager.create_state_for(&block_header).unwrap();

        // For now, we just check that it does not return Error.
        // After the bigger refactoring of the storage manager, this test should be extended.
        let (_storage_2, ledger_state_2) = storage_manager.create_state_for(&block_header).unwrap();

        let ledger_change_set_1: ChangeSet = ledger_state_1.into();
        let ledger_change_set_2: ChangeSet = ledger_state_2.into();
        // Simple check for same id.
        assert_eq!(ledger_change_set_1.id(), ledger_change_set_2.id());
    }

    #[test]
    #[should_panic(expected = "Cannot provide storage for corrupt block")]
    fn try_get_new_storage_corrupt_block() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_header = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([1; 32]),
            height: 1,
            time: Time::now(),
        };

        storage_manager.create_state_for(&block_header).unwrap();
    }

    #[test]
    fn read_state_before_parent_is_added() {
        // Blocks A -> B
        // snapshot A from block A
        // snapshot B from block B
        // query data from block B, before adding snapshot A back to the manager!
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_a = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 1,
            time: Time::now(),
        };
        let block_b = MockBlockHeader {
            prev_hash: MockHash::from([2; 32]),
            hash: MockHash::from([1; 32]),
            height: 2,
            time: Time::now(),
        };

        let _storage_a = storage_manager.create_state_for(&block_a).unwrap();

        // new storage can be crated only on top of saved snapshot.
        let result = storage_manager.create_state_for(&block_b);
        assert!(result.is_err());
        assert_eq!(
            "Snapshot for previous block has not been saved yet",
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn save_empty_change_set() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_header = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 1,
            time: Time::now(),
        };

        assert!(storage_manager.is_empty());
        let (storage, _) = storage_manager.create_state_for(&block_header).unwrap();
        assert!(!storage_manager.is_empty());

        let state_change_set = materialize_change_set(&storage, &Default::default(), &[], &[]);

        // We can save empty storage as well
        storage_manager
            .save_change_set(&block_header, state_change_set, SchemaBatch::new())
            .unwrap();

        assert!(!storage_manager.is_empty());
    }

    #[test]
    fn try_save_unknown_block_header() {
        let tmpdir_1 = tempfile::tempdir().unwrap();
        let tmpdir_2 = tempfile::tempdir().unwrap();

        let block_a = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 1,
            time: Time::now(),
        };

        let (storage_1, _) = {
            let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir_1.path());
            let mut storage_manager_temp =
                ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
            storage_manager_temp.create_state_for(&block_a).unwrap()
        };

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir_2.path());
        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);

        let stf_change_set = materialize_change_set(&storage_1, &Default::default(), &[], &[]);
        let result = storage_manager.save_change_set(&block_a, stf_change_set, SchemaBatch::new());
        assert!(result.is_err());
        let expected_error_msg = format!(
            "Attempt to save changeset for unknown block header {:?}",
            &block_a
        );
        assert_eq!(expected_error_msg, result.err().unwrap().to_string());
    }

    #[test]
    fn create_storage_after() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_header = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 1,
            time: Time::now(),
        };

        assert!(storage_manager.is_empty());
        let (stf_state, _) = storage_manager.create_state_for(&block_header).unwrap();
        assert!(!storage_manager.is_empty());

        let witness = ArrayWitness::default();
        let stf_change_set =
            materialize_change_set(&stf_state, &witness, &[(3, Some(4))], &[(50, Some(60))]);

        storage_manager
            .save_change_set(&block_header, stf_change_set, SchemaBatch::new())
            .unwrap();
        validate_internal_consistency(&storage_manager);
        let (stf_state_after, _) = storage_manager.create_state_after(&block_header).unwrap();
        validate_internal_consistency(&storage_manager);
        let check_storage_after_values = || {
            assert_eq!(
                Some(value_from(4)),
                stf_state_after.get::<User>(&key_from(3), None, &witness)
            );
            assert_eq!(
                Some(value_from(60)),
                stf_state_after.get_accessory(&key_from(50), None)
            );
        };
        check_storage_after_values();

        storage_manager.finalize(&block_header).unwrap();
        validate_internal_consistency(&storage_manager);
        check_storage_after_values();
    }

    #[test]
    fn try_create_storage_after_before_change_set_saved() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_header = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 1,
            time: Time::now(),
        };

        assert!(storage_manager.is_empty());
        let _storage = storage_manager.create_state_for(&block_header).unwrap();
        assert!(!storage_manager.is_empty());

        let result = storage_manager.create_state_after(&block_header);
        validate_internal_consistency(&storage_manager);
        assert!(result.is_err());
    }

    // ------------
    // More sophisticated tests
    use sov_state::storage::{SlotKey, SlotValue};

    #[test]
    fn linear_progression() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        let block_from_i = |i: u8| MockBlockHeader {
            prev_hash: MockHash::from([i; 32]),
            hash: MockHash::from([i + 1; 32]),
            height: i as u64 + 1,
            time: Time::now(),
        };

        for i in 0u8..4 {
            let block = block_from_i(i);
            let (stf_state, _) = storage_manager.create_state_for(&block).unwrap();
            let state_change_set =
                materialize_change_set(&stf_state, &Default::default(), &[], &[]);
            storage_manager
                .save_change_set(&block, state_change_set, SchemaBatch::new())
                .unwrap();
        }

        for i in 0u8..4 {
            let block = block_from_i(i);
            storage_manager.finalize(&block).unwrap();
            validate_internal_consistency(&storage_manager);
        }
        assert!(storage_manager.is_empty());
    }

    #[test]
    fn parallel_forks() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        // 1    2    3
        // / -> D -> E
        // A -> B -> C
        // \ -> F -> G

        // (height, prev_hash, current_hash)
        let blocks: Vec<(u8, u8, u8)> = vec![
            (1, 0, 1),   // A
            (2, 1, 2),   // B
            (2, 1, 12),  // D
            (2, 1, 22),  // F
            (3, 2, 3),   // C
            (3, 12, 13), // E
            (3, 22, 23), // G
        ];

        for (height, prev_hash, next_hash) in blocks {
            let block = MockBlockHeader {
                prev_hash: MockHash::from([prev_hash; 32]),
                hash: MockHash::from([next_hash; 32]),
                height: height as u64,
                time: Time::now(),
            };
            let (stf_state, _) = storage_manager.create_state_for(&block).unwrap();
            let stf_change_set = materialize_change_set(&stf_state, &Default::default(), &[], &[]);
            storage_manager
                .save_change_set(&block, stf_change_set, SchemaBatch::new())
                .unwrap();
        }

        for prev_hash in 0..3 {
            let block = MockBlockHeader {
                prev_hash: MockHash::from([prev_hash; 32]),
                hash: MockHash::from([prev_hash + 1; 32]),
                height: prev_hash as u64 + 1,
                time: Time::now(),
            };
            storage_manager.finalize(&block).unwrap();
            validate_internal_consistency(&storage_manager);
        }

        assert!(storage_manager.is_empty());
    }

    #[test]
    fn finalize_non_earliest_block() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        // Blocks A -> B -> C
        let block_a = MockBlockHeader::from_height(1);
        let block_b = MockBlockHeader::from_height(2);
        let block_c = MockBlockHeader::from_height(3);

        let (stf_state_a, _) = storage_manager.create_state_for(&block_a).unwrap();
        let witness = ArrayWitness::default();
        let stf_change_set =
            materialize_change_set(&stf_state_a, &witness, &[(1, Some(2))], &[(30, Some(40))]);
        storage_manager
            .save_change_set(&block_a, stf_change_set, SchemaBatch::new())
            .unwrap();

        let (stf_state_b, _) = storage_manager.create_state_for(&block_b).unwrap();
        let stf_change_set =
            materialize_change_set(&stf_state_b, &witness, &[(3, Some(4))], &[(50, Some(60))]);
        storage_manager
            .save_change_set(&block_b, stf_change_set, SchemaBatch::new())
            .unwrap();

        let (stf_state_c, _) = storage_manager.create_state_for(&block_c).unwrap();
        // Then finalize B
        storage_manager.finalize(&block_b).unwrap();

        assert_eq!(
            Some(value_from(2)),
            stf_state_c.get::<User>(&key_from(1), None, &witness)
        );
        assert_eq!(
            Some(value_from(4)),
            stf_state_c.get::<User>(&key_from(3), None, &witness)
        );
        assert_eq!(
            Some(value_from(40)),
            stf_state_c.get_accessory(&key_from(30), None)
        );
        assert_eq!(
            Some(value_from(60)),
            stf_state_c.get_accessory(&key_from(50), None)
        );

        // Finalize C now
        let stf_change_set = materialize_change_set(&stf_state_b, &witness, &[], &[]);
        storage_manager
            .save_change_set(&block_c, stf_change_set, SchemaBatch::new())
            .unwrap();
        storage_manager.finalize(&block_c).unwrap();
        assert!(storage_manager.is_empty());
    }

    #[test]
    fn lifecycle_simulation() {
        let tmpdir = tempfile::tempdir().unwrap();

        let (state_db, accessory_db, ledger_db) = build_dbs(tmpdir.path());

        let mut storage_manager =
            ProverStorageManager::<Da, S>::with_db_handles(state_db, accessory_db, ledger_db);
        assert!(storage_manager.is_empty());

        // Chains:
        // 1 -> 2 -> 3 -> 4 -> 5
        // ---------------------
        //      / -> L -> M
        // A -> B -> C -> D -> E
        // |    \ -> G -> H
        // \ -> F -> K
        // M, E, H, K: Observability snapshots.

        let block_a = MockBlockHeader {
            prev_hash: MockHash::from([0; 32]),
            hash: MockHash::from([1; 32]),
            height: 1,
            time: Time::now(),
        };
        let block_b = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([2; 32]),
            height: 2,
            time: Time::now(),
        };
        let block_c = MockBlockHeader {
            prev_hash: MockHash::from([2; 32]),
            hash: MockHash::from([3; 32]),
            height: 3,
            time: Time::now(),
        };
        let block_d = MockBlockHeader {
            prev_hash: MockHash::from([3; 32]),
            hash: MockHash::from([4; 32]),
            height: 4,
            time: Time::now(),
        };
        let block_e = MockBlockHeader {
            prev_hash: MockHash::from([4; 32]),
            hash: MockHash::from([5; 32]),
            height: 5,
            time: Time::now(),
        };
        let block_f = MockBlockHeader {
            prev_hash: MockHash::from([1; 32]),
            hash: MockHash::from([32; 32]),
            height: 2,
            time: Time::now(),
        };
        let block_g = MockBlockHeader {
            prev_hash: MockHash::from([2; 32]),
            hash: MockHash::from([23; 32]),
            height: 3,
            time: Time::now(),
        };
        let block_h = MockBlockHeader {
            prev_hash: MockHash::from([23; 32]),
            hash: MockHash::from([24; 32]),
            height: 4,
            time: Time::now(),
        };
        let block_k = MockBlockHeader {
            prev_hash: MockHash::from([32; 32]),
            hash: MockHash::from([33; 32]),
            height: 3,
            time: Time::now(),
        };
        let block_l = MockBlockHeader {
            prev_hash: MockHash::from([2; 32]),
            hash: MockHash::from([13; 32]),
            height: 3,
            time: Time::now(),
        };
        let block_m = MockBlockHeader {
            prev_hash: MockHash::from([13; 32]),
            hash: MockHash::from([14; 32]),
            height: 4,
            time: Time::now(),
        };

        // Data
        // | Block |    DB  | Key |  Operation |
        // |     A |  state |   1 |   write(3) |
        // |     A |  state |   3 |   write(4) |
        // |     A |    aux |   3 |  write(40) |
        // |     B |  state |   3 |   write(2) |
        // |     B |    aux |   3 |  write(50) |
        // |     C |  state |   1 |     delete |
        // |     C |  state |   4 |   write(5) |
        // |     C |    aux |   1 |  write(60) |
        // |     D |  state |   3 |   write(6) |
        // |     F |  state |   1 |   write(7) |
        // |     F |    aux |   3 |  write(70) |
        // |     F |  state |   3 |     delete |
        // |     F |    aux |   1 |     delete |
        // |     G |  state |   1 |   write(8) |
        // |     G |    aux |   2 |   write(9) |
        // |     L |  state |   1 |  write(10) |

        let witness = ArrayWitness::default();
        // A
        let (stf_state_a, _) = storage_manager.create_state_for(&block_a).unwrap();
        let stf_change_set = materialize_change_set(
            &stf_state_a,
            &witness,
            &[(1, Some(3)), (3, Some(4))],
            &[(3, Some(40))],
        );

        storage_manager
            .save_change_set(&block_a, stf_change_set, SchemaBatch::new())
            .unwrap();
        // B
        let (stf_state_b, _) = storage_manager.create_state_for(&block_b).unwrap();
        let stf_change_set =
            materialize_change_set(&stf_state_b, &witness, &[(3, Some(2))], &[(3, Some(50))]);
        storage_manager
            .save_change_set(&block_b, stf_change_set, SchemaBatch::new())
            .unwrap();
        // C
        let (stf_state_c, _) = storage_manager.create_state_for(&block_c).unwrap();
        let stf_change_set = materialize_change_set(
            &stf_state_c,
            &witness,
            &[(1, None), (4, Some(5))],
            &[(1, Some(60))],
        );
        storage_manager
            .save_change_set(&block_c, stf_change_set, SchemaBatch::new())
            .unwrap();
        // D
        let (stf_state_d, _) = storage_manager.create_state_for(&block_d).unwrap();
        let stf_change_set = materialize_change_set(&stf_state_d, &witness, &[(3, Some(6))], &[]);
        storage_manager
            .save_change_set(&block_d, stf_change_set, SchemaBatch::new())
            .unwrap();
        // F
        let (stf_state_f, _) = storage_manager.create_state_for(&block_f).unwrap();
        let stf_change_set = materialize_change_set(
            &stf_state_f,
            &witness,
            &[(1, Some(7)), (3, None)],
            &[(1, None), (3, Some(70))],
        );
        storage_manager
            .save_change_set(&block_f, stf_change_set, SchemaBatch::new())
            .unwrap();
        // G
        let (stf_state_g, _) = storage_manager.create_state_for(&block_g).unwrap();
        let stf_change_set =
            materialize_change_set(&stf_state_g, &witness, &[(1, Some(8))], &[(2, Some(9))]);
        storage_manager
            .save_change_set(&block_g, stf_change_set, SchemaBatch::new())
            .unwrap();
        // L
        let (storage_l, _) = storage_manager.create_state_for(&block_l).unwrap();
        let stf_change_set = materialize_change_set(&storage_l, &witness, &[(1, Some(10))], &[]);
        storage_manager
            .save_change_set(&block_l, stf_change_set, SchemaBatch::new())
            .unwrap();

        // VIEW: Before finalization of A
        // | snapshot |    DB  | Key |  Value |
        // |        E |  state |   1 |   None |
        // |        E |  state |   2 |   None |
        // |        E |  state |   3 |      6 |
        // |        E |  state |   4 |      5 |
        // |        E |    aux |   1 |     60 |
        // |        E |    aux |   2 |   None |
        // |        E |    aux |   3 |     50 |
        // |        M |  state |   1 |     10 |
        // |        M |  state |   2 |   None |
        // |        M |  state |   3 |      2 |
        // |        M |  state |   4 |   None |
        // |        M |    aux |   1 |   None |
        // |        M |    aux |   2 |   None |
        // |        M |    aux |   3 |     50 |
        // |        H |  state |   1 |      8 |
        // |        H |  state |   2 |   None |
        // |        H |  state |   3 |      2 |
        // |        H |  state |   4 |   None |
        // |        H |    aux |   1 |   None |
        // |        H |    aux |   2 |      9 |
        // |        H |    aux |   3 |     50 |
        // |        K |  state |   1 |      7 |
        // |        K |  state |   2 |   None |
        // |        K |  state |   3 |   None |
        // |        K |  state |   4 |   None |
        // |        K |    aux |   1 |   None |
        // |        K |    aux |   2 |   None |
        // |        K |    aux |   3 |     70 |

        let (stf_state_e, _) = storage_manager.create_state_for(&block_e).unwrap();
        let (stf_state_m, _) = storage_manager.create_state_for(&block_m).unwrap();
        let (stf_state_h, _) = storage_manager.create_state_for(&block_h).unwrap();
        let (stf_state_k, _) = storage_manager.create_state_for(&block_k).unwrap();

        let assert_main_fork = || {
            assert_eq!(None, stf_state_e.get::<User>(&key_from(1), None, &witness));
            assert_eq!(None, stf_state_e.get::<User>(&key_from(2), None, &witness));
            assert_eq!(
                Some(value_from(6)),
                stf_state_e.get::<User>(&key_from(3), None, &witness)
            );
            assert_eq!(
                Some(value_from(5)),
                stf_state_e.get::<User>(&key_from(4), None, &witness)
            );
            assert_eq!(
                Some(value_from(60)),
                stf_state_e.get_accessory(&key_from(1), None)
            );
            assert_eq!(None, stf_state_e.get_accessory(&key_from(2), None));
            assert_eq!(
                Some(value_from(50)),
                stf_state_e.get_accessory(&key_from(3), None)
            );
        };
        // Storage M
        let assert_storage_m = || {
            assert_eq!(
                Some(value_from(10)),
                stf_state_m.get::<User>(&key_from(1), None, &witness)
            );
            assert_eq!(None, stf_state_m.get::<User>(&key_from(2), None, &witness));
            assert_eq!(
                Some(value_from(2)),
                stf_state_m.get::<User>(&key_from(3), None, &witness)
            );
            assert_eq!(None, stf_state_m.get::<User>(&key_from(4), None, &witness));
            assert_eq!(None, stf_state_m.get_accessory(&key_from(1), None));
            assert_eq!(None, stf_state_m.get_accessory(&key_from(2), None));
            assert_eq!(
                Some(value_from(50)),
                stf_state_m.get_accessory(&key_from(3), None)
            );
        };
        // Storage H
        let assert_storage_h = || {
            assert_eq!(
                Some(value_from(8)),
                stf_state_h.get::<User>(&key_from(1), None, &witness)
            );
            assert_eq!(None, stf_state_h.get::<User>(&key_from(2), None, &witness));
            assert_eq!(
                Some(value_from(2)),
                stf_state_h.get::<User>(&key_from(3), None, &witness)
            );
            assert_eq!(None, stf_state_h.get::<User>(&key_from(4), None, &witness));
            assert_eq!(None, stf_state_h.get_accessory(&key_from(1), None));
            assert_eq!(
                Some(value_from(9)),
                stf_state_h.get_accessory(&key_from(2), None)
            );
            assert_eq!(
                Some(value_from(50)),
                stf_state_h.get_accessory(&key_from(3), None)
            );
        };
        assert_main_fork();
        assert_storage_m();
        assert_storage_h();
        // Storage K
        assert_eq!(
            Some(value_from(7)),
            stf_state_k.get::<User>(&key_from(1), None, &witness)
        );
        assert_eq!(None, stf_state_k.get::<User>(&key_from(2), None, &witness));
        assert_eq!(None, stf_state_k.get::<User>(&key_from(3), None, &witness));
        assert_eq!(None, stf_state_k.get::<User>(&key_from(4), None, &witness));
        assert_eq!(None, stf_state_k.get_accessory(&key_from(1), None));
        assert_eq!(None, stf_state_k.get_accessory(&key_from(2), None));
        assert_eq!(
            Some(value_from(70)),
            stf_state_k.get_accessory(&key_from(3), None)
        );
        validate_internal_consistency(&storage_manager);
        let stf_change_set_k = materialize_change_set(&stf_state_k, &Default::default(), &[], &[]);
        storage_manager
            .save_change_set(&block_k, stf_change_set_k, SchemaBatch::new())
            .unwrap();
        storage_manager.finalize(&block_a).unwrap();
        validate_internal_consistency(&storage_manager);
        assert_main_fork();
        assert_storage_m();
        assert_storage_h();

        // Finalizing the rest
        storage_manager.finalize(&block_b).unwrap();
        validate_internal_consistency(&storage_manager);
        assert_main_fork();
        storage_manager.finalize(&block_c).unwrap();
        validate_internal_consistency(&storage_manager);
        assert_main_fork();
        storage_manager.finalize(&block_d).unwrap();
        validate_internal_consistency(&storage_manager);
        assert_main_fork();
        let stf_change_set_e = materialize_change_set(&stf_state_e, &Default::default(), &[], &[]);
        storage_manager
            .save_change_set(&block_e, stf_change_set_e, SchemaBatch::new())
            .unwrap();
        storage_manager.finalize(&block_e).unwrap();
        assert!(storage_manager.is_empty());
        // Check that values are in the database.
        // The storage manager is empty, as checked before,
        // so new storage should read from the database
        let new_block_after_e = MockBlockHeader {
            prev_hash: MockHash::from([5; 32]),
            hash: MockHash::from([6; 32]),
            height: 6,
            time: Time::now(),
        };
        let (storage_last, _) = storage_manager
            .create_state_for(&new_block_after_e)
            .unwrap();
        assert_eq!(
            Some(value_from(6)),
            storage_last.get::<User>(&key_from(3), None, &witness)
        );
        assert_eq!(
            Some(value_from(50)),
            storage_last.get_accessory(&key_from(3), None)
        );
    }

    fn fill_storage_for_height(height: u64, stf_state: &ProverStorage<S>) -> ProverChangeSet {
        let witness = ArrayWitness::default();
        let mut state_ops = vec![];
        let mut accessory_ops = vec![];

        for x in height * 10..((height + 1) * 10) {
            if x % 2 == 0 {
                state_ops.push((x, Some(x)));
            } else {
                accessory_ops.push((x, Some(x)));
            }
        }
        materialize_change_set(stf_state, &witness, &state_ops, &accessory_ops)
    }

    fn check_storage_for_height(height: u64, stf_state: &ProverStorage<S>) {
        let witness = ArrayWitness::default();
        for x in height * 10..((height + 1) * 10) {
            if x % 2 == 0 {
                let state_value = stf_state.get::<User>(&key_from(x), None, &witness);
                assert_eq!(Some(value_from(x)), state_value);
                assert_eq!(None, stf_state.get_accessory(&key_from(x), None));
            } else {
                assert_eq!(None, stf_state.get::<User>(&key_from(x), None, &witness));
                let accessory_value = stf_state.get_accessory(&key_from(x), None);
                assert_eq!(Some(value_from(x)), accessory_value);
            }
        }
    }

    #[test]
    #[ignore = "known problem"]
    fn removed_fork_data_view() {
        // Test aims to test what data will be seen be

        // Create two branches
        // Chains:
        // 0    1    2    3
        //      / -> E -> F
        // A -> B -> C -> D

        // B is finalized and then C.

        // Would F see data from E?
        // In the current version, it won't, because it will not have a pointer to parent.
        // Фnd will read from the database.
        let tmpdir = tempfile::tempdir().unwrap();
        let storage_config = sov_state::config::Config {
            path: tmpdir.path().to_path_buf(),
        };
        let mut storage_manager = ProverStorageManager::<Da, S>::new(storage_config).unwrap();

        let main_chain_blocks: Vec<MockBlockHeader> =
            (0..=4).map(MockBlockHeader::from_height).collect();

        // Fill the data
        for header in &main_chain_blocks {
            let (stf_state, _) = storage_manager.create_state_for(header).unwrap();
            let change_set = fill_storage_for_height(header.height(), &stf_state);
            storage_manager
                .save_change_set(header, change_set, SchemaBatch::new())
                .unwrap();
        }

        // Fork
        let block_e = MockBlockHeader {
            prev_hash: main_chain_blocks[1].hash(),
            hash: MockHash::from([22; 32]),
            height: 2,
            time: Default::default(),
        };
        let (stf_state, _) = storage_manager.create_state_for(&block_e).unwrap();
        let witness = ArrayWitness::default();
        // Fill some special data for E
        let change_set = materialize_change_set(
            &stf_state,
            &witness,
            &[(30_000_000, Some(100)), (40_000_000, Some(200))],
            &[(50_000_000, Some(300)), (60_000_000, Some(400))],
        );

        storage_manager
            .save_change_set(&block_e, change_set, SchemaBatch::new())
            .unwrap();

        let block_f = MockBlockHeader {
            prev_hash: block_e.hash(),
            hash: MockHash([23; 32]),
            height: 3,
            time: Default::default(),
        };
        let (stf_state, _) = storage_manager.create_state_for(&block_f).unwrap();

        let check_f_state = || {
            // check that it has access to height 0 and 1
            check_storage_for_height(0, &stf_state);
            check_storage_for_height(1, &stf_state);
            assert_eq!(
                Some(value_from(100)),
                stf_state.get::<User>(&key_from(30_000_000), None, &witness)
            );
            assert_eq!(
                Some(value_from(200)),
                stf_state.get::<User>(&key_from(40_000_000), None, &witness)
            );
            assert_eq!(
                Some(value_from(300)),
                stf_state.get_accessory(&key_from(50_000_000), None)
            );
            assert_eq!(
                Some(value_from(400)),
                stf_state.get_accessory(&key_from(60_000_000), None)
            );
        };

        // First check
        check_f_state();

        // Finalizing height 0, its data moves to the db
        storage_manager.finalize(&main_chain_blocks[0]).unwrap();
        // Data is still accessible
        // 1    2    3
        // / -> E -> F
        // B -> C -> D
        check_f_state();

        // Finalizing height 1, same thing
        storage_manager.finalize(&main_chain_blocks[1]).unwrap();
        // Data is still accessible, both forks point to the database, winning one is not yet chosen
        // 2    3
        // E -> F
        // C -> D
        check_f_state();

        // this is a very interesting thing happens
        storage_manager.finalize(&main_chain_blocks[2]).unwrap();
        // E -> F fork becomes orphan and delete. But storage F is still here.
        // But its underlying view changes,
        check_f_state();
    }

    #[test]
    fn parallel_forks_reading_while_finalization_happens() {
        // 0    1    2    3    4    5    6    7
        //                               / -> E
        // A -> B -> C -> D -> E -> F -> G -> H
        //                               \ -> G

        // E, H, G are moved to a separate thread.
        // They read data from each snapshot all the time,
        // checking that data from each for is present
        // Data in each snapshot has `key == value`
        // for a key in `height*10..((height+1)*10)`
        // TBD: Delete operations!

        let tmpdir = tempfile::tempdir().unwrap();
        let storage_config = sov_state::config::Config {
            path: tmpdir.path().to_path_buf(),
        };
        let mut storage_manager = ProverStorageManager::<Da, S>::new(storage_config).unwrap();

        let height_to_fork = 6;
        let mut headers_to_finalize = vec![];
        for height in 0..=height_to_fork {
            let header = MockBlockHeader::from_height(height);
            let (stf_state, _) = storage_manager.create_state_for(&header).unwrap();

            let change_set = fill_storage_for_height(height, &stf_state);
            storage_manager
                .save_change_set(&header, change_set, SchemaBatch::new())
                .unwrap();
            headers_to_finalize.push(header);
        }

        let forking_header = headers_to_finalize.last().unwrap();

        let forks_count = 20;
        let mut handles = vec![];
        let duration_between_finalization = Duration::from_millis(20);
        let is_running = Arc::new(AtomicBool::new(true));

        for fork in 0..forks_count {
            let n: u64 = height_to_fork + 1 + (fork + 1) * 1000;
            let h = n.to_be_bytes().to_vec();
            let mut hash = [0; 32];
            hash[..8].copy_from_slice(&h);
            hash[8] = fork as u8;
            let forked_header = MockBlockHeader {
                prev_hash: forking_header.hash(),
                hash: MockHash::from(hash),
                height: height_to_fork + 1,
                time: Time::now(),
            };
            let (stf_state, _) = storage_manager.create_state_for(&forked_header).unwrap();
            let is_running = is_running.clone();
            let between = duration_between_finalization;
            let handle = std::thread::spawn(move || {
                let mut full_reads_completed = 0;
                while is_running.load(Ordering::Relaxed) {
                    // Do 10 rounds before checking is running a flag.
                    for _ in 0..10 {
                        for height in 0..=height_to_fork {
                            check_storage_for_height(height, &stf_state);
                        }
                        full_reads_completed += 1;
                    }
                }

                assert!(
                    full_reads_completed > 2,
                    "thread was unable to complete at least 2 full reads between {:?}",
                    between
                );
            });
            handles.push(handle);
        }

        // Do finalization here and stop threads afterwards
        for header in headers_to_finalize {
            storage_manager.finalize(&header).unwrap();
            std::thread::sleep(duration_between_finalization);
        }
        is_running.store(false, Ordering::Release);
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
    }
}
