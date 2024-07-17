use sov_db::ledger_db::{LedgerDb, SlotCommit};
use sov_mock_da::{MockBlob, MockBlock};
use sov_mock_zkvm::MockZkvm;
use sov_rollup_interface::rpc::LedgerStateProvider;
use sov_rollup_interface::zk::aggregated_proof::{
    AggregatedProof, AggregatedProofPublicData, CodeCommitment, SerializedAggregatedProof,
};
use sov_test_utils::ledger_db::sov_ledger_json_client::types::IntOrHash;
use sov_test_utils::ledger_db::{
    LedgerTestService, LedgerTestServiceData, SimpleLedgerStorageManager,
};

#[tokio::test(flavor = "multi_thread")]
async fn get_filtered_slot_events() {
    let ledger_service = LedgerTestService::new(LedgerTestServiceData::Simple)
        .await
        .unwrap();
    let client = ledger_service.axum_client;

    let events = &client
        .get_slot_filtered_events(&IntOrHash::Variant0(0), None)
        .await
        .unwrap()
        .data;

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].key, "foo");

    let events = &client
        .get_slot_filtered_events(&IntOrHash::Variant0(0), Some("bar"))
        .await
        .unwrap()
        .data;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].key, "bar");

    let events = &client
        .get_slot_filtered_events(&IntOrHash::Variant0(0), Some("")) // empty prefix
        .await
        .unwrap()
        .data;

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].key, "foo");
    assert_eq!(events[1].key, "bar");
}

#[test]
fn test_slot_subscription() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleLedgerStorageManager::new(temp_dir.path());
    let ledger_storage = storage_manager.create_ledger_storage();
    let ledger_db = LedgerDb::with_cache_db(ledger_storage).unwrap();

    let mut rx = ledger_db.subscribe_slots();
    let _ = ledger_db
        .materialize_slot(
            SlotCommit::<_, MockBlob, ()>::new(MockBlock::default()),
            b"state-root",
        )
        .unwrap();
    ledger_db.send_notifications();

    assert_eq!(rx.blocking_recv().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_save_aggregated_proof() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleLedgerStorageManager::new(temp_dir.path());
    let ledger_storage = storage_manager.create_ledger_storage();
    let ledger_db = LedgerDb::with_cache_db(ledger_storage).unwrap();
    let _rx = ledger_db.subscribe_proof_saved();

    let proof_from_db = ledger_db.get_latest_aggregated_proof().await.unwrap();
    assert_eq!(None, proof_from_db);

    for i in 0..10 {
        let public_data = AggregatedProofPublicData {
            validity_conditions: vec![],
            initial_slot_number: i as u64,
            final_slot_number: i as u64,
            genesis_state_root: vec![1],
            initial_state_root: vec![i],
            final_state_root: vec![i + 1],
            initial_slot_hash: vec![i + 2],
            final_slot_hash: vec![i + 3],
            code_commitment: CodeCommitment::default(),
            rewarded_addresses: Default::default(),
        };

        let raw_aggregated_proof = MockZkvm::create_serialized_proof(true, public_data.clone());

        let agg_proof = AggregatedProof::new(
            SerializedAggregatedProof {
                raw_aggregated_proof,
            },
            public_data.clone(),
        );

        let proof_change_set = ledger_db.materialize_aggregated_proof(agg_proof).unwrap();
        storage_manager.commit(proof_change_set);

        let proof_from_db = ledger_db
            .get_latest_aggregated_proof()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&public_data, proof_from_db.proof.public_data());
    }
}
