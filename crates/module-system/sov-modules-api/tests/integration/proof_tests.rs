use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::*;
use sov_prover_storage_manager::SimpleStorageManager;
use sov_state::{Prefix, Storage, StorageProof};
use unwrap_infallible::UnwrapInfallible;

type S = sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;

#[allow(clippy::type_complexity)]
fn make_user_map_proof(
    key: u32,
    value: u32,
) -> (
    <<S as Spec>::Storage as Storage>::Root,
    StorageProof<<<S as Spec>::Storage as Storage>::Proof>,
    StateMap<u32, u32>,
) {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let storage = storage_manager.create_storage();
    let mut state = StateCheckpoint::<S>::new(storage.clone());
    let map = StateMap::new(Prefix::new(vec![0]));
    map.set(&key, &value, &mut state).unwrap_infallible();

    let (cache_log, _, witness) = state.freeze();

    let (root, change_set) = storage
        .validate_and_materialize(cache_log, &witness)
        .expect("Native jmt validation should succeed");
    storage_manager.commit(change_set);
    let storage = storage_manager.create_storage();

    let mut working_set = WorkingSet::<S>::new_deprecated(storage);

    let proof = map.get_with_proof(&1, &mut working_set);
    (root, proof, map)
}

#[allow(clippy::type_complexity)]
fn make_user_value_proof(
    value: u32,
) -> (
    <<S as Spec>::Storage as Storage>::Root,
    StorageProof<<<S as Spec>::Storage as Storage>::Proof>,
    StateValue<u32>,
) {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let storage = storage_manager.create_storage();
    let mut working_set = StateCheckpoint::<S>::new(storage.clone());
    let state_val = StateValue::new(Prefix::new(vec![0]));
    state_val.set(&value, &mut working_set).unwrap_infallible();

    let (cache_log, _, witness) = working_set.freeze();

    let (root, change_set) = storage
        .validate_and_materialize(cache_log, &witness)
        .expect("Native jmt validation should succeed");
    storage_manager.commit(change_set);
    let storage = storage_manager.create_storage();

    let mut working_set = WorkingSet::<S>::new_deprecated(storage);

    let proof = state_val.get_with_proof(&mut working_set);
    (root, proof, state_val)
}

mod map {
    use sov_state::{Prefix, ProvableNamespace, SlotKey, SlotValue};

    use super::{make_user_map_proof, S};

    #[test]
    fn test_state_proof_roundtrip() {
        let (root, proof, map) = make_user_map_proof(1, 2);
        let (key, val) = map.verify_proof::<S>(root, proof).unwrap();
        assert_eq!(key, 1);
        assert_eq!(val, Some(2));
    }

    #[test]
    fn test_state_proof_wrong_namespace() {
        let (root, mut proof, map) = make_user_map_proof(1, 2);
        proof.namespace = ProvableNamespace::Kernel;
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }

    #[test]
    fn test_state_proof_wrong_key() {
        let (root, mut proof, map) = make_user_map_proof(1, 2);
        proof.key = SlotKey::new(&Prefix::new(b"wrong_prefix".to_vec()), &1, map.codec());
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }

    #[test]
    fn test_state_proof_missing_value() {
        let (root, mut proof, map) = make_user_map_proof(1, 2);
        proof.value = None;
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }

    #[test]
    fn test_state_proof_wrong_value() {
        let (root, mut proof, map) = make_user_map_proof(1, 2);
        proof.value = Some(SlotValue::new(&3, map.codec()));
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }
}

mod value {
    use sov_state::{Prefix, ProvableNamespace, SlotKey, SlotValue};

    use super::{make_user_value_proof, S};

    #[test]
    fn test_state_proof_roundtrip() {
        let (root, proof, map) = make_user_value_proof(1);
        let val = map.verify_proof::<S>(root, proof).unwrap();
        assert_eq!(val, Some(1));
    }

    #[test]
    fn test_state_proof_wrong_namespace() {
        let (root, mut proof, map) = make_user_value_proof(1);
        proof.namespace = ProvableNamespace::Kernel;
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }

    #[test]
    fn test_state_proof_wrong_key() {
        let (root, mut proof, map) = make_user_value_proof(1);
        proof.key = SlotKey::new(&Prefix::new(b"wrong_prefix".to_vec()), &1, map.codec());
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }

    #[test]
    fn test_state_proof_missing_value() {
        let (root, mut proof, map) = make_user_value_proof(1);
        proof.value = None;
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }

    #[test]
    fn test_state_proof_wrong_value() {
        let (root, mut proof, map) = make_user_value_proof(1);
        proof.value = Some(SlotValue::new(&3, map.codec()));
        assert!(map.verify_proof::<S>(root, proof).is_err());
    }
}

#[test]
fn test_archival_proof_gen() {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let state_val = StateValue::new(Prefix::new(vec![0]));

    // Update the state value and calculate a new root for each iteration
    let mut roots = vec![];
    for iter in 0..10 {
        let storage = storage_manager.create_storage();
        let mut state = StateCheckpoint::<S>::new(storage.clone());
        if iter % 2 == 0 {
            state_val.set(&iter, &mut state).unwrap_infallible();
        } else {
            state_val.delete(&mut state).unwrap_infallible();
        }

        let (cache_log, _, witness) = state.freeze();

        let (root, change_set) = storage
            .validate_and_materialize(cache_log, &witness)
            .expect("Native jmt validation should succeed");

        storage_manager.commit(change_set);

        roots.push(root);
    }

    let storage = storage_manager.create_storage();
    // Generate a proof at each archival state and validate it against the root
    let mut api_state_accessor = ApiStateAccessor::<S>::new(storage.clone());
    for iter in 0..10 {
        let mut archival_accessor = api_state_accessor.get_archival_at((iter + 1) as u64); // Versions are 1-indexed
        let proof = state_val.get_with_proof(&mut archival_accessor);
        let value = state_val
            .verify_proof::<S>(roots[iter as usize], proof)
            .unwrap();
        if iter % 2 == 0 {
            assert_eq!(value, Some(iter));
        } else {
            assert!(value.is_none());
        }
    }

    // Check that the default working_set use the latest state for archival proof generation
    let proof = state_val.get_with_proof(&mut api_state_accessor);
    let final_value = state_val.verify_proof::<S>(roots[9], proof).unwrap();
    assert_eq!(final_value, None);
}
