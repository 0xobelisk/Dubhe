use std::convert::Infallible;

use sov_bank::GAS_TOKEN_ID;
use sov_modules_api::optimistic::Attestation;
use sov_modules_api::{Context, StateCheckpoint};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_test_utils::TEST_DEFAULT_USER_STAKE;

use crate::call::AttesterIncentiveErrors;
use crate::tests::helpers::{
    commit_get_new_storage, setup, ExecutionSimulationVars, DEFAULT_ROLLUP_FINALITY, INIT_HEIGHT,
};
type S = sov_test_utils::TestSpec;

#[test]
fn test_two_phase_unbonding() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
    let storage = storage_manager.create_storage();
    let state = StateCheckpoint::new(storage.clone());
    let (module, attester_address, _, sequencer, mut state) = setup(state);

    // Assert that the attester has the correct bond amount before processing the proof
    assert_eq!(
        module
            .get_bond_amount(attester_address, crate::call::Role::Attester, &mut state)?
            .value,
        TEST_DEFAULT_USER_STAKE
    );

    let context = Context::<S>::new(
        attester_address,
        Default::default(),
        sequencer,
        INIT_HEIGHT + 2,
    );

    let mut state = state.to_working_set_unmetered();
    // Try to skip the first phase of the two-phase unbonding. Should fail.
    {
        // Should fail
        let err = module
            .end_unbond_attester(&context, &mut state)
            .unwrap_err();
        assert_eq!(err, AttesterIncentiveErrors::AttesterIsNotUnbonding);
    }

    let state_checkpoint = state.checkpoint().0;
    commit_get_new_storage(storage, state_checkpoint, &mut storage_manager);
    // Simulate the execution of a chain, with the genesis hash and two transitions after.
    // Update the chain_state module and the optimistic module accordingly
    let (mut exec_vars, state_checkpoint) = ExecutionSimulationVars::execute(
        3,
        &module,
        &mut storage_manager,
        &sequencer,
        &attester_address,
    );

    let mut state = state_checkpoint.to_working_set_unmetered();
    // Start unbonding and then try to prove a transition. User slashed
    module
        .begin_unbond_attester(&context, &mut state)
        .expect("Should succeed");

    let _transition_2 = exec_vars.pop().unwrap();
    let transition_1 = exec_vars.pop().unwrap();
    let initial_transition = exec_vars.pop().unwrap();

    // Process a valid attestation but get slashed because the attester was trying to unbond.
    {
        let attestation = Attestation {
            initial_state_root: initial_transition.state_root,
            slot_hash: [1; 32].into(),
            post_state_root: transition_1.state_root,
            proof_of_bond: sov_modules_api::optimistic::ProofOfBond {
                claimed_transition_num: INIT_HEIGHT + 1,
                proof: initial_transition.state_proof,
            },
        };

        let err = module
            .process_attestation(&context, attestation.into(), &mut state)
            .unwrap_err();

        assert_eq!(
            err,
            AttesterIncentiveErrors::UserNotBonded,
            "The attester should not be bonded"
        );

        // We cannot try to bond either
        let err = module
            .bond_user_helper(
                TEST_DEFAULT_USER_STAKE,
                &attester_address,
                crate::call::Role::Attester,
                &mut state,
            )
            .unwrap_err();

        assert_eq!(
            err,
            AttesterIncentiveErrors::AttesterIsUnbonding,
            "Should raise an AttesterIsUnbonding error"
        );
    }

    // Cannot bond again while unbonding
    {
        let err = module
            .bond_user_helper(
                TEST_DEFAULT_USER_STAKE,
                &attester_address,
                crate::call::Role::Attester,
                &mut state,
            )
            .unwrap_err();

        assert_eq!(
            err,
            AttesterIncentiveErrors::AttesterIsUnbonding,
            "Should raise that error"
        );
    }

    // Now try to complete the two-phase unbonding immediately:
    // the second phase should fail because the
    // first phase cannot get finalized
    {
        // Should fail
        let err = module
            .end_unbond_attester(&context, &mut state)
            .unwrap_err();
        assert_eq!(err, AttesterIncentiveErrors::UnbondingNotFinalized);
    }

    // Now unbond the right way.
    {
        let mut state = state.checkpoint().0;
        let initial_account_balance = module
            .bank
            .get_balance_of(&attester_address, GAS_TOKEN_ID, &mut state)?
            .unwrap();
        let mut state = state.to_working_set_unmetered();

        // Start unbonding the user: should succeed
        module.begin_unbond_attester(&context, &mut state).unwrap();

        let mut state = state.checkpoint().0;

        let unbonding_info = module
            .unbonding_attesters
            .get(&attester_address, &mut state)?
            .unwrap();

        assert_eq!(
            unbonding_info.unbonding_initiated_height, INIT_HEIGHT,
            "Invalid beginning unbonding height"
        );

        // Wait for the light client to finalize
        module
            .light_client_finalized_height
            .set(&(INIT_HEIGHT + DEFAULT_ROLLUP_FINALITY), &mut state)?;

        let mut state = state.to_working_set_unmetered();
        // Finish the unbonding: should succeed
        module.end_unbond_attester(&context, &mut state).unwrap();
        let mut state = state.checkpoint().0;

        // Check that the final balance is the same as the initial balance
        assert_eq!(
            initial_account_balance + TEST_DEFAULT_USER_STAKE,
            module
                .bank
                .get_balance_of(&attester_address, GAS_TOKEN_ID, &mut state)?
                .unwrap(),
            "The initial and final account balance don't match"
        );
    }

    Ok(())
}
