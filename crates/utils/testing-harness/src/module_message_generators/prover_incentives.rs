use std::marker::PhantomData;

use sov_modules_api::{DaSpec, Spec};
use sov_prover_incentives::ProverIncentives;

use crate::account_pool::AccountPool;
use crate::call_messages::PreparedCallMessage;
use crate::constants::DEFAULT_MAX_FEE;

#[derive(Clone)]
pub(crate) struct ProverIncentivesMessageGenerator<S: Spec, Da: DaSpec> {
    message_count: u64,
    account_pool: AccountPool<S>,
    account_pool_index: u64,
    _phantom: PhantomData<Da>,
}

impl<S: Spec, Da: DaSpec> ProverIncentivesMessageGenerator<S, Da> {
    pub(crate) fn new(account_pool: AccountPool<S>) -> Self {
        Self {
            account_pool,
            message_count: 0,
            account_pool_index: 0,
            _phantom: PhantomData,
        }
    }
}

impl<S: Spec, Da: DaSpec> Iterator for ProverIncentivesMessageGenerator<S, Da> {
    type Item = PreparedCallMessage<S, ProverIncentives<S, Da>>;

    fn next(&mut self) -> Option<Self::Item> {
        let account_pool_index = self.account_pool_index;

        let prepared_call_message = PreparedCallMessage::<S, ProverIncentives<S, Da>> {
            call_message: sov_prover_incentives::CallMessage::BondProver(self.message_count),
            account_pool_index,
            max_fee: DEFAULT_MAX_FEE,
        };

        // NOTE So that we iterate through the account pool when sending the messages.
        self.message_count += 1;

        // NOTE: So that we iterate over the account pool indefinitely.
        if self.account_pool.len() as u64 >= self.account_pool_index {
            self.account_pool_index = 0;
        } else {
            self.account_pool_index += 1;
        };

        Some(prepared_call_message)
    }
}
