use sov_mock_da::MockDaSpec;
use sov_modules_api::capabilities::{
    AuthorizationData, AuthorizeSequencerError, SequencerAuthorization,
};
use sov_modules_api::macros::config_value;
use sov_modules_api::runtime::capabilities::RuntimeAuthorization;
use sov_modules_api::transaction::{Credentials, UnsignedTransaction};
use sov_modules_api::{
    Batch, Context, CryptoSpec, DaSpec, EncodeCall, Gas, GasArray, KernelWorkingSet, PrivateKey,
    Spec, StateCheckpoint,
};
use sov_modules_stf_blueprint::TxEffect;
use sov_rollup_interface::crypto::PublicKey;
use sov_test_utils::auth::TestAuth;
use sov_test_utils::generators::value_setter::ValueSetterMessages;
use sov_test_utils::runtime::genesis::User;
use sov_test_utils::runtime::optimistic::{HighLevelOptimisticGenesisConfig, TestRuntime};
use sov_test_utils::runtime::{MessageType, SlotTestCase, TestRunner, TxOutcome, TxTestCase};
use sov_test_utils::{
    generate_optimistic_runtime, new_test_blob_from_batch, MessageGenerator, TestHasher,
    TEST_DEFAULT_USER_BALANCE,
};
use sov_value_setter::{CallMessage, ValueSetter};

use crate::helpers::{AttesterIncentivesParams, BankParams, Da, SequencerParams, TestRollup, S};

impl TestRollup {
    // Check the current kernel height and that the context are correctly built
    pub(crate) fn check_kernel_and_context_updates(
        &mut self,
        expected_height: u64,
        value_setter_messages: &ValueSetterMessages<S>,
        seq_da_addr: <Da as DaSpec>::Address,
        seq_rollup_addr: <S as Spec>::Address,
    ) {
        let mut state_checkpoint = StateCheckpoint::new(self.storage());
        let kernel = self.kernel();
        let kernel_working_set = KernelWorkingSet::from_kernel(kernel, &mut state_checkpoint);

        let height = kernel_working_set.current_slot();

        if height != expected_height {
            panic!(
                "The kernel height {height} is not equal to the expected height {expected_height}."
            );
        }

        let gas_price = &<<S as Spec>::Gas as Gas>::Price::from_slice(&[0; 2]);
        let transaction_scratchpad = state_checkpoint.to_tx_scratchpad();

        let mut pre_exec_ws = match self.stf().runtime().authorize_sequencer(
            &seq_da_addr,
            gas_price,
            transaction_scratchpad,
        ) {
            Ok(pre_exec_ws) => pre_exec_ws,
            Err(AuthorizeSequencerError {
                reason,
                tx_scratchpad: _,
            }) => {
                panic!("Sequencer authorization failed at height {height} for reason: {reason}")
            }
        };

        let admin_priv_key = &value_setter_messages.messages[0].admin;
        let admin_pub_key = value_setter_messages.messages[0].admin.pub_key();
        let admin_addr = admin_priv_key.to_address::<<S as Spec>::Address>();

        let contexts: Vec<Context<S>> = value_setter_messages
            .create_default_messages()
            .into_iter()
            .map(|m| {
                let tx = m.to_tx::<TestRuntime<S, Da>>();
                let pub_key = tx.pub_key().clone();
                let credential_id = pub_key.credential_id::<TestHasher>();
                let default_address = Some((&pub_key).into());

                let auth_data = AuthorizationData {
                    nonce: tx.nonce,
                    credential_id,
                    credentials: Credentials::new(pub_key),
                    default_address,
                };

                self.stf()
                    .runtime()
                    .resolve_context(&auth_data, &seq_da_addr, height, &mut pre_exec_ws)
                    .unwrap()
            })
            .collect();

        for context in contexts {
            assert_eq!(context.sender(), &admin_addr);
            assert_eq!(context.sequencer(), &seq_rollup_addr);
            assert_eq!(context.visible_slot_number(), height);
            assert_eq!(
                context
                    .get_sender_credential::<<<S as Spec>::CryptoSpec as CryptoSpec>::PublicKey>(),
                Some(&admin_pub_key)
            );
        }
    }
}

/// Tests that the execution values of the `StfBlueprint` are correctly set.
/// In particular, these tests check that the `Kernel` gets updated correctly and
/// that the `Context` can be correctly built out of the `Kernel`.

