use std::convert::Infallible;

use sov_bank::Payable;
use sov_mock_da::MockAddress;
use sov_modules_api::{Context, Module};
use sov_test_utils::{TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_STAKE};

use crate::tests::helpers::{
    generate_address, TestSequencer, ANOTHER_SEQUENCER_DA_ADDRESS, ANOTHER_SEQUENCER_KEY,
    GENESIS_SEQUENCER_DA_ADDRESS, GENESIS_SEQUENCER_KEY, LOW_FUND_KEY, REWARD_SEQUENCER_KEY,
    UNKNOWN_SEQUENCER_DA_ADDRESS,
};
use crate::{CallMessage, SequencerRegistryError};

type S = sov_test_utils::TestSpec;

// Happy path for registration and exit
// This test checks:
//  - genesis sequencer is present after genesis
//  - registration works, and funds are deducted
//  - exit works and funds are returned
#[test]
fn test_registration_lifecycle() -> Result<(), Infallible> {
    let (test_sequencer, mut state) =
        TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    // Check normal lifecycle

    let da_address = MockAddress::from(ANOTHER_SEQUENCER_DA_ADDRESS);

    let sequencer_address = generate_address(ANOTHER_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);

    let balance_before = test_sequencer
        .query_balance((&sequencer_address).as_token_holder(), &mut state)?
        .unwrap();

    let registry_response_before = test_sequencer
        .registry
        .get_sequencer_address(da_address, &mut state)?;
    assert!(registry_response_before.is_none());

    let mut state = state.to_working_set_unmetered();
    let register_message = CallMessage::Register {
        da_address: da_address.as_ref().to_vec(),
        amount: TEST_DEFAULT_USER_STAKE,
    };
    test_sequencer
        .registry
        .call(register_message, &sender_context, &mut state)
        .expect("Sequencer registration has failed");
    let mut state = state.checkpoint().0;

    let balance_after_registration = test_sequencer
        .query_balance((&sequencer_address).as_token_holder(), &mut state)?
        .unwrap();
    assert_eq!(
        balance_before - TEST_DEFAULT_USER_STAKE,
        balance_after_registration
    );

    let registry_response_after_registration = test_sequencer
        .registry
        .get_sequencer_address(da_address, &mut state)?;
    assert_eq!(
        Some(sequencer_address),
        registry_response_after_registration
    );

    let exit_message = CallMessage::Exit {
        da_address: da_address.as_ref().to_vec(),
    };
    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(exit_message, &sender_context, &mut state)
        .expect("Sequencer exit has failed");
    let mut state = state.checkpoint().0;

    let balance_after_exit = test_sequencer
        .query_balance((&sequencer_address).as_token_holder(), &mut state)?
        .unwrap();
    assert_eq!(balance_before, balance_after_exit);

    let registry_response_after_exit = test_sequencer
        .registry
        .get_sequencer_address(da_address, &mut state)?;
    assert!(registry_response_after_exit.is_none());

    Ok(())
}

#[test]
fn test_registration_not_enough_funds() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    let da_address = MockAddress::from(ANOTHER_SEQUENCER_DA_ADDRESS);

    let sequencer_address = generate_address(LOW_FUND_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);

    let mut state = state.to_working_set_unmetered();
    let response = test_sequencer.registry.register(
        &da_address,
        TEST_DEFAULT_USER_STAKE,
        &sender_context,
        &mut state,
    );

    // Note: the next PR will add a check for the error message back
    assert!(
        response.is_err(),
        "insufficient funds registration should fail"
    );

    assert_eq!(
        response.unwrap_err(),
        SequencerRegistryError::InsufficientFundsToRegister(TEST_DEFAULT_USER_STAKE)
    );

    Ok(())
}

#[test]
fn test_registration_second_time() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    let da_address = MockAddress::from(GENESIS_SEQUENCER_DA_ADDRESS);

    let sequencer_address = generate_address(GENESIS_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);

    let mut state = state.to_working_set_unmetered();
    let response = test_sequencer.registry.register(
        &da_address,
        TEST_DEFAULT_USER_STAKE,
        &sender_context,
        &mut state,
    );

    // Note: the next PR will add a check for the error message back
    assert!(response.is_err(), "duplicate registration should fail");

    assert_eq!(
        response.unwrap_err(),
        SequencerRegistryError::SequencerAlreadyRegistered(sequencer_address)
    );

    Ok(())
}

