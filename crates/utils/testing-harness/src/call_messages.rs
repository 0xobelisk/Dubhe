use derive_more::Constructor;
use sov_modules_api::{Module, Spec};

#[derive(Debug, Constructor, Clone, Copy)]
pub(crate) struct PreparedCallMessage<S: Spec, M: Module<Spec = S>> {
    pub(crate) call_message: M::CallMessage,
    pub(crate) account_pool_index: u64,
    pub(crate) max_fee: u64,
}

#[derive(Default)]
pub(crate) struct SerializedPreparedCallMessage {
    pub(crate) call_message: Vec<u8>,
    pub(crate) account_pool_index: u64,
    pub(crate) max_fee: u64,
}
