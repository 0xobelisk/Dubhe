use std::convert::Infallible;

use reth_primitives::TransactionSignedNoHash;
use revm::primitives::{CfgEnvWithHandlerCfg, EVMError, Env, EnvWithHandlerCfg, ExecutionResult};
use revm::{Database, DatabaseCommit, EvmBuilder};
use revm_primitives::{Address, BlockEnv};

use crate::evm::conversions::create_tx_env;

pub(crate) fn execute_tx<DB: Database<Error = Infallible> + DatabaseCommit>(
    db: DB,
    block_env: &BlockEnv,
    tx: &TransactionSignedNoHash,
    signer: Address,
    config_env: CfgEnvWithHandlerCfg,
) -> Result<ExecutionResult, EVMError<Infallible>> {
    let CfgEnvWithHandlerCfg {
        cfg_env,
        handler_cfg,
    } = config_env;

    let env_with_handler_cfg = EnvWithHandlerCfg {
        env: Box::new(Env {
            cfg: cfg_env,
            block: block_env.clone(),
            tx: create_tx_env(tx, signer),
        }),
        handler_cfg,
    };

    let mut evm = EvmBuilder::default()
        .with_db(db)
        .with_env_with_handler_cfg(env_with_handler_cfg)
        .build();

    evm.transact_commit()
}

#[cfg(feature = "native")]
pub(crate) fn inspect<DB: Database<Error = Infallible> + DatabaseCommit>(
    db: DB,
    block_env: &BlockEnv,
    tx: revm::primitives::TxEnv,
    config_env: CfgEnvWithHandlerCfg,
) -> Result<revm::primitives::ResultAndState, EVMError<Infallible>> {
    let CfgEnvWithHandlerCfg {
        cfg_env,
        handler_cfg,
    } = config_env;

    let env_with_handler_cfg = EnvWithHandlerCfg {
        env: Box::new(Env {
            cfg: cfg_env,
            block: block_env.clone(),
            tx,
        }),
        handler_cfg,
    };

    let config = revm_inspectors::tracing::TracingInspectorConfig::all();
    let mut inspector = revm_inspectors::tracing::TracingInspector::new(config);

    let mut evm = EvmBuilder::default()
        .with_external_context(&mut inspector)
        .with_db(db)
        .with_env_with_handler_cfg(env_with_handler_cfg)
        .build();

    evm.transact()
}
