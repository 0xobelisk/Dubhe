use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures::StreamExt;
use sov_ledger_json_client::types as ledger_api_types;
use sov_modules_api::prelude::tokio;
use tokio::task::JoinHandle;

use crate::args::Args;

pub(crate) fn start_slot_watcher_task(
    config: &Args,
    should_stop: Arc<AtomicBool>, // FIXME use this instead of updating slot numbers
) -> JoinHandle<(u64, u64)> {
    let rest_url = format!("{}/ledger", config.rest_url.clone());
    tracing::info!("using REST URL: {rest_url}");

    tokio::spawn(async move {
        let mut successful_count = 0;
        let mut error_count = 0;

        let ledger_client = sov_ledger_json_client::Client::new(&rest_url);

        tracing::info!("Starting slot watcher");

        let mut slot_subscription = ledger_client
            .subscribe_slots()
            .await
            .expect("should be able to subscribe to slots");

        loop {
            let slot_number = match slot_subscription.next().await.transpose() {
                Ok(slot) => slot.map(|s| s.number).unwrap_or_default(),
                Err(e) => {
                    tracing::error!(error = ?e, "Error during next slot subscription, resubscribing");
                    slot_subscription = ledger_client
                        .subscribe_slots()
                        .await
                        .expect("should be able to subscribe to slots");
                    continue;
                }
            };
            tracing::info!(slot = ?slot_number, "Received processed slot");

            let slot_response = ledger_client
                .get_slot_by_id(
                    &ledger_api_types::IntOrHash::Variant0(slot_number),
                    Some(ledger_api_types::GetSlotByIdChildren::_0),
                )
                .await
                .expect("should be able to get slots by id");
            let slot = &slot_response.data;

            let batches = &slot.batches;
            tracing::debug!(
                hash = slot.hash.as_str(),
                batches = batches.len(),
                "Inspecting slot"
            );

            for batch in batches {
                let txs = &batch.txs;
                tracing::debug!(txs = txs.len(), "Inspecting batch");
                for tx_response in txs {
                    match tx_response.receipt.result {
                        ledger_api_types::TxReceiptResult::Reverted
                        | ledger_api_types::TxReceiptResult::Skipped => {
                            error_count += 1;
                        }
                        ledger_api_types::TxReceiptResult::Successful => {
                            successful_count += 1;
                        }
                    }
                }
            }

            if should_stop.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::info!(slot = ?slot_number, "slot watcher exiting loop");
                break;
            }
        }
        (successful_count, error_count)
    })
}
