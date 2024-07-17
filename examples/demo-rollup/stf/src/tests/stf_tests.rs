use std::convert::Infallible;
use std::vec;

use sov_mock_da::{MockAddress, MockBlock, MockDaSpec, MOCK_SEQUENCER_DA_ADDRESS};
use sov_modules_api::transaction::SequencerReward;
use sov_modules_api::{ApiStateAccessor, Batch, Spec};
use sov_modules_stf_blueprint::{StfBlueprint, TxEffect};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_rollup_interface::da::RelevantBlobs;
use sov_rollup_interface::services::da::SlotData;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_sequencer_registry::BatchSequencerOutcome;
use sov_test_utils::generators::bank::get_default_token_id;
use sov_test_utils::{has_tx_events, new_test_blob_from_batch, SchemaBatch, TestSpec};

use super::da_simulation::simulate_da_with_multiple_direct_registration_msg;
use crate::runtime::Runtime;
use crate::tests::da_simulation::{
    simulate_da, simulate_da_with_incorrect_direct_registration_msg,
};
use crate::tests::{
    create_genesis_config_for_tests, create_storage_manager_for_tests, read_private_keys,
    StfBlueprintTest, S,
};

#[test]
fn test_demo_values_in_db() -> Result<(), Infallible> {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let mut storage_manager = create_storage_manager_for_tests(path);
    let config = create_genesis_config_for_tests();

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();

    let admin_key_and_address = read_private_keys::<TestSpec>().tx_signer;
    let admin_address: <TestSpec as Spec>::Address = admin_key_and_address.address;
    let admin_private_key = admin_key_and_address.private_key;

    let last_block = {
        let stf: StfBlueprintTest = StfBlueprint::new();
        let (stf_state, _) = storage_manager
            .create_state_for(genesis_block.header())
            .unwrap();
        let (genesis_root, stf_change_set) = stf.init_chain(stf_state, config);
        storage_manager
            .save_change_set(genesis_block.header(), stf_change_set, SchemaBatch::new())
            .unwrap();

        let txs = simulate_da(admin_private_key);
        let blob = new_test_blob_from_batch(Batch { txs }, &MOCK_SEQUENCER_DA_ADDRESS, [0; 32]);

        let mut relevant_blobs = RelevantBlobs {
            proof_blobs: Default::default(),
            batch_blobs: vec![blob],
        };

        let (stf_state, _) = storage_manager.create_state_for(block_1.header()).unwrap();

        let result = stf.apply_slot(
            &genesis_root,
            stf_state,
            Default::default(),
            &block_1.header,
            &block_1.validity_cond,
            relevant_blobs.as_iters(),
        );
        assert_eq!(1, result.batch_receipts.len());
        // 2 transactions from value setter
        // 2 transactions from bank
        assert_eq!(4, result.batch_receipts[0].tx_receipts.len());

        let apply_blob_outcome = result.batch_receipts[0].clone();
        assert_eq!(
            BatchSequencerOutcome::Rewarded(SequencerReward::ZERO),
            apply_blob_outcome.inner,
            "Sequencer execution should have succeeded but failed "
        );

        assert!(has_tx_events(&apply_blob_outcome),);
        storage_manager
            .save_change_set(block_1.header(), result.change_set, SchemaBatch::new())
            .unwrap();
        block_1
    };

    // Generate a new storage instance after dumping data to the db.
    {
        let next_block = last_block.next_mock();
        let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
        let (stf_state, _ledger_state) = storage_manager
            .create_state_for(next_block.header())
            .unwrap();
        let mut state = ApiStateAccessor::new(stf_state);
        let resp = runtime
            .bank
            .supply_of(None, get_default_token_id::<S>(&admin_address), &mut state)
            .unwrap();
        assert_eq!(resp, sov_bank::TotalSupplyResponse { amount: Some(1000) });

        assert_eq!(runtime.value_setter.value.get(&mut state)?, Some(33));
    }

    Ok(())
}

