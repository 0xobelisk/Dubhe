use std::convert::Infallible;

use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::*;
use sov_prover_storage_manager::{new_orphan_storage, SimpleStorageManager};
use sov_rollup_interface::execution_mode::{self, Native};
use sov_state::{ArrayWitness, Prefix, ProvableNamespace, ProverStorage, Storage, ZkStorage};
use unwrap_infallible::UnwrapInfallible;

type S = sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;
type Zk =
    sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, execution_mode::Zk>;
pub type TestHasher = <<S as Spec>::CryptoSpec as CryptoSpec>::Hasher;
pub type StorageSpec = sov_state::DefaultStorageSpec<TestHasher>;

trait StateThing {
    type Value: core::fmt::Debug + Eq + PartialEq;

    /// Write itself to WorkingSet
    fn create<S: Spec>(state: &mut WorkingSet<S>) -> Self;

    /// Gets owb value from [`WorkingSet`]
    fn value<S: Spec>(&self, state: &mut WorkingSet<S>) -> Self::Value;

    /// Changes itself in [`WorkingSet`]
    fn change<S: Spec>(&self, state: &mut WorkingSet<S>);
}

#[allow(dead_code)]
enum Condition {
    Checkpoint,
    Revert,
}

#[allow(dead_code)]
impl Condition {
    fn replace_working_set<S: Spec>(&self, working_set: WorkingSet<S>) -> WorkingSet<S> {
        match self {
            Condition::Checkpoint => {
                let (checkpoint, _tx_consumption, _events) = working_set.checkpoint();
                checkpoint.to_working_set_unmetered()
            }
            Condition::Revert => {
                let (tx_scratchpad, _tx_consumption) = working_set.revert();
                let checkpoint = tx_scratchpad.revert();
                checkpoint.to_working_set_unmetered()
            }
        }
    }

    fn process_thing<S: Spec, St: StateThing>(
        &self,
        thing: &St,
        mut working_set: WorkingSet<S>,
    ) -> WorkingSet<S> {
        let value_before = thing.value(&mut working_set);
        thing.change(&mut working_set);
        working_set = self.replace_working_set(working_set);
        let value_after = thing.value(&mut working_set);
        match self {
            Condition::Checkpoint => {
                assert_ne!(
                    value_before, value_after,
                    "Value hasn't been changed after `.checkpoint()`"
                );
            }
            Condition::Revert => {
                assert_eq!(
                    value_before, value_after,
                    "Value has changed after `.revert()`"
                );
            }
        }
        working_set
    }
}

/// Creates thing and checks it with all condition combinations
fn test_state_thing<S: Spec<Storage = ProverStorage<StorageSpec>>, St: StateThing>(
    conditions: &[Condition],
) {
    let tmpdir = tempfile::tempdir().unwrap();
    let storage = new_orphan_storage(tmpdir.path()).unwrap();
    let mut working_set = WorkingSet::new_deprecated(storage);
    let thing = St::create::<S>(&mut working_set);

    for condition in conditions {
        working_set = condition.process_thing(&thing, working_set);
    }
}

struct StateValueSet(StateValue<u32>);

impl StateThing for StateValueSet {
    type Value = u32;

    fn create<S: Spec>(state: &mut WorkingSet<S>) -> Self {
        let state_value = StateValue::new(Prefix::new(vec![0]));
        state_value
            .set(&10, &mut state.to_unmetered())
            .unwrap_infallible();
        StateValueSet(state_value)
    }

    fn value<S: Spec>(&self, state: &mut WorkingSet<S>) -> Self::Value {
        self.0
            .get(&mut state.to_unmetered())
            .unwrap_infallible()
            .expect("Value wasn't set")
    }

    fn change<S: Spec>(&self, state: &mut WorkingSet<S>) {
        let mut value = self.value(state);
        value += 1;
        self.0
            .set(&value, &mut state.to_unmetered())
            .unwrap_infallible();
    }
}

struct StateVecSet(StateVec<u32>);

impl StateThing for StateVecSet {
    type Value = Vec<u32>;

    fn create<S: Spec>(state: &mut WorkingSet<S>) -> Self {
        let state_vec = StateVec::new(Prefix::new(vec![0]));
        state_vec
            .set_all(vec![10, 20, 30, 40, 50, 60], &mut state.to_unmetered())
            .unwrap_infallible();
        StateVecSet(state_vec)
    }

    fn value<S: Spec>(&self, state: &mut WorkingSet<S>) -> Self::Value {
        self.0.iter(&mut state.to_unmetered()).collect()
    }

    fn change<S: Spec>(&self, state: &mut WorkingSet<S>) {
        let mut value = self.value(state);
        for v in value.iter_mut() {
            // TODO: More sophisticated ways of updating it
            *v += 1;
        }
        self.0
            .set_all(value, &mut state.to_unmetered())
            .unwrap_infallible();
    }
}

struct StateVecPush(StateVec<u32>);

