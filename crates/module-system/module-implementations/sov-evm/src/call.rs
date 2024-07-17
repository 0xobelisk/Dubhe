use anyhow::Result;
use reth_primitives::{Log as RethLog, TransactionSignedNoHash};
use revm::primitives::{Address, CfgEnv, CfgEnvWithHandlerCfg, EVMError, Log};
use revm_primitives::BlockEnv;
use sov_modules_api::{CallResponse, Context, TxState};

use crate::evm::db::EvmDb;
use crate::evm::executor::{self};
use crate::evm::primitive_types::{Receipt, TransactionSignedAndRecovered};
use crate::evm::{EvmChainConfig, RlpEvmTransaction};
use crate::{Evm, PendingTransaction, SpecId};

/// EVM call message.
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub struct CallMessage {
    /// RLP encoded transaction.
    pub rlp: RlpEvmTransaction,
}

impl<S: sov_modules_api::Spec> Evm<S> {
    pub(crate) fn execute_call(
        &self,
        message: CallMessage,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        // Check if the tx went through the EVM authenticator.
        let signer = *context
            .get_sender_credential::<Address>()
            .ok_or(anyhow::anyhow!(
                "EVM transaction must be authenticated by the EVM authenticator"
            ))?;

        let evm_tx: TransactionSignedNoHash = message.rlp.try_into()?;

        let block_env = self
            .block_env
            .get(state)?
            .expect("Pending block must be set");

        let cfg = self.cfg.get(state)?.expect("Evm config must be set");
        let cfg_env = get_cfg_env_with_handler(&block_env, cfg, None);

        let evm_db: EvmDb<_> = self.get_db(state);
        let result = executor::execute_tx(evm_db, &block_env, &evm_tx, signer, cfg_env);

        let previous_transaction = self.pending_transactions.last(state)?;
        let previous_transaction_cumulative_gas_used = previous_transaction
            .as_ref()
            .map_or(0u64, |tx| tx.receipt.receipt.cumulative_gas_used);
        let log_index_start = previous_transaction.as_ref().map_or(0u64, |tx| {
            tx.receipt
                .log_index_start
                .saturating_add(tx.receipt.receipt.logs.len() as u64)
        });

        let receipt = match result {
            Ok(result) => {
                let is_success = result.is_success();
                let gas_used = result.gas_used();
                let logs: Vec<_> = result.into_logs().into_iter().map(into_reth_log).collect();
                tracing::debug!(
                    hash = hex::encode(evm_tx.hash()),
                    gas_used,
                    "EVM transaction has been executed"
                );
                Receipt {
                    receipt: reth_primitives::Receipt {
                        tx_type: evm_tx.tx_type(),
                        success: is_success,
                        cumulative_gas_used: previous_transaction_cumulative_gas_used
                            .saturating_add(gas_used),
                        logs,
                    },
                    gas_used,
                    log_index_start,
                    error: None,
                }
            }
            // Adopted from https://github.com/paradigmxyz/reth/blob/main/crates/payload/basic/src/lib.rs#L884
            Err(err) => {
                tracing::debug!(
                    tx_hash = hex::encode(evm_tx.hash()),
                    error = ?err,
                    "EVM transaction has been reverted"
                );
                return match err {
                    EVMError::Transaction(_) => {
                        // This is a transactional error, so we can skip it without doing anything.
                        Ok(CallResponse::default())
                    }
                    err => {
                        // This is a fatal error, so we need to return it.
                        Err(err.into())
                    }
                };
            }
        };

        let pending_transaction = PendingTransaction {
            transaction: TransactionSignedAndRecovered {
                signer,
                signed_transaction: evm_tx.into(),
                block_number: block_env.number.to(),
            },
            receipt,
        };

        self.pending_transactions
            .push(&pending_transaction, state)?;

        Ok(CallResponse::default())
    }
}

/// builds CfgEnvWithHandlerCfg
/// Returns correct config depending on spec for given block number
// Copies context-dependent values from template_cfg or default if not provided
pub(crate) fn get_cfg_env_with_handler(
    block_env: &BlockEnv,
    cfg: EvmChainConfig,
    template_cfg: Option<CfgEnv>,
) -> CfgEnvWithHandlerCfg {
    let mut cfg_env = template_cfg.unwrap_or_default();
    cfg_env.chain_id = cfg.chain_id;
    cfg_env.limit_contract_code_size = cfg.limit_contract_code_size;
    let spec_id = get_spec_id(cfg.spec, block_env.number.to());
    CfgEnvWithHandlerCfg::new(cfg_env, revm_primitives::HandlerCfg { spec_id })
}

/// Get spec id for a given block number
/// Returns the first spec id defined for block >= block_number
pub(crate) fn get_spec_id(spec: Vec<(u64, SpecId)>, block_number: u64) -> SpecId {
    match spec.binary_search_by(|&(k, _)| k.cmp(&block_number)) {
        Ok(index) => spec[index].1,
        Err(index) => {
            if index > 0 {
                spec[index.checked_sub(1).expect("invalid spec index")].1
            } else {
                // this should never happen as we cover this in genesis
                panic!("EVM spec must start from block 0")
            }
        }
    }
}

/// Copied from <https://github.com/paradigmxyz/reth/blob/e83d3aa704f87825ca8cab6f593ab4d4adbf6792/crates/revm/revm-primitives/src/compat.rs#L17-L23>.
/// All rights reserved.
///
/// By copying the code, we can avoid depending on the whole crate.
pub fn into_reth_log(log: Log) -> RethLog {
    RethLog {
        address: Address(log.address.0),
        topics: log.topics().to_vec(),
        data: log.data.data,
    }
}
