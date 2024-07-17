use std::convert::Infallible;
use std::vec;

use sov_attester_incentives::{CallMessage, Role, WrappedAttestation};
use sov_bank::GAS_TOKEN_ID;
use sov_modules_api::optimistic::Attestation;
use sov_modules_api::{Batch, StateCheckpoint};
use sov_modules_stf_blueprint::TxEffect;
use sov_state::StorageRoot;
use sov_test_utils::auth::TestAuth;
use sov_test_utils::generators::attester_incentive::AttesterIncentivesMessageGenerator;
use sov_test_utils::runtime::optimistic::TestRuntime;
use sov_test_utils::{new_test_blob_from_batch, MessageGenerator, TestStorageSpec};

use super::{AttesterIncentivesTestHandler, TEST_DEFAULT_USER_BALANCE};
use crate::helpers::{Da, ExecutionSimulationVars, TestRollup, S};

impl AttesterIncentivesTestHandler {
    // The current maximum attested height is 0 and the attester is bonded
    fn check_initial_attestation_conditions(
        &self,
        rollup: &mut TestRollup,
    ) -> Result<(), Infallible> {
        assert_eq!(rollup.get_maximum_attested_height()?, 0);

        assert_eq!(
            rollup.get_user_bond(Role::Attester, self.attester_addr())?,
            self.attester_stake
        );

        let mut state = StateCheckpoint::<S>::new(rollup.storage());
        assert_eq!(
            rollup
                .bank()
                .get_balance_of(&self.attester_addr(), GAS_TOKEN_ID, &mut state)?,
            Some(self.attester_balance - self.attester_stake)
        );

        Ok(())
    }

    // Checks that the first attestation was processed correctly
    fn check_first_attestation_processing(
        &self,
        attestation_tx: &ExecutionSimulationVars,
        honest_attester_new_balance: u64,
        rollup: &mut TestRollup,
    ) -> Result<(), Infallible> {
        assert_eq!(attestation_tx.batch_receipts.len(), 1);
        let batch_receipt = attestation_tx.batch_receipts.first().unwrap();
        assert_eq!(batch_receipt.tx_receipts.len(), 1);
        let tx_receipt = batch_receipt.tx_receipts.first().unwrap();
        assert_eq!(tx_receipt.receipt, TxEffect::Successful(()));

        // We have to check that the attester was rewarded with its stake.
        let mut state = StateCheckpoint::<S>::new(rollup.storage());
        assert_eq!(
            rollup
                .bank()
                .get_balance_of(&self.attester_addr(), GAS_TOKEN_ID, &mut state)?,
            Some(honest_attester_new_balance)
        );

        Ok(())
    }

    // Attest only the first transition and check that the attestation was processed correctly
    fn try_attest_first_transition(
        &self,
        genesis_root: StorageRoot<TestStorageSpec>,
        execution_vars: Vec<ExecutionSimulationVars>,
        honest_attester_balance: u64,
        rollup: &mut TestRollup,
    ) -> Result<(u64, ExecutionSimulationVars), Infallible> {
        assert!(execution_vars.len() >= 2);

        let ExecutionSimulationVars {
            state_root: first_state_root,
            state_proof: first_state_proof,
            ..
        } = execution_vars[0].clone();

        let ExecutionSimulationVars {
            state_root: snd_state_root,
            ..
        } = execution_vars[1].clone();

        let attestation = Attestation {
            initial_state_root: genesis_root,
            slot_hash: [10; 32].into(),
            post_state_root: first_state_root,
            proof_of_bond: sov_modules_api::optimistic::ProofOfBond {
                claimed_transition_num: 1,
                proof: first_state_proof.unwrap(),
            },
        };

        let txs = AttesterIncentivesMessageGenerator::from(vec![(
            self.attester_private_key.clone(),
            CallMessage::ProcessAttestation::<S, Da>(WrappedAttestation::from(attestation)),
        )])
        .create_default_raw_txs::<TestRuntime<S, Da>, TestAuth<S, Da>>();

        let attestation_blob =
            new_test_blob_from_batch(Batch { txs }, self.seq_da_addr.as_ref(), [3; 32]);

        let exec_vars = rollup.execution_simulation(
            1,
            snd_state_root,
            vec![attestation_blob],
            2,
            Some(self.attester_addr()),
        );

        assert_eq!(exec_vars.len(), 1, "There should be one slot processed");

        let attestation_tx = exec_vars
            .first()
            .expect("The attestation execution simulation failed");

        // The new attester balance is the initial attester balance minus the gas cost of the transaction
        // plus the burn rate applied to the amount of gas proved in the attestation
        let gas_proved_value = execution_vars[0].gas_consumed_value();

        let burn_rate = rollup.burn_rate();

        // The new attester balance is the initial attester balance minus the gas cost of the transaction
        // plus the burn rate applied to the amount of gas proved in the attestation
        let gas_consumed_attestation_value: u64 = attestation_tx.gas_consumed_value();

        let new_attester_balance = honest_attester_balance - gas_consumed_attestation_value
            + burn_rate.apply(gas_proved_value);

        self.check_first_attestation_processing(attestation_tx, new_attester_balance, rollup)?;

        Ok((new_attester_balance, attestation_tx.clone()))
    }

