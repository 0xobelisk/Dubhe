use sov_modules_api::{CryptoSpec, ModuleId, ModuleInfo, Spec, StateMap};

#[derive(ModuleInfo)]
struct TestStruct<S: Spec> {
    #[id]
    id: ModuleId,

    // Unsupported attributes should be ignored to guarantee compatibility with
    // other macros.
    #[allow(dead_code)]
    #[state]
    test_state1: StateMap<u32, String>,

    #[state]
    test_state2: StateMap<<S::CryptoSpec as CryptoSpec>::PublicKey, String>,
}

fn main() {}
