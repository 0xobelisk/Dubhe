use std::convert::Infallible;

use sov_bank::GAS_TOKEN_ID;
use sov_mock_da::MockValidityCond;
use sov_mock_zkvm::MockZkvm;
use sov_modules_api::{
    AggregatedProofPublicData, CodeCommitment, Spec, StateCheckpoint, TypedEvent,
};
use sov_test_utils::TEST_DEFAULT_USER_STAKE;

use super::helpers::{get_transition_unwrap, MAX_TX_GAS_AMOUNT, MOCK_PROVER_ADDRESS};
use crate::event::Event;
use crate::tests::helpers::{
    setup, simulate_chain_state_execution, INITIAL_PROVER_BALANCE, MOCK_CODE_COMMITMENT, S,
};
const FIRST_SLOT_NUM: u64 = 1;
const LAST_SLOT_NUM: u64 = 2;

/// Builds a valid proof log that proves the transitions between [`FIRST_SLOT_NUM`] and [`LAST_SLOT_NUM`]
fn build_proof_log(
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    state: &mut StateCheckpoint<S>,
) -> Result<AggregatedProofPublicData, Infallible> {
    let genesis_hash = module
        .chain_state
        .get_genesis_hash(state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, module, state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, module, state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    Ok(AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM,
        initial_state_root: genesis_hash.as_ref().to_vec(),
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    })
}

/// Simulates the execution of the chain state and processes a valid proof of the transitions between
/// [`FIRST_SLOT_NUM`] and [`LAST_SLOT_NUM`] (included). Returns the working set and the total amount
/// of gas token consumed in each step
fn execute_txs_and_process_valid_proof(
    prover_address: <S as Spec>::Address,
    sequencer: <S as Spec>::Address,
    max_gas_used_per_step: &<S as Spec>::Gas,
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    state: StateCheckpoint<S>,
) -> Result<(u64, StateCheckpoint<S>, Vec<TypedEvent>), Infallible> {
    // The first transition is the genesis transition
    // Then we have two more transitions
    let (mut state, total_gas_used) = simulate_chain_state_execution(
        module,
        sequencer,
        ((LAST_SLOT_NUM - FIRST_SLOT_NUM + 1) + 1)
            .try_into()
            .unwrap(),
        max_gas_used_per_step,
        state,
    );

    // We remove the last element because we don't want to include the gas used for the last transition
    let total_gas_used: u64 = total_gas_used[..total_gas_used.len() - 1].iter().sum();

    let aggregated_proof = &build_proof_log(module, &mut state)?;

    let proof = MockZkvm::create_serialized_proof(true, aggregated_proof);

    // We use the unmetered working set, because we don't want to charge for the gas used in the last transition (this makes the test simpler)
    let mut state = state.to_working_set_unmetered();

    if let Err(err) = module.process_proof(&proof, &prover_address, &mut state) {
        panic!("Error when processing proof: {:?}", err);
    }

    let (state, _, events) = state.checkpoint();

    Ok((total_gas_used, state, events))
}

// Performs a sequence of checks to ensure that the prover has been rewarded correctly
fn check_reward(
    prover_address: <S as Spec>::Address,
    total_gas_used: u64,
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    state: &mut StateCheckpoint<S>,
    events: &mut Vec<TypedEvent>,
) -> Result<u64, Infallible> {
    // Compute the proof reward
    // Reward = total_gas_used * (1-burn_rate)%
    let reward = module.burn_rate().apply(total_gas_used);

    // Assert that the working set contains a rewarded event
    assert_eq!(events.len(), 1);
    let event: Event<S> = events.pop().unwrap().downcast().unwrap();

    assert_eq!(
        event,
        Event::ProcessedValidProof {
            prover: prover_address,
            reward,
        }
    );

    // Assert that the prover has been rewarded on his account
    // The outstanding balance is the initial balance plus the reward minus the bond amount
    let token_addr = GAS_TOKEN_ID;

    assert_eq!(
        module
            .bank
            .get_balance_of(&prover_address, token_addr, state)?
            .unwrap_or_default(),
        reward + INITIAL_PROVER_BALANCE - TEST_DEFAULT_USER_STAKE
    );

    // Assert that the prover's bond amount has not been burned
    assert_eq!(
        module.get_bond_amount(prover_address, state)?,
        TEST_DEFAULT_USER_STAKE
    );

    Ok(reward)
}

