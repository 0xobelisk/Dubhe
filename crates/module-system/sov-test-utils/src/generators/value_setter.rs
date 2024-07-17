use sov_modules_api::{CryptoSpec, PrivateKey, Spec};
use sov_value_setter::ValueSetter;

use crate::*;

pub struct ValueSetterMessage<S: Spec> {
    pub admin: Rc<<S::CryptoSpec as CryptoSpec>::PrivateKey>,
    pub messages: Vec<u32>,
}

pub struct ValueSetterMessages<S: Spec> {
    pub messages: Vec<ValueSetterMessage<S>>,
}

impl<S: Spec> ValueSetterMessages<S> {
    pub fn new(messages: Vec<ValueSetterMessage<S>>) -> Self {
        Self { messages }
    }

    /// Returns a message containing one admin and two value setter messages.
    pub fn prepopulated() -> Self {
        Self::new(vec![ValueSetterMessage {
            admin: Rc::new(<<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey::generate()),
            messages: vec![99, 33],
        }])
    }
}

impl<S: Spec> MessageGenerator for ValueSetterMessages<S> {
    type Module = ValueSetter<S>;
    type Spec = S;

    fn create_messages(
        &self,
        chain_id: u64,
        max_priority_fee_bips: PriorityFeeBips,
        max_fee: u64,
        gas_usage: Option<<Self::Spec as Spec>::Gas>,
    ) -> Vec<Message<Self::Spec, Self::Module>> {
        let mut messages = Vec::default();
        for value_setter_message in &self.messages {
            let admin = value_setter_message.admin.clone();

            for (value_setter_admin_nonce, new_value) in
                value_setter_message.messages.iter().enumerate()
            {
                let set_value_msg: sov_value_setter::CallMessage =
                    sov_value_setter::CallMessage::SetValue(*new_value);

                messages.push(Message::new(
                    admin.clone(),
                    set_value_msg,
                    chain_id,
                    max_priority_fee_bips,
                    max_fee,
                    gas_usage.clone(),
                    value_setter_admin_nonce.try_into().unwrap(),
                ));
            }
        }
        messages
    }
}
