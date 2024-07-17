use std::sync::{Arc, Mutex};

use rockbound::cache::cache_db::CacheDb;
use rockbound::{Schema, SchemaBatch, SeekKeyEncoder};
use serde::Serialize;
use sov_rollup_interface::rpc::AggregatedProofResponse;
use sov_rollup_interface::services::da::SlotData;
use sov_rollup_interface::stf::{BatchReceipt, StoredEvent, TxReceiptContents};
use sov_rollup_interface::zk::aggregated_proof::AggregatedProof;

use crate::schema::tables::{
    BatchByHash, BatchByNumber, EventByKey, EventByNumber, FinalizedSlots, ProofByUniqueId,
    SlotByHash, SlotByNumber, TxByHash, TxByNumber, LEDGER_TABLES,
};
use crate::schema::types::{
    split_tx_for_storage, BatchNumber, EventNumber, LatestFinalizedSlotSingleton, ProofUniqueId,
    SlotNumber, StoredBatch, StoredSlot, StoredTransaction, TxNumber,
};
use crate::DbOptions;

/// Helper functions to query from events.
pub mod event_helper;
mod rpc;
mod rpc_constants;

/// A SlotNumber, BatchNumber, TxNumber, and EventNumber which are grouped together, typically representing
/// the respective heights at the start or end of slot processing.
#[derive(Default, Clone, Debug)]
#[cfg_attr(feature = "arbitrary", derive(proptest_derive::Arbitrary))]
pub struct ItemNumbers {
    /// The slot number
    pub slot_number: u64,
    /// The batch number
    pub batch_number: u64,
    /// The transaction number
    pub tx_number: u64,
    /// The event number
    pub event_number: u64,
}

/// All of the data to be committed to the ledger db for a single slot.
#[derive(Debug)]
pub struct SlotCommit<S: SlotData, B, T: TxReceiptContents> {
    slot_data: S,
    batch_receipts: Vec<BatchReceipt<B, T>>,
    num_txs: usize,
    num_events: usize,
}

impl<S: SlotData, B, T: TxReceiptContents> SlotCommit<S, B, T> {
    /// Returns a reference to the commit's slot_data
    pub fn slot_data(&self) -> &S {
        &self.slot_data
    }

    /// Returns a reference to the commit's batch_receipts
    pub fn batch_receipts(&self) -> &[BatchReceipt<B, T>] {
        &self.batch_receipts
    }

    /// Create a new SlotCommit from the given slot data
    pub fn new(slot_data: S) -> Self {
        Self {
            slot_data,
            batch_receipts: vec![],
            num_txs: 0,
            num_events: 0,
        }
    }
    /// Add a `batch` (of transactions) to the commit
    pub fn add_batch(&mut self, batch: BatchReceipt<B, T>) {
        self.num_txs += batch.tx_receipts.len();
        let events_this_batch: usize = batch.tx_receipts.iter().map(|r| r.events.len()).sum();
        self.batch_receipts.push(batch);
        self.num_events += events_this_batch;
    }
}

/// Single struct responsible for aggregating and sending all notifications.
#[derive(Debug, Clone)]
pub(crate) struct LedgerNotificationService {
    // Regular slots
    slot_notifications: Arc<Mutex<Vec<u64>>>,
    pub(crate) slot_subscriptions: tokio::sync::broadcast::Sender<u64>,
    // Finalized slots
    finalized_slot_notifications: Arc<Mutex<Vec<u64>>>,
    pub(crate) finalized_slot_subscriptions: tokio::sync::watch::Sender<u64>,
    // Proofs
    proof_notifications: Arc<Mutex<Vec<AggregatedProofResponse>>>,
    pub(crate) proof_subscriptions: tokio::sync::broadcast::Sender<AggregatedProofResponse>,
}

impl LedgerNotificationService {
    pub(crate) fn new() -> Self {
        LedgerNotificationService {
            slot_notifications: Default::default(),
            slot_subscriptions: tokio::sync::broadcast::channel(10).0,
            finalized_slot_notifications: Default::default(),
            finalized_slot_subscriptions: tokio::sync::watch::Sender::new(0),
            proof_notifications: Default::default(),
            proof_subscriptions: tokio::sync::broadcast::channel(10).0,
        }
    }

