use reth_primitives::{
    Bytes as RethBytes, TransactionSigned, TransactionSignedEcRecovered, TransactionSignedNoHash,
};
use revm::primitives::{CreateScheme, TransactTo, TxEnv, U256};
use revm_primitives::{Address, BlockEnv};
use thiserror::Error;

use super::primitive_types::SealedBlock;
use crate::RlpEvmTransaction;

// BlockEnv from SealedBlock
impl From<SealedBlock> for BlockEnv {
    fn from(block: SealedBlock) -> Self {
        Self {
            number: U256::from(block.header.number),
            coinbase: block.header.beneficiary,
            timestamp: U256::from(block.header.timestamp),
            prevrandao: Some(block.header.mix_hash),
            basefee: block.header.base_fee_per_gas.map_or(U256::ZERO, U256::from),
            gas_limit: U256::from(block.header.gas_limit),
            // Not used fields:
            blob_excess_gas_and_price: None,
            difficulty: Default::default(),
        }
    }
}

pub(crate) fn create_tx_env(tx: &TransactionSignedNoHash, signer: Address) -> TxEnv {
    let to = match tx.to() {
        Some(addr) => TransactTo::Call(addr),
        None => TransactTo::Create(CreateScheme::Create),
    };

    TxEnv {
        caller: signer,
        gas_limit: tx.gas_limit(),
        gas_price: U256::from(tx.effective_gas_price(None)),
        gas_priority_fee: tx.max_priority_fee_per_gas().map(U256::from),
        transact_to: to,
        value: tx.value(),
        data: tx.input().clone(),
        chain_id: tx.chain_id(),
        nonce: Some(tx.nonce()),
        // TODO handle access list
        access_list: vec![],
        // EIP-4844 related fields
        blob_hashes: Default::default(),
        max_fee_per_blob_gas: None,
    }
}

/// Error that happened during conversion between types
#[derive(Debug, Error)]
pub enum RlpConversionError {
    /// Raw transaction is empty.
    EmptyRawTx,
    /// Deserialization has failed.
    DeserializationFailed,
}

impl core::fmt::Display for RlpConversionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RlpConversionError::EmptyRawTx => write!(f, "Empty raw transaction"),
            RlpConversionError::DeserializationFailed => write!(f, "Deserialization failed"),
        }
    }
}
// And convert it to original EthApiError ourselves or directly to RPC

impl TryFrom<RlpEvmTransaction> for TransactionSignedNoHash {
    type Error = RlpConversionError;

    fn try_from(data: RlpEvmTransaction) -> Result<Self, Self::Error> {
        let data = RethBytes::from(data.rlp);
        // We can skip that, it is done inside `decode_enveloped` method
        if data.is_empty() {
            return Err(RlpConversionError::EmptyRawTx);
        }

        let transaction = TransactionSigned::decode_enveloped(&mut data.as_ref())
            .map_err(|_| RlpConversionError::DeserializationFailed)?;

        Ok(transaction.into())
    }
}

impl TryFrom<RlpEvmTransaction> for TransactionSignedEcRecovered {
    type Error = RlpConversionError;

    fn try_from(evm_tx: RlpEvmTransaction) -> Result<Self, Self::Error> {
        let tx = TransactionSignedNoHash::try_from(evm_tx)?;
        let tx: TransactionSigned = tx.into();
        let tx = tx
            .into_ecrecovered()
            .ok_or(RlpConversionError::DeserializationFailed)?;

        Ok(tx)
    }
}