#[test]
fn test_demo_values_in_cache() -> Result<(), Infallible> {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let mut storage_manager = create_storage_manager_for_tests(path);

    let stf: StfBlueprintTest = StfBlueprint::new();

    let config = create_genesis_config_for_tests();

    let genesis_block = MockBlock::default();
    let (stf_state, _) = storage_manager
        .create_state_for(genesis_block.header())
        .unwrap();
    let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
    storage_manager
        .save_change_set(genesis_block.header(), stf_state, SchemaBatch::new())
        .unwrap();

    let admin_private_key_and_address = read_private_keys::<TestSpec>().tx_signer;
    let admin_private_key = admin_private_key_and_address.private_key;
    let admin_address: <S as Spec>::Address = admin_private_key_and_address.address;

    let txs = simulate_da(admin_private_key);

    let blob = new_test_blob_from_batch(Batch { txs }, &MOCK_SEQUENCER_DA_ADDRESS, [0; 32]);

    let mut relevant_blobs = RelevantBlobs {
        proof_blobs: Default::default(),
        batch_blobs: vec![blob],
    };

    let block_1 = genesis_block.next_mock();
    let (stf_state, _) = storage_manager.create_state_for(block_1.header()).unwrap();

    let apply_block_result = stf.apply_slot(
        &genesis_root,
        stf_state,
        Default::default(),
        &block_1.header,
        &block_1.validity_cond,
        relevant_blobs.as_iters(),
    );

    assert_eq!(1, apply_block_result.batch_receipts.len());
    let apply_blob_outcome = apply_block_result.batch_receipts[0].clone();

    assert_eq!(
        BatchSequencerOutcome::Rewarded(SequencerReward::ZERO),
        apply_blob_outcome.inner,
        "Sequencer execution should have succeeded but failed"
    );

    assert!(has_tx_events(&apply_blob_outcome),);

    let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();

    storage_manager
        .save_change_set(
            block_1.header(),
            apply_block_result.change_set,
            SchemaBatch::new(),
        )
        .unwrap();

    let (stf_storage, _) = storage_manager
        .create_state_after(block_1.header())
        .unwrap();

    let mut state = ApiStateAccessor::new(stf_storage);

    let resp = runtime
        .bank
        .supply_of(None, get_default_token_id::<S>(&admin_address), &mut state)
        .unwrap();
    assert_eq!(resp, sov_bank::TotalSupplyResponse { amount: Some(1000) });

    assert_eq!(runtime.value_setter.value.get(&mut state)?, Some(33));

    Ok(())
}

