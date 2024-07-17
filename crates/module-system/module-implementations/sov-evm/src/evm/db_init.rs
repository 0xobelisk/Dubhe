use reth_primitives::Bytes;
#[cfg(test)]
use revm::db::{CacheDB, EmptyDB};
use revm::primitives::{AccountInfo, Address, B256};
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::InfallibleStateAccessor;

use super::db::EvmDb;
use super::DbAccount;

/// Initializes database with a predefined account.
pub(crate) trait InitEvmDb {
    fn insert_account_info(&mut self, address: Address, acc: AccountInfo);
    fn insert_code(&mut self, code_hash: B256, code: Bytes);
}

impl<Accessor: InfallibleStateAccessor> InitEvmDb for EvmDb<Accessor> {
    fn insert_account_info(&mut self, sender: Address, info: AccountInfo) {
        let parent_prefix = self.accounts.prefix();
        let db_account = DbAccount::new_with_info(parent_prefix, sender, info);

        self.accounts
            .set(&sender, &db_account, &mut self.state)
            .unwrap_infallible();
    }

    fn insert_code(&mut self, code_hash: B256, code: Bytes) {
        self.code
            .set(&code_hash, &code, &mut self.state)
            .unwrap_infallible();
    }
}

#[cfg(test)]
impl InitEvmDb for CacheDB<EmptyDB> {
    fn insert_account_info(&mut self, sender: Address, acc: AccountInfo) {
        self.insert_account_info(sender, acc);
    }

    fn insert_code(&mut self, code_hash: B256, code: Bytes) {
        self.contracts
            .insert(code_hash, revm::primitives::Bytecode::new_raw(code));
    }
}
