mod modules;
use modules::third_test_module::{self, ModuleThreeStorable};
use modules::{first_test_module, second_test_module};
use sov_modules_api::{
    Address, Context, DispatchCall, EncodeCall, Genesis, MessageCodec, ModuleInfo, Spec,
};
use sov_state::ZkStorage;
use sov_test_utils::ZkTestSpec;

#[derive(Default, Genesis, DispatchCall, MessageCodec)]
#[serialization(borsh::BorshDeserialize, borsh::BorshSerialize)]
struct Runtime<S, T>
where
    S: Spec,
    T: ModuleThreeStorable,
{
    pub first: first_test_module::FirstTestStruct<S>,
    pub second: second_test_module::SecondTestStruct<S>,
    pub third: third_test_module::ThirdTestStruct<S, T>,
}

fn main() {
    type RT = Runtime<ZkTestSpec, u32>;
    let runtime = &mut RT::default();

    let storage = ZkStorage::new();

    let state = sov_modules_api::StateCheckpoint::new(storage);
    let config = GenesisConfig::new((), (), ());
    let mut genesis_state = state.to_genesis_state_accessor::<Runtime<ZkTestSpec, u32>>(&config);
    runtime.genesis(&config, &mut genesis_state).unwrap();
    let mut working_set = genesis_state.checkpoint().to_working_set_unmetered();

    let sender = Address::try_from([0; 32].as_ref()).unwrap();
    let sequencer = Address::try_from([1; 32].as_ref()).unwrap();
    let context: Context<ZkTestSpec> = Context::new(sender, Default::default(), sequencer, 1);

    let value = 11;
    {
        let message = value;
        let serialized_message = <RT as EncodeCall<
            first_test_module::FirstTestStruct<ZkTestSpec>,
        >>::encode_call(message);
        let module = RT::decode_call(&serialized_message, &mut working_set).unwrap();

        assert_eq!(runtime.module_id(&module), runtime.first.id());
        let _ = runtime
            .dispatch_call(module, &mut working_set, &context)
            .unwrap();
    }

    {
        let response = runtime
            .first
            .get_state_value(&mut working_set)
            .expect("The working set should be unmetered");
        assert_eq!(response, value);
    }

    let value = 22;
    {
        let message = value;
        let serialized_message = <RT as EncodeCall<
            second_test_module::SecondTestStruct<ZkTestSpec>,
        >>::encode_call(message);
        let module = RT::decode_call(&serialized_message, &mut working_set).unwrap();

        assert_eq!(runtime.module_id(&module), runtime.second.id());

        let _ = runtime
            .dispatch_call(module, &mut working_set, &context)
            .unwrap();
    }

    {
        let response = runtime
            .second
            .get_state_value(&mut &mut working_set)
            .expect("The working set should be unmetered");
        assert_eq!(response, value);
    }
}
