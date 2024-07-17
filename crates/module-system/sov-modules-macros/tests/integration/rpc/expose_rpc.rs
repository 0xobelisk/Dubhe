use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::{expose_rpc, rpc_gen};
use sov_modules_api::{
    prelude::UnwrapInfallible, ApiStateAccessor, CallResponse, Context, Error, Module, ModuleId,
    ModuleInfo, Spec, StateValue, TxState,
};

#[derive(ModuleInfo)]
pub struct QueryModule<S: Spec> {
    #[id]
    pub id: ModuleId,

    #[state]
    pub data: StateValue<u8>,

    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

impl<S: Spec> Module for QueryModule<S> {
    type Spec = S;
    type Config = u8;
    type CallMessage = u8;
    type Event = ();

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl sov_modules_api::GenesisState<S>,
    ) -> Result<(), Error> {
        self.data.set(config, state).unwrap_infallible();
        Ok(())
    }

    fn call(
        &self,
        msg: Self::CallMessage,
        _context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, Error> {
        self.data
            .set(&msg, state)
            .map_err(|e| Error::ModuleError(e.into()))?;
        Ok(CallResponse::default())
    }
}

#[derive(Debug, Eq, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryResponse {
    pub value: Option<u8>,
}

#[rpc_gen(client, server, namespace = "queryModule")]
impl<S: Spec> QueryModule<S> {
    #[rpc_method(name = "queryValue")]
    pub fn query_value(&self, state: &mut ApiStateAccessor<S>) -> RpcResult<QueryResponse> {
        Ok(QueryResponse {
            value: self.data.get(state).unwrap_infallible(),
        })
    }
}

#[expose_rpc]
#[derive(Default)]
struct Runtime<S: Spec> {
    pub first: QueryModule<S>,
}

fn main() {}
