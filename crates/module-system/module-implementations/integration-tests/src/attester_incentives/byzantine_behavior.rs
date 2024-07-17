use std::convert::Infallible;

use sov_attester_incentives::{CallMessage, Role, WrappedAttestation};
use sov_bank::GAS_TOKEN_ID;
use sov_mock_da::{MockAddress, MockValidityCond};
use sov_mock_zkvm::MockZkvm;
use sov_modules_api::optimistic::Attestation;
use sov_modules_api::{Batch, Gas, GasArray, Spec, StateCheckpoint, StateTransitionPublicData};
use sov_modules_stf_blueprint::TxEffect;
use sov_state::jmt::RootHash;
use sov_state::StorageRoot;
use sov_test_utils::auth::TestAuth;
use sov_test_utils::generators::attester_incentive::AttesterIncentivesMessageGenerator;
use sov_test_utils::runtime::optimistic::TestRuntime;
use sov_test_utils::{new_test_blob_from_batch, MessageGenerator, TestStorageSpec as Storage};

use super::AttesterIncentivesTestHandler;
use crate::attester_incentives::get_first_transaction_receipt;
use crate::helpers::{Da, ExecutionSimulationVars, TestRollup, S};

impl AttesterIncentivesTestHandler {
    fn check_attester_bonded(&self, rollup: &mut TestRollup) -> Result<(), Infallible> {
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

    // Attest only one transition
    // Let's try to produce a faulty attestation for the last transition
    fn try_produce_faulty_attestation(
        &self,
        init_state_root: StorageRoot<Storage>,
        exec_result: Vec<ExecutionSimulationVars>,
        rollup: &mut TestRollup,
    ) -> Result<Vec<StorageRoot<Storage>>, Infallible> {
        let ExecutionSimulationVars {
            state_root: fst_state_root,
            state_proof: first_state_proof,
            batch_receipts: _first_batch_receipts,
            ..
        } = exec_result[0].clone();

        // We produce a fake attestation that has the wrong post state root
        let fake_attestation = Attestation {
            initial_state_root: init_state_root,
            slot_hash: [10; 32].into(),
            post_state_root: StorageRoot::new(RootHash([0; 32]), RootHash([0; 32])),
            proof_of_bond: sov_modules_api::optimistic::ProofOfBond {
                claimed_transition_num: 1,
                proof: first_state_proof.unwrap(),
            },
        };

        let txs = AttesterIncentivesMessageGenerator::from(vec![(
            self.attester_private_key.clone(),
            CallMessage::ProcessAttestation::<S, Da>(WrappedAttestation::from(fake_attestation)),
        )])
        .create_default_raw_txs::<TestRuntime<S, Da>, TestAuth<S, Da>>();

        let fake_attestation_blob =
            new_test_blob_from_batch(Batch { txs }, self.seq_da_addr.as_ref(), [2; 32]);

        let exec_vars =
            rollup.execution_simulation(1, fst_state_root, vec![fake_attestation_blob], 1, None);

        let attestation_res = exec_vars
            .first()
            .expect("The attestation execution simulation failed");

        let snd_state_root = attestation_res.state_root;

        // Let's check that the attester was slashed
        {
            // The transaction was successful (we need to gracefully exit to be able to update the state)
            assert_eq!(attestation_res.batch_receipts.len(), 1);

            assert!(get_first_transaction_receipt(attestation_res)
                .receipt
                .is_successful(),);

            // The attester is slashed
            assert_eq!(rollup.get_bad_transition_reward(1)?, self.attester_stake);

            assert_eq!(
                rollup.get_user_bond(
                    Role::Attester,
                    self.attester_private_key
                        .to_address::<<S as Spec>::Address>()
                )?,
                0
            );
        }

        Ok(vec![fst_state_root, snd_state_root])
    }

    fn try_challenge_faulty_attestation(
        &self,
        init_state_root: StorageRoot<Storage>,
        transition_roots: Vec<StorageRoot<Storage>>,
        rollup: &mut TestRollup,
    ) -> Result<(), Infallible> {
        let first_state_root = transition_roots[0];
        let snd_state_root = transition_roots[1];

        // A challenger can now claim the stake from the bad attestation
        // We build the challenge
        let transition = StateTransitionPublicData::<MockAddress, Da, _> {
            initial_state_root: init_state_root,
            slot_hash: [10; 32].into(),
            final_state_root: first_state_root,
            validity_condition: MockValidityCond { is_valid: true },
            prover_address: Default::default(),
        };

        let proof = MockZkvm::create_serialized_proof(true, transition);

        let txs = AttesterIncentivesMessageGenerator::from(vec![
            (
                self.challenger_private_key.clone(),
                CallMessage::BondChallenger::<S, Da>(self.challenger_stake),
            ),
            (
                self.challenger_private_key.clone(),
                CallMessage::ProcessChallenge(proof, 1),
            ),
        ])
        .create_default_raw_txs::<TestRuntime<S, Da>, TestAuth<S, Da>>();

        // The challenger has to bond first, then he can send the attestation.
        let challenger_bond_blob =
            new_test_blob_from_batch(Batch { txs }, self.seq_da_addr.as_ref(), [3; 32]);

        let exec_vars =
            rollup.execution_simulation(1, snd_state_root, vec![challenger_bond_blob], 3, None);

        let challenge_tx = exec_vars
            .first()
            .expect("The challenge execution simulation failed");

        // The challenger has successfully bonded and challenged the attester
        {
            assert_eq!(challenge_tx.batch_receipts.len(), 1);
            let batch_receipt = challenge_tx.batch_receipts.first().unwrap();

            let mut tx_receipts = batch_receipt.tx_receipts.clone();
            assert_eq!(tx_receipts.len(), 2);

            let total_gas_consumed =
                tx_receipts
                    .iter()
                    .fold(<S as Spec>::Gas::zero(), |mut acc, receipt| {
                        acc.combine(&<S as Spec>::Gas::from_slice(&receipt.gas_used));
                        acc
                    });

            let snd_tx_receipt = tx_receipts.pop().unwrap();
            let fst_tx_receipt = tx_receipts.pop().unwrap();
            assert_eq!(fst_tx_receipt.receipt, TxEffect::Successful(()));
            assert_eq!(snd_tx_receipt.receipt, TxEffect::Successful(()));

            let mut state = StateCheckpoint::<S>::new(rollup.storage());

            // The challenger has bonded
            assert_eq!(
                rollup.get_user_bond(
                    Role::Challenger,
                    self.challenger_private_key
                        .to_address::<<S as Spec>::Address>()
                )?,
                self.challenger_stake
            );

            // The transition has been removed from the bad transition pool
            assert_eq!(rollup.get_bad_transition_reward(1)?, 0);

            // The challenger has been rewarded half of the pool's reward (to avoid a DoS attack)
            let burn_rate = rollup.burn_rate();
            // The challenger has sent 2 transactions, so the gas consumed is 2x the gas per transaction
            // The first transaction is for bonding, the second is for challenging
            let gas_price =
                &<<S as Spec>::Gas as Gas>::Price::from_slice(batch_receipt.gas_price.as_slice());

            let gas_consumed = total_gas_consumed.value(gas_price);
            assert_eq!(
                rollup.bank().get_balance_of(
                    &self
                        .challenger_private_key
                        .to_address::<<S as Spec>::Address>(),
                    GAS_TOKEN_ID,
                    &mut state
                )?,
                Some(
                    self.challenger_balance - self.challenger_stake - gas_consumed
                        + burn_rate.apply(self.attester_stake)
                )
            );

            Ok(())
        }
    }
}

// This test checks that the `attester_incentives` module works correctly with a value setter module
// for a byzantine attester.
#[test]
fn test_byzantine_value_setter_process_attestation() -> Result<(), Infallible> {
    // Build a STF blueprint with the module configurations
    let mut rollup = TestRollup::new();

    let test_handler = AttesterIncentivesTestHandler::byzantine_test_config();

    // Genesis
    let init_state_root = rollup.genesis(
        test_handler.admin_public_key,
        test_handler.sequencer_params(),
        test_handler.bank_params(),
        test_handler.attester_incentives_params(),
    );

    // Check that the attester is bonded
    test_handler.check_attester_bonded(&mut rollup)?;

    // Tries to execute two value setter transactions in a single block
    let exec_result =
        test_handler.try_execute_two_value_setter_transactions(init_state_root, &mut rollup);

    // Tries to produce a faulty attestation
    let transition_roots =
        test_handler.try_produce_faulty_attestation(init_state_root, exec_result, &mut rollup)?;

    // Tries to challenge the faulty attestation produced above
    test_handler.try_challenge_faulty_attestation(
        init_state_root,
        transition_roots,
        &mut rollup,
    )?;

    Ok(())
}
