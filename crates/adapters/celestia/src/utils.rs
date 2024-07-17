use prost::encoding::decode_varint;
use prost::DecodeError;
use sov_rollup_interface::Buf;

pub type BoxError = anyhow::Error;

/// Read a varint. Returns the value (as a u64) and the number of bytes read
pub fn read_varint(mut bytes: impl Buf) -> Result<(u64, usize), DecodeError> {
    let original_len = bytes.remaining();
    let varint = decode_varint(&mut bytes)?;
    let amount = original_len
        .checked_sub(bytes.remaining())
        .ok_or_else(|| DecodeError::new("invalid buffer state"))?;
    Ok((varint, amount))
}
