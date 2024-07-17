use std::convert::Infallible;

use sov_bank::GAS_TOKEN_ID;
use sov_mock_da::{MockAddress, MockDaSpec, MockValidityCond};
use sov_mock_zkvm::MockZkvm;
use sov_modules_api::{Context, Spec, StateCheckpoint, WorkingSet};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_rollup_interface::zk::StateTransitionPublicData;
use sov_state::StorageRoot;
use sov_test_utils::{TestStorageSpec, TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_STAKE};

use crate::call::AttesterIncentiveErrors;
use crate::tests::helpers::{commit_get_new_storage, setup, ExecutionSimulationVars, INIT_HEIGHT};
use crate::SlashingReason;

type S = sov_test_utils::TestSpec;

/// Test that given an invalid transition, a challenger can successfully challenge it and get rewarded
#[test]
fn test_valid_challenge() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let storage = storage_manager.create_storage();
    let state = StateCheckpoint::new(storage.clone());
    let (module, attester_address, challenger_address, sequencer, state) = setup(state);

    // Simulate the execution of a chain, with the genesis hash and two transitions after.
    // Update the chain_state module and the optimistic module accordingly
    commit_get_new_storage(storage, state, &mut storage_manager);
    let (mut exec_vars, state_checkpoint) = ExecutionSimulationVars::execute(
        3,
        &module,
        &mut storage_manager,
        &sequencer,
        &attester_address,
    );

    let mut state = state_checkpoint.to_working_set_unmetered();

    let _ = exec_vars.pop().unwrap();
    let transition_1 = exec_vars.pop().unwrap();
    let initial_transition = exec_vars.pop().unwrap();

    module
        .bond_user_helper(
            TEST_DEFAULT_USER_STAKE,
            &challenger_address,
            crate::call::Role::Challenger,
            &mut state,
        )
        .unwrap();

    let mut state = state.checkpoint().0;

    // Assert that the challenger has the correct bond amount before processing the proof
    assert_eq!(
        module
            .get_bond_amount(
                challenger_address,
                crate::call::Role::Challenger,
                &mut state
            )?
            .value,
        TEST_DEFAULT_USER_STAKE
    );

    // Set a bad transition to get a reward from
    module
        .bad_transition_pool
        .set(&(INIT_HEIGHT + 1), &TEST_DEFAULT_USER_STAKE, &mut state)?;

    let context = Context::<S>::new(
        challenger_address,
        Default::default(),
        sequencer,
        INIT_HEIGHT + 2,
    );

    {
        let transition = StateTransitionPublicData::<MockAddress, MockDaSpec, _> {
            initial_state_root: initial_transition.state_root,
            slot_hash: [1; 32].into(),
            final_state_root: transition_1.state_root,
            validity_condition: MockValidityCond { is_valid: true },
            prover_address: Default::default(),
        };

        let proof = &MockZkvm::create_serialized_proof(true, transition);

        let mut working_set = state.to_working_set_unmetered();

        module
            .process_challenge(
                &context,
                proof.as_slice(),
                &(INIT_HEIGHT + 1),
                &mut working_set,
            )
            .expect("Should not fail");

        state = working_set.checkpoint().0;

        // Check that the challenger was rewarded
        assert_eq!(
            module
                .bank
                .get_balance_of(&challenger_address, GAS_TOKEN_ID, &mut state)?
                .unwrap(),
            TEST_DEFAULT_USER_BALANCE - TEST_DEFAULT_USER_STAKE
                + module.burn_rate().apply(TEST_DEFAULT_USER_STAKE),
            "The challenger should have been rewarded"
        );

        // Check that the challenge set is empty
        assert_eq!(
            module
                .bad_transition_pool
                .get(&(INIT_HEIGHT + 1), &mut state)?,
            None,
            "The transition should have disappeared"
        );
    }

    {
        // Now try to unbond the challenger
        let mut working_set = state.to_working_set_unmetered();
        module
            .unbond_challenger(&context, &mut working_set)
            .expect("The challenger should be able to unbond");
        state = working_set.checkpoint().0;

        // Check the final balance of the challenger
        assert_eq!(
            module
                .bank
                .get_balance_of(&challenger_address, GAS_TOKEN_ID, &mut state)?
                .unwrap(),
            TEST_DEFAULT_USER_BALANCE + module.burn_rate().apply(TEST_DEFAULT_USER_STAKE),
            "The challenger should have been unbonded"
        );
    }

    Ok(())
}

