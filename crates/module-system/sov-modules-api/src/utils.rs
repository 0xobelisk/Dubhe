use jsonrpsee::types::ErrorObjectOwned;
use sov_rollup_interface::digest::Digest;
use sov_rollup_interface::zk::CryptoSpec;

use crate::Spec;

pub fn generate_address<S: Spec>(key: &str) -> S::Address
where
    S::Address: From<[u8; 32]>,
{
    let hash: [u8; 32] = <S::CryptoSpec as CryptoSpec>::Hasher::digest(key.as_bytes()).into();
    S::Address::from(hash)
}

pub fn to_jsonrpsee_error_object(err: impl ToString, message: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        jsonrpsee::types::error::UNKNOWN_ERROR_CODE,
        message,
        Some(err.to_string()),
    )
}
