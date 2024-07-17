use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;

use demo_stf::genesis_config::GenesisPaths;
use sha2::Sha256;
use sov_cli::wallet_state::PrivateKeyAndAddress;
use sov_demo_rollup::MockDemoRollup;
use sov_kernels::basic::{BasicKernelGenesisConfig, BasicKernelGenesisPaths};
use sov_mock_da::MockDaConfig;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::{Address, Spec};
use sov_modules_rollup_blueprint::{FullNodeBlueprint, Rollup};
use sov_stf_runner::{
    HttpServerConfig, ProofManagerConfig, RollupConfig, RollupProverConfig, RunnerConfig,
    StorageConfig,
};
use tokio::sync::oneshot;

const PROVER_ADDRESS: &str = "sov1pv9skzctpv9skzctpv9skzctpv9skzctpv9skzctpv9skzctpv9stup8tx";

pub fn read_private_keys<S: Spec>(suffix: &str) -> PrivateKeyAndAddress<S> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    let private_keys_dir = Path::new(&manifest_dir).join("../test-data/keys");

    let data = std::fs::read_to_string(private_keys_dir.join(suffix))
        .expect("Unable to read file to string");

    let key_and_address: PrivateKeyAndAddress<S> =
        serde_json::from_str(&data).unwrap_or_else(|_| {
            panic!("Unable to convert data {} to PrivateKeyAndAddress", &data);
        });

    assert!(
        key_and_address.is_matching_to_default(),
        "Inconsistent key data"
    );

    key_and_address
}

pub async fn construct_rollup(
    rt_genesis_paths: GenesisPaths,
    kernel_genesis_paths: BasicKernelGenesisPaths,
    rollup_prover_config: RollupProverConfig,
    da_config: MockDaConfig,
) -> Rollup<MockDemoRollup<Native>, Native> {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    let rollup_config = RollupConfig {
        storage: StorageConfig {
            path: temp_path.to_path_buf(),
        },
        runner: RunnerConfig {
            genesis_height: 0,
            da_polling_interval_ms: 1000,
            rpc_config: HttpServerConfig {
                bind_host: "127.0.0.1".into(),
                bind_port: 0,
            },
            axum_config: HttpServerConfig {
                bind_host: "127.0.0.1".into(),
                bind_port: 0,
            },
        },
        da: da_config,
        proof_manager: ProofManagerConfig {
            aggregated_proof_block_jump: 1,
            prover_address: Address::<Sha256>::from_str(PROVER_ADDRESS)
                .expect("Prover address is not valid"),
        },
    };

    let mock_demo_rollup = MockDemoRollup::<Native>::default();

    let kernel_genesis = BasicKernelGenesisConfig {
        chain_state: serde_json::from_reader(
            std::fs::File::open(&kernel_genesis_paths.chain_state)
                .expect("Failed to read chain_state genesis config"),
        )
        .expect("Failed to parse chain_state genesis config"),
    };

    mock_demo_rollup
        .create_new_rollup(
            &rt_genesis_paths,
            kernel_genesis,
            rollup_config,
            Some(rollup_prover_config),
        )
        .await
        .unwrap()
}

pub async fn start_rollup(
    rpc_reporting_channel: oneshot::Sender<SocketAddr>,
    rest_reporting_channel: oneshot::Sender<SocketAddr>,
    rt_genesis_paths: GenesisPaths,
    kernel_genesis_paths: BasicKernelGenesisPaths,
    rollup_prover_config: RollupProverConfig,
    da_config: MockDaConfig,
) {
    construct_rollup(
        rt_genesis_paths,
        kernel_genesis_paths,
        rollup_prover_config,
        da_config,
    )
    .await
    .run_and_report_addr(Some(rpc_reporting_channel), Some(rest_reporting_channel))
    .await
    .unwrap();
}

pub fn get_appropriate_rollup_prover_config() -> RollupProverConfig {
    let skip_guest_build = std::env::var("SKIP_GUEST_BUILD").unwrap_or_else(|_| "0".to_string());
    if skip_guest_build == "1" {
        RollupProverConfig::Skip
    } else {
        RollupProverConfig::Execute
    }
}