    // Checks that the second and third attestations were processed correctly
    fn check_second_and_third_attestation_processing(
        &self,
        attestation_exec_res: &ExecutionSimulationVars,
        expected_attester_balance: u64,
        rollup: &mut TestRollup,
    ) -> Result<(), Infallible> {
        assert_eq!(attestation_exec_res.batch_receipts.len(), 1);
        let mut tx_receipts = attestation_exec_res
            .batch_receipts
            .first()
            .unwrap()
            .tx_receipts
            .clone();
        assert_eq!(tx_receipts.len(), 2);
        let snd_tx_receipt = tx_receipts.pop().unwrap();
        let fst_tx_receipt = tx_receipts.pop().unwrap();
        assert_eq!(fst_tx_receipt.receipt, TxEffect::Successful(()));
        assert_eq!(snd_tx_receipt.receipt, TxEffect::Successful(()));

        // The current maximum attested height is 3
        assert_eq!(rollup.get_maximum_attested_height()?, 3);

        // We have to check that the attester was rewarded correctly.
        let mut state = StateCheckpoint::<S>::new(rollup.storage());

        assert_eq!(
            rollup
                .bank()
                .get_balance_of(&self.attester_addr(), GAS_TOKEN_ID, &mut state)?,
            Some(expected_attester_balance)
        );
        Ok(())
    }

    // Attest multiple transitions within one block
    // Let's try to attest the second transition and the first attestation
    fn try_attest_second_transition_and_first_attestation(
        &self,
        prev_exec_vars: Vec<ExecutionSimulationVars>,
        honest_attester_balance: u64,
        rollup: &mut TestRollup,
    ) -> Result<(), Infallible> {
        assert!(prev_exec_vars.len() >= 3);
        let ExecutionSimulationVars {
            state_root: first_state_root,
            ..
        } = prev_exec_vars[0].clone();

        let ExecutionSimulationVars {
            state_root: snd_state_root,
            state_proof: snd_state_proof,
            ..
        } = prev_exec_vars[1].clone();

        let ExecutionSimulationVars {
            state_root: attestation_state_root,
            state_proof: attestation_state_proof,
            ..
        } = prev_exec_vars[2].clone();

        let fst_attestation = Attestation {
            initial_state_root: first_state_root,
            slot_hash: [20; 32].into(),
            post_state_root: snd_state_root,
            proof_of_bond: sov_modules_api::optimistic::ProofOfBond {
                claimed_transition_num: 2,
                proof: snd_state_proof.unwrap(),
            },
        };

        let snd_attestation = Attestation {
            initial_state_root: snd_state_root,
            slot_hash: [30; 32].into(),
            post_state_root: attestation_state_root,
            proof_of_bond: sov_modules_api::optimistic::ProofOfBond {
                claimed_transition_num: 3,
                proof: attestation_state_proof.unwrap(),
            },
        };

        let txs = AttesterIncentivesMessageGenerator::from(vec![
            (
                self.attester_private_key.clone(),
                CallMessage::ProcessAttestation::<S, Da>(WrappedAttestation::from(fst_attestation)),
            ),
            (
                self.attester_private_key.clone(),
                CallMessage::ProcessAttestation::<S, Da>(WrappedAttestation::from(snd_attestation)),
            ),
        ])
        .create_default_raw_txs::<TestRuntime<S, Da>, TestAuth<S, Da>>();

        let attestation_blob =
            new_test_blob_from_batch(Batch { txs }, self.seq_da_addr.as_ref(), [4; 32]);

        let exec_vars =
            rollup.execution_simulation(1, attestation_state_root, vec![attestation_blob], 3, None);

        let attestation_transition = exec_vars
            .first()
            .expect("The rollup panicked while processing the second attestation");

        let gas_consumed_attestations_value = exec_vars[0].gas_consumed_value();

        let gas_proved_first_attestation_value = prev_exec_vars[1].gas_consumed_value();

        let gas_proved_second_attestation_value = prev_exec_vars[2].gas_consumed_value();

        // Formula: new_balance = old_balance + burn_rate * (gas_proved_first_attestation + gas_proved_second_attestation) - tx_cost
        let burn_rate = rollup.burn_rate();
        let expected_attester_balance = honest_attester_balance
            + burn_rate.apply(gas_proved_first_attestation_value)
            + burn_rate.apply(gas_proved_second_attestation_value)
            - gas_consumed_attestations_value;

        self.check_second_and_third_attestation_processing(
            attestation_transition,
            expected_attester_balance,
            rollup,
        )?;

        Ok(())
    }
}

// This tests that the `attester_incentives` module works correctly with a simple value setter module.
// The transactions of the value setter execute correcly and the state transitions are correctly stored and updated.
// This checks that the module correctly processes the attestations, rewards the attesters and updates the maximum attested height.
#[test]
fn test_honest_value_setter_process_attestation() -> Result<(), Infallible> {
    // Build a STF blueprint with the module configurations
    let mut rollup = TestRollup::new();

    let test_handler = AttesterIncentivesTestHandler::honest_attester_test_config();

    // Genesis
    let init_state_root = rollup.genesis(
        test_handler.admin_public_key,
        test_handler.sequencer_params(),
        test_handler.bank_params(),
        test_handler.attester_incentives_params(),
    );

    test_handler.check_initial_attestation_conditions(&mut rollup)?;

    let mut exec_vars =
        test_handler.try_execute_two_value_setter_transactions(init_state_root, &mut rollup);

    let (new_attester_balance, first_attestation_exec_vars) = test_handler
        .try_attest_first_transition(
            init_state_root,
            exec_vars.clone(),
            TEST_DEFAULT_USER_BALANCE - test_handler.attester_stake,
            &mut rollup,
        )?;

    exec_vars.push(first_attestation_exec_vars);

    test_handler.try_attest_second_transition_and_first_attestation(
        exec_vars,
        new_attester_balance,
        &mut rollup,
    )?;

    Ok(())
}