// Ensure 1 sequencer be registered per batch
// This test has 2 batches each submitted by unregistered sequencers, given they are in different
// batches then both unregistered sequencers should be registered
#[test]
fn test_multiple_batches_registering_unregistered_sequencers_allows_both_to_register() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let mut config = create_genesis_config_for_tests();
    config.runtime.sequencer_registry.is_preferred_sequencer = false;

    let mut storage_manager = SimpleStorageManager::new(path);
    let stf: StfBlueprintTest = StfBlueprint::new();
    let stf_state = storage_manager.create_storage();
    let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
    storage_manager.commit(stf_state);

    let direct_sequencer: [u8; 32] = [121; 32];
    let other_sequencer: [u8; 32] = [86; 32];

    let private_key = read_private_keys::<TestSpec>().tx_signer.private_key;
    let mut txs = simulate_da_with_multiple_direct_registration_msg(
        vec![direct_sequencer.to_vec(), other_sequencer.to_vec()],
        private_key,
    );

    let blob1 = new_test_blob_from_batch(
        Batch {
            txs: vec![txs.remove(0)],
        },
        &direct_sequencer,
        [0; 32],
    );
    let blob2 = new_test_blob_from_batch(
        Batch {
            txs: vec![txs.remove(0)],
        },
        &other_sequencer,
        [1; 32],
    );

    let mut relevant_blobs = RelevantBlobs {
        proof_blobs: Default::default(),
        batch_blobs: vec![blob1, blob2],
    };

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();
    let stf_state = storage_manager.create_storage();

    let apply_block_result = stf.apply_slot(
        &genesis_root,
        stf_state,
        Default::default(),
        &block_1.header,
        &block_1.validity_cond,
        relevant_blobs.as_iters(),
    );

    assert_eq!(2, apply_block_result.batch_receipts.len());
    for batch_receipt in apply_block_result.batch_receipts.iter() {
        assert_eq!(batch_receipt.inner, BatchSequencerOutcome::NotRewardable);
        let tx_receipt = &batch_receipt.tx_receipts;

        assert_eq!(1, tx_receipt.len());
        assert_eq!(tx_receipt[0].receipt, TxEffect::Successful(()));
    }

    let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
    storage_manager.commit(apply_block_result.change_set);

    let stf_storage = storage_manager.create_storage();
    let mut state = ApiStateAccessor::<TestSpec>::new(stf_storage);
    let successful_reg = runtime
        .sequencer_registry
        .is_registered_sequencer(&direct_sequencer.into(), &mut state)
        .unwrap();

    assert!(successful_reg);

    let other_seq = runtime
        .sequencer_registry
        .is_registered_sequencer(&other_sequencer.into(), &mut state)
        .unwrap();

    assert!(other_seq);
}

#[test]
fn test_unregistered_sequencer_registration_is_limited_to_one_per_batch() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let mut config = create_genesis_config_for_tests();
    config.runtime.sequencer_registry.is_preferred_sequencer = false;

    let mut storage_manager = SimpleStorageManager::new(path);
    let stf: StfBlueprintTest = StfBlueprint::new();
    let stf_state = storage_manager.create_storage();
    let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
    storage_manager.commit(stf_state);

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();

    let direct_sequencer: [u8; 32] = [121; 32];
    let other_sequencer: [u8; 32] = [86; 32];

    let private_key = read_private_keys::<TestSpec>().tx_signer.private_key;
    let txs = simulate_da_with_multiple_direct_registration_msg(
        vec![direct_sequencer.to_vec(), other_sequencer.to_vec()],
        private_key,
    );

    // ensure there's more than 1 tx, later we check there's only 1 receipt indicating only 1 tx
    // was processed - the rest of the txs are dropped by blob-storage / kernel
    assert!(txs.len() > 1);

    let blob = new_test_blob_from_batch(Batch { txs }, &direct_sequencer, [0; 32]);

    let mut relevant_blobs = RelevantBlobs {
        proof_blobs: Default::default(),
        batch_blobs: vec![blob],
    };

    let apply_block_result = stf.apply_slot(
        &genesis_root,
        storage_manager.create_storage(),
        Default::default(),
        &block_1.header,
        &block_1.validity_cond,
        relevant_blobs.as_iters(),
    );

    let batch_receipt = &apply_block_result.batch_receipts[0];
    assert_eq!(batch_receipt.inner, BatchSequencerOutcome::NotRewardable);
    let tx_receipts = &batch_receipt.tx_receipts;
    assert_eq!(1, tx_receipts.len());
    assert_eq!(tx_receipts[0].receipt, TxEffect::Successful(()));

    let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
    storage_manager.commit(apply_block_result.change_set);

    let mut state = ApiStateAccessor::<TestSpec>::new(storage_manager.create_storage());
    let successful_reg = runtime
        .sequencer_registry
        .is_registered_sequencer(&direct_sequencer.into(), &mut state)
        .unwrap();

    assert!(successful_reg);

    let other_seq = runtime
        .sequencer_registry
        .is_registered_sequencer(&other_sequencer.into(), &mut state)
        .unwrap();

    assert!(!other_seq);
}