impl StateThing for StateVecPush {
    type Value = Vec<u32>;

    fn create<S: Spec>(state: &mut WorkingSet<S>) -> Self {
        let state_vec = StateVec::new(Prefix::new(vec![0]));
        state_vec
            .set_all(vec![10], &mut state.to_unmetered())
            .unwrap_infallible();
        StateVecPush(state_vec)
    }

    fn value<S: Spec>(&self, state: &mut WorkingSet<S>) -> Self::Value {
        self.0.iter(&mut state.to_unmetered()).collect()
    }

    fn change<S: Spec>(&self, state: &mut WorkingSet<S>) {
        let value = self
            .0
            .get(0, &mut state.to_unmetered())
            .unwrap_infallible()
            .expect("Value wasn't set");
        self.0
            .push(&(value + 1), &mut state.to_unmetered())
            .unwrap_infallible();
    }
}

struct StateVecRemove(StateVec<u32>);

impl StateThing for StateVecRemove {
    type Value = Vec<u32>;

    fn create<S: Spec>(state: &mut WorkingSet<S>) -> Self {
        let state_vec = StateVec::new(Prefix::new(vec![0]));
        state_vec
            .set_all(vec![3u32; 100], &mut state.to_unmetered())
            .unwrap_infallible();
        StateVecRemove(state_vec)
    }

    fn value<S: Spec>(&self, state: &mut WorkingSet<S>) -> Self::Value {
        let mut unmetered_ws = state.to_unmetered();
        self.0.iter(&mut unmetered_ws).collect()
    }

    fn change<S: Spec>(&self, state: &mut WorkingSet<S>) {
        self.0.pop(&mut state.to_unmetered()).unwrap_infallible();
    }
}

const CONDITIONS: [Condition; 8] = [
    Condition::Checkpoint,
    Condition::Revert,
    Condition::Checkpoint,
    Condition::Revert,
    Condition::Checkpoint,
    Condition::Revert,
    Condition::Revert,
    Condition::Checkpoint,
];

#[test]
fn test_state_value_set() {
    test_state_thing::<S, StateValueSet>(&CONDITIONS[..]);
}

#[test]
fn test_state_vec_set() {
    test_state_thing::<S, StateVecSet>(&CONDITIONS[..]);
}

#[test]
fn test_state_vec_push() {
    test_state_thing::<S, StateVecPush>(&CONDITIONS[..]);
}

#[test]
fn test_state_vec_remove() {
    test_state_thing::<S, StateVecRemove>(&CONDITIONS[..]);
}

#[test]
fn test_witness_round_trip() -> Result<(), Infallible> {
    let tempdir = tempfile::tempdir().unwrap();
    let state_value = StateValue::new(Prefix::new(vec![0]));

    // Native execution
    let witness: ArrayWitness = {
        let storage = new_orphan_storage::<StorageSpec>(tempdir.path()).unwrap();
        let mut state: StateCheckpoint<S> = StateCheckpoint::new(storage.clone());
        state_value.set(&11, &mut state)?;
        let _ = state_value.get(&mut state);
        state_value.set(&22, &mut state)?;
        let (cache_log, _, witness) = state.freeze();

        let _ = storage
            .validate_and_materialize(cache_log, &witness)
            .expect("Native jmt validation should succeed");
        witness
    };

    {
        let storage = ZkStorage::<StorageSpec>::new();
        let mut state_checkpoint: StateCheckpoint<Zk> =
            StateCheckpoint::with_witness(storage.clone(), witness);
        state_value.set(&11, &mut state_checkpoint)?;
        let _ = state_value.get(&mut state_checkpoint);
        state_value.set(&22, &mut state_checkpoint)?;
        let (cache_log, _, witness) = state_checkpoint.freeze();

        let _ = storage
            .validate_and_materialize(cache_log, &witness)
            .expect("ZK validation should succeed");
    };

    Ok(())
}

/// Test that the state values with a standard working set get written to the user space
#[test]
fn test_state_value_user_namespace() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::<StorageSpec>::new(tmpdir.path());
    let storage = storage_manager.create_storage();

    let state_value = StateValue::new(Prefix::new(vec![0]));

    // Native execution
    let mut state: StateCheckpoint<S> = StateCheckpoint::new(storage.clone());
    state_value.set(&11, &mut state)?;
    let _ = state_value.get(&mut state);
    state_value.set(&22, &mut state)?;
    let (cache_log, _, witness) = state.freeze();

    let (_, change_set) = storage
        .validate_and_materialize(cache_log, &witness)
        .expect("Native JMT validation should succeed");
    storage_manager.commit(change_set);
    let storage = storage_manager.create_storage();

    // In the first version the user and the kernel root hashes are the same
    let kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 0)
        .unwrap();
    let user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 0)
        .unwrap();
    assert_eq!(kernel_root_hash, user_root_hash);

    // Then the kernel is the same but the user root hash changes
    let new_kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 1)
        .unwrap();
    let new_user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 1)
        .unwrap();
    assert_eq!(kernel_root_hash, new_kernel_root_hash);
    assert_ne!(new_kernel_root_hash, new_user_root_hash);

    Ok(())
}