fn invalid_proof_helper(
    context: &Context<S>,
    proof: &Vec<u8>,
    reason: SlashingReason,
    challenger_address: <S as Spec>::Address,
    module: &crate::AttesterIncentives<S, MockDaSpec>,
    state: &mut WorkingSet<S>,
) {
    // Let's bond the challenger and try to publish a false challenge
    module
        .bond_user_helper(
            TEST_DEFAULT_USER_STAKE,
            &challenger_address,
            crate::call::Role::Challenger,
            state,
        )
        .expect("Should be able to bond");

    module
        .process_challenge(context, proof.as_slice(), &(INIT_HEIGHT + 1), state)
        .expect("Since the challenger is slashed this should exit gracefully");

    // We get the last event from the working set to check the slashing reason
    let mut events = state.take_events();
    let slash_event = events.pop().unwrap();
    let slash_event = slash_event.downcast::<crate::Event<S>>().unwrap();

    // Check the error raised
    assert_eq!(
        slash_event,
        crate::Event::UserSlashed {
            address: challenger_address,
            reason
        },
        "The challenge processing should fail with an invalid proof error"
    );
}

#[test]
fn test_invalid_challenge() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let storage = storage_manager.create_storage();
    let state = StateCheckpoint::new(storage.clone());
    let (module, attester_address, challenger_address, sequencer, state) = setup(state);

    // Simulate the execution of a chain, with the genesis hash and two transitions after.
    // Update the chain_state module and the optimistic module accordingly
    commit_get_new_storage(storage, state, &mut storage_manager);
    let (mut exec_vars, mut state_checkpoint) = ExecutionSimulationVars::execute(
        3,
        &module,
        &mut storage_manager,
        &sequencer,
        &attester_address,
    );

    let _ = exec_vars.pop().unwrap();
    let transition_1 = exec_vars.pop().unwrap();
    let initial_transition = exec_vars.pop().unwrap();

    // Set a bad transition to get a reward from
    module.bad_transition_pool.set(
        &(INIT_HEIGHT + 1),
        &TEST_DEFAULT_USER_STAKE,
        &mut state_checkpoint,
    )?;

    let context = Context::<S>::new(
        challenger_address,
        Default::default(),
        sequencer,
        INIT_HEIGHT + 2,
    );
    let transition: StateTransitionPublicData<MockAddress, MockDaSpec, _> =
        create_transition_public_data_default(
            initial_transition.state_root,
            transition_1.state_root,
            [1; 32],
        );

    let mut state = state_checkpoint.to_working_set_unmetered();

    {
        // A valid proof
        let proof = MockZkvm::create_serialized_proof(true, &transition);

        let err = module
            .process_challenge(&context, proof.as_slice(), &(INIT_HEIGHT + 1), &mut state)
            .unwrap_err();

        // Check the error raised
        assert_eq!(
            err,
            AttesterIncentiveErrors::UserNotBonded,
            "The challenge processing should fail with an unbonded error"
        );
    }

    // Invalid proofs
    {
        // An invalid proof
        let proof = &MockZkvm::create_serialized_proof(false, &transition);

        invalid_proof_helper(
            &context,
            proof,
            SlashingReason::InvalidProofOutputs,
            challenger_address,
            &module,
            &mut state,
        );

        // Bad slot hash
        let bad_transition = create_transition_public_data_default(
            initial_transition.state_root,
            transition_1.state_root,
            [2; 32],
        );

        // An invalid proof
        let proof = &MockZkvm::create_serialized_proof(true, bad_transition);

        invalid_proof_helper(
            &context,
            proof,
            SlashingReason::TransitionInvalid,
            challenger_address,
            &module,
            &mut state,
        );

        // Bad validity condition
        let bad_transition = StateTransitionPublicData::<MockAddress, MockDaSpec, _> {
            initial_state_root: initial_transition.state_root,
            slot_hash: [1; 32].into(),
            final_state_root: transition_1.state_root,
            validity_condition: MockValidityCond { is_valid: false },
            prover_address: Default::default(),
        };

        let proof = &MockZkvm::create_serialized_proof(true, bad_transition);

        invalid_proof_helper(
            &context,
            proof,
            SlashingReason::TransitionInvalid,
            challenger_address,
            &module,
            &mut state,
        );

        // Bad initial root
        let bad_transition = create_transition_public_data_default(
            transition_1.state_root,
            transition_1.state_root,
            [1; 32],
        );

        let proof = &MockZkvm::create_serialized_proof(true, bad_transition);

        invalid_proof_helper(
            &context,
            proof,
            SlashingReason::InvalidInitialHash,
            challenger_address,
            &module,
            &mut state,
        );
    }

    Ok(())
}

fn create_transition_public_data_default(
    initial_state_root: StorageRoot<TestStorageSpec>,
    final_state_root: StorageRoot<TestStorageSpec>,
    slot_hash: [u8; 32],
) -> StateTransitionPublicData<MockAddress, MockDaSpec, StorageRoot<TestStorageSpec>> {
    StateTransitionPublicData {
        initial_state_root,
        slot_hash: slot_hash.into(),
        final_state_root,
        validity_condition: MockValidityCond { is_valid: true },
        prover_address: Default::default(),
    }
}
