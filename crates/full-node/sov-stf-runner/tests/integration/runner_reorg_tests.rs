use std::sync::Arc;

use sov_db::ledger_db::LedgerDb;
use sov_mock_da::{
    MockAddress, MockBlob, MockBlock, MockBlockHeader, MockDaService, MockDaSpec, MockValidityCond,
    PlannedFork,
};
use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::{BlobData, RawTx, StateTransitionFunction};
use sov_prover_storage_manager::{ProverStorageManager, SimpleStorageManager};
use sov_rollup_interface::services::da::{DaService, DaServiceWithRetries};
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_state::storage::NativeStorage;
use sov_state::{ArrayWitness, ProverStorage, Storage};
use sov_stf_runner::InitVariant;
use tempfile::TempDir;

use crate::helpers::hash_stf::{HashStf, S};
use crate::helpers::runner_init::initialize_runner;

type MockInitVariant = InitVariant<
    HashStf<MockValidityCond>,
    MockZkVerifier,
    MockZkVerifier,
    DaServiceWithRetries<MockDaService>,
>;

#[tokio::test]
async fn test_simple_reorg_case() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let sequencer_address = MockAddress::new([11u8; 32]);
    let genesis_params = vec![1, 2, 3, 4, 5];

    let main_chain_blobs = vec![
        batch(vec![1, 1, 1, 1]),
        batch(vec![2, 2, 2, 2]),
        batch(vec![3, 3, 3, 3]),
        batch(vec![4, 4, 4, 4]),
    ];
    let fork_blobs = vec![
        batch(vec![13, 13, 13, 13]),
        batch(vec![14, 14, 14, 14]),
        batch(vec![15, 15, 15, 15]),
    ];
    let expected_final_blobs = vec![
        batch(vec![1, 1, 1, 1]),
        batch(vec![2, 2, 2, 2]),
        batch(vec![13, 13, 13, 13]),
        batch(vec![14, 14, 14, 14]),
        batch(vec![15, 15, 15, 15]),
    ];

    let mut da_service = DaServiceWithRetries::new_fast(
        MockDaService::new(sequencer_address)
            .with_finality(4)
            .with_wait_attempts(2),
    );

    let genesis_block = da_service.get_block_at(0).await.unwrap();

    let planned_fork = PlannedFork::new(5, 2, fork_blobs.clone());
    da_service
        .da_service_mut()
        .set_planned_fork(planned_fork)
        .await
        .unwrap();

    let da_service = Arc::new(da_service);
    for data in main_chain_blobs {
        let fee = da_service.estimate_fee(data.len()).await.unwrap();
        da_service.send_transaction(&data, fee).await.unwrap();
    }

    let (expected_state_root, _expected_final_root_hash) =
        get_expected_execution_hash_from(&genesis_params, expected_final_blobs);

    let (_expected_committed_state_root, expected_committed_root_hash) =
        get_expected_execution_hash_from(&genesis_params, vec![batch(vec![1, 1, 1, 1])]);

    let init_variant: MockInitVariant = InitVariant::Genesis {
        block: genesis_block,
        genesis_params,
    };

    check_runner(da_service, &tmp_dir, init_variant, expected_state_root).await;

    let committed_root_hash = get_saved_root_hash(tmp_dir.path()).unwrap().unwrap();
    assert_eq!(expected_committed_root_hash.unwrap(), committed_root_hash);
}

#[tokio::test]
#[ignore = "TBD"]
async fn test_several_reorgs() {}

#[tokio::test]
async fn test_instant_finality_data_stored() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let sequencer_address = MockAddress::new([11u8; 32]);
    let genesis_params = vec![1, 2, 3, 4, 5];

    let da_service = Arc::new(DaServiceWithRetries::new_fast(
        MockDaService::new(sequencer_address).with_wait_attempts(2),
    ));

    let genesis_block = da_service.get_block_at(0).await.unwrap();
    let fee = da_service.estimate_fee(4).await.unwrap();

    let serialized_blob_1 = batch(vec![1, 1, 1, 1]);

    da_service
        .send_transaction(&serialized_blob_1, fee)
        .await
        .unwrap();

    let serialized_blob_2 = batch(vec![2, 2, 2, 2]);

    da_service
        .send_transaction(&serialized_blob_2, fee)
        .await
        .unwrap();

    let serialized_blob_3 = batch(vec![3, 3, 3, 3]);

    da_service
        .send_transaction(&serialized_blob_3, fee)
        .await
        .unwrap();

    let (expected_state_root, expected_root_hash) = get_expected_execution_hash_from(
        &genesis_params,
        vec![serialized_blob_1, serialized_blob_2, serialized_blob_3],
    );

    let init_variant: MockInitVariant = InitVariant::Genesis {
        block: genesis_block,
        genesis_params,
    };

    check_runner(da_service, &tmp_dir, init_variant, expected_state_root).await;

    let saved_root_hash = get_saved_root_hash(tmp_dir.path()).unwrap().unwrap();
    assert_eq!(expected_root_hash.unwrap(), saved_root_hash);
}

