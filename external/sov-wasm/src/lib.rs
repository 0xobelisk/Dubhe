// #![no_std]

extern crate alloc;

use crate::alloc::string::ToString;
pub use alloc::vec::Vec;
use core::{mem, slice};

use borsh::BorshSerialize;
use serde::de::DeserializeOwned;
use serde_json::Error;

use sov_mock_da::MockDaSpec;
use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::default_spec::ZkDefaultSpec;
use sov_modules_api::transaction::{Transaction, UnsignedTransaction};
use sov_risc0_adapter::Risc0Verifier;
use sov_rollup_interface::zk::CryptoSpec;

use sov_modules_api::Spec;
use stf_starter::runtime::RuntimeCall;

pub type ZkSpec = ZkDefaultSpec<Risc0Verifier, MockZkVerifier>;

pub type CSpec = <ZkSpec as Spec>::CryptoSpec;
pub type PublicKey = <CSpec as CryptoSpec>::PublicKey;
pub type Signature = <CSpec as CryptoSpec>::Signature;
pub type Address = <ZkSpec as Spec>::Address;

pub type Call = RuntimeCall<ZkSpec, MockDaSpec>;
pub type UnsignedTx = UnsignedTransaction<ZkSpec>;
pub type Tx = Transaction<ZkSpec>;

#[no_mangle]
pub fn serialize_call(ptr: usize, len: usize) -> usize {
    get_argument_from_memory_borsh_serialize_and_return::<Call>(ptr, len)
}

#[no_mangle]
pub fn serialize_unsigned_transaction(ptr: usize, len: usize) -> usize {
    get_argument_from_memory_borsh_serialize_and_return::<UnsignedTx>(ptr, len)
}

#[no_mangle]
pub fn serialize_transaction(ptr: usize, len: usize) -> usize {
    get_argument_from_memory_borsh_serialize_and_return::<Tx>(ptr, len)
}

