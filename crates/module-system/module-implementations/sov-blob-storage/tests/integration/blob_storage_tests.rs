use sov_blob_storage::BlobStorage;
use sov_chain_state::{ChainState, ChainStateConfig};
use sov_mock_da::{MockAddress, MockDaSpec};
use sov_modules_api::{
    BlobData, BlobDataWithId, KernelModule, KernelWorkingSet, RawTx, StateCheckpoint,
};
use sov_prover_storage_manager::new_orphan_storage;

type S = sov_test_utils::TestSpec;
type Da = MockDaSpec;

#[test]
fn empty_test() {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut working_set = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());

    let chain_state = ChainState::<S, Da>::default();
    let initial_slot_number = 1;
    let chain_state_config = ChainStateConfig {
        current_time: Default::default(),
        genesis_da_height: 0,
        inner_code_commitment: Default::default(),
        outer_code_commitment: Default::default(),
    };
    chain_state
        .genesis_unchecked(
            &chain_state_config,
            &mut KernelWorkingSet::uninitialized(&mut working_set),
        )
        .unwrap();

    let blob_storage = BlobStorage::<S, Da>::default();

    let blobs = blob_storage.take_blobs_for_slot_number(initial_slot_number, &mut working_set);

    assert!(blobs.is_empty());
}

#[test]
fn store_and_retrieve_standard() {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());

    let chain_state = ChainState::<S, Da>::default();
    let chain_state_config = ChainStateConfig {
        current_time: Default::default(),
        genesis_da_height: 0,
        inner_code_commitment: Default::default(),
        outer_code_commitment: Default::default(),
    };
    chain_state
        .genesis_unchecked(
            &chain_state_config,
            &mut KernelWorkingSet::uninitialized(&mut state_checkpoint),
        )
        .unwrap();

    let blob_storage = BlobStorage::<S, Da>::default();

    assert!(blob_storage
        .take_blobs_for_slot_number(1, &mut state_checkpoint)
        .is_empty());
    assert!(blob_storage
        .take_blobs_for_slot_number(2, &mut state_checkpoint)
        .is_empty());
    assert!(blob_storage
        .take_blobs_for_slot_number(3, &mut state_checkpoint)
        .is_empty());
    assert!(blob_storage
        .take_blobs_for_slot_number(4, &mut state_checkpoint)
        .is_empty());

    let sender = MockAddress::from([1u8; 32]);

    let mut batches = Vec::new();
    for i in 1..=5 {
        let txs = vec![RawTx {
            data: vec![i * 3 + 1, i * 3 + 2, i * 3 + 3],
        }];

        let batch = BlobDataWithId {
            data: BlobData::new_batch(txs),
            id: [i; 32],
            from_registered_sequencer: true,
        };
        batches.push((batch, sender));
    }

    let slot_2_batches = &batches[..3];
    let slot_3_batches = &batches[3..4];
    let slot_4_batches = &batches[4..5];

    blob_storage.store_batches(2, &slot_2_batches.to_vec(), &mut state_checkpoint);
    blob_storage.store_batches(3, &slot_3_batches.to_vec(), &mut state_checkpoint);
    blob_storage.store_batches(4, &slot_4_batches.to_vec(), &mut state_checkpoint);

    assert_eq!(
        slot_2_batches,
        blob_storage.take_blobs_for_slot_number(2, &mut state_checkpoint)
    );
    assert!(blob_storage
        .take_blobs_for_slot_number(2, &mut state_checkpoint)
        .is_empty());

    assert_eq!(
        slot_3_batches,
        blob_storage
            .take_blobs_for_slot_number(3, &mut state_checkpoint)
            .as_slice()
    );
    assert!(blob_storage
        .take_blobs_for_slot_number(3, &mut state_checkpoint)
        .is_empty());

    assert_eq!(
        slot_4_batches,
        blob_storage
            .take_blobs_for_slot_number(4, &mut state_checkpoint)
            .as_slice()
    );
    assert!(blob_storage
        .take_blobs_for_slot_number(4, &mut state_checkpoint)
        .is_empty());
}