#[test]
fn test_exit_different_sender() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    let sequencer_address = generate_address(ANOTHER_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);
    let attacker_address = generate_address("some_random_key");
    let attacker_context =
        Context::<S>::new(attacker_address, Default::default(), reward_address, 1);

    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .register(
            &MockAddress::new(ANOTHER_SEQUENCER_DA_ADDRESS),
            TEST_DEFAULT_USER_STAKE,
            &sender_context,
            &mut state,
        )
        .expect("Sequencer registration has failed");

    let response = test_sequencer.registry.exit(
        &MockAddress::new(ANOTHER_SEQUENCER_DA_ADDRESS),
        &attacker_context,
        &mut state,
    );

    // Note: the next PR will add a check for the error message back
    assert!(
        response.is_err(),
        "exit by non authorized sender should fail"
    );

    assert_eq!(
        response.unwrap_err(),
        SequencerRegistryError::SuppliedAddressDoesNotMatchTxSender {
            parameter: sequencer_address,
            sender: attacker_address,
        }
    );

    Ok(())
}

#[test]
fn test_allow_exit_last_sequencer() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    let sequencer_address = generate_address(GENESIS_SEQUENCER_KEY);
    let rewards_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), rewards_address, 1);
    let exit_message = CallMessage::Exit {
        da_address: GENESIS_SEQUENCER_DA_ADDRESS.to_vec(),
    };
    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(exit_message, &sender_context, &mut state)
        .expect("Last sequencer exit has failed");

    Ok(())
}

/// This regression test ensures that a sequencer cannot exit during processing
/// of a batch that they've submitted.
#[test]
fn test_prevent_exit_during_own_batch() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    let sequencer_address = generate_address(GENESIS_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), sequencer_address, 1);
    let exit_message = CallMessage::Exit {
        da_address: GENESIS_SEQUENCER_DA_ADDRESS.to_vec(),
    };
    let mut state = state.to_working_set_unmetered();
    assert!(test_sequencer
        .registry
        .call(exit_message, &sender_context, &mut state)
        .is_err());

    Ok(())
}

#[test]
fn test_preferred_sequencer_returned_and_removed() -> Result<(), Infallible> {
    let (test_sequencer, mut state) =
        TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, true)?;

    assert_eq!(
        Some(test_sequencer.sequencer_config.seq_da_address),
        test_sequencer
            .registry
            .get_preferred_sequencer(&mut state)?
    );

    let sequencer_address = generate_address(GENESIS_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);
    let exit_message = CallMessage::Exit {
        da_address: GENESIS_SEQUENCER_DA_ADDRESS.to_vec(),
    };
    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(exit_message, &sender_context, &mut state)
        .expect("Last sequencer exit has failed");
    let mut state = state.checkpoint().0;

    // Preferred sequencer exited, so the result is none
    assert!(test_sequencer
        .registry
        .get_preferred_sequencer(&mut state)?
        .is_none());

    Ok(())
}

#[test]
fn test_registration_balance_increase() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    // created settings

    let initial_balance = test_sequencer
        .bank_config
        .gas_token_config
        .address_and_balances[0]
        .1;
    let stake_amount = test_sequencer.sequencer_config.minimum_bond;
    let stake_increase = 1;

    // Register a sequencer

    let da_address = MockAddress::from(ANOTHER_SEQUENCER_DA_ADDRESS);

    let sequencer_address = generate_address(ANOTHER_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);

    let register_message = CallMessage::Register {
        da_address: da_address.as_ref().to_vec(),
        amount: TEST_DEFAULT_USER_STAKE,
    };

    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(register_message, &sender_context, &mut state)
        .expect("Sequencer registration has failed");
    let mut state = state.checkpoint().0;

    // Sanity check

    let balance_after_registration = test_sequencer
        .query_balance((&sequencer_address).as_token_holder(), &mut state)?
        .unwrap();
    assert_eq!(initial_balance - stake_amount, balance_after_registration);

    // Assert the registry balance is the staked amount

    let sender_balance = test_sequencer
        .query_sender_balance(&da_address, &mut state)?
        .unwrap();
    assert_eq!(stake_amount, sender_balance);

    // Sanity check the sequencer allowed status

    assert!(test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut state));

    // Increases the stake value and expects the sequencer to preserve his balance and no longer be
    // allowed

    let stake_amount = stake_amount + stake_increase;
    test_sequencer.set_coins_amount_to_lock(stake_amount, &mut state)?;

    let balance_after_update = test_sequencer
        .query_balance((&sequencer_address).as_token_holder(), &mut state)?
        .unwrap();

    assert_eq!(balance_after_registration, balance_after_update);
    assert!(!test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut state));

    // Increase the balance of the sequencer and assert sequencer is allowed

    let deposit_message = CallMessage::Deposit {
        da_address: da_address.as_ref().to_vec(),
        amount: stake_increase,
    };

    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(deposit_message, &sender_context, &mut state)
        .expect("Sequencer deposit has failed");
    let mut state = state.checkpoint().0;

    let new_sender_balance = test_sequencer
        .query_sender_balance(&da_address, &mut state)?
        .unwrap();

    assert_eq!(sender_balance + stake_increase, new_sender_balance);
    assert!(test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut state));

    Ok(())
}

