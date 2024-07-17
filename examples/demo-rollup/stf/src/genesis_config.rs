//! While the `GenesisConfig` type for `Rollup` is generated from the underlying runtime through a macro,
//! specific module configurations are obtained from files. This code is responsible for the logic
//! that transforms module genesis data into Rollup genesis data.

use std::convert::AsRef;
use std::path::{Path, PathBuf};

pub use sov_accounts::{AccountConfig, AccountData};
pub use sov_bank::{BankConfig, Coins, TokenConfig};
pub use sov_chain_state::ChainStateConfig;
pub use sov_evm::EvmConfig;
use sov_modules_api::Spec;
use sov_modules_stf_blueprint::Runtime as RuntimeTrait;
pub use sov_nft_module::NonFungibleTokenConfig;
use sov_prover_incentives::ProverIncentivesConfig;
use sov_rollup_interface::da::DaSpec;
pub use sov_sequencer_registry::SequencerConfig;
pub use sov_state::config::Config as StorageConfig;
use sov_stf_runner::read_json_file;
pub use sov_value_setter::ValueSetterConfig;

/// Creates config for a rollup with some default settings, the config is used in demos and tests.
use crate::runtime::GenesisConfig;
use crate::runtime::Runtime;

/// Paths pointing to genesis files.
pub struct GenesisPaths {
    /// Bank genesis path.
    pub bank_genesis_path: PathBuf,
    /// Sequencer Registry genesis path.
    pub sequencer_genesis_path: PathBuf,
    /// Value Setter genesis path.
    pub value_setter_genesis_path: PathBuf,
    /// Accounts genesis path.
    pub accounts_genesis_path: PathBuf,
    /// Prover Incentives genesis path.
    pub prover_incentives_genesis_path: PathBuf,
    /// NFT genesis path.
    pub nft_path: PathBuf,
    /// EVM genesis path.
    pub evm_genesis_path: PathBuf,
}

impl GenesisPaths {
    /// Creates a new [`GenesisPaths`] from the files contained in the given
    /// directory.
    ///
    /// Take a look at the contents of the `test-data` directory to see the
    /// expected files.
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            bank_genesis_path: dir.as_ref().join("bank.json"),
            sequencer_genesis_path: dir.as_ref().join("sequencer_registry.json"),
            value_setter_genesis_path: dir.as_ref().join("value_setter.json"),
            accounts_genesis_path: dir.as_ref().join("accounts.json"),
            prover_incentives_genesis_path: dir.as_ref().join("prover_incentives.json"),
            nft_path: dir.as_ref().join("nft.json"),
            evm_genesis_path: dir.as_ref().join("evm.json"),
        }
    }
}

/// Creates a new [`RuntimeTrait::GenesisConfig`] from the files contained in
/// the given directory.
pub fn create_genesis_config<S: Spec, Da: DaSpec>(
    genesis_paths: &GenesisPaths,
) -> anyhow::Result<<Runtime<S, Da> as RuntimeTrait<S, Da>>::GenesisConfig> {
    let bank_config: BankConfig<S> = read_json_file(&genesis_paths.bank_genesis_path)?;

    let sequencer_registry_config: SequencerConfig<S, Da> =
        read_json_file(&genesis_paths.sequencer_genesis_path)?;

    let value_setter_config: ValueSetterConfig<S> =
        read_json_file(&genesis_paths.value_setter_genesis_path)?;

    let prover_incentives_config: ProverIncentivesConfig<S> =
        read_json_file(&genesis_paths.prover_incentives_genesis_path)?;

    let accounts_config: AccountConfig<S> = read_json_file(&genesis_paths.accounts_genesis_path)?;

    let nonces_config = ();

    let nft_config: NonFungibleTokenConfig = read_json_file(&genesis_paths.nft_path)?;

    let evm_config: EvmConfig = read_json_file(&genesis_paths.evm_genesis_path)?;

    Ok(GenesisConfig::new(
        bank_config,
        sequencer_registry_config,
        value_setter_config,
        prover_incentives_config,
        accounts_config,
        nonces_config,
        nft_config,
        evm_config,
    ))
}
