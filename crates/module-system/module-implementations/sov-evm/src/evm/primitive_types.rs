use std::ops::Range;

use reth_primitives::{Header, SealedHeader, TransactionSigned, TransactionSignedEcRecovered};
use revm::primitives::{Address, EVMError};

/// RLP encoded evm transaction.
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    Debug,
    PartialEq,
    Clone,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct RlpEvmTransaction {
    /// Rlp data.
    pub rlp: Vec<u8>,
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TransactionSignedAndRecovered {
    /// Signer of the transaction
    pub(crate) signer: Address,
    /// Signed transaction
    pub(crate) signed_transaction: TransactionSigned,
    /// Block the transaction was added to
    pub(crate) block_number: u64,
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Block {
    /// Block header.
    pub(crate) header: Header,

    /// Transactions in this block.
    pub(crate) transactions: Range<u64>,
}

impl Block {
    pub(crate) fn seal(self) -> SealedBlock {
        SealedBlock {
            header: self.header.seal_slow(),
            transactions: self.transactions,
        }
    }
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SealedBlock {
    /// Block header.
    pub(crate) header: SealedHeader,

    /// Transactions in this block.
    pub(crate) transactions: Range<u64>,
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Receipt {
    pub(crate) receipt: reth_primitives::Receipt,
    pub(crate) gas_used: u64,
    pub(crate) log_index_start: u64,
    pub(crate) error: Option<EVMError<u8>>,
}

impl From<TransactionSignedAndRecovered> for TransactionSignedEcRecovered {
    fn from(value: TransactionSignedAndRecovered) -> Self {
        TransactionSignedEcRecovered::from_signed_transaction(
            value.signed_transaction,
            value.signer,
        )
    }
}
