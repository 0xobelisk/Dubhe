use module_template::{CallMessage, Event, ExampleModule, ExampleModuleConfig};
use sov_modules_api::{Address, Context, Module, Spec, StateCheckpoint};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::ZkStorage;
use sov_test_utils::{TestSpec, TestStorageSpec, ZkTestSpec};

#[test]
fn test_value_setter() {
    let tmpdir = tempfile::tempdir().unwrap();

    let storage = new_orphan_storage::<TestStorageSpec>(tmpdir.path()).unwrap();
    let state = StateCheckpoint::new(storage);

    let admin = Address::from([1; 32]);
    let sequencer = Address::from([2; 32]);

    // Test Native-Context
    let state = {
        let config = ExampleModuleConfig {};
        let context = Context::<TestSpec>::new(admin, Default::default(), sequencer, 1);
        test_value_setter_helper(context, &config, state)
    };

    let (_, _, witness) = state.freeze();

    // Test Zk-Context
    {
        let config = ExampleModuleConfig {};
        let zk_context = Context::<ZkTestSpec>::new(admin, Default::default(), sequencer, 1);
        let zk_state = StateCheckpoint::with_witness(ZkStorage::new(), witness);
        test_value_setter_helper(zk_context, &config, zk_state);
    }
}

fn test_value_setter_helper<S: Spec>(
    context: Context<S>,
    config: &ExampleModuleConfig,
    state: StateCheckpoint<S>,
) -> StateCheckpoint<S> {
    let module = ExampleModule::<S>::default();
    let mut genesis_state = state.to_genesis_state_accessor::<ExampleModule<S>>(config);
    module.genesis(config, &mut genesis_state).unwrap();

    let mut state = genesis_state.checkpoint();

    let new_value = 99;
    let call_msg = CallMessage::SetValue(new_value);

    // Test events
    {
        let mut working_set = state.to_working_set_unmetered();
        module.call(call_msg, &context, &mut working_set).unwrap();
        let typed_event = working_set.take_event(0).unwrap();
        assert_eq!(
            typed_event.downcast::<Event>().unwrap(),
            Event::Set { value: 99 }
        );
        state = working_set.checkpoint().0;
    }

    state

    // TODO(@theochap): fix this test (no more api access without storage commit)
    // Test query
    //
    // #[cfg(feature = "native")]
    // {
    //     let query_response = module.query_value(state);
    //     assert_eq!(
    //         Response {
    //             value: Some(new_value)
    //         },
    //         query_response
    //     );
    // }
}
