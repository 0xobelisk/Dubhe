use std::marker::PhantomData;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::anyhow;
use sov_celestia_adapter::CelestiaService;
use sov_modules_api::capabilities::Authenticator;
use sov_modules_api::transaction::{PriorityFeeBips, Transaction, UnsignedTransaction};
use sov_modules_api::{BlobData, RawTx, Spec};
use sov_rollup_interface::services::da::{DaService, DaServiceWithRetries};
use tokio::sync::mpsc::Receiver;
use tokio::time::sleep;

use crate::account_pool::{Account, AccountPool};
use crate::args::Args;
use crate::call_messages::SerializedPreparedCallMessage;
use crate::constants::TIME_OUT_DURATION;

pub(crate) struct DaBlobSender<S: Spec, Auth: Authenticator> {
    config: Args,
    account_pool: AccountPool<S>,
    da_service: DaServiceWithRetries<CelestiaService>,
    receiver: Receiver<SerializedPreparedCallMessage>,
    should_stop: Arc<AtomicBool>,
    _phantom: PhantomData<Auth>,
}

impl<S: Spec, Auth: Authenticator> DaBlobSender<S, Auth> {
    pub(crate) fn new(
        config: Args,
        account_pool: AccountPool<S>,
        da_service: DaServiceWithRetries<CelestiaService>,
        receiver: Receiver<SerializedPreparedCallMessage>,
        should_stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            account_pool,
            da_service,
            receiver,
            should_stop,
            _phantom: PhantomData,
        }
    }

    pub(crate) async fn send_messages_to_da(self) {
        let mut receiver = self.receiver;
        let num_txs_per_batch = self.config.max_batch_size_tx;
        // TODO: Check batch size in bytes too!
        let mut tx_batch = Vec::with_capacity(num_txs_per_batch as usize);

        loop {
            if self.should_stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            };

            // NOTE: We race against a countdown and exit if no new messages are seen after
            // `TIME_OUT_DURATION`
            tokio::select! {
                _ = sleep(TIME_OUT_DURATION) => {
                    tracing::warn!("{}s have passed with no new messages, exiting blob sender loop", TIME_OUT_DURATION.as_secs());
                    self.should_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    break
                },
                maybe_message = receiver.recv() => {
                    if let Some(serialized_message) = maybe_message {

                        tracing::debug!("message received!");
                        let account_pool_index = serialized_message.account_pool_index;

                        let account = self.account_pool.get_by_index(&account_pool_index).expect(
                            "there should be an account at account pool index: {account_pool_index}",
                        );

                        match authorize_serialized_call_message::<S, Auth>(
                            &self.config,
                            account,
                            serialized_message,
                        ) {
                            Err(err) => {
                                tracing::error!("error when signing transaction: {err}");
                                tracing::info!(
                                    "ignoring transaction dues to failing to sign it, continuing..."
                                );
                                continue;
                            }
                            Ok(signed_tx) => {
                                // NOTE We only increment the nonce if we succeeded in signing the tx.
                                self.account_pool.inc_nonce(&account_pool_index);
                                tx_batch.push(signed_tx);

                                let num_txs_in_batch = tx_batch.len();
                                let batch_is_full = !tx_batch.is_empty()
                                    && num_txs_in_batch as u64 % num_txs_per_batch == 0;
                                tracing::debug!(
                                    num_txs_per_batch = num_txs_per_batch,
                                    num_txs_in_batch = num_txs_in_batch,
                                    "batch status:"
                                );

                                if batch_is_full {
                                    tracing::info!("batch is full of txs, submitting to DA layer...");
                                    let batch_to_submit = std::mem::take(&mut tx_batch);
                                    if let Err(err) =
                                        submit_transactions(&self.da_service, batch_to_submit).await
                                    {
                                        tracing::error!("error submitting batch to DA layer: {err}");
                                        // TODO now the nonces are non subsequent, handle this case.
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// TODO Move to own mod?
pub(crate) async fn submit_transactions<Da: DaService>(
    da_service: &Da,
    txs: Vec<RawTx>,
) -> anyhow::Result<()>
where
    Da::TransactionId: std::fmt::Display,
{
    let batch = BlobData::new_batch(txs);
    let batch_bytes = borsh::to_vec(&batch).expect("Failed to serialize batch");
    let fee = da_service
        .estimate_fee(batch_bytes.len())
        .await
        .map_err(|err| anyhow!(err))?;
    let tx_hash = da_service
        .send_transaction(&batch_bytes, fee)
        .await
        .map_err(|err| anyhow!(err))?;
    tracing::info!("Submitted tx, hash: {tx_hash}");
    Ok(())
}

pub(crate) fn authorize_serialized_call_message<S: Spec, Auth: Authenticator>(
    config: &Args,
    account: &Account<S>,
    serialized_message: SerializedPreparedCallMessage,
) -> anyhow::Result<RawTx> {
    let unsigned_tx = UnsignedTransaction::new(
        serialized_message.call_message,
        config.chain_id,
        PriorityFeeBips::from_percentage(config.priority_fee_percent),
        serialized_message.max_fee,
        account.nonce.load(std::sync::atomic::Ordering::Relaxed), // NOTE: The nonce is updated in the message sender above
        None,
    );

    let signed_tx = Transaction::<S>::new_signed_tx(&account.private_key, unsigned_tx);
    let signed_and_encoded_tx = Auth::encode(borsh::to_vec(&signed_tx)?)?;
    Ok(signed_and_encoded_tx)
}