    pub(crate) fn register_slot_notification(&self, slot_number: u64) {
        self.slot_notifications
            .lock()
            .expect("Slot notification lock is poisoned")
            .push(slot_number);
    }

    pub(crate) fn register_finalized_slot_notification(&self, slot_number: u64) {
        self.finalized_slot_notifications
            .lock()
            .expect("Finalized slot notification lock is poisoned")
            .push(slot_number);
    }

    pub(crate) fn register_aggregated_proof_notification(
        &self,
        aggregated_proof: AggregatedProofResponse,
    ) {
        self.proof_notifications
            .lock()
            .expect("Aggregated proof notification lock is poisoned")
            .push(aggregated_proof);
    }

    pub(crate) fn send_notifications(&self) {
        {
            let mut slot_notifications = self
                .slot_notifications
                .lock()
                .expect("Slot notification lock is poisoned");
            let slot_numbers = std::mem::take(&mut *slot_notifications);
            for slot_number in slot_numbers {
                // Notify subscribers.
                // This call returns an error if there are no subscribers,
                // so we don't need to check the result
                let _ = self.slot_subscriptions.send(slot_number);
            }
        }

        {
            let mut finalized_slot_notifications = self
                .finalized_slot_notifications
                .lock()
                .expect("Finalized slot notification lock is poisoned");
            let finalized_slot_numbers = std::mem::take(&mut *finalized_slot_notifications);
            for slot_number in finalized_slot_numbers {
                // Notify subscribers. This call returns an error if there are no subscribers, so we don't need to check the result
                let _ = self.finalized_slot_subscriptions.send(slot_number);
            }
        }

        {
            let mut proof_notifications = self
                .proof_notifications
                .lock()
                .expect("Proof notification lock is poisoned");
            let aggregated_proofs = std::mem::take(&mut *proof_notifications);
            for agg_proof in aggregated_proofs {
                let _ = self.proof_subscriptions.send(agg_proof);
            }
        }
    }
}

#[derive(Clone, Debug)]
/// A database which stores the ledger history (slots, transactions, events, etc).
/// Ledger data is first ingested into an in-memory map
/// before being fed to the state-transition function.
/// Once the state-transition function has been executed and finalized,
/// the results are committed to the final db
pub struct LedgerDb {
    /// The database which stores the committed ledger.
    /// Uses an optimized layout which
    /// requires transactions to be executed before being committed.
    db: Arc<CacheDb>,
    notification_service: LedgerNotificationService,
}

impl LedgerDb {
    const DB_PATH_SUFFIX: &'static str = "ledger";
    const DB_NAME: &'static str = "ledger-db";

    /// Create [`DbOptions`] for [`LedgerDb`].
    pub fn get_rockbound_options() -> DbOptions {
        DbOptions {
            name: Self::DB_NAME,
            path_suffix: Self::DB_PATH_SUFFIX,
            columns: LEDGER_TABLES.to_vec(),
        }
    }

    /// Initialize a new [`LedgerDb`] with an provided [`CacheDb`].
    pub fn with_cache_db(db: CacheDb) -> anyhow::Result<Self> {
        Ok(Self {
            db: Arc::new(db),
            notification_service: LedgerNotificationService::new(),
        })
    }

    /// Replace underlying [`CacheDb`] with provided one.
    /// Keeps the underlying broadcast channel open.
    pub fn replace_db(&mut self, db: CacheDb) -> anyhow::Result<()> {
        self.db.overwrite_change_set(db);
        Ok(())
    }

    /// Get the next slot, block, transaction, and event numbers.
    pub fn get_next_items_numbers(&self) -> anyhow::Result<ItemNumbers> {
        Ok(ItemNumbers {
            slot_number: Self::last_version_written(&self.db, SlotByNumber)?
                .map(|x| x + 1)
                .unwrap_or_default(),
            batch_number: Self::last_version_written(&self.db, BatchByNumber)?
                .map(|x| x + 1)
                .unwrap_or_default(),
            tx_number: Self::last_version_written(&self.db, TxByNumber)?
                .map(|x| x + 1)
                .unwrap_or_default(),
            event_number: Self::last_version_written(&self.db, EventByNumber)?
                .map(|x| x + 1)
                .unwrap_or_default(),
        })
    }

