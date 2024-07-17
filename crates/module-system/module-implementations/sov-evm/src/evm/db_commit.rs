use revm::primitives::{Account, Address, HashMap};
use revm::DatabaseCommit;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::InfallibleStateAccessor;

use super::db::EvmDb;
use super::DbAccount;

impl<Ws: InfallibleStateAccessor> DatabaseCommit for EvmDb<Ws> {
    fn commit(&mut self, mut changes: HashMap<Address, Account>) {
        // Cloned to release borrow
        let mut addresses: Vec<_> = changes.keys().cloned().collect();
        // Sort addresses to avoid non-determinism in ZK
        addresses.sort();

        for address in addresses {
            // Unwrap because we took key from map itself, so key exists by definition.
            let account = changes.remove(&address).unwrap();

            // TODO figure out what to do when account is destroyed.
            // https://github.com/Sovereign-Labs/sovereign-sdk/issues/425
            if account.is_selfdestructed() {
                todo!("Account destruction not supported")
            }

            let accounts_prefix = self.accounts.prefix();

            let mut db_account = self
                .accounts
                .get(&address, &mut self.state)
                .unwrap_infallible()
                .unwrap_or_else(|| DbAccount::new(accounts_prefix, address));

            let account_info = account.info;

            if let Some(ref code) = account_info.code {
                if !code.is_empty() {
                    // TODO: would be good to have a contains_key method on the StateMap that would be optimized, so we can check the hash before storing the code
                    self.code
                        .set(&account_info.code_hash, &code.bytecode, &mut self.state)
                        .unwrap_infallible();
                }
            }

            db_account.info = account_info;

            // Sort keys explicitly to avoid non-determinism.
            let mut account_storage_keys: Vec<_> = account.storage.keys().collect();
            account_storage_keys.sort();

            for key in account_storage_keys {
                // Unwrap because we took key from map itself, so key exists by definition.
                let value = account.storage.get(key).unwrap();
                let value = value.present_value();
                db_account
                    .storage
                    .set(key, &value, &mut self.state)
                    .unwrap_infallible();
            }

            self.accounts
                .set(&address, &db_account, &mut self.state)
                .unwrap_infallible();
        }
    }
}
