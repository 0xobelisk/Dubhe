use sov_modules_api::{Context, Module, ModuleId, ModuleInfo, Spec, StateMap, TxState};

#[derive(ModuleInfo)]
struct TestStruct<S: Spec> {
    #[id]
    pub id: ModuleId,

    test_state1: StateMap<u32, u32>,

    #[state]
    test_state2: StateMap<Vec<u8>, u64>,

    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

impl<S: Spec> Module for TestStruct<S> {
    type Spec = S;

    type Config = ();

    type CallMessage = ();

    type Event = ();

    fn call(
        &self,
        _message: Self::CallMessage,
        _context: &Context<Self::Spec>,
        _state: &mut impl TxState<S>,
    ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
        todo!()
    }
}

fn main() {}
