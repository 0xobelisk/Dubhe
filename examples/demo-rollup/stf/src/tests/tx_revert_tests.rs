use std::convert::Infallible;

use sov_mock_da::{MockAddress, MockBlock, MockDaSpec, MOCK_SEQUENCER_DA_ADDRESS};
use sov_modules_api::runtime::capabilities::FatalError;
use sov_modules_api::transaction::SequencerReward;
use sov_modules_api::{ApiStateAccessor, Batch, PrivateKey, PublicKey, Spec, StateCheckpoint};
use sov_modules_stf_blueprint::{SkippedReason, StfBlueprint, TxEffect};
use sov_prover_storage_manager::ProverStorageManager;
use sov_rollup_interface::da::RelevantBlobs;
use sov_rollup_interface::services::da::SlotData;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_sequencer_registry::BatchSequencerOutcome;
use sov_state::DefaultStorageSpec;
use sov_test_utils::generators::bank::get_default_token_id;
use sov_test_utils::{has_tx_events, new_test_blob_from_batch, SchemaBatch, TestHasher, TestSpec};

use super::{
    create_genesis_config_for_tests, create_storage_manager_for_tests, read_private_keys,
    RuntimeTest,
};
use crate::runtime::Runtime;
use crate::tests::da_simulation::{
    simulate_da_with_bad_nonce, simulate_da_with_bad_serialization, simulate_da_with_bad_sig,
    simulate_da_with_revert_msg,
};
use crate::tests::StfBlueprintTest;

// Assume there was a proper address and we converted it to bytes already.
const SEQUENCER_DA_ADDRESS: [u8; 32] = [1; 32];

#[test]
fn test_tx_revert() -> Result<(), Infallible> {
    // Test checks:
    //  - Batch is successfully applied even with incorrect txs
    //  - Nonce for bad transactions has increased

    let tempdir = tempfile::tempdir().unwrap();

    let config = create_genesis_config_for_tests();
    let sequencer_rollup_address = config.runtime.sequencer_registry.seq_rollup_address;

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();
    let admin_key = read_private_keys::<TestSpec>().token_deployer.private_key;
    let admin_address: <TestSpec as Spec>::Address = admin_key.to_address();

    let storage = {
        let mut storage_manager = create_storage_manager_for_tests(tempdir.path());
        let stf: StfBlueprintTest = StfBlueprint::new();

        let (stf_state, _) = storage_manager
            .create_state_for(genesis_block.header())
            .unwrap();
        let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
        storage_manager
            .save_change_set(genesis_block.header(), stf_state, SchemaBatch::new())
            .unwrap();

        let txs = simulate_da_with_revert_msg(admin_key.clone());
        let blob = new_test_blob_from_batch(Batch { txs }, &MOCK_SEQUENCER_DA_ADDRESS, [0; 32]);

        let mut relevant_blobs = RelevantBlobs {
            proof_blobs: Default::default(),
            batch_blobs: vec![blob],
        };

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
            "Unexpected outcome: Batch execution should have succeeded",
        );

        let txn_receipts = apply_block_result.batch_receipts[0].tx_receipts.clone();
        // 3 transactions
        // create 1000 tokens
        // transfer 15 tokens
        // transfer 5000 tokens // this should be reverted
        assert!(txn_receipts[0].receipt.is_successful());
        assert!(txn_receipts[1].receipt.is_successful());
        assert!(txn_receipts[2].receipt.is_reverted());

        storage_manager
            .save_change_set(
                block_1.header(),
                apply_block_result.change_set,
                SchemaBatch::new(),
            )
            .unwrap();
        let (storage, _) = storage_manager
            .create_state_after(block_1.header())
            .unwrap();
        storage
    };

    // Checks on storage after execution
    {
        let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
        let mut state = ApiStateAccessor::new(storage);
        let resp = runtime
            .bank
            .balance_of(
                None,
                admin_address,
                get_default_token_id::<TestSpec>(&admin_address),
                &mut state,
            )
            .unwrap();

        assert_eq!(resp.amount, Some(985));

        let resp = runtime
            .sequencer_registry
            .sequencer_address(MockAddress::from(MOCK_SEQUENCER_DA_ADDRESS), &mut state)
            .unwrap();
        // Sequencer is not excluded from the list of allowed!
        assert_eq!(Some(sequencer_rollup_address), resp.address);

        let nonce = runtime
            .nonces
            .nonce(
                &admin_key.pub_key().credential_id::<TestHasher>(),
                &mut state,
            )?
            .unwrap();

        // with 3 transactions, the final nonce should be 3
        // 0 -> 1
        // 1 -> 2
        // 2 -> 3
        // The minter account should have its nonce increased for 3 transactions
        assert_eq!(3, nonce);
    }

    Ok(())
}

