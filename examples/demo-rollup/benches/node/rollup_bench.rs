#![allow(clippy::float_arithmetic)]

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use criterion::{criterion_group, criterion_main, Criterion};
use demo_stf::genesis_config::{create_genesis_config, GenesisPaths};
use demo_stf::runtime::Runtime;
use sha2::Sha256;
use sov_db::ledger_db::{LedgerDb, SlotCommit};
use sov_db::schema::SchemaBatch;
use sov_kernels::basic::{BasicKernel, BasicKernelGenesisConfig};
use sov_mock_da::{MockBlock, MockBlockHeader, MockDaSpec};
use sov_modules_api::Address;
use sov_modules_stf_blueprint::{GenesisParams, StfBlueprint};
use sov_prover_storage_manager::ProverStorageManager;
use sov_rng_da_service::{RngDaService, RngDaSpec};
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_stf_runner::{from_toml_path, read_json_file, RollupConfig};
use sov_test_utils::TestStorageSpec;
use tempfile::TempDir;

type BenchSpec = sov_test_utils::TestSpec;

fn rollup_bench(_bench: &mut Criterion) {
    let genesis_height: u64 = 0u64;
    let mut end_height: u64 = 100u64;
    if let Ok(val) = env::var("BLOCKS") {
        end_height = val.parse().expect("BLOCKS var should be a +ve number");
    }

    let mut c = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(20));
    let rollup_config_path = "benches/node/rollup_config.toml".to_string();
    let mut rollup_config: RollupConfig<Address<Sha256>, sov_celestia_adapter::CelestiaConfig> =
        from_toml_path(rollup_config_path)
            .context("Failed to read rollup configuration")
            .unwrap();

    let temp_dir = TempDir::new().expect("Unable to create temporary directory");
    rollup_config.storage.path = PathBuf::from(temp_dir.path());

    let da_service = Arc::new(RngDaService::new());

    let storage_config = sov_state::config::Config {
        path: rollup_config.storage.path,
    };
    let mut storage_manager =
        ProverStorageManager::<MockDaSpec, TestStorageSpec>::new(storage_config)
            .expect("ProverStorage initialization failed");
    let block_0 = MockBlockHeader::from_height(0);
    let (stf_state, ledger_state) = storage_manager
        .create_state_for(&block_0)
        .expect("Getting genesis storage failed");

    let ledger_db = LedgerDb::with_cache_db(ledger_state).unwrap();

    let stf = StfBlueprint::<
        BenchSpec,
        RngDaSpec,
        Runtime<BenchSpec, RngDaSpec>,
        BasicKernel<BenchSpec, _>,
    >::new();

    let demo_genesis_config = {
        let tests_path: &Path = "../../test-data/genesis/integration-tests".as_ref();
        let rt_params =
            create_genesis_config::<BenchSpec, _>(&GenesisPaths::from_dir(tests_path)).unwrap();

        let chain_state = read_json_file(tests_path.join("chain_state.json")).unwrap();
        let kernel_params = BasicKernelGenesisConfig { chain_state };
        GenesisParams {
            runtime: rt_params,
            kernel: kernel_params,
        }
    };

    let (mut current_root, stf_change_set) = stf.init_chain(stf_state, demo_genesis_config);

    storage_manager
        .save_change_set(&block_0, stf_change_set, SchemaBatch::new())
        .unwrap();

    // data generation
    let mut blobs = vec![];
    let mut blocks = vec![];
    for height in genesis_height..end_height {
        let filtered_block = MockBlock {
            header: MockBlockHeader::from_height(height),
            validity_cond: Default::default(),
            batch_blobs: Default::default(),
            proof_blobs: Default::default(),
        };
        blocks.push(filtered_block.clone());

        let blob_txs = da_service.extract_relevant_blobs(&filtered_block);
        blobs.push(blob_txs.clone());
    }

    let (stf_storage, _) = storage_manager.create_state_after(&block_0).unwrap();
    let mut height = 0u64;
    c.bench_function("rollup main loop", |b| {
        b.iter(|| {
            let filtered_block = &blocks[height as usize];

            let mut data_to_commit = SlotCommit::new(filtered_block.clone());
            let apply_block_result = stf.apply_slot(
                &current_root,
                stf_storage.clone(),
                Default::default(),
                &filtered_block.header,
                &filtered_block.validity_cond,
                blobs[height as usize].as_iters(),
            );
            current_root = apply_block_result.state_root;
            for receipts in apply_block_result.batch_receipts {
                data_to_commit.add_batch(receipts);
            }

            // Throwing away, the same way as with
            let _change_set = ledger_db
                .materialize_slot(data_to_commit, current_root.as_ref())
                .unwrap();
            height += 1;
        });
    });
}

criterion_group!(benches, rollup_bench);
criterion_main!(benches);
