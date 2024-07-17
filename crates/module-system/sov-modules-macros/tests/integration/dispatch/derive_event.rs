mod modules;
use modules::{first_test_module, second_test_module};
use sov_modules_api::{DispatchCall, Event, Genesis, MessageCodec, Spec};
use sov_test_utils::TestSpec;

#[derive(Default, Genesis, DispatchCall, Event, MessageCodec)]
#[serialization(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshDeserialize,
    borsh::BorshSerialize
)]
struct Runtime<S: Spec> {
    pub first: first_test_module::FirstTestStruct<S>,
    pub second: second_test_module::SecondTestStruct<S>,
}

fn main() {
    // Check to see if the runtime events are getting initialized correctly
    let _event = RuntimeEvent::<TestSpec>::first(first_test_module::Event::FirstModuleEnum1(10));
    let _event = RuntimeEvent::<TestSpec>::first(first_test_module::Event::FirstModuleEnum2);
    let _event =
        RuntimeEvent::<TestSpec>::first(first_test_module::Event::FirstModuleEnum3(vec![1; 3]));
    let _event = RuntimeEvent::<TestSpec>::second(second_test_module::Event::SecondModuleEnum);
}
