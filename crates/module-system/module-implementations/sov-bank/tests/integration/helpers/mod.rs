use sov_bank::{BankConfig, GasTokenConfig};
use sov_modules_api::utils::generate_address as gen_address_generic;
use sov_modules_api::Spec;

type S = sov_test_utils::TestSpec;

// This code is not actually dead; rustc treats each test file as a separate crate
// so this code looks unused during some of the compilations.
pub fn generate_address(name: &str) -> <S as Spec>::Address {
    gen_address_generic::<S>(name)
}

pub fn create_bank_config_with_token(
    addresses_count: usize,
    initial_balance: u64,
) -> BankConfig<S> {
    let address_and_balances = (0..addresses_count)
        .map(|i| {
            let key = format!("key_{}", i);
            let addr = generate_address(&key);
            (addr, initial_balance)
        })
        .collect();

    let token_name = "InitialToken".to_owned();

    BankConfig {
        gas_token_config: GasTokenConfig {
            token_name,
            address_and_balances,
            authorized_minters: vec![],
        },
        tokens: vec![],
    }
}