/// Checks that the prover gets penalized if he tries to prove the same transitions again
fn check_penalization_if_proven_again(
    prover_address: <S as Spec>::Address,
    proving_penalty: u64,
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    mut state: StateCheckpoint<S>,
) -> Result<StateCheckpoint<S>, Infallible> {
    assert_eq!(
        module
            .last_claimed_reward
            .get(&mut state)?
            .expect("This slot height should be present in the claimed_rewards map"),
        LAST_SLOT_NUM,
        "The reward for the slot height {} should be claimed",
        LAST_SLOT_NUM
    );

    let proof_log = build_proof_log(module, &mut state)?;
    let proof = MockZkvm::create_serialized_proof(true, proof_log);

    let mut state = state.to_working_set_unmetered();
    module
        .process_proof(&proof, &prover_address, &mut state)
        .expect("The proof should not be rejected");

    // Assert that the working set contains a penalized event
    assert_eq!(state.events().len(), 1);
    let event: Event<S> = state.take_event(0).unwrap().downcast().unwrap();
    assert_eq!(
        event,
        Event::ProverPenalized {
            prover: prover_address,
            amount: proving_penalty,
            reason: crate::event::PenalizationReason::ProofAlreadyProcessed
        }
    );

    let (mut checkpoint, _, _) = state.checkpoint();

    // Assert that the prover's bond amount has been penalized
    assert_eq!(
        module.get_bond_amount(prover_address, &mut checkpoint)?,
        TEST_DEFAULT_USER_STAKE - proving_penalty
    );

    Ok(checkpoint)
}

fn check_unbonding(
    prover_address: <S as Spec>::Address,
    expected_amount_withdrawn: u64,
    old_balance: u64,
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    state: StateCheckpoint<S>,
) -> Result<StateCheckpoint<S>, Infallible> {
    let mut state = state.to_working_set_unmetered();
    module
        .unbond_prover(&prover_address, &mut state)
        .expect("The proof should not be rejected");

    let (mut checkpoint, _, mut events) = state.checkpoint();

    assert_eq!(events.len(), 1);
    let event: Event<S> = events.pop().unwrap().downcast().unwrap();
    assert_eq!(
        event,
        Event::UnBondedProver {
            prover: prover_address,
            amount_withdrawn: expected_amount_withdrawn
        }
    );

    // Check that the prover has been unbonded
    assert_eq!(module.get_bond_amount(prover_address, &mut checkpoint)?, 0);

    // Check the amount on the prover's balance
    assert_eq!(
        module
            .bank
            .get_balance_of(&prover_address, GAS_TOKEN_ID, &mut checkpoint)?
            .unwrap(),
        old_balance + expected_amount_withdrawn
    );

    Ok(checkpoint)
}

#[test]
/// Macro-test for the happy path of processing a valid proof.
fn test_valid_proof() -> Result<(), Infallible> {
    let (module, prover_address, sequencer, state) = setup();

    let max_gas_used_per_step = <S as Spec>::Gas::from([MAX_TX_GAS_AMOUNT / 100; 2]);

    // Process a valid proof
    let (gas_token_used, mut state, mut events) = execute_txs_and_process_valid_proof(
        prover_address,
        sequencer,
        &max_gas_used_per_step,
        &module,
        state,
    )?;

    let reward = check_reward(
        prover_address,
        gas_token_used,
        &module,
        &mut state,
        &mut events,
    )?;

    // Now we have to check we can unbond
    check_unbonding(
        prover_address,
        TEST_DEFAULT_USER_STAKE,
        INITIAL_PROVER_BALANCE - TEST_DEFAULT_USER_STAKE + reward,
        &module,
        state,
    )?;
    Ok(())
}

#[test]
/// Macro-test for the happy path of processing a valid proof with penalization.
fn test_valid_proof_with_penalization() -> Result<(), Infallible> {
    let (module, prover_address, sequencer, state) = setup();

    let max_gas_used_per_step = <S as Spec>::Gas::from([MAX_TX_GAS_AMOUNT / 100; 2]);

    // Process a valid proof
    let (total_gas_used, mut state, mut events) = execute_txs_and_process_valid_proof(
        prover_address,
        sequencer,
        &max_gas_used_per_step,
        &module,
        state,
    )?;

    let reward = check_reward(
        prover_address,
        total_gas_used,
        &module,
        &mut state,
        &mut events,
    )?;

    let proving_penalty = module
        .proving_penalty
        .get(&mut state)?
        .expect("The proving penalty should be set at genesis");

    // Now we have to check that we cannot prove the same transitions again
    let state =
        check_penalization_if_proven_again(prover_address, proving_penalty, &module, state)?;

    // Now we have to check we can unbond
    check_unbonding(
        prover_address,
        TEST_DEFAULT_USER_STAKE - proving_penalty,
        INITIAL_PROVER_BALANCE - TEST_DEFAULT_USER_STAKE + reward,
        &module,
        state,
    )?;

    Ok(())
}
