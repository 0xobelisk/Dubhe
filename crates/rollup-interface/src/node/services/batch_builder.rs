//! This module defines the trait that is used to build batches of transactions.

use async_trait::async_trait;

/// A rollup transaction hash. Used by [`BatchBuilder`].
pub type TxHash = [u8; 32];

/// BlockBuilder trait is responsible for managing mempool and building batches.
#[async_trait]
pub trait BatchBuilder {
    /// Accept a new transaction.
    /// Can return error if transaction is invalid or mempool is full.
    async fn accept_tx(&mut self, tx: Vec<u8>) -> anyhow::Result<TxHash>;

    /// Checks whether a transaction with the given `hash` is already in the
    /// mempool.
    async fn contains(&self, hash: &TxHash) -> anyhow::Result<bool>;

    /// Builds a new batch out of transactions in mempool.
    /// The logic of which transactions and how many of them are included in batch is up to implementation.
    async fn get_next_blob(&mut self, height: u64) -> anyhow::Result<Vec<TxWithHash>>;
}

/// An encoded transaction with its hash as returned by
/// [`BatchBuilder::get_next_blob`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxWithHash {
    /// Encoded transaction.
    pub raw_tx: Vec<u8>,
    /// Transaction hash.
    pub hash: TxHash,
}