/// Builds and executes a simple rollup and checks that the kernel and the context updates correctly.
#[test]
fn test_stf_internal_updates() {
    // Build a STF blueprint with the module configurations
    let mut rollup = TestRollup::new();

    let value_setter_messages = ValueSetterMessages::prepopulated();
    let value_setter = value_setter_messages
        .create_default_raw_txs::<TestRuntime<S, MockDaSpec>, TestAuth<S, MockDaSpec>>();
    let num_tx_per_slot = value_setter.len();

    let admin_pub_key = value_setter_messages.messages[0]
        .admin
        .to_address::<<S as Spec>::Address>();

    let seq_params = SequencerParams::default();
    let seq_rollup_addr = seq_params.rollup_address;
    let seq_da_addr = seq_params.da_address;
    let bank_params = BankParams::with_addresses_and_balances(vec![
        (seq_params.rollup_address, TEST_DEFAULT_USER_BALANCE),
        (admin_pub_key, TEST_DEFAULT_USER_BALANCE),
    ]);
    let attester_params = AttesterIncentivesParams::default();

    // Genesis
    let init_root_hash = rollup.genesis(admin_pub_key, seq_params, bank_params, attester_params);

    rollup.check_kernel_and_context_updates(
        0,
        &value_setter_messages,
        seq_da_addr,
        seq_rollup_addr,
    );

    let blob = new_test_blob_from_batch(Batch { txs: value_setter }, seq_da_addr.as_ref(), [0; 32]);

    let exec_simulation =
        rollup.execution_simulation(5, init_root_hash, vec![blob.clone()], 0, None);

    assert_eq!(exec_simulation.len(), 5, "The execution simulation failed");

    for (i, exec_i) in exec_simulation.into_iter().enumerate() {
        assert_eq!(exec_i.batch_receipts.len(), 1);
        assert_eq!(
            exec_i.batch_receipts[0].tx_receipts.len(),
            num_tx_per_slot,
            "Not all the transactions have been executed for slot {i}"
        );
        for (j, tx) in exec_i.batch_receipts[0]
            .tx_receipts
            .clone()
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                tx.receipt,
                TxEffect::Successful(()),
                "The transaction {i} failed in slot at height {j}"
            );
        }
    }

    rollup.check_kernel_and_context_updates(
        5,
        &value_setter_messages,
        seq_da_addr,
        seq_rollup_addr,
    );
}

#[test]
fn test_enforces_chain_id() {
    generate_optimistic_runtime!(IntegTestRuntime <= value_setter: ValueSetter<S>);

    // Run an indivdual transaction with the given chain id on a fresh chain. Assert that the outcome is as expected.
    fn test_tx_with_chain_id(
        chain_id: u64,
        expected_outcome: TxOutcome<IntegTestRuntime<S, MockDaSpec>>,
    ) {
        let mut genesis_config = HighLevelOptimisticGenesisConfig::generate();
        genesis_config
            .additional_accounts
            .push(User::<S>::generate(TEST_DEFAULT_USER_BALANCE));

        let admin_account = genesis_config.additional_accounts[0].clone();

        let genesis = GenesisConfig::from_minimal_config(
            genesis_config.clone().into(),
            sov_value_setter::ValueSetterConfig {
                admin: admin_account.address(),
            },
        );

        let encoded_message =
            <IntegTestRuntime<S, MockDaSpec> as EncodeCall<ValueSetter<S>>>::encode_call(
                CallMessage::SetValue(8),
            );

        let utx =
            UnsignedTransaction::new(encoded_message, chain_id, 100.into(), 100_000_000, 0, None);
        TestRunner::run_test(
            genesis.into_genesis_params(),
            vec![SlotTestCase::from_txs(vec![TxTestCase {
                outcome: expected_outcome,
                message: MessageType::<ValueSetter<S>, S>::pre_signed(
                    utx,
                    admin_account.private_key(),
                ),
            }])],
            Default::default(),
        );
    }

    let real_chain_id = config_value!("CHAIN_ID");
    let fake_chain_id = real_chain_id + 1;

    test_tx_with_chain_id(real_chain_id, TxOutcome::applied());
    test_tx_with_chain_id(fake_chain_id, TxOutcome::Reverted);
}
