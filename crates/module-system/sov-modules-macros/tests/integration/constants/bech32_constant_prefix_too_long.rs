use sov_modules_api::impl_hash32_type;
use sov_modules_api::macros::config_bech32;

impl_hash32_type!(MyTokenId, MyTokenBech, "tok");

const TEST_TOKEN_ID: MyTokenId = config_bech32!("TEST_TOKEN_ID", MyTokenId);

fn main() {
    assert_eq!(TEST_TOKEN_ID, MyTokenId::from([0; 32]));
}
