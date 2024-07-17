use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use borsh::{BorshDeserialize, BorshSerialize};
use rockbound::SchemaBatch;
use sov_db::rocks_db_config::gen_rocksdb_options;
use sov_db::{
    define_table_with_seek_key_codec, define_table_without_codec, impl_borsh_value_codec,
};
use sov_rollup_interface::services::batch_builder::TxHash;

/// A database holding transactions that have been submitted to the sequencer
/// and other related data.
#[derive(Clone, Debug)]
pub struct SequencerDb {
    db: Arc<rockbound::DB>,
}

impl SequencerDb {
    const DB_PATH_SUFFIX: &'static str = "mempool";
    const DB_NAME: &'static str = "mempool-db";

    const TABLES: &'static [&'static str] = &[MempoolTxByHash::table_name()];

    /// Initializes a new [`SequencerDb`] at the given path.
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().join(Self::DB_PATH_SUFFIX);
        let db = rockbound::DB::open(
            path,
            Self::DB_NAME,
            Self::TABLES.iter().copied(),
            &gen_rocksdb_options(&Default::default(), false),
        )?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Returns all transactions stored in the mempool, keyed by their hash.
    pub fn read_all(&self) -> anyhow::Result<HashMap<TxHash, MempoolTx>> {
        let mut txs = HashMap::new();
        for iter_res in self.db.iter::<MempoolTxByHash>()? {
            let item = iter_res?;
            txs.insert(item.key, item.value);
        }
        Ok(txs)
    }

    /// Deletes a group of transactions from the mempool (atomically).
    pub fn remove(&self, hashes: &[TxHash]) -> anyhow::Result<()> {
        let mut batch = SchemaBatch::new();
        for hash in hashes {
            batch.delete::<MempoolTxByHash>(hash)?;
        }
        self.db.write_schemas(&batch)?;
        Ok(())
    }

    /// Inserts a single transaction into the mempool.
    pub fn insert(&self, tx: &MempoolTx) -> anyhow::Result<()> {
        self.db.put::<MempoolTxByHash>(&tx.hash, tx)?;
        Ok(())
    }
}

/// A transaction as stored inside [`SequencerDb`].
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct MempoolTx {
    /// The raw, unmodified transaction bytes.
    pub tx_bytes: Vec<u8>,
    /// The hash of the transaction.
    pub hash: TxHash,
    /// An incremental counter used to order transactions by insertion time.
    /// Gaps are allowed.
    pub incremental_id: u64,
}

impl MempoolTx {
    /// Creates a new [`MempoolTx`] from the given transaction bytes.
    pub fn new(hash: TxHash, tx_bytes: Vec<u8>, incremental_id: u64) -> Self {
        Self {
            tx_bytes,
            hash,
            incremental_id,
        }
    }
}

define_table_with_seek_key_codec!(
    /// Transactions stored in the mempool, keyed by hash.
    (MempoolTxByHash) TxHash => MempoolTx
);