    /// Gets all slots with numbers `range.start` to `range.end`. If `range.end` is outside
    /// the range of the database, the result will smaller than the requested range.
    /// Note that this method blindly preallocates for the requested range, so it should not be exposed
    /// directly via rpc.
    pub(crate) async fn _get_slot_range(
        &self,
        range: &std::ops::Range<SlotNumber>,
    ) -> Result<Vec<StoredSlot>, anyhow::Error> {
        self.get_data_range::<SlotByNumber, _, _>(range).await
    }

    /// Gets all batches with numbers `range.start` to `range.end`. If `range.end` is outside
    /// the range of the database, the result will smaller than the requested range.
    /// Note that this method blindly preallocates for the requested range, so it should not be exposed
    /// directly via rpc.
    pub(crate) async fn get_batch_range(
        &self,
        range: &std::ops::Range<BatchNumber>,
    ) -> Result<Vec<StoredBatch>, anyhow::Error> {
        self.get_data_range::<BatchByNumber, _, _>(range).await
    }

    /// Gets all transactions with numbers `range.start` to `range.end`. If `range.end` is outside
    /// the range of the database, the result will smaller than the requested range.
    /// Note that this method blindly preallocates for the requested range, so it should not be exposed
    /// directly via rpc.
    pub(crate) async fn get_tx_range(
        &self,
        range: &std::ops::Range<TxNumber>,
    ) -> Result<Vec<StoredTransaction>, anyhow::Error> {
        self.get_data_range::<TxByNumber, _, _>(range).await
    }

    /// Gets all data with identifier in `range.start` to `range.end`. If `range.end` is outside
    /// the range of the database, the result will smaller than the requested range.
    /// Note that this method blindly preallocates for the requested range, so it should not be exposed
    /// directly via RPC.
    async fn get_data_range<T, K, V>(
        &self,
        range: &std::ops::Range<K>,
    ) -> Result<Vec<V>, anyhow::Error>
    where
        T: Schema<Key = K, Value = V>,
        K: Into<u64> + Copy + SeekKeyEncoder<T>,
    {
        let raw_out = self.db.collect_in_range_async(range.clone()).await?;
        let mut out = Vec::with_capacity(raw_out.len());
        for (_, value) in raw_out {
            out.push(value);
        }
        Ok(out)
    }

    fn put_slot(
        &self,
        slot: &StoredSlot,
        slot_number: &SlotNumber,
        schema_batch: &mut SchemaBatch,
    ) -> Result<(), anyhow::Error> {
        schema_batch.put::<SlotByNumber>(slot_number, slot)?;
        schema_batch.put::<SlotByHash>(&slot.hash, slot_number)
    }

    fn put_batch(
        &self,
        batch: &StoredBatch,
        batch_number: &BatchNumber,
        schema_batch: &mut SchemaBatch,
    ) -> Result<(), anyhow::Error> {
        schema_batch.put::<BatchByNumber>(batch_number, batch)?;
        schema_batch.put::<BatchByHash>(&batch.hash, batch_number)
    }

    fn put_transaction(
        &self,
        tx: &StoredTransaction,
        tx_number: &TxNumber,
        schema_batch: &mut SchemaBatch,
    ) -> Result<(), anyhow::Error> {
        schema_batch.put::<TxByNumber>(tx_number, tx)?;
        schema_batch.put::<TxByHash>(&(tx.hash, *tx_number), &())
    }

    fn put_event(
        &self,
        event: &StoredEvent,
        event_number: &EventNumber,
        tx_number: TxNumber,
        schema_batch: &mut SchemaBatch,
    ) -> Result<(), anyhow::Error> {
        schema_batch.put::<EventByNumber>(event_number, event)?;
        schema_batch.put::<EventByKey>(&(event.key().clone(), tx_number, *event_number), &())
    }

