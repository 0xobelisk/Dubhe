use std::marker::PhantomData;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use sov_modules_api::{DaSpec, EncodeCall, Module, Spec};
use sov_modules_stf_blueprint::Runtime;
use tokio::sync::mpsc::Sender;
use tokio::time::{self, Duration};

use crate::call_messages::{PreparedCallMessage, SerializedPreparedCallMessage};

#[async_trait]
pub(crate) trait MessageSenderT {
    async fn send_messages(self: Box<Self>, max_num_txs: Option<usize>, interval: Option<u64>);
}

pub(crate) struct MessageSender<R: Runtime<S, Da>, Da: DaSpec, S: Spec, M: Module<Spec = S>> {
    name: String,
    should_stop: Arc<AtomicBool>,
    message_generator: Box<dyn Iterator<Item = PreparedCallMessage<S, M>> + Send + Sync>,
    sender: Sender<SerializedPreparedCallMessage>,
    _phantom: PhantomData<(R, Da)>,
}

impl<R: Runtime<S, Da>, Da: DaSpec, S: Spec, M: Module<Spec = S>> MessageSender<R, Da, S, M> {
    pub(crate) fn new(
        name: &str,
        should_stop: Arc<AtomicBool>,
        message_generator: Box<dyn Iterator<Item = PreparedCallMessage<S, M>> + Send + Sync>,
        sender: Sender<SerializedPreparedCallMessage>,
    ) -> Self {
        Self {
            sender,
            should_stop,
            message_generator,
            _phantom: PhantomData,
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl<
        R: Runtime<S, Da> + EncodeCall<M>,
        Da: DaSpec,
        S: Spec,
        M: Module<Spec = S> + Send + Sync + 'static,
    > MessageSenderT for MessageSender<R, Da, S, M>
where
    M::CallMessage: Send + Sync,
{
    async fn send_messages(
        self: Box<Self>,
        max_num_txs: Option<usize>,
        maybe_interval: Option<u64>,
    ) {
        let log_prefix = format!("{} message sender:", self.name);
        let mut interval = if let Some(interval_milliseconds) = maybe_interval {
            tracing::info!("{log_prefix} sending messages roughly every {interval_milliseconds}ms");
            time::interval(Duration::from_millis(interval_milliseconds))
        } else {
            tracing::debug!("{log_prefix} is not using an interval when sending messages");
            // NOTE This panics if set to zero, so we use a tiny interval to mimic no interval instead.
            time::interval(Duration::from_millis(1))
        };

        interval.tick().await; // NOTE The first tick completes immediately.

        let sender = self.sender.clone();
        let should_stop = self.should_stop.clone();
        let generators = self.message_generator;

        tokio::spawn(async move {
            let generators = generators.into_iter();
            let mut num_messages = 0;

            for message in generators {
                if should_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    tracing::debug!("{log_prefix} should stop switch is flipped");
                    break;
                };

                if Some(num_messages) >= max_num_txs {
                    tracing::debug!("{log_prefix} hit maximum number of messages");
                    break;
                };

                let runtime_msg = <R as EncodeCall<M>>::encode_call(message.call_message);

                let serialized_msg = SerializedPreparedCallMessage {
                    max_fee: message.max_fee,
                    call_message: runtime_msg,
                    account_pool_index: message.account_pool_index,
                };

                match sender.send(serialized_msg).await {
                    Ok(_) => {
                        num_messages += 1;
                        tracing::debug!("{log_prefix} message #{num_messages} sent");
                    }
                    Err(err) => {
                        tracing::error!("{log_prefix} error sending serialized message #{num_messages} to sender: {err}");
                    }
                };

                interval.tick().await;
            }
        });
    }
}
