use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::mpsc::Sender;

use crate::call_messages::SerializedPreparedCallMessage;

pub(crate) fn start_ctrl_c_handler(
    should_stop: Arc<AtomicBool>,
    serialized_messages_tx: Sender<SerializedPreparedCallMessage>,
) {
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!("ctrl c handler errored: {err}");
        }
        tracing::info!("ctrl c signal caught");
        should_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        // NOTE: Send a message to the blob sender so it turns the loop one last time and exits
        // due to above switch flip.
        let _ = serialized_messages_tx
            .send(SerializedPreparedCallMessage::default())
            .await;
    });
}
