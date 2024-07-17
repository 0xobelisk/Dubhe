use std::rc::Rc;

use sov_attester_incentives::CallMessage;
use sov_modules_api::transaction::PriorityFeeBips;
use sov_modules_api::{CryptoSpec, DaSpec, Spec};

use crate::{Message, MessageGenerator};

/// Generates messages for the attester incentives module.
pub struct AttesterIncentivesMessageGenerator<S: Spec, Da: DaSpec>(
    #[allow(clippy::type_complexity)]
    Vec<(
        <S::CryptoSpec as CryptoSpec>::PrivateKey,
        CallMessage<S, Da>,
    )>,
);

impl<S: Spec, Da: DaSpec>
    From<
        Vec<(
            <S::CryptoSpec as CryptoSpec>::PrivateKey,
            CallMessage<S, Da>,
        )>,
    > for AttesterIncentivesMessageGenerator<S, Da>
{
    fn from(
        messages: Vec<(
            <S::CryptoSpec as CryptoSpec>::PrivateKey,
            CallMessage<S, Da>,
        )>,
    ) -> Self {
        Self(messages)
    }
}

impl<S: Spec, Da: DaSpec> MessageGenerator for AttesterIncentivesMessageGenerator<S, Da> {
    type Module = sov_attester_incentives::AttesterIncentives<S, Da>;
    type Spec = S;

    fn create_messages(
        &self,
        chain_id: u64,
        max_priority_fee_bips: PriorityFeeBips,
        max_fee: u64,
        gas_usage: Option<<Self::Spec as Spec>::Gas>,
    ) -> Vec<Message<Self::Spec, Self::Module>> {
        let mut nonce = 0;

        self.0
            .iter()
            .map(|(addr, call_message)| {
                nonce += 1;

                Message::new(
                    Rc::new(addr.clone()),
                    call_message.clone(),
                    chain_id,
                    max_priority_fee_bips,
                    max_fee,
                    gas_usage.clone(),
                    nonce,
                )
            })
            .collect::<Vec<Message<Self::Spec, Self::Module>>>()
    }
}
