//! Regression for <https://github.com/Sovereign-Labs/sovereign-sdk/issues/635>.

#![allow(unused_imports)]

use sov_modules_api::{ModuleId, ModuleInfo, RollupAddress, Spec};

#[derive(ModuleInfo)]
struct TestModule<S: Spec> {
    #[id]
    id: ModuleId,

    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

fn main() {}
