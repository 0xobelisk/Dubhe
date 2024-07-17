//! Defines traits for interacting with the ledger when using the sov module system.
use async_trait::async_trait;
use sov_db::ledger_db::event_helper::{
    get_events_by_key_helper, get_events_by_key_slot_range_helper,
};
use sov_db::ledger_db::LedgerDb;
use sov_rollup_interface::rpc::{LedgerStateProvider, PaginatedEventResponse};
use sov_rollup_interface::stf::StoredEvent;

/// A [`LedgerStateProviderExt`] provides a way to query the ledger for events by module.
#[async_trait]
pub trait LedgerStateProviderExt: LedgerStateProvider {
    /// Get events by key.
    async fn get_events_by_key<E>(
        &self,
        event_key: &str,
        txn_range: Option<(u64, u64)>,
        num_events: usize,
        next: Option<&str>,
    ) -> Result<PaginatedEventResponse<E>, Self::Error>
    where
        E: TryFrom<(u64, StoredEvent), Error = anyhow::Error> + Send + Sync;

    /// Get events by a range of slots and key.
    async fn get_events_by_slot_range_key<E>(
        &self,
        event_key: &str,
        slot_height_start: u64,
        slot_height_end: u64,
        num_events: usize,
        next: Option<&str>,
    ) -> Result<PaginatedEventResponse<E>, Self::Error>
    where
        E: TryFrom<(u64, StoredEvent), Error = anyhow::Error> + Send + Sync;
}

#[async_trait]
impl LedgerStateProviderExt for LedgerDb {
    async fn get_events_by_key<E>(
        &self,
        event_key: &str,
        txn_range: Option<(u64, u64)>,
        num_events: usize,
        next: Option<&str>,
    ) -> Result<PaginatedEventResponse<E>, anyhow::Error>
    where
        E: TryFrom<(u64, StoredEvent), Error = anyhow::Error> + Send + Sync,
    {
        get_events_by_key_helper::<E>(self, event_key, txn_range, num_events, next).await
    }

    async fn get_events_by_slot_range_key<E>(
        &self,
        event_key: &str,
        slot_height_start: u64,
        slot_height_end: u64,
        num_events: usize,
        next: Option<&str>,
    ) -> Result<PaginatedEventResponse<E>, anyhow::Error>
    where
        E: TryFrom<(u64, StoredEvent), Error = anyhow::Error> + Send + Sync,
    {
        get_events_by_key_slot_range_helper::<E>(
            self,
            event_key,
            slot_height_start,
            slot_height_end,
            num_events,
            next,
        )
        .await
    }
}
