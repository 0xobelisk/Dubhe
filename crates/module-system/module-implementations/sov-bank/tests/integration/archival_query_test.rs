use sov_bank::{Amount, Bank, Coins, TokenId};
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{
    ApiStateAccessor, InfallibleStateAccessor, Module, Spec, StateCheckpoint, StateReader,
    StateWriter,
};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_state::namespaces::Accessory;
use sov_state::storage::{SlotKey, SlotValue, StateUpdate};
use sov_state::{ProverStorage, Storage};
use sov_test_utils::{TestStorageSpec as StorageSpec, TEST_DEFAULT_USER_BALANCE};

use crate::helpers::*;

type S = sov_test_utils::TestSpec;

#[test]
fn transfer_initial_token() -> Result<(), anyhow::Error> {
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    // The amount per transfer for the non-archival, most recent state.
    const AMOUNT_PER_TRANSFER: u64 = 10;
    // The amount per transfer for the archival state.
    const AMOUNT_PER_ARCHIVAL_TRANSFER: u64 = AMOUNT_PER_TRANSFER / 2;
    // Another amount per transfer for a different archival state.
    const AMOUNT_PER_ARCHIVAL_TRANSFER_2: u64 = AMOUNT_PER_TRANSFER / 3;

    let bank_config = create_bank_config_with_token(4, initial_balance);
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let prover_storage = storage_manager.create_storage();
    let state = StateCheckpoint::new(prover_storage.clone());
    let bank = Bank::default();
    let mut genesis_state = state.to_genesis_state_accessor::<Bank<S>>(&bank_config);
    bank.genesis(&bank_config, &mut genesis_state).unwrap();
    let mut state = genesis_state.checkpoint();

    let token_id = sov_bank::GAS_TOKEN_ID;
    let sender_address = bank_config.gas_token_config.address_and_balances[0].0;
    let receiver_address = bank_config.gas_token_config.address_and_balances[1].0;
    assert_ne!(sender_address, receiver_address);

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        &mut state,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_BALANCE)
    );
    let prover_storage = commit(state, prover_storage, &mut storage_manager);
    let mut state = StateCheckpoint::<S>::new(prover_storage.clone());

    transfer(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        AMOUNT_PER_TRANSFER,
        &mut state,
    );

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        &mut state,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - AMOUNT_PER_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + AMOUNT_PER_TRANSFER
        )
    );

    let prover_storage = commit(state, prover_storage, &mut storage_manager);

    let mut state = StateCheckpoint::<S>::new(prover_storage.clone());

    transfer(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        AMOUNT_PER_TRANSFER,
        &mut state,
    );

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        &mut state,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - 2 * AMOUNT_PER_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + 2 * AMOUNT_PER_TRANSFER
        )
    );
    let prover_storage = commit(state, prover_storage, &mut storage_manager);

    // Archival tests

    let archival_slot: u64 = 2;
    let state = &mut ApiStateAccessor::<S>::new(prover_storage.clone());
    let archival = &mut state.get_archival_at(archival_slot);

    let (sender_balance, receiver_balance) =
        query_sender_receiver_balances(&bank, token_id, sender_address, receiver_address, archival);
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - AMOUNT_PER_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + AMOUNT_PER_TRANSFER
        )
    );

    // We want to transfer a different amount in the archival mode so that there is no collision with the `normal` transfers.
    // modify in archival
    transfer(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        AMOUNT_PER_ARCHIVAL_TRANSFER,
        archival,
    );

    let (sender_balance, receiver_balance) =
        query_sender_receiver_balances(&bank, token_id, sender_address, receiver_address, archival);
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - AMOUNT_PER_TRANSFER - AMOUNT_PER_ARCHIVAL_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + AMOUNT_PER_TRANSFER + AMOUNT_PER_ARCHIVAL_TRANSFER
        )
    );

    let archival_slot: u64 = 1;
    let api_state = &mut ApiStateAccessor::<S>::new(prover_storage.clone());
    let api_archival = &mut api_state.get_archival_at(archival_slot);
    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        api_archival,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_BALANCE)
    );

    // We want to transfer a different amount in the archival mode so that there is no collision with the `normal` transfers.
    transfer(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        AMOUNT_PER_ARCHIVAL_TRANSFER_2,
        api_archival,
    );

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        api_archival,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - AMOUNT_PER_ARCHIVAL_TRANSFER_2,
            TEST_DEFAULT_USER_BALANCE + AMOUNT_PER_ARCHIVAL_TRANSFER_2
        )
    );

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        api_state,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - 2 * AMOUNT_PER_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + 2 * AMOUNT_PER_TRANSFER
        )
    );

    // Accessory tests
    let mut state = StateCheckpoint::<S>::new(prover_storage.clone());
    transfer(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        AMOUNT_PER_TRANSFER,
        &mut state,
    );

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        &mut state,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - 3 * AMOUNT_PER_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + 3 * AMOUNT_PER_TRANSFER
        )
    );

    StateWriter::<Accessory>::set(
        &mut state,
        &SlotKey::from_slice(b"k"),
        SlotValue::from(b"v1".to_vec()),
    )?;

    let prover_storage = commit(state, prover_storage, &mut storage_manager);

    let api_state = &mut ApiStateAccessor::<S>::new(prover_storage.clone());
    let val = StateReader::<Accessory>::get(api_state, &SlotKey::from_slice(b"k"))?.unwrap();
    assert_eq!("v1", String::from_utf8(val.value().to_vec()).unwrap());

    // next block

    let mut state = StateCheckpoint::<S>::new(prover_storage.clone());
    transfer(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        AMOUNT_PER_TRANSFER,
        &mut state,
    );

    let (sender_balance, receiver_balance) = query_sender_receiver_balances(
        &bank,
        token_id,
        sender_address,
        receiver_address,
        &mut state,
    );
    assert_eq!(
        (sender_balance, receiver_balance),
        (
            TEST_DEFAULT_USER_BALANCE - 4 * AMOUNT_PER_TRANSFER,
            TEST_DEFAULT_USER_BALANCE + 4 * AMOUNT_PER_TRANSFER
        )
    );
    StateWriter::<Accessory>::set(
        &mut state,
        &SlotKey::from_slice(b"k"),
        SlotValue::from(b"v2".to_vec()),
    )?;

    let prover_storage = commit(state, prover_storage, &mut storage_manager);

    let api_state = &mut ApiStateAccessor::<S>::new(prover_storage.clone());
    let val = StateReader::<Accessory>::get(api_state, &SlotKey::from_slice(b"k"))?.unwrap();
    assert_eq!("v2", String::from_utf8(val.value().to_vec()).unwrap());

    // archival versioned state query

    let archival_slot = 3;
    let mut state: ApiStateAccessor<S> = ApiStateAccessor::new(prover_storage.clone());
    let mut archival = state.get_archival_at(archival_slot);
    let val = StateReader::<Accessory>::get(&mut archival, &SlotKey::from_slice(b"k"))?.unwrap();
    assert_eq!("v1", String::from_utf8(val.value().to_vec()).unwrap());

    // archival accessory set

    StateWriter::<Accessory>::set(
        &mut archival,
        &SlotKey::from_slice(b"k"),
        SlotValue::from(b"v3".to_vec()),
    )
    .expect("Should be able to set state");
    let val = StateReader::<Accessory>::get(&mut archival, &SlotKey::from_slice(b"k"))?.unwrap();
    assert_eq!("v3", String::from_utf8(val.value().to_vec()).unwrap());

    let val = StateReader::<Accessory>::get(&mut state, &SlotKey::from_slice(b"k"))?.unwrap();
    assert_eq!("v2", String::from_utf8(val.value().to_vec()).unwrap());

    Ok(())
}

