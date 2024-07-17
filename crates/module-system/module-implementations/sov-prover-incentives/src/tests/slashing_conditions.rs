//! Tests for slashing conditions
//! We are using the unmetered working set to test the slashing conditions so that we can keep these tests simple.
use std::convert::Infallible;

use sov_mock_da::MockValidityCond;
use sov_mock_zkvm::MockZkvm;
use sov_modules_api::{
    AggregatedProofPublicData, CodeCommitment, Spec, StateAccessor, StateCheckpoint, WorkingSet,
};

use super::helpers::{
    get_transition_unwrap, simulate_chain_state_execution, MAX_TX_GAS_AMOUNT, MOCK_PROVER_ADDRESS,
};
use crate::event::SlashingReason;
use crate::tests::helpers::{setup, MOCK_CODE_COMMITMENT, S};
use crate::Event;

const FIRST_SLOT_NUM: u64 = 1;
const LAST_SLOT_NUM: u64 = 2;

/// Setups the slashing tests
fn slashing_setup() -> (
    crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    <S as Spec>::Address,
    StateCheckpoint<S>,
) {
    let (module, prover_address, sequencer, state) = setup();

    // Simulate execution of the chain-state

    let gas_used_per_step = <S as Spec>::Gas::from([MAX_TX_GAS_AMOUNT / 100; 2]);
    // The first transition is the genesis transition
    // Then we have two more transitions
    let (state_checkpoint, _) = simulate_chain_state_execution(
        &module,
        sequencer,
        ((LAST_SLOT_NUM - FIRST_SLOT_NUM + 1) + 1)
            .try_into()
            .unwrap(),
        &gas_used_per_step,
        state,
    );

    (module, prover_address, state_checkpoint)
}

/// Proves a transition log
fn prove_transition_log(
    aggregated_proof: AggregatedProofPublicData,
    prover_address: <S as Spec>::Address,
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    state: &mut WorkingSet<S>,
) {
    let proof = MockZkvm::create_serialized_proof(true, aggregated_proof);

    module
        .process_proof(&proof, &prover_address, state)
        .expect("An invalid proof is not an error");
}

/// Checks if the prover has been slashed for the correct reason.
fn check_prover_slashed(
    reason: SlashingReason,
    prover_address: <S as Spec>::Address,
    module: &crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    state: &mut WorkingSet<S>,
) -> Result<(), Infallible> {
    // Check that the prover is slashed
    assert_eq!(state.events().len(), 1);
    let event: Event<S> = state.take_event(0).unwrap().downcast().unwrap();
    assert_eq!(
        event,
        Event::ProverSlashed {
            prover: prover_address,
            reason
        }
    );

    // Assert that the prover's bond amount has been burned
    assert_eq!(
        module.get_bond_amount(prover_address, &mut state.to_unmetered())?,
        0
    );

    Ok(())
}

