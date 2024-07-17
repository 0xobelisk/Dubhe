use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::{expose_rpc, rpc_gen};
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{
    Address, ApiStateAccessor, CallResponse, Context, DispatchCall, EncodeCall, Error, Genesis,
    MessageCodec, Module, ModuleId, ModuleInfo, Spec, StateCheckpoint, StateValue, TxState,
};
use sov_state::ZkStorage;
use sov_test_utils::ZkTestSpec;

pub trait TestSpec: Default + Send + Sync + std::fmt::Debug + 'static {
    type Data: Data;
}

pub trait Data:
    Clone
    + Eq
    + PartialEq
    + std::fmt::Debug
    + serde::Serialize
    + serde::de::DeserializeOwned
    + borsh::BorshSerialize
    + borsh::BorshDeserialize
    + Send
    + Sync
    + 'static
{
}

impl Data for u32 {}

pub mod my_module {
    use super::*;

    #[derive(ModuleInfo)]
    pub struct QueryModule<S: Spec, D: Data> {
        #[id]
        pub id: ModuleId,

        #[state]
        pub data: StateValue<D>,

        #[phantom]
        phantom: std::marker::PhantomData<S>,
    }

    impl<S: Spec, D> Module for QueryModule<S, D>
    where
        D: Data,
    {
        type Spec = S;
        type Config = D;
        type CallMessage = D;
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

    pub mod rpc {
        use super::*;
        use crate::my_module::QueryModule;

        #[derive(Debug, Eq, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
        pub struct QueryResponse {
            pub value: Option<String>,
        }

        #[rpc_gen(client, server, namespace = "queryModule")]
        impl<S, D: Data> QueryModule<S, D>
        where
            S: Spec,
        {
            #[rpc_method(name = "queryValue")]
            pub fn query_value(&self, state: &mut ApiStateAccessor<S>) -> RpcResult<QueryResponse> {
                let value = self
                    .data
                    .get(api_state_accessor)
                    .unwrap_infallible()
                    .map(|d| format!("{:?}", d));
                Ok(QueryResponse { value })
            }
        }
    }
}

#[expose_rpc]
#[derive(Default, Genesis, DispatchCall, MessageCodec)]
#[serialization(borsh::BorshDeserialize, borsh::BorshSerialize)]
struct Runtime<T: TestSpec, S: Spec> {
    pub first: my_module::QueryModule<S, T::Data>,
}

#[derive(Default)]
struct ActualSpec;

impl TestSpec for ActualSpec {
    type Data = u32;
}

fn main() {
    type S = ZkTestSpec;
    type RT = Runtime<S, ActualSpec>;
    let storage = ZkStorage::new();
    let state = StateCheckpoint::new(storage);
    let runtime = &mut Runtime::<S, ActualSpec>::default();
    let config = GenesisConfig::new(22);
    let mut genesis_state = state.to_genesis_state_accessor::<RT>(&config);
    runtime.genesis(&config, &mut genesis_state).unwrap();
    let mut working_set = genesis_state.checkpoint().to_working_set_unmetered();

    let message: u32 = 33;
    let serialized_message =
        <RT as EncodeCall<my_module::QueryModule<S, u32>>>::encode_call(message);
    let module = RT::decode_call(&serialized_message, &mut working_set).unwrap();
    let sender = Address::try_from([11; 32].as_ref()).unwrap();
    let sequencer = Address::try_from([11; 32].as_ref()).unwrap();
    let context = Context::<S>::new(sender, sequencer, 1);

    let _ = runtime
        .dispatch_call(module, &mut working_set, &context)
        .unwrap();
}
