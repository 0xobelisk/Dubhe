//! Regression test for <https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/163>.

#![deny(missing_docs)]
use sov_modules_api::ModuleId;
use sov_modules_api::macros::rpc_gen;

/// docs
#[derive(sov_modules_api::ModuleInfo)]
pub struct TestStruct<S: sov_modules_api::Spec> {
    /// docs
    #[id]
    pub(crate) id: ModuleId,

    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

#[rpc_gen(client, server, namespace = "test")]
impl<S: sov_modules_api::Spec> TestStruct<S> {
    /// docs
    #[rpc_method(name = "foo")]
    pub fn foo(&self) -> jsonrpsee::core::RpcResult<u32> {
        Ok(42)
    }
}

/// docs
fn main() {}
