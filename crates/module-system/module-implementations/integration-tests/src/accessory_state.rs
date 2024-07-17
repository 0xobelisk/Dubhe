use std::convert::Infallible;

use sov_modules_api::{
    AccessoryStateValue, ApiStateAccessor, CallResponse, Context, GenesisState, Module,
    ModuleError, ModuleId, ModuleInfo, Spec, StateCheckpoint, TxState, WorkingSet,
};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_state::Storage;
use sov_test_utils::{TestSpec, TestStorageSpec as StorageSpec};

#[derive(ModuleInfo)]
pub struct TestModule<S: Spec> {
    #[id]
    id: ModuleId,

    #[state]
    accessory_state: AccessoryStateValue<u32>,

    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

impl<S: Spec> Module for TestModule<S> {
    type Spec = S;
    type Config = ();
    type CallMessage = ();
    type Event = ();

    fn genesis(
        &self,
        _config: &Self::Config,
        _state: &mut impl GenesisState<S>,
    ) -> Result<(), ModuleError> {
        Ok(())
    }

    fn call(
        &self,
        _msg: Self::CallMessage,
        _context: &Context<Self::Spec>,
        _state: &mut impl TxState<S>,
    ) -> Result<CallResponse, ModuleError> {
        unimplemented!()
    }
}

/// Check that:
/// 1. Accessory state does not change normal state root hash.
/// 2. Accessory state is reverted together with normal state.
/// Changes are returned explicitly by storage trait.
#[test]
fn test_accessory_value_setter() -> Result<(), Infallible> {
    let module = TestModule::<TestSpec>::default();

    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::<StorageSpec>::new(tmpdir.path());

    // 0. Genesis
    let storage = storage_manager.create_storage();
    let state = <StateCheckpoint<TestSpec>>::new(storage.clone());

    let mut genesis_state = state.to_genesis_state_accessor::<TestModule<TestSpec>>(&());

    module.genesis(&(), &mut genesis_state).unwrap();

    let (reads_writes, _, witness) = genesis_state.checkpoint().freeze();
    let (state_root_hash_initial, change_set_genesis) = storage
        .validate_and_materialize(reads_writes, &witness)
        .unwrap();
    storage_manager.commit(change_set_genesis);

    // 1. Check that root hash is not changed after
    let storage = storage_manager.create_storage();
    let mut state = <StateCheckpoint<TestSpec>>::new(storage.clone());

    module.accessory_state.set(&42, &mut state)?;

    let (state_writes, accessory_writes, witness) = state.freeze();
    let (state_root_hash_after, change_set_after) = storage
        .validate_and_materialize_with_accessory_update(
            state_writes,
            &witness,
            accessory_writes.freeze(),
        )
        .unwrap();

    assert_eq!(
        state_root_hash_initial, state_root_hash_after,
        "State root has been changed by accessory writes"
    );

    storage_manager.commit(change_set_after);
    let storage = storage_manager.create_storage();

    let mut api_accessor = <ApiStateAccessor<TestSpec>>::new(storage.clone());

    assert_eq!(
        42,
        module.accessory_state.get(&mut api_accessor)?.unwrap(),
        "AccessoryStateValue read has returned an incorrect value"
    );

    let mut ws = <WorkingSet<TestSpec>>::new_deprecated(storage.clone());

    module.accessory_state.set(&1000, &mut ws)?;

    let (tx_scratchpad, _gas_meter) = ws.revert();
    let checkpoint = tx_scratchpad.revert();
    let (state_writes, accessory_writes, witness) = checkpoint.freeze();
    let (state_root_hash_after, change_set_after) = storage
        .validate_and_materialize_with_accessory_update(
            state_writes,
            &witness,
            accessory_writes.freeze(),
        )
        .unwrap();

    assert_eq!(
        state_root_hash_initial, state_root_hash_after,
        "State root has been changed by accessory revert"
    );

    storage_manager.commit(change_set_after);
    let storage = storage_manager.create_storage();
    let mut api_state_accessor = ApiStateAccessor::<TestSpec>::new(storage.clone());

    assert_eq!(
        42,
        module
            .accessory_state
            .get(&mut api_state_accessor)?
            .unwrap(),
        "AccessoryStateValue revert has failed"
    );

    Ok(())
}