#[test]
fn test_unregistered_sequencer_registration_incorrect_call_message() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let mut config = create_genesis_config_for_tests();
    config.runtime.sequencer_registry.is_preferred_sequencer = false;

    let mut storage_manager = SimpleStorageManager::new(path);
    let stf: StfBlueprintTest = StfBlueprint::new();
    let stf_state = storage_manager.create_storage();
    let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
    storage_manager.commit(stf_state);

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();

    let some_sequencer: [u8; 32] = [121; 32];

    let private_key = read_private_keys::<TestSpec>().tx_signer.private_key;
    let txs = simulate_da_with_incorrect_direct_registration_msg(private_key);
    let blob = new_test_blob_from_batch(Batch { txs }, &some_sequencer, [0; 32]);

    let mut relevant_blobs = RelevantBlobs {
        proof_blobs: Default::default(),
        batch_blobs: vec![blob],
    };

    let apply_block_result = stf.apply_slot(
        &genesis_root,
        storage_manager.create_storage(),
        Default::default(),
        &block_1.header,
        &block_1.validity_cond,
        relevant_blobs.as_iters(),
    );

    assert_eq!(1, apply_block_result.batch_receipts.len());
    let receipt = &apply_block_result.batch_receipts[0];
    assert_eq!(
        receipt.inner,
        BatchSequencerOutcome::Ignored(
            "The runtime call included in the transaction was invalid.".to_string()
        )
    );

    let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
    storage_manager.commit(apply_block_result.change_set);

    let mut state = ApiStateAccessor::<TestSpec>::new(storage_manager.create_storage());
    let registered = runtime
        .sequencer_registry
        .is_registered_sequencer(&MockAddress::new(some_sequencer), &mut state)
        .unwrap();

    assert!(!registered);
}

// Ensure that if there's a valid register call message it must be the first tx in the batch
// If it is not then the batch should be ignored because we only check the first tx
#[test]
fn test_unregistered_sequencer_first_batch_tx_must_be_register_call_message() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let mut config = create_genesis_config_for_tests();
    config.runtime.sequencer_registry.is_preferred_sequencer = false;

    let mut storage_manager = SimpleStorageManager::new(path);
    let stf: StfBlueprintTest = StfBlueprint::new();
    let stf_state = storage_manager.create_storage();
    let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
    storage_manager.commit(stf_state);

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();

    let some_sequencer: [u8; 32] = [121; 32];

    let private_key = read_private_keys::<TestSpec>().tx_signer.private_key;
    let mut register_tx = simulate_da_with_multiple_direct_registration_msg(
        vec![some_sequencer.to_vec()],
        private_key.clone(),
    );
    let mut incorrect_tx = simulate_da_with_incorrect_direct_registration_msg(private_key);
    let blob = new_test_blob_from_batch(
        Batch {
            txs: vec![
                incorrect_tx.remove(0).clone(),
                register_tx.remove(0).clone(),
            ],
        },
        &some_sequencer,
        [0; 32],
    );

    let mut relevant_blobs = RelevantBlobs {
        proof_blobs: Default::default(),
        batch_blobs: vec![blob],
    };

    let apply_block_result = stf.apply_slot(
        &genesis_root,
        storage_manager.create_storage(),
        Default::default(),
        &block_1.header,
        &block_1.validity_cond,
        relevant_blobs.as_iters(),
    );

    assert_eq!(1, apply_block_result.batch_receipts.len());
    let receipt = &apply_block_result.batch_receipts[0];
    assert_eq!(
        receipt.inner,
        BatchSequencerOutcome::Ignored(
            "The runtime call included in the transaction was invalid.".to_string()
        )
    );

    let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
    storage_manager.commit(apply_block_result.change_set);

    let mut state = ApiStateAccessor::<TestSpec>::new(storage_manager.create_storage());
    let registered = runtime
        .sequencer_registry
        .is_registered_sequencer(&MockAddress::new(some_sequencer), &mut state)
        .unwrap();

    assert!(!registered);
}

