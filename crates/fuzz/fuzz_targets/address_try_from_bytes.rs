#![no_main]

use libfuzzer_sys::fuzz_target;
use sha2::Sha256;
use sov_modules_api::Address;

fuzz_target!(|data: &[u8]| {
    let _ = Address::<Sha256>::try_from(data);
});