#[test]
/// The prover gets slashed if they submit an invalid zk-proof
fn test_slash_on_invalid_proof() -> Result<(), Infallible> {
    let (module, prover_address, state_checkpoint) = slashing_setup();

    let mut state = state_checkpoint.to_working_set_unmetered();

    // Process an invalid proof
    {
        let proof = &MockZkvm::create_serialized_proof(false, ());
        module
            .process_proof(proof, &prover_address, &mut state)
            .expect("An invalid proof is not an error");
    }

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::ProofInvalid,
        prover_address,
        &module,
        &mut state,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid genesis_hash
fn test_slash_on_invalid_genesis_hash() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    // Process an invalid proof
    let mut state = {
        let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
        let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

        let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
        let log_with_wrong_initial_state_root = AggregatedProofPublicData {
            validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
            initial_slot_number: FIRST_SLOT_NUM,
            final_slot_number: LAST_SLOT_NUM,
            genesis_state_root: first_transition.post_state_root().as_ref().to_vec(),
            initial_state_root: genesis_hash.as_ref().to_vec(),
            final_state_root: last_transition.post_state_root().as_ref().to_vec(),
            initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
            final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
            code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
            rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
        };

        let mut state = state.to_working_set_unmetered();

        prove_transition_log(
            log_with_wrong_initial_state_root,
            prover_address,
            &module,
            &mut state,
        );

        state
    };

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::IncorrectGenesisHash,
        prover_address,
        &module,
        &mut state,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid final slot hash
fn test_slash_on_invalid_initial_state_root() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    // Process an invalid proof

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let log_with_wrong_initial_state_root = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM,
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        initial_state_root: last_transition.post_state_root().as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut state = state.to_working_set_unmetered();

    prove_transition_log(
        log_with_wrong_initial_state_root,
        prover_address,
        &module,
        &mut state,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::IncorrectInitialStateRoot,
        prover_address,
        &module,
        &mut state,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid final slot hash
fn test_slash_on_invalid_final_slot_hash() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    // Process an invalid proof
    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let log_with_wrong_initial_state_root = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM,
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        initial_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut state = state.to_working_set_unmetered();

    prove_transition_log(
        log_with_wrong_initial_state_root,
        prover_address,
        &module,
        &mut state,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::IncorrectFinalSlotHash,
        prover_address,
        &module,
        &mut state,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid final state root
fn test_slash_on_invalid_final_state_root() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    // Process an invalid proof

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let log_with_wrong_final_state_root = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM,
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        initial_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: first_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut state = state.to_working_set_unmetered();

    prove_transition_log(
        log_with_wrong_final_state_root,
        prover_address,
        &module,
        &mut state,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::IncorrectFinalStateRoot,
        prover_address,
        &module,
        &mut state,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid initial slot hash
fn test_slash_on_invalid_initial_slot_hash() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let log_with_wrong_initial_slot_hash = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM,
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        initial_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut state = state.to_working_set_unmetered();

    prove_transition_log(
        log_with_wrong_initial_slot_hash,
        prover_address,
        &module,
        &mut state,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::IncorrectInitialSlotHash,
        prover_address,
        &module,
        &mut state,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid initial transition
fn test_slash_on_invalid_initial_transition() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    // Process an invalid proof

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let log_with_wrong_initial_state_root = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: LAST_SLOT_NUM + 1,
        final_slot_number: LAST_SLOT_NUM,
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        initial_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut working_set = state.to_working_set_unmetered();

    prove_transition_log(
        log_with_wrong_initial_state_root,
        prover_address,
        &module,
        &mut working_set,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::InitialTransitionDoesNotExist,
        prover_address,
        &module,
        &mut working_set,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for an invalid final transition
fn test_slash_on_invalid_final_transition() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    // Process an invalid proof

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let log_with_wrong_initial_state_root = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM + 1,
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        initial_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut working_set = state.to_working_set_unmetered();
    prove_transition_log(
        log_with_wrong_initial_state_root,
        prover_address,
        &module,
        &mut working_set,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::FinalTransitionDoesNotExist,
        prover_address,
        &module,
        &mut working_set,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for which the output cannot be deserialized properly
fn test_slash_on_invalid_output_format() -> Result<(), Infallible> {
    let (module, prover_address, state_checkpoint) = slashing_setup();

    let mut working_set = state_checkpoint.to_working_set_unmetered();

    // Process an invalid proof
    {
        let proof = MockZkvm::create_serialized_proof(true, ());

        module
            .process_proof(&proof, &prover_address, &mut working_set)
            .expect("An invalid proof is not an error");
    }

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::ProofInvalid,
        prover_address,
        &module,
        &mut working_set,
    )
}

#[test]
/// The prover gets slashed if they submit a valid proof for which the validity conditions are not correctly stored in the chain-state
fn test_slash_on_invalid_validity_cond() -> Result<(), Infallible> {
    let (module, prover_address, mut state) = slashing_setup();

    // Process an invalid proof

    let genesis_hash = module
        .chain_state
        .get_genesis_hash(&mut state)?
        .expect("Genesis hash must be set at genesis");

    let first_transition = get_transition_unwrap(FIRST_SLOT_NUM, &module, &mut state);
    let last_transition = get_transition_unwrap(LAST_SLOT_NUM, &module, &mut state);

    let vec_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: true }).unwrap();
    let vec_false_validity_cond = borsh::to_vec(&MockValidityCond { is_valid: false }).unwrap();
    let log_with_wrong_initial_state_root = AggregatedProofPublicData {
        validity_conditions: vec![vec_validity_cond.clone(), vec_false_validity_cond],
        initial_slot_number: FIRST_SLOT_NUM,
        final_slot_number: LAST_SLOT_NUM,
        initial_state_root: genesis_hash.as_ref().to_vec(),
        genesis_state_root: genesis_hash.as_ref().to_vec(),
        final_state_root: last_transition.post_state_root().as_ref().to_vec(),
        initial_slot_hash: first_transition.slot_hash().as_ref().to_vec(),
        final_slot_hash: last_transition.slot_hash().as_ref().to_vec(),
        code_commitment: CodeCommitment(MOCK_CODE_COMMITMENT.0.to_vec()),
        rewarded_addresses: vec![MOCK_PROVER_ADDRESS.as_ref().to_vec()],
    };

    let mut working_set = state.to_working_set_unmetered();

    prove_transition_log(
        log_with_wrong_initial_state_root,
        prover_address,
        &module,
        &mut working_set,
    );

    // Check that the prover is slashed
    check_prover_slashed(
        SlashingReason::IncorrectValidityConditions,
        prover_address,
        &module,
        &mut working_set,
    )
}
