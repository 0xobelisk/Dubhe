use std::path::Path;

use demo_stf::genesis_config::{create_genesis_config, GenesisPaths};
use demo_stf::runtime::Runtime;
use risc0::MOCK_DA_ELF;
use sov_db::schema::SchemaBatch;
use sov_kernels::basic::{BasicKernel, BasicKernelGenesisConfig};
use sov_mock_da::{MockAddress, MockBlock, MockDaService, MockDaSpec};
use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::execution_mode::WitnessGeneration;
use sov_modules_api::SlotData;
use sov_modules_stf_blueprint::{GenesisParams, StfBlueprint};
use sov_prover_storage_manager::ProverStorageManager;
use sov_risc0_adapter::host::Risc0Host;
use sov_risc0_adapter::Risc0Verifier;
use sov_rollup_interface::da::BlockHeaderTrait;
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_rollup_interface::zk::{
    StateTransitionWitness, StateTransitionWitnessWithAddress, ZkvmHost,
};
use sov_stf_runner::read_json_file;
use sov_test_utils::TestStorageSpec;
use tempfile::TempDir;

use crate::prover::datagen::get_blocks_from_da;

type DefaultSpec = sov_modules_api::default_spec::DefaultSpec<
    sov_risc0_adapter::Risc0Verifier,
    sov_mock_zkvm::MockZkVerifier,
    WitnessGeneration,
>;

mod datagen;

const DEFAULT_GENESIS_CONFIG_DIR: &str = "../test-data/genesis/integration-tests";

type TestSTF<'a> = StfBlueprint<
    DefaultSpec,
    MockDaSpec,
    Runtime<DefaultSpec, MockDaSpec>,
    BasicKernel<DefaultSpec, MockDaSpec>,
>;

/// This test reproduces the proof generation process for the rollup used in benchmarks.
#[tokio::test]
#[cfg_attr(skip_guest_build, ignore)]
async fn test_proof_generation() {
    sov_test_utils::logging::initialize_logging();
    let genesis_conf_dir = String::from(DEFAULT_GENESIS_CONFIG_DIR);

    let temp_dir = TempDir::new().expect("Unable to create temporary directory");
    tracing::info!("Creating temp dir at {}", temp_dir.path().display());
    let da_service = MockDaService::new(MockAddress::default());
    let storage_config = sov_state::config::Config {
        path: temp_dir.path().into(),
    };

    let mut storage_manager =
        ProverStorageManager::<MockDaSpec, TestStorageSpec>::new(storage_config)
            .expect("ProverStorageManager initialization has failed");
    let stf = TestSTF::new();

    let genesis_config = {
        let rt_params = create_genesis_config::<DefaultSpec, _>(&GenesisPaths::from_dir(
            genesis_conf_dir.as_str(),
        ))
        .unwrap();

        let chain_state =
            read_json_file(Path::new(genesis_conf_dir.as_str()).join("chain_state.json")).unwrap();
        let kernel_params = BasicKernelGenesisConfig { chain_state };
        GenesisParams {
            runtime: rt_params,
            kernel: kernel_params,
        }
    };

    tracing::info!("Starting from empty storage, initialization chain");
    let genesis_block = MockBlock::default();
    let (stf_state, _) = storage_manager
        .create_state_for(genesis_block.header())
        .unwrap();
    let (mut prev_state_root, stf_state) = stf.init_chain(stf_state, genesis_config);
    storage_manager
        .save_change_set(genesis_block.header(), stf_state, SchemaBatch::new())
        .unwrap();
    // Write it to the database immediately!
    storage_manager.finalize(&genesis_block.header).unwrap();

    // TODO: Fix this with genesis logic.
    let blocks = get_blocks_from_da().await.expect("Failed to get DA blocks");

    for filtered_block in &blocks[..2] {
        let mut host = Risc0Host::new(MOCK_DA_ELF);

        let height = filtered_block.header().height();
        tracing::info!(
            "Requesting data for height {} and prev_state_root 0x{}",
            height,
            hex::encode(prev_state_root.root_hash())
        );
        let (mut relevant_blobs, relevant_proofs) = da_service
            .extract_relevant_blobs_with_proof(filtered_block)
            .await;

        let (stf_state, _) = storage_manager
            .create_state_for(filtered_block.header())
            .unwrap();

        let result = stf.apply_slot(
            &prev_state_root,
            stf_state,
            Default::default(),
            filtered_block.header(),
            &filtered_block.validity_condition(),
            relevant_blobs.as_iters(),
        );

        let data = StateTransitionWitness::<
            <TestSTF as StateTransitionFunction<Risc0Verifier, MockZkVerifier, MockDaSpec>>::StateRoot,
            <TestSTF as StateTransitionFunction<Risc0Verifier, MockZkVerifier, MockDaSpec>>::Witness,
            MockDaSpec,
        > {
            initial_state_root: prev_state_root,
            da_block_header: filtered_block.header().clone(),
            relevant_proofs,
            witness: result.witness,
            relevant_blobs,
            final_state_root: result.state_root,
        };

        let data = StateTransitionWitnessWithAddress {
            stf_witness: data,
            prover_address: MockAddress::default(),
        };

        host.add_hint(data);

        tracing::info!("Run prover without generating a proof for block {height}\n");
        let _receipt = host
            .run_without_proving()
            .expect("Prover should run successfully");
        tracing::info!("==================================================\n");

        prev_state_root = result.state_root;
        storage_manager
            .save_change_set(
                filtered_block.header(),
                result.change_set,
                SchemaBatch::new(),
            )
            .unwrap();
    }
}
