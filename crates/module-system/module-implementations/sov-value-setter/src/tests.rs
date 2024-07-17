use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{Address, Context, Module, Spec, StateCheckpoint};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::ZkStorage;
use sov_test_utils::{TestSpec, ZkTestSpec};

use super::{Event, ValueSetter};
use crate::{call, ValueSetterConfig};
#[test]
fn test_value_setter() {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut state = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let admin = Address::from([1; 32]);
    let sequencer = Address::from([2; 32]);
    // Test Native-Context
    #[cfg(feature = "native")]
    {
        let config = ValueSetterConfig { admin };
        let context = Context::<TestSpec>::new(admin, Default::default(), sequencer, 1);
        state = test_value_setter_helper(context, &config, state);
    }

    let (_, _, witness) = state.freeze();

    // Test Zk-Context
    {
        let config = ValueSetterConfig { admin };
        let zk_context = Context::<ZkTestSpec>::new(admin, Default::default(), sequencer, 1);
        let state = StateCheckpoint::with_witness(ZkStorage::new(), witness);
        test_value_setter_helper(zk_context, &config, state);
    }
}

fn test_value_setter_helper<S: Spec>(
    context: Context<S>,
    config: &ValueSetterConfig<S>,
    state: StateCheckpoint<S>,
) -> StateCheckpoint<S> {
    let module = ValueSetter::<S>::default();
    let mut genesis_state = state.to_genesis_state_accessor::<ValueSetter<S>>(config);
    module.genesis(config, &mut genesis_state).unwrap();
    let mut state = genesis_state.checkpoint();

    let new_value = 99;
    let call_msg = call::CallMessage::SetValue(new_value);

    // Test events
    {
        let mut working_set = state.to_working_set_unmetered();
        module.call(call_msg, &context, &mut working_set).unwrap();
        let typed_event = working_set.take_event(0).unwrap();
        assert_eq!(
            typed_event.downcast::<Event>().unwrap(),
            Event::NewValue(99)
        );
        state = working_set.checkpoint().0;
    }

    // Test query
    assert_eq!(
        module.value.get(&mut state).unwrap_infallible(),
        Some(new_value)
    );
    state
}

#[test]
fn test_err_on_sender_is_not_admin() {
    let sender = Address::from([1; 32]);
    let sequencer = Address::from([2; 32]);

    let tmpdir = tempfile::tempdir().unwrap();
    let storage = new_orphan_storage(tmpdir.path()).unwrap();
    let mut prover_state_checkpoint = StateCheckpoint::new(storage);

    let sender_not_admin = Address::from([2; 32]);
    // Test Prover-Context
    {
        let config = ValueSetterConfig {
            admin: sender_not_admin,
        };
        let context = Context::<TestSpec>::new(sender, Default::default(), sequencer, 1);
        prover_state_checkpoint =
            test_err_on_sender_is_not_admin_helper(context, &config, prover_state_checkpoint);
    }
    let (_, _, witness) = prover_state_checkpoint.freeze();

    // Test Zk-Context
    {
        let config = ValueSetterConfig {
            admin: sender_not_admin,
        };
        let zk_backing_store = ZkStorage::new();
        let zk_context = Context::<ZkTestSpec>::new(sender, Default::default(), sequencer, 1);
        let zk_state_checkpoint = StateCheckpoint::with_witness(zk_backing_store, witness);
        test_err_on_sender_is_not_admin_helper(zk_context, &config, zk_state_checkpoint);
    }
}

fn test_err_on_sender_is_not_admin_helper<S: Spec>(
    context: Context<S>,
    config: &ValueSetterConfig<S>,
    state: StateCheckpoint<S>,
) -> StateCheckpoint<S> {
    let module = ValueSetter::<S>::default();
    let mut genesis_state = state.to_genesis_state_accessor::<ValueSetter<S>>(config);
    module.genesis(config, &mut genesis_state).unwrap();
    let mut working_set = genesis_state.checkpoint().to_working_set_unmetered();
    let resp = module.set_value(11, &context, &mut working_set);
    let state = working_set.checkpoint().0;

    assert!(resp.is_err());
    state
}