#[test]
fn test_tx_bad_signature() -> Result<(), Infallible> {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let config = create_genesis_config_for_tests();

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();
    let admin_key = read_private_keys::<TestSpec>().token_deployer.private_key;
    let storage = {
        let mut storage_manager = create_storage_manager_for_tests(path);
        let stf: StfBlueprintTest = StfBlueprint::new();
        let (stf_state, _) = storage_manager
            .create_state_for(genesis_block.header())
            .unwrap();
        let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
        storage_manager
            .save_change_set(genesis_block.header(), stf_state, SchemaBatch::new())
            .unwrap();

        let txs = simulate_da_with_bad_sig(admin_key.clone());

        let blob = new_test_blob_from_batch(Batch { txs }, &MOCK_SEQUENCER_DA_ADDRESS, [0; 32]);

        let mut relevant_blobs = RelevantBlobs {
            proof_blobs: Default::default(),
            batch_blobs: vec![blob],
        };

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
            BatchSequencerOutcome::Slashed(
                FatalError::SigVerificationFailed(
                    "Signature verification error: Bad signature error: signature error: Verification equation was not satisfied".to_string()
                ),
            ),
            apply_blob_outcome.inner,
            "Unexpected outcome: Stateless verification should have failed due to invalid signature"
        );

        // The batch receipt contains no events.
        assert!(!has_tx_events(&apply_blob_outcome));
        storage_manager
            .save_change_set(
                block_1.header(),
                apply_block_result.change_set,
                SchemaBatch::new(),
            )
            .unwrap();
        let (storage, _) = storage_manager
            .create_state_after(block_1.header())
            .unwrap();
        storage
    };

    {
        let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
        let mut state = ApiStateAccessor::<TestSpec>::new(storage);
        let nonce = runtime
            .nonces
            .nonce(
                &admin_key.pub_key().credential_id::<TestHasher>(),
                &mut state,
            )?
            .unwrap_or_default();

        assert_eq!(0, nonce);
    }

    Ok(())
}

fn get_attester_stake_for_block(
    block: &MockBlock,
    storage_manager: &mut ProverStorageManager<MockDaSpec, DefaultStorageSpec<TestHasher>>,
    stf: &StfBlueprintTest,
) -> Result<u64, Infallible> {
    let (stf_state, _ledger_state) = storage_manager.create_state_for(block.header()).unwrap();

    let mut state: StateCheckpoint<TestSpec> = StateCheckpoint::new(stf_state);
    Ok(stf
        .runtime()
        .sequencer_registry
        .get_sender_balance(&(MOCK_SEQUENCER_DA_ADDRESS.into()), &mut state)?
        .expect("The sequencer should be registered"))
}

/// This test ensures that the sequencer gets penalized for submitting a proof that has a wrong nonce.
#[test]
fn test_tx_bad_nonce() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let config = create_genesis_config_for_tests();
    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();
    let block_2 = block_1.next_mock();
    let admin_key = read_private_keys::<TestSpec>().token_deployer.private_key;
    {
        let mut storage_manager = create_storage_manager_for_tests(path);
        let stf: StfBlueprintTest = StfBlueprint::new();
        let (stf_state, _) = storage_manager
            .create_state_for(genesis_block.header())
            .unwrap();
        let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
        storage_manager
            .save_change_set(genesis_block.header(), stf_state, SchemaBatch::new())
            .unwrap();

        let txs = simulate_da_with_bad_nonce(admin_key);

        let blob = new_test_blob_from_batch(Batch { txs }, &MOCK_SEQUENCER_DA_ADDRESS, [0; 32]);

        let mut relevant_blobs = RelevantBlobs {
            proof_blobs: Default::default(),
            batch_blobs: vec![blob],
        };

        let initial_sequencer_stake =
            get_attester_stake_for_block(&block_1, &mut storage_manager, &stf);

        let (stf_state, _) = storage_manager.create_state_for(block_1.header()).unwrap();

        let apply_block_result = stf.apply_slot(
            &genesis_root,
            stf_state,
            Default::default(),
            &block_1.header,
            &block_1.validity_cond,
            relevant_blobs.as_iters(),
        );

        // When the nonce is not correct, the transaction receipt does not appear in the block
        assert_eq!(1, apply_block_result.batch_receipts.len());
        let tx_receipts = apply_block_result.batch_receipts[0].tx_receipts.clone();
        // Bad nonce means that the transaction has to be reverted
        assert_eq!(
            tx_receipts[0].receipt,
            TxEffect::Skipped(SkippedReason::IncorrectNonce(
                "Tx bad nonce for credential id: 0xfea6ac5b8751120fb62fff67b54d2eac66aef307c7dde1d394dea1e09e43dd44, expected: 0, but found: 18446744073709551615".to_string()
            ))
        );

        // We don't slash the sequencer for a bad nonce, since the nonce change might have
        // happened while the transaction was in-flight. However, we do *penalize* the sequencer
        // in this case.
        // We're asserting that here to track if the logic changes

        // Since the sequencer is penalized, he is rewarded with 0 tokens.
        let sequencer_outcome = apply_block_result.batch_receipts[0].inner.clone();
        assert_eq!(
            sequencer_outcome,
            BatchSequencerOutcome::Rewarded(SequencerReward::ZERO),
            "Unexpected outcome: Batch execution should have succeeded"
        );

        // We can check that the sequencer staked amount went down.
        storage_manager
            .save_change_set(
                block_1.header(),
                apply_block_result.change_set,
                SchemaBatch::new(),
            )
            .expect("Saving the change set should not fail");

        let final_sequencer_stake =
            get_attester_stake_for_block(&block_2, &mut storage_manager, &stf);

        assert!(
            final_sequencer_stake < initial_sequencer_stake,
            "The sequencer stake should have decreased, final_sequencer_stake = {:?}, initial_sequencer_stake = {:?}",
            final_sequencer_stake, initial_sequencer_stake
        );
    }
}

