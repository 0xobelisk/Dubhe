use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;
use std::sync::Arc;

use sov_rollup_interface::services::batch_builder::TxHash;

use crate::db::{MempoolTx, SequencerDb};

/// The mempool MUST ALWAYS persist changes before modifying the in-memory state,
/// otherwise a DB error would leave the two out of sync. (Unlike DB operations
/// which can fail, we know that mempool state changes are infallible.)
#[derive(Debug)]
pub struct FairMempool {
    pub mempool_max_txs_count: usize,
    sequencer_db: SequencerDb,
    next_incr_id: u64,
    // Transaction data
    // ----------------
    txs_ordered_by_most_fair_fit: BTreeMap<MempoolCursor, Arc<MempoolTx>>,
    txs_ordered_by_incremental_id: BTreeMap<u64, Arc<MempoolTx>>,
    txs_by_hash: HashMap<TxHash, Arc<MempoolTx>>,
}

impl FairMempool {
    pub fn new(sequencer_db: SequencerDb, mempool_max_txs_count: usize) -> anyhow::Result<Self> {
        let mut mempool = Self {
            mempool_max_txs_count,
            sequencer_db,
            next_incr_id: 0,
            txs_ordered_by_incremental_id: BTreeMap::new(),
            txs_ordered_by_most_fair_fit: BTreeMap::new(),
            txs_by_hash: HashMap::new(),
        };

        let txs = mempool.sequencer_db.read_all()?;
        for (_, tx) in txs.into_iter() {
            mempool.add(Arc::new(tx))?;
        }

        mempool.next_incr_id = mempool
            .txs_ordered_by_incremental_id
            .last_key_value()
            .map(|(k, _v)| *k)
            // If the mempool is empty, we start at 1. 0 is reserved as a special value for the
            // cursor.
            .unwrap_or(0)
            + 1;

        Ok(mempool)
    }

    pub fn len(&self) -> usize {
        self.txs_by_hash.len()
    }

    pub fn contains(&self, hash: &TxHash) -> bool {
        self.txs_by_hash.contains_key(hash)
    }

    /// Fetches the next transaction to include in the batch, if a suitable one
    /// exists.
    pub fn next(&self, cursor: &mut MempoolCursor) -> Option<Arc<MempoolTx>> {
        let mut iter = self
            .txs_ordered_by_most_fair_fit
            // The lower bound is always ignored, so we don't fetch the last
            // transaction again but we go to the next one.
            .range((Bound::Excluded(*cursor), Bound::Unbounded));
        let (next_cursor, tx) = iter.next()?;

        // Important: we update the cursor so the caller can make another call
        // and get the next transaction.
        *cursor = *next_cursor;

        Some(tx.clone())
    }

    fn remove_tx_from_memory(&mut self, hash: &TxHash) {
        let tx = self.txs_by_hash.remove(hash).unwrap();
        let cursor = MempoolCursor {
            tx_size_in_bytes: tx.tx_bytes.len(),
            incremental_id: tx.incremental_id,
        };

        self.txs_ordered_by_incremental_id
            .remove(&tx.incremental_id)
            .unwrap();
        self.txs_ordered_by_most_fair_fit.remove(&cursor).unwrap();
    }

    fn evict(&mut self) -> anyhow::Result<()> {
        while self.len() > self.mempool_max_txs_count {
            let tx_hash = self
                // We always evict the oldest transaction first.
                .txs_ordered_by_incremental_id
                .first_key_value()
                .expect("Mempool is empty but it doesn't have size zero; this is a bug")
                .1
                .hash;

            // We always persist changes to the DB first.
            self.sequencer_db.remove(&[tx_hash])?;
            self.remove_tx_from_memory(&tx_hash);
        }

        Ok(())
    }

    pub fn remove_atomically(&mut self, hashes: &[TxHash]) -> anyhow::Result<()> {
        // We always persist changes to the DB first.
        self.sequencer_db.remove(hashes)?;
        for hash in hashes {
            self.remove_tx_from_memory(hash);
        }

        Ok(())
    }

    pub fn add_new_tx(&mut self, hash: TxHash, raw: Vec<u8>) -> anyhow::Result<Arc<MempoolTx>> {
        if let Some(tx) = self.txs_by_hash.get(&hash) {
            // We already have this transaction in the mempool; simply return a
            // reference to it (don't re-add it!).
            return Ok(tx.clone());
        }

        let tx = Arc::new(MempoolTx::new(hash, raw, self.next_incr_id));

        self.add(tx.clone())?;
        self.next_incr_id += 1;

        Ok(tx)
    }

    pub fn add(&mut self, tx: Arc<MempoolTx>) -> anyhow::Result<()> {
        // We always persist changes to the DB first.
        self.sequencer_db.insert(&tx)?;

        let cursor = MempoolCursor {
            tx_size_in_bytes: tx.tx_bytes.len(),
            incremental_id: tx.incremental_id,
        };

        self.txs_ordered_by_incremental_id
            .insert(tx.incremental_id, tx.clone());
        self.txs_ordered_by_most_fair_fit.insert(cursor, tx.clone());
        self.txs_by_hash.insert(tx.hash, tx.clone());

        // If this fails, we're potentially left with a SequencerDb with more
        // transactions that it should have. Not a problem because eviction is
        // on a best-effort basis, but good to keep it in mind.
        self.evict()?;

        Ok(())
    }
}

/// An opaque cursor for [`FairMempool`] iteration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct MempoolCursor {
    tx_size_in_bytes: usize,
    incremental_id: u64,
}

impl MempoolCursor {
    pub fn new(tx_size_in_bytes: usize) -> Self {
        Self {
            tx_size_in_bytes,
            incremental_id: 0,
        }
    }
}

impl PartialOrd for MempoolCursor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MempoolCursor {
    fn cmp(&self, other: &Self) -> Ordering {
        // Transactions in the mempool are ordered:
        // 1. from largest to smallest, and
        // 2. by least recent to most recent after that.
        let size_ordering = self.tx_size_in_bytes.cmp(&other.tx_size_in_bytes).reverse();
        let temporal_ordering = self.incremental_id.cmp(&other.incremental_id);

        size_ordering.then(temporal_ordering)
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use super::*;

    proptest! {
        #[test]
        fn mempool_cursor_ordering_is_correct(mc1: MempoolCursor, mc2: MempoolCursor, mc3: MempoolCursor) {
            reltester::ord(&mc1, &mc2, &mc3).unwrap();
        }
    }
}
