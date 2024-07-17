use sov_bank::Bank;
use sov_modules_api::Spec;

use crate::account_pool::AccountPool;
use crate::call_messages::PreparedCallMessage;
use crate::constants::DEFAULT_MAX_FEE;

#[derive(Clone)]
pub(crate) struct BankMessageGenerator<S: Spec> {
    message_count: u64,
    account_pool: AccountPool<S>,
    account_pool_index: u64,
}

impl<S: Spec> BankMessageGenerator<S> {
    pub(crate) fn new(account_pool: AccountPool<S>) -> Self {
        Self {
            message_count: 0,
            account_pool_index: 0,
            account_pool,
        }
    }
}

impl<S: Spec> Iterator for BankMessageGenerator<S> {
    type Item = PreparedCallMessage<S, Bank<S>>;

    fn next(&mut self) -> Option<Self::Item> {
        let account_pool_index = self.account_pool_index;
        let account = self
            .account_pool
            .get_by_index(&account_pool_index)
            .expect("could not get account from account pool at index: {index}");
        let address = account.address.clone();
        let message_count = self.message_count;
        let prepared_call_message = PreparedCallMessage::<S, Bank<S>> {
            call_message: sov_bank::CallMessage::CreateToken {
                salt: message_count,
                initial_balance: u64::MAX,
                token_name: format!("token-{message_count}"),
                mint_to_address: address.clone(),
                authorized_minters: vec![address],
            },
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