    /// Materializes [`SlotCommit`] into [`SchemaBatch`] by inserting its events,
    /// transactions, and batches before inserting the slot metadata.
    pub fn materialize_slot<S: SlotData, B: Serialize, T: TxReceiptContents>(
        &self,
        data_to_commit: SlotCommit<S, B, T>,
        state_root: &[u8],
    ) -> anyhow::Result<SchemaBatch> {
        // Create a scope to ensure that the lock is released before we materialize data
        let mut current_item_numbers = self.get_next_items_numbers()?;
        let mut schema_batch = SchemaBatch::new();

        let first_batch_number = current_item_numbers.batch_number;
        let last_batch_number = first_batch_number + data_to_commit.batch_receipts.len() as u64;
        // Insert data from "bottom up" to ensure consistency if the application crashes during insertion
        for batch_receipt in data_to_commit.batch_receipts.into_iter() {
            let first_tx_number = current_item_numbers.tx_number;
            let last_tx_number = first_tx_number + batch_receipt.tx_receipts.len() as u64;
            // Insert transactions and events from each batch before inserting the batch
            for tx in batch_receipt.tx_receipts.into_iter() {
                let (tx_to_store, events) =
                    split_tx_for_storage(tx, current_item_numbers.event_number);
                for event in events.into_iter() {
                    self.put_event(
                        &event,
                        &EventNumber(current_item_numbers.event_number),
                        TxNumber(current_item_numbers.tx_number),
                        &mut schema_batch,
                    )?;
                    current_item_numbers.event_number += 1;
                }
                self.put_transaction(
                    &tx_to_store,
                    &TxNumber(current_item_numbers.tx_number),
                    &mut schema_batch,
                )?;
                current_item_numbers.tx_number += 1;
            }

            // Insert batch
            let batch_to_store = StoredBatch {
                hash: batch_receipt.batch_hash,
                txs: TxNumber(first_tx_number)..TxNumber(last_tx_number),
                receipt: bincode::serialize(&batch_receipt.inner)
                    .expect("serialization to vec is infallible")
                    .into(),
            };
            self.put_batch(
                &batch_to_store,
                &BatchNumber(current_item_numbers.batch_number),
                &mut schema_batch,
            )?;
            current_item_numbers.batch_number += 1;
        }

        // Once all batches are inserted, Insert slot
        let slot_to_store = StoredSlot {
            hash: data_to_commit.slot_data.hash(),
            state_root: state_root.to_vec().into(),
            // TODO: Add a method to the slot data trait allowing additional data to be stored
            extra_data: vec![].into(),
            batches: BatchNumber(first_batch_number)..BatchNumber(last_batch_number),
        };
        self.put_slot(
            &slot_to_store,
            &SlotNumber(current_item_numbers.slot_number),
            &mut schema_batch,
        )?;

        self.notification_service
            .register_slot_notification(current_item_numbers.slot_number);

        Ok(schema_batch)
    }

    /// Sending all previously registered notifications.
    pub fn send_notifications(&self) {
        self.notification_service.send_notifications();
    }

    /// Materializes latest finalized slot and registers notification.
    pub fn materialize_latest_finalize_slot(
        &self,
        slot_number: u64,
    ) -> anyhow::Result<SchemaBatch> {
        let mut schema_batch = SchemaBatch::new();
        schema_batch
            .put::<FinalizedSlots>(&LatestFinalizedSlotSingleton, &SlotNumber(slot_number))?;
        self.notification_service
            .register_finalized_slot_notification(slot_number);
        Ok(schema_batch)
    }

    fn last_version_written<T: Schema<Key = U>, U: Into<u64>>(
        db: &CacheDb,
        _schema: T,
    ) -> anyhow::Result<Option<u64>> {
        let largest = db.get_largest::<T>()?;

        match largest {
            Some((k, _v)) => Ok(Some(k.into())),
            _ => Ok(None),
        }
    }

    /// Get the most recent committed slot, if any.
    pub fn get_head_slot(&self) -> anyhow::Result<Option<(SlotNumber, StoredSlot)>> {
        self.db.get_largest::<SlotByNumber>()
    }

    /// Materializes aggregated zk proof
    pub fn materialize_aggregated_proof(
        &self,
        agg_proof: AggregatedProof,
    ) -> Result<SchemaBatch, anyhow::Error> {
        let mut schema_batch = SchemaBatch::new();
        let unique_id = 0;
        schema_batch.put::<ProofByUniqueId>(&ProofUniqueId(unique_id), &agg_proof)?;

        self.notification_service
            .register_aggregated_proof_notification(AggregatedProofResponse { proof: agg_proof });
        Ok(schema_batch)
    }
}
