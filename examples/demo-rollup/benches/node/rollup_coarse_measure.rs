#![allow(clippy::float_arithmetic)]

#[macro_use]
extern crate prettytable;

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use demo_stf::genesis_config::{create_genesis_config, GenesisPaths};
use demo_stf::runtime::Runtime;
use humantime::format_duration;
use prettytable::Table;
use prometheus::{Histogram, HistogramOpts, Registry};
use sha2::Sha256;
use sov_db::ledger_db::{LedgerDb, SlotCommit};
use sov_db::schema::SchemaBatch;
use sov_kernels::basic::{BasicKernel, BasicKernelGenesisConfig};
use sov_mock_da::{MockBlock, MockBlockHeader, MockDaSpec};
use sov_modules_api::Address;
use sov_modules_stf_blueprint::{GenesisParams, StfBlueprint};
use sov_prover_storage_manager::ProverStorageManager;
use sov_rng_da_service::{RngDaService, RngDaSpec};
use sov_rollup_interface::da::BlockHeaderTrait;
use sov_rollup_interface::services::da::{DaService, SlotData};
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_stf_runner::{from_toml_path, read_json_file, RollupConfig};
use sov_test_utils::{TestSpec, TestStorageSpec};
use tempfile::TempDir;

fn print_times(
    total: Duration,
    apply_block_time: Duration,
    blocks: u64,
    num_txns: u64,
    num_success_txns: u64,
) {
    let mut table = Table::new();

    let total_txns = blocks * num_txns;
    table.add_row(row!["Blocks", format!("{:?}", blocks)]);
    table.add_row(row!["Transactions per block", format!("{:?}", num_txns)]);
    table.add_row(row![
        "Processed transactions (success/total)",
        format!("{:?}/{:?}", num_success_txns, total_txns)
    ]);
    table.add_row(row!["Total", format_duration(total)]);
    table.add_row(row!["Apply block", format_duration(apply_block_time)]);
    let tps = (total_txns as f64) / total.as_secs_f64();
    table.add_row(row!["Transactions per sec (TPS)", format!("{:.1}", tps)]);

    // Print the table to stdout
    table.printstd();
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let registry = Registry::new();
    let h_apply_block = Histogram::with_opts(HistogramOpts::new(
        "block_processing_apply_block",
        "Histogram of block processing - apply blob times",
    ))
    .expect("Failed to create histogram");

    registry
        .register(Box::new(h_apply_block.clone()))
        .expect("Failed to register apply blob histogram");

    let mut end_height: u64 = 10;
    let mut num_success_txns = 0;
    let mut num_txns_per_block = 10000;
    let mut timer_output = true;
    let mut prometheus_output = false;
    if let Ok(val) = env::var("TXNS_PER_BLOCK") {
        num_txns_per_block = val
            .parse()
            .expect("TXNS_PER_BLOCK var should be a +ve number");
    }
    if let Ok(val) = env::var("BLOCKS") {
        end_height = val
            .parse::<u64>()
            .expect("BLOCKS var should be a +ve number")
            + 1;
    }
    if let Ok(_val) = env::var("PROMETHEUS_OUTPUT") {
        prometheus_output = true;
        timer_output = false;
    }
    if let Ok(_val) = env::var("TIMER_OUTPUT") {
        timer_output = true;
    }

    let rollup_config_path = "benches/node/rollup_config.toml".to_string();
    let mut rollup_config: RollupConfig<Address<Sha256>, sov_celestia_adapter::CelestiaConfig> =
        from_toml_path(rollup_config_path)
            .context("Failed to read rollup configuration")
            .unwrap();

    let temp_dir = TempDir::new().expect("Unable to create temporary directory");
    rollup_config.storage.path = PathBuf::from(temp_dir.path());

    let da_service = Arc::new(RngDaService::new());

    let storage_config = sov_state::config::Config {
        path: rollup_config.storage.path.clone(),
    };
    let mut storage_manager =
        ProverStorageManager::<MockDaSpec, TestStorageSpec>::new(storage_config)
            .expect("ProverStorage initialization failed");

    let genesis_block_header = MockBlockHeader::from_height(0);

    let (stf_state, ledger_state) = storage_manager
        .create_state_for(&genesis_block_header)
        .expect("Getting genesis storage failed");

    let ledger_db = LedgerDb::with_cache_db(ledger_state).unwrap();

    let stf = StfBlueprint::<
        TestSpec,
        RngDaSpec,
        Runtime<TestSpec, RngDaSpec>,
        BasicKernel<TestSpec, _>,
    >::new();

    let demo_genesis_config = {
        let stf_tests_conf_dir: &Path = "../test-data/genesis/stf-tests".as_ref();
        let rt_params =
            create_genesis_config::<TestSpec, _>(&GenesisPaths::from_dir(stf_tests_conf_dir))
                .unwrap();

        let chain_state = read_json_file(stf_tests_conf_dir.join("chain_state.json")).unwrap();
        let kernel_params = BasicKernelGenesisConfig { chain_state };
        GenesisParams {
            runtime: rt_params,
            kernel: kernel_params,
        }
    };

    let (mut current_root, stf_state) = stf.init_chain(stf_state, demo_genesis_config);

    storage_manager
        .save_change_set(&genesis_block_header, stf_state, SchemaBatch::new())
        .expect("Saving genesis storage failed");
    storage_manager.finalize(&genesis_block_header).unwrap();

    // data generation
    let mut blobs = vec![];
    let mut blocks = vec![];
    for height in 1..=end_height {
        let filtered_block = MockBlock {
            header: MockBlockHeader::from_height(height),
            validity_cond: Default::default(),
            batch_blobs: Default::default(),
            proof_blobs: Default::default(),
        };
        let relevant_blobs = da_service.extract_relevant_blobs(&filtered_block);
        blocks.push(filtered_block);
        blobs.push(relevant_blobs);
    }

    // Setup. Block h=1 has a single tx that creates the token. Exclude from timers
    let filtered_block = blocks.remove(0);
    let (stf_state, _) = storage_manager
        .create_state_for(filtered_block.header())
        .unwrap();
    let apply_block_result = stf.apply_slot(
        &current_root,
        stf_state,
        Default::default(),
        filtered_block.header(),
        &filtered_block.validity_cond,
        blobs.remove(0).as_iters(),
    );
    current_root = apply_block_result.state_root;

    let mut data_to_commit = SlotCommit::new(filtered_block.clone());
    data_to_commit.add_batch(apply_block_result.batch_receipts[0].clone());
    let ledger_change_set = ledger_db
        .materialize_slot(data_to_commit, current_root.as_ref())
        .unwrap();

    storage_manager
        .save_change_set(
            filtered_block.header(),
            apply_block_result.change_set,
            ledger_change_set,
        )
        .unwrap();
    storage_manager.finalize(filtered_block.header()).unwrap();

    // 3 blocks to finalization
    let fork_length = 3;
    let blocks_num = blocks.len() as u64;
    // Rollup processing. Block h=2 -> end are the transfer transactions. Timers start here
    let total = Instant::now();
    let mut apply_block_time = Duration::new(0, 0);
    for (filtered_block, mut relevant_blobs) in blocks.into_iter().zip(blobs.into_iter()) {
        let (stf_state, _) = storage_manager
            .create_state_for(filtered_block.header())
            .unwrap();
        // We don't need to replace ledgerDb database, because data goes immediately to rocksdb on
        // each finalization, and it reads from there.
        let now = Instant::now();
        let apply_block_result = stf.apply_slot(
            &current_root,
            stf_state,
            Default::default(),
            filtered_block.header(),
            &filtered_block.validity_cond,
            relevant_blobs.as_iters(),
        );
        apply_block_time += now.elapsed();
        h_apply_block.observe(now.elapsed().as_secs_f64());
        current_root = apply_block_result.state_root;

        let filtered_header = filtered_block.header().clone();
        let mut data_to_commit = SlotCommit::new(filtered_block);
        for receipt in apply_block_result.batch_receipts {
            for t in &receipt.tx_receipts {
                if t.receipt.is_successful() {
                    num_success_txns += 1;
                }
            }
            data_to_commit.add_batch(receipt);
        }

        let ledger_changes = ledger_db
            .materialize_slot(data_to_commit, current_root.as_ref())
            .unwrap();

        storage_manager
            .save_change_set(
                &filtered_header,
                apply_block_result.change_set,
                ledger_changes,
            )
            .unwrap();

        if let Some(height_to_finalize) = filtered_header.height().checked_sub(fork_length) {
            // Blocks 0 & 1 has been finalized before
            if height_to_finalize > 1 {
                let header_to_finalize = MockBlockHeader::from_height(height_to_finalize);
                storage_manager.finalize(&header_to_finalize).unwrap();
            }
        }

        ledger_db.send_notifications();
    }

    let total = total.elapsed();
    if timer_output {
        print_times(
            total,
            apply_block_time,
            blocks_num,
            num_txns_per_block,
            num_success_txns,
        );
    }
    if prometheus_output {
        println!("{:#?}", registry.gather());
    }
    Ok(())
}
