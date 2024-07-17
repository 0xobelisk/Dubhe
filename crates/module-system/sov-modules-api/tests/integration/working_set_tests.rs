use sov_mock_da::MockDaSpec;
use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::capabilities::mocks::MockKernel;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::{KernelWorkingSet, StateCheckpoint, StateReader, StateWriter, WorkingSet};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::codec::BcsCodec;
use sov_state::namespaces::{Kernel, User};
use sov_state::{SlotKey, SlotValue};

type TestSpec = sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;

#[test]
fn test_workingset_get() {
    let tempdir = tempfile::tempdir().unwrap();
    let codec = BcsCodec {};
    let storage = new_orphan_storage(tempdir.path()).unwrap();

    let prefix = sov_state::Prefix::new(vec![1, 2, 3]);
    let storage_key = SlotKey::new(&prefix, &vec![4, 5, 6], &codec);
    let storage_value = SlotValue::new(&vec![7, 8, 9], &codec);

    let mut working_set = WorkingSet::<TestSpec>::new_deprecated(storage.clone());
    StateWriter::<User>::set(&mut working_set, &storage_key, storage_value.clone()).expect("The set operation should succeed because there should be enough funds in the metered working set");
    let value = StateReader::<User>::get(&mut working_set, &storage_key).expect("The get operation should succeed because there should be enough funds in the metered working set");

    assert_eq!(Some(storage_value), value);
}

#[test]
fn test_kernel_workingset_get() {
    let tempdir = tempfile::tempdir().unwrap();
    let codec = BcsCodec {};
    let storage = new_orphan_storage(tempdir.path()).unwrap();

    let prefix = sov_state::Prefix::new(vec![1, 2, 3]);
    let storage_key = SlotKey::new(&prefix, &vec![4, 5, 6], &codec);
    let storage_value = SlotValue::new(&vec![7, 8, 9], &codec);
    let kernel: MockKernel<TestSpec, MockDaSpec> = MockKernel::new(4, 1);

    let mut working_set = StateCheckpoint::<TestSpec>::new(storage.clone());
    let mut working_set = KernelWorkingSet::from_kernel(&kernel, &mut working_set);

    StateWriter::<Kernel>::set(&mut working_set, &storage_key, storage_value.clone())
        .expect("This should be unfaillible");

    assert_eq!(
        Some(storage_value),
        StateReader::<Kernel>::get(&mut working_set, &storage_key)
            .expect("This should be unfaillible")
    );
}