/// Test that the state values with a kernel working set get written to the kernel space
#[test]
fn test_state_value_kernel_namespace() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::<StorageSpec>::new(tmpdir.path());
    let storage = storage_manager.create_storage();

    let state_value = KernelStateValue::new(Prefix::new(vec![0]));

    // Native execution
    let mut state: StateCheckpoint<S> = StateCheckpoint::new(storage.clone());

    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state);
    state_value.set(&11, &mut kernel_working_set)?;
    let _ = state_value.get(&mut kernel_working_set);
    state_value.set(&22, &mut kernel_working_set)?;

    let (cache_log, _, witness) = state.freeze();

    let (_, change_set) = storage
        .validate_and_materialize(cache_log, &witness)
        .expect("Native JMT validation should succeed");
    storage_manager.commit(change_set);
    let storage = storage_manager.create_storage();

    // In the first version the user and the kernel root hashes are the same
    let kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 0)
        .unwrap();
    let user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 0)
        .unwrap();
    assert_eq!(kernel_root_hash, user_root_hash);

    // Then the kernel is the same but the user root hash changes
    let new_kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 1)
        .unwrap();
    let new_user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 1)
        .unwrap();
    assert_eq!(user_root_hash, new_user_root_hash);
    assert_ne!(new_kernel_root_hash, new_user_root_hash);

    Ok(())
}

/// Test that the state maps with a standard working set get written to the user space
#[test]
fn test_state_map_user_namespace() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::<StorageSpec>::new(tmpdir.path());
    let storage = storage_manager.create_storage();

    let state_value = StateMap::new(Prefix::new(vec![0]));

    // Native execution
    let mut state: StateCheckpoint<S> = StateCheckpoint::new(storage.clone());
    state_value.set(&11, &0, &mut state)?;
    let _ = state_value.get(&0, &mut state);
    state_value.set(&22, &0, &mut state)?;
    let (cache_log, _, witness) = state.freeze();

    let (_, change_set) = storage
        .validate_and_materialize(cache_log, &witness)
        .expect("Native JMT validation should succeed");
    storage_manager.commit(change_set);
    let storage = storage_manager.create_storage();

    // In the first version the user and the kernel root hashes are the same
    let kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 0)
        .unwrap();
    let user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 0)
        .unwrap();
    assert_eq!(kernel_root_hash, user_root_hash);

    // Then the kernel is the same but the user root hash changes
    let new_kernel_root_hash: sov_state::jmt::RootHash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 1)
        .unwrap();
    let new_user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 1)
        .unwrap();
    assert_eq!(kernel_root_hash, new_kernel_root_hash);
    assert_ne!(new_kernel_root_hash, new_user_root_hash);

    Ok(())
}

/// Test that the kernel state maps with a kernel working set get written to the kernel space
#[test]
fn test_versioned_state_value_kernel_namespace() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::<StorageSpec>::new(tmpdir.path());
    let storage = storage_manager.create_storage();

    let state_value = VersionedStateValue::new(Prefix::new(vec![0]));

    // Native execution
    let working_set: WorkingSet<S> = WorkingSet::new_deprecated(storage.clone());

    let mut state_checkpoint = working_set.checkpoint().0;
    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    state_value.set_true_current(&11, &mut kernel_working_set);
    let _ = state_value.get_current(&mut kernel_working_set);
    state_value.set_true_current(&22, &mut kernel_working_set);

    let (cache_log, _, witness) = state_checkpoint.freeze();

    let (_, change_set) = storage
        .validate_and_materialize(cache_log, &witness)
        .expect("Native JMT validation should succeed");
    storage_manager.commit(change_set);
    let storage = storage_manager.create_storage();

    // In the first version the user and the kernel root hashes are the same
    let kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 0)
        .unwrap();
    let user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 0)
        .unwrap();
    assert_eq!(kernel_root_hash, user_root_hash);

    // Then the kernel is the same but the user root hash changes
    let new_kernel_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::Kernel, 1)
        .unwrap();
    let new_user_root_hash = storage
        .get_root_hash_namespace(ProvableNamespace::User, 1)
        .unwrap();
    assert_eq!(user_root_hash, new_user_root_hash);
    assert_ne!(new_kernel_root_hash, new_user_root_hash);

    // Check that we can get the current value with a standard working set
    let working_set: WorkingSet<S> = WorkingSet::new_deprecated(storage.clone());
    let mut state_checkpoint = working_set.checkpoint().0;
    let kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    let mut versioned_reader = VersionedStateReadWriter::from_kernel_ws_virtual(kernel_working_set);
    let val = state_value
        .get_current(&mut versioned_reader)?
        .expect("We should be able to retrieve the state value");

    assert_eq!(val, 22);

    Ok(())
}
