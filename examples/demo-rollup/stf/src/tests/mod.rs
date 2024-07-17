use std::path::Path;

use sov_cli::wallet_state::PrivateKeyAndAddress;
use sov_kernels::basic::{BasicKernel, BasicKernelGenesisConfig};
use sov_mock_da::MockDaSpec;
use sov_modules_api::{DaSpec, Spec};
use sov_modules_stf_blueprint::{GenesisParams, StfBlueprint};
use sov_prover_storage_manager::ProverStorageManager;
use sov_state::DefaultStorageSpec;
use sov_stf_runner::read_json_file;

use crate::genesis_config::{create_genesis_config, GenesisPaths};
use crate::runtime::{GenesisConfig, Runtime};

mod da_simulation;
mod stf_tests;
mod tx_revert_tests;

pub(crate) type S = sov_test_utils::TestSpec;
pub(crate) type Da = MockDaSpec;
pub(crate) type RuntimeTest = Runtime<S, Da>;
pub(crate) type StfBlueprintTest = StfBlueprint<S, Da, RuntimeTest, BasicKernel<S, Da>>;

pub(crate) struct TestPrivateKeys<S: Spec> {
    pub token_deployer: PrivateKeyAndAddress<S>,
    pub tx_signer: PrivateKeyAndAddress<S>,
}

pub(crate) fn create_storage_manager_for_tests(
    path: impl AsRef<Path>,
) -> ProverStorageManager<MockDaSpec, DefaultStorageSpec<sov_test_utils::TestHasher>> {
    let config = sov_state::config::Config {
        path: path.as_ref().to_path_buf(),
    };
    ProverStorageManager::new(config).unwrap()
}

pub(crate) fn create_genesis_config_for_tests<Da: DaSpec>(
) -> GenesisParams<GenesisConfig<S, Da>, BasicKernelGenesisConfig<S, Da>> {
    let integ_test_conf_dir: &Path = "../../test-data/genesis/stf-tests".as_ref();
    let rt_params =
        create_genesis_config::<S, Da>(&GenesisPaths::from_dir(integ_test_conf_dir)).unwrap();

    let chain_state = read_json_file(integ_test_conf_dir.join("chain_state.json")).unwrap();
    let kernel_params = BasicKernelGenesisConfig { chain_state };
    GenesisParams {
        runtime: rt_params,
        kernel: kernel_params,
    }
}

const PRIVATE_KEYS_DIR: &str = "../../test-data/keys";

fn read_and_parse_private_key<S: Spec>(suffix: &str) -> PrivateKeyAndAddress<S> {
    PrivateKeyAndAddress::from_json_file(Path::new(PRIVATE_KEYS_DIR).join(suffix)).unwrap()
}

pub(crate) fn read_private_keys<S: Spec>() -> TestPrivateKeys<S> {
    let token_deployer = read_and_parse_private_key::<S>("token_deployer_private_key.json");
    let tx_signer = read_and_parse_private_key::<S>("tx_signer_private_key.json");

    TestPrivateKeys::<S> {
        token_deployer,
        tx_signer,
    }
}
