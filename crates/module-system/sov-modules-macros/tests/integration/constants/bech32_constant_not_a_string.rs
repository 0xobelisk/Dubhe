use sov_bank::TokenId;
use sov_modules_api::macros::config_bech32;

const TEST_ARRAY_OF_U8: TokenId = config_bech32!("TEST_ARRAY_OF_U8", TokenId);

fn main() {
    assert_eq!(TEST_ARRAY_OF_U8, TokenId::from([0; 32]));
}
