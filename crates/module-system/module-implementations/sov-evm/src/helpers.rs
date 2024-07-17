use alloy_rpc_types::AccessListItem;
use reth_primitives::{
    BlockNumber, Transaction as PrimitiveTransaction, TransactionKind as PrimitiveTransactionKind,
    TransactionSignedEcRecovered, TxType, U128, U64,
};
use reth_rpc_types::{Header, Parity, Signature, TransactionRequest};
use revm::primitives::{TransactTo, TxEnv, B256, U256};
use revm_primitives::BlockEnv;

use crate::rpc::error::{EthApiError, EthResult, RpcInvalidTransactionError};

/// Helper type for representing the fees of a [CallRequest]
struct CallFees {
    /// EIP-1559 priority fee
    max_priority_fee_per_gas: Option<U256>,
    /// Unified gas price setting
    ///
    /// Will be the configured `basefee` if unset in the request
    ///
    /// `gasPrice` for legacy,
    /// `maxFeePerGas` for EIP-1559
    gas_price: U256,
}

// === impl CallFees ===

impl CallFees {
    /// Ensures the fields of a [CallRequest] are not conflicting.
    ///
    /// If no `gasPrice` or `maxFeePerGas` is set, then the `gas_price` in the returned `gas_price`
    /// will be `0`. See: <https://github.com/ethereum/go-ethereum/blob/2754b197c935ee63101cbbca2752338246384fec/internal/ethapi/transaction_args.go#L242-L255>
    ///
    /// EIP-4844 transactions are not supported by the rollup by design.
    fn ensure_fees(
        call_gas_price: Option<U256>,
        call_max_fee: Option<U256>,
        call_priority_fee: Option<U256>,
        block_base_fee: U256,
    ) -> EthResult<CallFees> {
        /// Ensures that the transaction's max fee is lower than the priority fee, if any.
        fn ensure_valid_fee_cap(
            max_fee: U256,
            max_priority_fee_per_gas: Option<U256>,
        ) -> EthResult<()> {
            if let Some(max_priority) = max_priority_fee_per_gas {
                if max_priority > max_fee {
                    // Fail early
                    return Err(
                        // `max_priority_fee_per_gas` is greater than the `max_fee_per_gas`
                        RpcInvalidTransactionError::TipAboveFeeCap.into(),
                    );
                }
            }
            Ok(())
        }

        match (call_gas_price, call_max_fee, call_priority_fee) {
            (gas_price, None, None) => {
                // either legacy transaction or no fee fields are specified
                // when no fields are specified, set gas price to zero
                let gas_price = gas_price.unwrap_or(U256::ZERO);
                Ok(CallFees {
                    gas_price,
                    max_priority_fee_per_gas: None,
                })
            }
            (None, max_fee_per_gas, max_priority_fee_per_gas) => {
                let max_fee = max_fee_per_gas.unwrap_or(block_base_fee);
                ensure_valid_fee_cap(max_fee, max_priority_fee_per_gas)?;

                Ok(CallFees {
                    gas_price: max_fee,
                    max_priority_fee_per_gas,
                })
            }
            _ => {
                // this fallback covers incompatible combinations of fields
                Err(EthApiError::ConflictingFeeFieldsInRequest)
            }
        }
    }
}

// https://github.com/paradigmxyz/reth/blob/d8677b4146f77c7c82d659c59b79b38caca78778/crates/rpc/rpc/src/eth/revm_utils.rs#L201
// it is `pub(crate)` only for tests
pub(crate) fn prepare_call_env(
    block_env: &BlockEnv,
    request: TransactionRequest,
) -> EthResult<TxEnv> {
    let TransactionRequest {
        from,
        to,
        gas_price,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        gas,
        value,
        input,
        nonce,
        access_list,
        chain_id,
        ..
    } = request;

    let CallFees {
        max_priority_fee_per_gas,
        gas_price,
    } = CallFees::ensure_fees(
        gas_price,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        U256::from(block_env.basefee),
    )?;

    let gas_limit = gas.unwrap_or(U256::from(block_env.gas_limit.min(U256::MAX)));

    let env = TxEnv {
        gas_limit: gas_limit
            .try_into()
            .map_err(|_| RpcInvalidTransactionError::GasUintOverflow)?,
        nonce: nonce
            .map(|n| {
                n.try_into()
                    .map_err(|_| RpcInvalidTransactionError::NonceTooHigh)
            })
            .transpose()?,
        caller: from.unwrap_or_default(),
        gas_price,
        gas_priority_fee: max_priority_fee_per_gas,
        transact_to: to.map(TransactTo::Call).unwrap_or_else(TransactTo::create),
        value: value.unwrap_or_default(),
        data: input.try_into_unique_input()?.unwrap_or_default(),
        chain_id: chain_id.map(|c| c.to()),
        access_list: access_list.map(|x| x.flattened()).unwrap_or_default(),
        // EIP-4844 related fields:
        blob_hashes: Default::default(),
        max_fee_per_blob_gas: None,
    };

    Ok(env)
}

