use std::convert::Infallible;
use std::rc::Rc;

use sov_attester_incentives::Role;
use sov_bank::BurnRate;
use sov_mock_zkvm::crypto::private_key::Ed25519PrivateKey;
use sov_mock_zkvm::MockCodeCommitment;
use sov_modules_api::{Batch, CryptoSpec, DaSpec, PrivateKey, RawTx, Spec, StateCheckpoint};
use sov_modules_stf_blueprint::TransactionReceipt;
use sov_state::{Storage, StorageRoot};
use sov_test_utils::auth::TestAuth;
use sov_test_utils::generators::value_setter::ValueSetterMessages;
use sov_test_utils::runtime::optimistic::TestRuntime;
use sov_test_utils::{
    new_test_blob_from_batch, MessageGenerator, TestPrivateKey, TestStorageSpec as StorageSpec,
    TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_STAKE,
};

use crate::helpers::{
    AttesterIncentivesParams, BankParams, Da, ExecutionSimulationVars, SequencerParams, TestRollup,
    S,
};

mod byzantine_behavior;
mod process_attestation;

mod unbond;

const ROLLUP_FINALITY_PERIOD: u64 = 2;

fn get_first_transaction_receipt(env: &ExecutionSimulationVars) -> &TransactionReceipt {
    env.batch_receipts
        .first()
        .expect("Should contain a batch receipt")
        .tx_receipts
        .first()
        .expect("Should contain a transaction receipt")
}

impl TestRollup {
    pub(crate) fn burn_rate(&self) -> BurnRate {
        self.attester_incentives().burn_rate()
    }

    pub(crate) fn increase_and_commit_light_client_attested_height(
        &mut self,
        height: u64,
    ) -> Result<StorageRoot<StorageSpec>, Infallible> {
        let mut state = StateCheckpoint::<S>::new(self.storage());
        let attester_incentives = &self.attester_incentives();

        attester_incentives
            .light_client_finalized_height
            .set(&(height), &mut state)?;

        let (reads_writes, _, _) = state.freeze();

        let storage = self.storage();

        let (new_state_root, change_set) = storage
            .validate_and_materialize(reads_writes, &Default::default())
            .unwrap();

        self.storage_manager().commit(change_set);

        Ok(new_state_root)
    }

    pub(crate) fn get_user_bond(
        &mut self,
        role: Role,
        user_addr: <S as Spec>::Address,
    ) -> Result<u64, Infallible> {
        let mut state = StateCheckpoint::<S>::new(self.storage());

        Ok(match role {
            Role::Attester => self
                .attester_incentives()
                .bonded_attesters
                .get(&user_addr, &mut state)?
                .unwrap_or_default(),
            Role::Challenger => self
                .attester_incentives()
                .bonded_challengers
                .get(&user_addr, &mut state)?
                .unwrap_or_default(),
        })
    }

    pub(crate) fn get_bad_transition_reward(&mut self, height: u64) -> Result<u64, Infallible> {
        let mut state = StateCheckpoint::<S>::new(self.storage());

        Ok(self
            .attester_incentives()
            .bad_transition_pool
            .get(&height, &mut state)?
            .unwrap_or_default())
    }

    pub(crate) fn is_attester_unbonding(
        &mut self,
        user_addr: <S as Spec>::Address,
    ) -> Result<bool, Infallible> {
        let mut state = StateCheckpoint::<S>::new(self.storage());

        Ok(self
            .attester_incentives()
            .unbonding_attesters
            .get(&user_addr, &mut state)?
            .is_some())
    }

    pub fn get_maximum_attested_height(&mut self) -> Result<u64, Infallible> {
        let mut state = StateCheckpoint::<S>::new(self.storage());

        Ok(self
            .attester_incentives()
            .maximum_attested_height
            .get(&mut state)?
            .unwrap_or_default())
    }
}

struct AttesterIncentivesTestHandler {
    pub(crate) admin_public_key: <S as Spec>::Address,
    pub(crate) value_setter: Vec<RawTx>,

    pub(crate) attester_private_key: <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey,
    pub(crate) challenger_private_key: <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey,

    pub(crate) attester_stake: u64,
    pub(crate) challenger_stake: u64,

    pub(crate) attester_balance: u64,
    pub(crate) challenger_balance: u64,

    pub(crate) seq_rollup_addr: <S as Spec>::Address,
    pub(crate) seq_da_addr: <Da as DaSpec>::Address,
}

impl AttesterIncentivesTestHandler {
    pub fn attester_addr(&self) -> <S as Spec>::Address {
        self.attester_private_key.to_address::<_>()
    }

    pub fn sequencer_params(&self) -> SequencerParams<S, Da> {
        SequencerParams {
            rollup_address: self.seq_rollup_addr,
            da_address: self.seq_da_addr,
            stake_amount: TEST_DEFAULT_USER_STAKE,
        }
    }

