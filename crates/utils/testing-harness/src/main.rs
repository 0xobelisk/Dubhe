#![doc = include_str!("../README.md")]

mod account_pool;
mod args;
mod call_messages;
mod constants;
mod ctrl_c_handler;
mod da_blob_sender;
mod harness_config;
mod logging;
mod module_message_generators;
mod slot_watcher;
mod types;
mod utils;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use account_pool::AccountPool;
use call_messages::SerializedPreparedCallMessage;
use clap::Parser;
use constants::DEFAULT_CHANNEL_SIZE;
use harness_config::HarnessConfig;
use slot_watcher::start_slot_watcher_task;
use sov_celestia_adapter::types::Namespace;
use sov_celestia_adapter::verifier::RollupParams;
use sov_celestia_adapter::CelestiaService;
use sov_modules_api::capabilities::Authenticator;
use sov_modules_api::prelude::tokio;
use sov_modules_api::Spec;
use sov_rollup_interface::services::da::{DaService, DaServiceWithRetries};
use types::{ThisAuth, ThisDaService, ThisSpec};

use crate::args::Args;
use crate::ctrl_c_handler::start_ctrl_c_handler;
use crate::da_blob_sender::DaBlobSender;
use crate::logging::initialize_logging;
use crate::module_message_generators::{get_gas_funding_message_sender, get_message_senders};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    start::<ThisSpec, ThisDaService, ThisAuth>().await
}

async fn start<S: Spec, Da: DaService, Auth: Authenticator>() -> anyhow::Result<()> {
    initialize_logging();
    let config = Args::parse();

    let account_pool = AccountPool::new_from_config(&config).await?;

    let should_stop = Arc::new(AtomicBool::new(false));

    let (serialized_messages_tx, serialized_messages_rx) =
        tokio::sync::mpsc::channel::<SerializedPreparedCallMessage>(DEFAULT_CHANNEL_SIZE);
    let harness_config = HarnessConfig::from_toml_path(&config.rollup_config_path)?;
    let da_service = CelestiaService::new(
        harness_config.da,
        RollupParams {
            rollup_batch_namespace: config.get_rollup_batch_namespace()?,
            // We don't need proof namespace for this iteration
            rollup_proof_namespace: Namespace::MAX_PRIMARY_RESERVED,
        },
    )
    .await;

    let gas_funding_message_sender = get_gas_funding_message_sender::<S, Da>(
        &config,
        account_pool.clone(),
        serialized_messages_tx.clone(),
        should_stop.clone(),
    )
    .await?;

    let module_message_senders = get_message_senders::<S, Da>(
        should_stop.clone(),
        account_pool.clone(),
        serialized_messages_tx.clone(),
    );

    // NOTE: We send the funding messages first without waiting on an interval. We know this
    // iterator has a finite length.
    Box::new(gas_funding_message_sender)
        .send_messages(config.max_num_txs, None)
        .await;

    // Now we set the other senders running, these iterators may or may not end.
    for sender in module_message_senders.into_iter() {
        sender
            .send_messages(config.max_num_txs, config.interval)
            .await;
    }

    let da_blob_sender: DaBlobSender<S, Auth> = DaBlobSender::new(
        config.clone(),
        account_pool.clone(),
        DaServiceWithRetries::new_fast(da_service),
        serialized_messages_rx,
        should_stop.clone(),
    );

    let slot_watcher_handle = start_slot_watcher_task(&config, should_stop.clone());

    start_ctrl_c_handler(should_stop.clone(), serialized_messages_tx.clone());

    da_blob_sender.send_messages_to_da().await;

    // Collating results...
    tracing::info!("collecting results...");
    let (successful_count, error_count) = slot_watcher_handle.await?;

    tracing::info!(
        successful = successful_count,
        error = error_count,
        "All transactions has been submitted and should've been processed by now"
    );

    Ok(())
}