async fn check_runner(
    da_service: Arc<DaServiceWithRetries<MockDaService>>,
    tmpdir: &TempDir,
    init_variant: MockInitVariant,
    expected_state_root: [u8; 32],
) {
    let (mut runner, _) = initialize_runner(da_service, tmpdir.path(), init_variant, 1, None);
    let before = *runner.get_state_root();
    let end = runner.run_in_process().await;
    assert!(end.is_err());
    let after = *runner.get_state_root();

    assert_ne!(before, after);
    assert_eq!(expected_state_root, after);
}

fn get_saved_root_hash(
    path: &std::path::Path,
) -> anyhow::Result<Option<<ProverStorage<S> as Storage>::Root>> {
    let storage_config = sov_state::config::Config {
        path: path.to_path_buf(),
    };
    let mut storage_manager = ProverStorageManager::<MockDaSpec, S>::new(storage_config).unwrap();
    let mock_block_header = MockBlockHeader::from_height(1000000);
    let (stf_state, ledger_state) = storage_manager.create_state_for(&mock_block_header)?;

    let ledger_db = LedgerDb::with_cache_db(ledger_state).unwrap();

    ledger_db
        .get_head_slot()?
        .map(|(number, _)| stf_state.get_root_hash(number.0))
        .transpose()
}

fn get_expected_execution_hash_from(
    genesis_params: &[u8],
    blobs: Vec<Vec<u8>>,
) -> ([u8; 32], Option<<ProverStorage<S> as Storage>::Root>) {
    let blocks: Vec<MockBlock> = blobs
        .into_iter()
        .enumerate()
        .map(|(idx, blob)| MockBlock {
            header: MockBlockHeader::from_height((idx + 1) as u64),
            validity_cond: MockValidityCond::default(),
            batch_blobs: vec![MockBlob::new(
                blob,
                MockAddress::new([11u8; 32]),
                [idx as u8; 32],
            )],
            proof_blobs: Default::default(),
        })
        .collect();

    get_result_from_blocks(genesis_params, &blocks[..])
}

// Returns final data hash and root hash
fn get_result_from_blocks(
    genesis_params: &[u8],
    blocks: &[MockBlock],
) -> ([u8; 32], Option<<ProverStorage<S> as Storage>::Root>) {
    let tmpdir = tempfile::tempdir().unwrap();

    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let storage = storage_manager.create_storage();

    let stf = HashStf::<MockValidityCond>::new();

    let (genesis_state_root, change_set) =
        <HashStf<MockValidityCond> as StateTransitionFunction<
            MockZkVerifier,
            MockZkVerifier,
            MockDaSpec,
        >>::init_chain(&stf, storage, genesis_params.to_vec());
    storage_manager.commit(change_set);

    let mut state_root = genesis_state_root;

    let l = blocks.len();

    for block in blocks {
        let mut relevant_blobs = block.as_relevant_blobs();

        let storage = storage_manager.create_storage();
        let result = <HashStf<MockValidityCond> as StateTransitionFunction<
            MockZkVerifier,
            MockZkVerifier,
            MockDaSpec,
        >>::apply_slot::<&mut [MockBlob]>(
            &stf,
            &state_root,
            storage,
            ArrayWitness::default(),
            &block.header,
            &block.validity_cond,
            relevant_blobs.as_iters(),
        );

        state_root = result.state_root;
        storage_manager.commit(result.change_set);
    }

    let storage = storage_manager.create_storage();
    let root_hash = storage.get_root_hash(l as u64).ok();
    (state_root, root_hash)
}

fn batch(data: Vec<u8>) -> Vec<u8> {
    borsh::to_vec(&BlobData::new_batch(vec![RawTx { data }])).unwrap()
}