#[no_mangle]
pub fn bech_encode_public_key(ptr: usize, len: usize) -> usize {
    let public_key: PublicKey = match get_json_argument_as_type::<PublicKey>(ptr as *const u8, len)
    {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let address: Address = (&public_key).into();

    match return_bytes(address.to_string().into_bytes()) {
        Ok(c) => c,
        Err(_) => 2,
    }
}

/// Allocates memory into the module.
#[no_mangle]
pub fn alloc(len: usize) -> usize {
    let mut buf: Vec<u8> = Vec::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    mem::forget(buf);
    ptr as usize
}

/// Deallocates memory from the module.
#[no_mangle]
pub fn dealloc(ptr: usize, len: usize) {
    let data = unsafe { Vec::from_raw_parts(ptr as *mut u8, len, len) };
    mem::drop(data);
}

/// Deallocates an array from the memory of the module.
#[no_mangle]
pub fn dealloc_array(ptr: usize) {
    let len = unsafe { slice::from_raw_parts(ptr as *const u8, 4) };
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(len);
    let len = u32::from_le_bytes(len_bytes) as usize;
    let data = unsafe { Vec::from_raw_parts(ptr as *mut u8, len, len) };
    mem::drop(data);
}

fn get_argument_from_memory_borsh_serialize_and_return<T>(ptr: usize, len: usize) -> usize
where
    T: DeserializeOwned + BorshSerialize,
{
    let tx: T = match get_json_argument_as_type::<T>(ptr as *const u8, len) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut bytes: Vec<u8> = Vec::new();
    if BorshSerialize::serialize(&tx, &mut bytes).is_err() {
        return 1;
    }

    match return_bytes(bytes) {
        Ok(c) => c,
        Err(_) => 2,
    }
}

/// Gets JSON argument from Javascript environment
///
/// Assumes that there is a Serde/JSON serialized object of type T at the given pointer
/// location with length len and returns the object as type T.
fn get_json_argument_as_type<T>(ptr: *const u8, len: usize) -> Result<T, Error>
where
    T: DeserializeOwned + BorshSerialize,
{
    let json = unsafe { slice::from_raw_parts(ptr, len) };

    serde_json::from_slice(json)
}

/// Returns bytes to Javascript environment
///
/// Prepends the byte vector with the length of the vector, forgets the allocated byte vector so that
/// the byte vector does not get dropped as we exit this WASM environment, and lastly returns the
/// pointer address so that we can retrieve the byte vector from the Javascript environment.
fn return_bytes(bytes: Vec<u8>) -> Result<usize, Error> {
    let mut result = Vec::with_capacity(4 + bytes.len());
    result.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    result.extend_from_slice(&bytes);
    let ptr = result.as_ptr();

    // Ensure the vector is not deallocated
    mem::forget(result);

    Ok(ptr as usize)
}

#[cfg(test)]
mod tests {
    use crate::*;
    use core::slice;

    const RUNTIME_CALL: &str = r#"{"bank":{"Freeze":{"token_id":"token_1rwrh8gn2py0dl4vv65twgctmlwck6esm2as9dftumcw89kqqn3nqrduss6"}}}"#;

    const SIGNATURE: &str = r#"{"msg_sig": [229, 55, 228, 74, 152, 226, 164, 73, 9, 241, 162, 220, 213, 41, 102, 30, 179, 59, 57, 69, 197, 168, 107, 172, 149, 255, 191, 210, 69, 236, 249, 120, 150, 59, 1, 169, 146, 3, 199, 54, 80, 181, 59, 34, 181, 115, 10, 241, 42, 30, 1, 248, 92, 56, 227, 211, 134, 105, 117, 254, 133, 222, 76, 6]}"#;

    const PUBKEY: &str = r#""e9cf61f2b4c9402e3b4742b44ec2b8cbc6184a3a6d7c64c7e2e371f6cae160ff""#;

    const UNSIGNED_TX_JSON: &str = r#"{"runtime_msg":[1,0,11,0,0,0,0,0,0,0,14,0,0,0,115,111,118,45,116,101,115,116,45,116,111,107,101,110,64,66,15,0,0,0,0,0,163,32,25,84,247,10,214,34,48,220,61,132,10,91,247,103,112,44,4,134,158,133,171,62,238,11,150,40,87,186,117,152,2,0,0,0,254,166,172,91,135,81,18,15,182,47,255,103,181,77,46,172,102,174,243,7,199,221,225,211,148,222,161,224,158,67,221,68,163,32,25,84,247,10,214,34,48,220,61,132,10,91,247,103,112,44,4,134,158,133,171,62,238,11,150,40,87,186,117,152],"nonce":0,"chain_id":0,"max_priority_fee_bips":0,"max_fee":3000}"#;

    const ADDRESS_BYTES: &str = r#"[11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11]"#;

    const PUBKEY_BYTES_JSON_ENCODED: [u8; 66] = [
        34, 101, 57, 99, 102, 54, 49, 102, 50, 98, 52, 99, 57, 52, 48, 50, 101, 51, 98, 52, 55, 52,
        50, 98, 52, 52, 101, 99, 50, 98, 56, 99, 98, 99, 54, 49, 56, 52, 97, 51, 97, 54, 100, 55,
        99, 54, 52, 99, 55, 101, 50, 101, 51, 55, 49, 102, 54, 99, 97, 101, 49, 54, 48, 102, 102,
        34,
    ];

    fn take_bytes(ptr: usize) -> Vec<u8> {
        let ptr = ptr as *mut u8;
        let len = unsafe { slice::from_raw_parts(ptr, 4) };
        let len: [u8; 4] = len.try_into().unwrap();
        let len = u32::from_le_bytes(len) as usize;
        unsafe { Vec::from_raw_parts(ptr, len + 4, len + 4).split_off(4) }
    }

    #[test]
    fn test_serializing_unsigned_tx() {
        let chain_id: u64 = 0;
        let nonce: u64 = 0;
        let max_priority_fee: u64 = 0;
        let max_fee: u64 = 0;
        let gas_limit: Option<<ZkSpec as Spec>::Gas> = None;

        let call_ptr = RUNTIME_CALL.as_ptr() as usize;
        let call_len = RUNTIME_CALL.len();

        let runtime_msg_ptr = serialize_call(call_ptr, call_len);
        let runtime_msg = take_bytes(runtime_msg_ptr);

        let unsigned_tx = UnsignedTx::new(
            runtime_msg,
            chain_id,
            max_priority_fee.into(),
            max_fee,
            nonce,
            gas_limit,
        );
        let unsigned_tx_json_ser = serde_json::to_string(&unsigned_tx).unwrap();
        let unsigned_tx_json_ser_ptr = unsigned_tx_json_ser.as_ptr() as usize;
        let unsigned_tx_json_ser_len = unsigned_tx_json_ser.len();

        let unsigned_tx_ptr =
            serialize_unsigned_transaction(unsigned_tx_json_ser_ptr, unsigned_tx_json_ser_len);
        let _ = take_bytes(unsigned_tx_ptr);

        let dummy_pub_key: PublicKey = serde_json::from_str(&PUBKEY).unwrap();
        let dummy_sig: Signature = serde_json::from_str(SIGNATURE).unwrap();
        let tx = unsigned_tx.to_signed_tx(dummy_pub_key, dummy_sig);

        let tx_json_ser = serde_json::to_string(&tx).unwrap();
        let tx_ptr = tx_json_ser.as_ptr() as usize;
        let tx_len = tx_json_ser.len();

        let tx_ptr = serialize_unsigned_transaction(tx_ptr, tx_len);
        let _ = take_bytes(tx_ptr);
    }

    #[test]
    fn test_unsigned_transaction_camel_case_deser() {
        let call_ptr = UNSIGNED_TX_JSON.as_ptr() as usize;
        let call_len = UNSIGNED_TX_JSON.len();

        let utx: UnsignedTx =
            get_json_argument_as_type::<UnsignedTx>(call_ptr as *const u8, call_len).unwrap();
        assert_eq!(utx.chain_id, 0);
        assert_eq!(utx.nonce, 0);
        assert_eq!(utx.max_fee, 3000);
    }

    #[test]
    fn test_pubkey_deser() {
        let _: PublicKey = serde_json::from_str(&PUBKEY).unwrap();
    }

    #[test]
    fn test_sig_deser() {
        let _: Signature = serde_json::from_str(SIGNATURE).unwrap();
    }

    #[test]
    fn test_address_deserialization() {
        let address_ptr = ADDRESS_BYTES.as_ptr() as usize;
        let address_len = ADDRESS_BYTES.len();

        let address_bytes: [u8; 32] =
            get_json_argument_as_type::<[u8; 32]>(address_ptr as *const u8, address_len).unwrap();
        let address = Address::from(address_bytes);

        assert_eq!(
            address.to_string(),
            "sov1pv9skzctpv9skzctpv9skzctpv9skzctpv9skzctpv9skzctpv9stup8tx"
        );
    }

    #[test]
    fn test_public_key_to_bech_encoded_address() {
        let public_key: PublicKey = serde_json::from_str(&PUBKEY).unwrap();
        let address: Address = (&public_key).into();

        assert_eq!(
            address.to_string(),
            "sov1dpvv2cvyv3cfsflx4hz67gyqcm3xlrj5v2ldyagep2uf8fwt7drsv936j0"
        );
    }

    #[test]
    fn test_bech_encode() {
        let pubkey_ptr = PUBKEY_BYTES_JSON_ENCODED.as_ptr() as usize;
        let pubkey_len = PUBKEY_BYTES_JSON_ENCODED.len();

        assert_ne!(0, bech_encode_public_key(pubkey_ptr, pubkey_len));
    }
}