pub(crate) fn from_primitive_with_hash(primitive_header: reth_primitives::SealedHeader) -> Header {
    let (header, hash) = primitive_header.split();
    let reth_primitives::Header {
        parent_hash,
        ommers_hash,
        beneficiary,
        state_root,
        transactions_root,
        receipts_root,
        logs_bloom,
        difficulty,
        number,
        gas_limit,
        gas_used,
        timestamp,
        mix_hash,
        nonce,
        base_fee_per_gas,
        extra_data,
        withdrawals_root,
        blob_gas_used,
        excess_blob_gas,
        parent_beacon_block_root,
    } = header;

    Header {
        hash: Some(hash),
        parent_hash,
        uncles_hash: ommers_hash,
        miner: beneficiary,
        state_root,
        transactions_root,
        receipts_root,
        withdrawals_root,
        number: Some(U256::from(number)),
        gas_used: U256::from(gas_used),
        gas_limit: U256::from(gas_limit),
        extra_data,
        logs_bloom,
        timestamp: U256::from(timestamp),
        difficulty,
        mix_hash: Some(mix_hash),
        nonce: Some(nonce.to_be_bytes().into()),
        base_fee_per_gas: base_fee_per_gas.map(U256::from),
        blob_gas_used: blob_gas_used.map(U64::from),
        excess_blob_gas: excess_blob_gas.map(U64::from),
        parent_beacon_block_root,
        total_difficulty: None,
    }
}

/// copy from [`reth_rpc_types_compat::transaction::from_recovered_with_block_context`]
pub fn from_recovered_with_block_context(
    tx: TransactionSignedEcRecovered,
    block_hash: B256,
    block_number: BlockNumber,
    base_fee: Option<u64>,
    tx_index: U256,
) -> alloy_rpc_types::Transaction {
    let block_hash = Some(block_hash);
    let block_number = Some(block_number);
    let transaction_index = Some(tx_index);

    let signer = tx.signer();
    let mut signed_tx = tx.into_signed();

    let to = match signed_tx.kind() {
        PrimitiveTransactionKind::Create => None,
        PrimitiveTransactionKind::Call(to) => Some(*to),
    };

    let (gas_price, max_fee_per_gas) = match signed_tx.tx_type() {
        TxType::Legacy | TxType::Eip2930 => (Some(U128::from(signed_tx.max_fee_per_gas())), None),
        TxType::Eip1559 => {
            // the gas price field for EIP-1559 is set to
            // `min(tip, gasFeeCap - baseFee) + baseFee`
            let gas_price = base_fee
                .and_then(|base_fee| {
                    signed_tx
                        .effective_tip_per_gas(Some(base_fee))
                        .map(|tip| tip + base_fee as u128)
                })
                .unwrap_or_else(|| signed_tx.max_fee_per_gas());

            (
                Some(U128::from(gas_price)),
                Some(U128::from(signed_tx.max_fee_per_gas())),
            )
        }
        TxType::Eip4844 => {
            panic!("EIP-4844 transactions are not supported by the rollup")
        }
    };

    let chain_id = signed_tx.chain_id().map(U64::from);

    let access_list = match &mut signed_tx.transaction {
        PrimitiveTransaction::Legacy(_) => None,
        PrimitiveTransaction::Eip2930(tx) => Some(
            tx.access_list
                .0
                .iter()
                .map(|item| AccessListItem {
                    address: item.address.0.into(),
                    storage_keys: item.storage_keys.iter().map(|key| key.0.into()).collect(),
                })
                .collect(),
        ),
        PrimitiveTransaction::Eip1559(tx) => Some(
            tx.access_list
                .0
                .iter()
                .map(|item| AccessListItem {
                    address: item.address.0.into(),
                    storage_keys: item.storage_keys.iter().map(|key| key.0.into()).collect(),
                })
                .collect(),
        ),
        PrimitiveTransaction::Eip4844(_tx) => {
            panic!("EIP-4844 transactions are not supported by the rollup");
        }
    };

    let signature = from_primitive_signature(
        *signed_tx.signature(),
        signed_tx.tx_type(),
        signed_tx.chain_id(),
    );

    alloy_rpc_types::Transaction {
        hash: signed_tx.hash(),
        nonce: U64::from(signed_tx.nonce()),
        from: signer,
        to,
        value: signed_tx.value(),
        gas_price,
        max_fee_per_gas,
        max_priority_fee_per_gas: signed_tx.max_priority_fee_per_gas().map(U128::from),
        signature: Some(signature),
        gas: U256::from(signed_tx.gas_limit()),
        input: signed_tx.input().clone(),
        chain_id,
        access_list,
        transaction_type: Some(U64::from(signed_tx.tx_type() as u8)),

        // These fields are set to None because they are not stored as part of the transaction
        block_hash,
        block_number: block_number.map(U256::from),
        transaction_index,
        // EIP-4844 fields
        max_fee_per_blob_gas: Default::default(),
        blob_versioned_hashes: Default::default(),
        // Other fields
        other: Default::default(),
    }
}

pub(crate) fn from_primitive_signature(
    signature: reth_primitives::Signature,
    tx_type: TxType,
    chain_id: Option<u64>,
) -> Signature {
    match tx_type {
        TxType::Legacy => Signature {
            r: signature.r,
            s: signature.s,
            v: U256::from(signature.v(chain_id)),
            y_parity: None,
        },
        _ => Signature {
            r: signature.r,
            s: signature.s,
            v: U256::from(signature.odd_y_parity as u8),
            y_parity: Some(Parity(signature.odd_y_parity)),
        },
    }
}