    pub fn bank_params(&self) -> BankParams {
        BankParams {
            token_name: "TOKEN_TEST".to_string(),
            addresses_and_balances: vec![
                (self.admin_public_key, TEST_DEFAULT_USER_BALANCE),
                (
                    self.attester_private_key
                        .to_address::<<S as Spec>::Address>(),
                    self.attester_balance,
                ),
                (
                    self.challenger_private_key
                        .to_address::<<S as Spec>::Address>(),
                    self.challenger_balance,
                ),
                (self.seq_rollup_addr, TEST_DEFAULT_USER_BALANCE),
            ],
        }
    }

    pub fn attester_incentives_params(&self) -> AttesterIncentivesParams<S> {
        AttesterIncentivesParams {
            initial_attesters: vec![(
                self.attester_private_key
                    .to_address::<<S as Spec>::Address>(),
                self.attester_stake,
            )],
            rollup_finality_period: ROLLUP_FINALITY_PERIOD,
            minimum_attester_bond: TEST_DEFAULT_USER_STAKE,
            minimum_challenger_bond: TEST_DEFAULT_USER_STAKE,
            maximum_attested_height: 0,
            light_client_finalized_height: 0,
            commitment_to_allowed_challenge_method: MockCodeCommitment([0; 32]),
        }
    }

    fn byzantine_test_config() -> AttesterIncentivesTestHandler {
        // Build a STF blueprint with the module configurations
        let value_setter_messages = ValueSetterMessages::prepopulated();
        let sequencer_params = SequencerParams::default();

        AttesterIncentivesTestHandler {
            value_setter: value_setter_messages
                .create_default_raw_txs::<TestRuntime<S, Da>, TestAuth<S, Da>>(),
            admin_public_key: value_setter_messages.messages[0]
                .admin
                .to_address::<<S as Spec>::Address>(),
            attester_private_key: TestPrivateKey::generate(),
            challenger_private_key: TestPrivateKey::generate(),
            attester_stake: TEST_DEFAULT_USER_STAKE,
            challenger_stake: TEST_DEFAULT_USER_STAKE,
            attester_balance: TEST_DEFAULT_USER_BALANCE,
            challenger_balance: TEST_DEFAULT_USER_BALANCE,
            seq_da_addr: sequencer_params.da_address,
            seq_rollup_addr: sequencer_params.rollup_address,
        }
    }

    fn honest_attester_test_config() -> AttesterIncentivesTestHandler {
        let value_setter_messages = ValueSetterMessages::prepopulated();

        let seq_params = SequencerParams::default();

        let value_setter =
            value_setter_messages.create_default_raw_txs::<TestRuntime<S, Da>, TestAuth<S, Da>>();
        let admin_private_key: Rc<Ed25519PrivateKey> =
            value_setter_messages.messages[0].admin.clone();

        AttesterIncentivesTestHandler {
            admin_public_key: admin_private_key.to_address::<<S as Spec>::Address>(),
            value_setter,
            attester_private_key: TestPrivateKey::generate(),
            challenger_private_key: TestPrivateKey::generate(),
            attester_stake: TEST_DEFAULT_USER_STAKE,
            challenger_stake: TEST_DEFAULT_USER_STAKE,
            attester_balance: TEST_DEFAULT_USER_BALANCE,
            challenger_balance: TEST_DEFAULT_USER_BALANCE,
            seq_da_addr: seq_params.da_address,
            seq_rollup_addr: seq_params.rollup_address,
        }
    }

    // Try to execute two value setter transactions in one single slot.
    fn try_execute_two_value_setter_transactions(
        &self,
        genesis_root: StorageRoot<StorageSpec>,
        rollup: &mut TestRollup,
    ) -> Vec<ExecutionSimulationVars> {
        let blob = new_test_blob_from_batch(
            Batch {
                txs: self.value_setter.clone(),
            },
            self.seq_da_addr.as_ref(),
            [2; 32],
        );

        let mut exec_vars =
            rollup.execution_simulation(2, genesis_root, vec![blob], 0, Some(self.attester_addr()));

        assert_eq!(exec_vars.len(), 2, "The execution simulation failed");
        let snd_res = exec_vars.pop().expect("The execution simulation failed");
        let fst_res = exec_vars.pop().expect("The execution simulation failed");

        // Both executions have succeeded
        {
            assert_eq!(fst_res.batch_receipts.len(), 1);
            assert!(get_first_transaction_receipt(&fst_res)
                .receipt
                .is_successful());
            assert_eq!(snd_res.batch_receipts.len(), 1);
            assert!(get_first_transaction_receipt(&snd_res)
                .receipt
                .is_successful());
        }

        vec![fst_res, snd_res]
    }
}