#[test]
fn test_tx_bad_serialization() -> Result<(), Infallible> {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();

    let config = create_genesis_config_for_tests();
    let sequencer_rollup_address = config.runtime.sequencer_registry.seq_rollup_address;

    let genesis_block = MockBlock::default();
    let block_1 = genesis_block.next_mock();
    let mut storage_manager = create_storage_manager_for_tests(path);
    let admin_key = read_private_keys::<TestSpec>().token_deployer.private_key;

    let (genesis_root, sequencer_balance_before) = {
        let stf: StfBlueprintTest = StfBlueprint::new();

        let (stf_state, _) = storage_manager
            .create_state_for(genesis_block.header())
            .unwrap();
        let (genesis_root, stf_state) = stf.init_chain(stf_state, config);
        storage_manager
            .save_change_set(genesis_block.header(), stf_state, SchemaBatch::new())
            .unwrap();

        let balance = {
            let (stf_state, _) = storage_manager
                .create_state_after(genesis_block.header())
                .unwrap();
            let runtime: RuntimeTest = Runtime::default();
            let mut state = StateCheckpoint::<TestSpec>::new(stf_state.clone());

            let coins = runtime.sequencer_registry.get_coins_to_lock(&mut state)?;

            runtime
                .bank
                .get_balance_of(&sequencer_rollup_address, coins.token_id, &mut state)?
                .unwrap()
        };
        (genesis_root, balance)
    };

    let storage = {
        let stf: StfBlueprintTest = StfBlueprint::new();

        let txs = simulate_da_with_bad_serialization(admin_key.clone());
        let blob = new_test_blob_from_batch(Batch { txs }, &MOCK_SEQUENCER_DA_ADDRESS, [0; 32]);

        let mut relevant_blobs = RelevantBlobs {
            proof_blobs: Default::default(),
            batch_blobs: vec![blob],
        };

        let (storage, _) = storage_manager.create_state_for(block_1.header()).unwrap();
        let apply_block_result = stf.apply_slot(
            &genesis_root,
            storage,
            Default::default(),
            &block_1.header,
            &block_1.validity_cond,
            relevant_blobs.as_iters(),
        );

        assert_eq!(1, apply_block_result.batch_receipts.len());
        let apply_blob_outcome = apply_block_result.batch_receipts[0].clone();

        assert!(
            matches!(
                apply_blob_outcome.inner,
                BatchSequencerOutcome::Slashed(FatalError::MessageDecodingFailed(_, _))
            ),
            "Unexpected outcome: Stateless verification should have failed due to invalid signature"
        );

        // The batch receipt contains no events.
        assert!(!has_tx_events(&apply_blob_outcome));
        storage_manager
            .save_change_set(
                block_1.header(),
                apply_block_result.change_set,
                SchemaBatch::new(),
            )
            .unwrap();
        let (storage, _) = storage_manager
            .create_state_after(block_1.header())
            .unwrap();
        storage
    };

    {
        let runtime = &mut Runtime::<TestSpec, MockDaSpec>::default();
        let mut state = ApiStateAccessor::<TestSpec>::new(storage);

        // Sequencer is not in the list of allowed sequencers

        let allowed_sequencer = runtime
            .sequencer_registry
            .sequencer_address(MockAddress::from(SEQUENCER_DA_ADDRESS), &mut state)
            .unwrap();
        assert!(allowed_sequencer.address.is_none());

        // Balance of sequencer is not increased
        let coins = runtime.sequencer_registry.get_coins_to_lock(&mut state)?;
        let sequencer_balance_after = runtime
            .bank
            .get_balance_of(&sequencer_rollup_address, coins.token_id, &mut state)?
            .unwrap();
        assert_eq!(sequencer_balance_before, sequencer_balance_after);
    }

    Ok(())
}