#[test]
fn test_balance_increase_fails_if_insufficient_funds() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    // created settings

    let initial_balance = test_sequencer
        .bank_config
        .gas_token_config
        .address_and_balances[0]
        .1;
    let stake_amount = test_sequencer.sequencer_config.minimum_bond;
    let stake_increase = initial_balance;

    // Register a sequencer

    let da_address = MockAddress::from(ANOTHER_SEQUENCER_DA_ADDRESS);

    let sequencer_address = generate_address(ANOTHER_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);

    let register_message = CallMessage::Register {
        da_address: da_address.as_ref().to_vec(),
        amount: TEST_DEFAULT_USER_STAKE,
    };

    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(register_message, &sender_context, &mut state)
        .expect("Sequencer registration has failed");
    let mut state = state.checkpoint().0;

    // Sanity check the sequencer allowed status

    assert!(test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut state));

    // Increase the stake value and assert the sequencer is no longer allowed

    let stake_amount = stake_amount + stake_increase;
    test_sequencer.set_coins_amount_to_lock(stake_amount, &mut state)?;
    assert!(!test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut state));

    // Attempt to deposit the required amount and expect failure

    let deposit_message = CallMessage::Deposit {
        da_address: da_address.as_ref().to_vec(),
        amount: stake_increase,
    };

    let mut state = state.to_working_set_unmetered();
    let res = test_sequencer
        .registry
        .call(deposit_message, &sender_context, &mut state);
    let mut state = state.checkpoint().0;

    assert!(res.is_err());
    assert!(!test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut state));

    Ok(())
}

#[test]
fn test_non_registered_sequencer_is_not_allowed() -> Result<(), Infallible> {
    let (test_sequencer, mut working_set) =
        TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    let da_address = MockAddress::from(ANOTHER_SEQUENCER_DA_ADDRESS);

    assert!(!test_sequencer.query_if_sequencer_is_allowed(&da_address, &mut working_set));

    Ok(())
}

#[test]
fn test_balance_increase_fails_for_unknown_sequencer() -> Result<(), Infallible> {
    let (test_sequencer, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;

    // created settings

    let stake_amount = test_sequencer.sequencer_config.minimum_bond;

    // Register a sequencer

    let da_address = MockAddress::from(ANOTHER_SEQUENCER_DA_ADDRESS);
    let unknown_address = MockAddress::from(UNKNOWN_SEQUENCER_DA_ADDRESS);

    let sequencer_address = generate_address(ANOTHER_SEQUENCER_KEY);
    let reward_address = generate_address(REWARD_SEQUENCER_KEY);
    let sender_context =
        Context::<S>::new(sequencer_address, Default::default(), reward_address, 1);

    let register_message = CallMessage::Register {
        da_address: da_address.as_ref().to_vec(),
        amount: TEST_DEFAULT_USER_STAKE,
    };

    let mut state = state.to_working_set_unmetered();
    test_sequencer
        .registry
        .call(register_message, &sender_context, &mut state)
        .expect("Sequencer registration has failed");
    let mut state = state.checkpoint().0;

    // Sanity check the sequencer allowed status

    assert!(!test_sequencer.query_if_sequencer_is_allowed(&unknown_address, &mut state));

    // Attempt to deposit 1 coin into unknown sequencer

    let deposit_message = CallMessage::Deposit {
        da_address: unknown_address.as_ref().to_vec(),
        amount: 1,
    };

    let mut state = state.to_working_set_unmetered();
    let res = test_sequencer
        .registry
        .call(deposit_message, &sender_context, &mut state);
    let mut state = state.checkpoint().0;

    assert!(res.is_err());
    assert!(!test_sequencer.query_if_sequencer_is_allowed(&unknown_address, &mut state));

    // Attempt to deposit stake amount into unknown sequencer

    let deposit_message = CallMessage::Deposit {
        da_address: unknown_address.as_ref().to_vec(),
        amount: stake_amount,
    };

    let mut state = state.to_working_set_unmetered();
    let res = test_sequencer
        .registry
        .call(deposit_message, &sender_context, &mut state);
    let mut state = state.checkpoint().0;

    assert!(res.is_err());
    assert!(!test_sequencer.query_if_sequencer_is_allowed(&unknown_address, &mut state));

    Ok(())
}
