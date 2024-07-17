#![no_main]

use libfuzzer_sys::fuzz_target;
use sha2::Sha256;
use sov_modules_api::Address;

fuzz_target!(|data: &[u8]| {
    serde_json::from_slice::<Address<Sha256>>(data).ok();
});
