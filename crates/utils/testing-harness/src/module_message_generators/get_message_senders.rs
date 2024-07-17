use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use sov_bank::Bank;
use sov_modules_api::prelude::tokio;
use sov_modules_api::Spec;
use sov_prover_incentives::ProverIncentives;
use sov_rollup_interface::services::da::DaService;
use tokio::sync::mpsc::Sender;

use super::bank::BankMessageGenerator;
use super::prover_incentives::ProverIncentivesMessageGenerator;
use super::{MessageSender, MessageSenderT};
use crate::account_pool::AccountPool;
use crate::call_messages::SerializedPreparedCallMessage;

pub(crate) fn get_message_senders<S: Spec, Da: DaService>(
    should_stop: Arc<AtomicBool>,
    account_pool: AccountPool<S>,
    serialized_messages_tx: Sender<SerializedPreparedCallMessage>,
) -> Vec<Box<dyn MessageSenderT>> {
    let message_sender_1: MessageSender<
        demo_stf::runtime::Runtime<S, <Da as DaService>::Spec>,
        <Da as DaService>::Spec,
        S,
        Bank<S>,
    > = MessageSender::new(
        "bank",
        should_stop.clone(),
        Box::new(BankMessageGenerator::new(account_pool.clone())),
        serialized_messages_tx.clone(),
    );

    #[allow(clippy::type_complexity)]
    let message_sender_2: MessageSender<
        demo_stf::runtime::Runtime<S, <Da as DaService>::Spec>,
        <Da as DaService>::Spec,
        S,
        ProverIncentives<S, <Da as DaService>::Spec>,
    > = MessageSender::new(
        "prover incentives",
        should_stop.clone(),
        Box::new(ProverIncentivesMessageGenerator::new(account_pool.clone())),
        serialized_messages_tx.clone(),
    );

    vec![Box::new(message_sender_1), Box::new(message_sender_2)]
}