fn query_sender_receiver_balances(
    bank: &Bank<S>,
    token_id: TokenId,
    sender_address: <S as Spec>::Address,
    receiver_address: <S as Spec>::Address,
    state: &mut impl InfallibleStateAccessor,
) -> (u64, u64) {
    let sender_balance = bank
        .get_balance_of(&sender_address, token_id, state)
        .unwrap_infallible()
        .unwrap();
    let receiver_balance = bank
        .get_balance_of(&receiver_address, token_id, state)
        .unwrap_infallible()
        .unwrap();
    (sender_balance, receiver_balance)
}

fn transfer(
    bank: &Bank<S>,
    token_id: TokenId,
    sender_address: <S as Spec>::Address,
    receiver_address: <S as Spec>::Address,
    transfer_amount: Amount,
    state: &mut impl InfallibleStateAccessor,
) {
    let to = receiver_address;
    let coin_amount = Coins {
        amount: transfer_amount,
        token_id,
    };

    bank.transfer_from(&sender_address, &to, coin_amount, state)
        .unwrap();
}

fn commit(
    state: StateCheckpoint<S>,
    storage: ProverStorage<StorageSpec>,
    storage_manager: &mut SimpleStorageManager<StorageSpec>,
) -> ProverStorage<StorageSpec> {
    // Save checkpoint
    let (cache_log, accessory_delta, witness) = state.freeze();

    let (_, mut state_update) = storage
        .compute_state_update(cache_log, &witness)
        .expect("JMT update must succeed");

    state_update.add_accessory_items(accessory_delta.freeze());

    let change_set = storage.materialize_changes(&state_update);
    storage_manager.commit(change_set);
    storage_manager.create_storage()
}
