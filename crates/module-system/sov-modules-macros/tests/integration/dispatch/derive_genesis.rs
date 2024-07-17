mod modules;

use modules::third_test_module::{self, ModuleThreeStorable};
use modules::{first_test_module, second_test_module};
use sov_modules_api::{DispatchCall, Genesis, MessageCodec, Spec};
use sov_state::ZkStorage;
use sov_test_utils::ZkTestSpec;

// Debugging hint: To expand the macro in tests run: `cargo expand --test tests`
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
    let storage = ZkStorage::new();
    let state = sov_modules_api::StateCheckpoint::new(storage);
    let runtime = &mut Runtime::<ZkTestSpec, u32>::default();
    let config = GenesisConfig::new((), (), ());
    let mut genesis_state = state.to_genesis_state_accessor::<Runtime<ZkTestSpec, u32>>(&config);
    runtime.genesis(&config, &mut genesis_state).unwrap();
    let mut working_set = genesis_state.checkpoint().to_working_set_unmetered();

    {
        let response = runtime
            .first
            .get_state_value(&mut working_set)
            .expect("The working set should be unmetered");
        assert_eq!(response, 1);
    }

    {
        let response = runtime
            .second
            .get_state_value(&mut working_set)
            .expect("The working set should be unmetered");
        assert_eq!(response, 2);
    }

    {
        let response = runtime
            .third
            .get_state_value(&mut working_set)
            .expect("The working set should be unmetered");
        assert_eq!(response, Some(0));
    }
}
