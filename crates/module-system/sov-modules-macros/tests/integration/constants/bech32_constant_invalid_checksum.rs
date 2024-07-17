use sov_bank::TokenId;
use sov_modules_api::macros::config_bech32;

const TEST_TOKEN_ID_INVALID_CHECKSUM: TokenId =
    config_bech32!("TEST_TOKEN_ID_INVALID_CHECKSUM", TokenId);

fn main() {
    assert_eq!(TEST_TOKEN_ID_INVALID_CHECKSUM, TokenId::from([0; 32]));
}