#[test]
fn test_unregistered_sequencer_batches_are_limited_to_the_configured_amount_per_slot() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let mut config = create_genesis_config_for_tests();
    config.runtime.sequencer_registry.is_preferred_sequencer = false;

    let mut storage_manager = SimpleStorageManager::new(path);
    let stf: StfBlueprintTest = StfBlueprint::new();
    let stf_state = storage_manager.create_storage();
    let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
    storage_manager.commit(stf_state);

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();

    let some_sequencer: [u8; 32] = [121; 32];
    let another_unregistered_sequencer: [u8; 32] = [86; 32];
    // specified in constants config file: "constants.testing.toml", `UNREGISTERED_BLOBS_PER_SLOT`
    let unregistered_blobs_per_slot = 5;
    let mut blobs = vec![];

    let private_key = read_private_keys::<TestSpec>().tx_signer.private_key;
    let register_tx = simulate_da_with_multiple_direct_registration_msg(
        vec![some_sequencer.to_vec()],
        private_key.clone(),
    );

    blobs.push(new_test_blob_from_batch(
        Batch { txs: register_tx },
        &some_sequencer,
        [0; 32],
    ));

    // fill the unregistered blobs per slot quota with invalid messages
    for _ in 0..unregistered_blobs_per_slot {
        let txs = simulate_da_with_incorrect_direct_registration_msg(private_key.clone());
        let blob = new_test_blob_from_batch(Batch { txs }, &some_sequencer, [0; 32]);
        blobs.push(blob);
    }

    // ensure we have too many blobs
    assert!(blobs.len() > unregistered_blobs_per_slot);

    // this one is outside the limit of allowed unregistered blobs
    // the sequencer should not be registered and this blob should not have been executed
    let register_tx2 = simulate_da_with_multiple_direct_registration_msg(
        vec![another_unregistered_sequencer.to_vec()],
        private_key.clone(),
    );

    blobs.push(new_test_blob_from_batch(
        Batch { txs: register_tx2 },
        &some_sequencer,
        [0; 32],
    ));

    let mut relevant_blobs = RelevantBlobs {
        proof_blobs: Default::default(),
        batch_blobs: blobs,
    };

    let apply_block_result = stf.apply_slot(
        &genesis_root,
        storage_manager.create_storage(),
        Default::default(),
        &block_1.header,
        &block_1.validity_cond,
        relevant_blobs.as_iters(),
    );

    assert_eq!(
        unregistered_blobs_per_slot,
        apply_block_result.batch_receipts.len()
    );
    // check the first blob, that contained a valid register tx
    let first_registered_receipt = &apply_block_result.batch_receipts[0];
    assert_eq!(
        first_registered_receipt.inner,
        BatchSequencerOutcome::NotRewardable
    );

    // ensure the filler blobs have the right outcome
    for i in 1..unregistered_blobs_per_slot {
        let receipt = &apply_block_result.batch_receipts[i];
        assert_eq!(
            receipt.inner,
            BatchSequencerOutcome::Ignored(
                "The runtime call included in the transaction was invalid.".to_string()
            )
        );
    }

    let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
    storage_manager.commit(apply_block_result.change_set);

    // unregistered sequencer tx in the first blob was successfully applied
    let mut state = ApiStateAccessor::<TestSpec>::new(storage_manager.create_storage());
    let registered = runtime
        .sequencer_registry
        .is_registered_sequencer(&MockAddress::new(some_sequencer), &mut state)
        .unwrap();

    assert!(registered);

    // unregistered sequencer tx in the blob that fell outside the allowed quota was not applied
    let excessive_blob_sequencer = runtime
        .sequencer_registry
        .is_registered_sequencer(
            &MockAddress::new(another_unregistered_sequencer),
            &mut state,
        )
        .unwrap();

    assert!(!excessive_blob_sequencer);
}
