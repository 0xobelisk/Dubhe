use std::convert::Infallible;

use sov_bank::{IntoPayable, GAS_TOKEN_ID};
use sov_mock_da::MockDaSpec;
use sov_modules_api::macros::config_value;
use sov_modules_api::transaction::PriorityFeeBips;
use sov_modules_api::{Batch, Gas, GasArray, ModuleInfo, Spec};
use sov_modules_stf_blueprint::TxEffect;
use sov_sequencer_registry::BatchSequencerOutcome;
use sov_test_utils::auth::TestAuth;
use sov_test_utils::generators::value_setter::ValueSetterMessages;
use sov_test_utils::runtime::optimistic::TestRuntime;
use sov_test_utils::{
    new_test_blob_from_batch, MessageGenerator, TEST_DEFAULT_GAS_LIMIT, TEST_DEFAULT_USER_BALANCE,
    TEST_DEFAULT_USER_STAKE,
};

use crate::helpers::{AttesterIncentivesParams, BankParams, SequencerParams, TestRollup, S};

const TEST_PRIORITY_FEE: PriorityFeeBips = PriorityFeeBips::from_percentage(10);
const NUM_TXS_PER_BATCH: u64 = 2;

fn check_sequencer_and_registry_balances(
    rollup: &mut TestRollup,
    seq_rollup_addr: &<S as Spec>::Address,
    expected_sequencer_balance: u64,
    expected_registry_balance: u64,
) -> Result<(), Infallible> {
    let mut checkpoint = rollup.new_state_checkpoint();

    let initial_sequencer_balance = rollup
        .bank()
        .get_balance_of(seq_rollup_addr, GAS_TOKEN_ID, &mut checkpoint)?
        .unwrap();

    assert_eq!(initial_sequencer_balance, expected_sequencer_balance);

    // Assert the initial balance of the sequencer registry is equal to the stake amount of the sequencer.
    let seq_registry_balance = rollup
        .bank()
        .get_balance_of(
            rollup.sequencer_registry().id().to_payable(),
            GAS_TOKEN_ID,
            &mut checkpoint,
        )?
        .unwrap();

    assert_eq!(seq_registry_balance, expected_registry_balance);
    Ok(())
}

fn test_sequencer_reward_in_stf(rollup: &mut TestRollup, max_fee: u64) -> Result<(), Infallible> {
    let value_setter_messages = ValueSetterMessages::prepopulated();
    let value_setter = value_setter_messages
        .create_raw_txs::<TestRuntime<S, MockDaSpec>, TestAuth<S, MockDaSpec>>(
            config_value!("CHAIN_ID"),
            TEST_PRIORITY_FEE,
            max_fee,
            None,
        );

    assert_eq!(value_setter.len() as u64, NUM_TXS_PER_BATCH);

    let admin_pub_key = value_setter_messages.messages[0].admin.to_address();

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

    let post_genesis_sequencer_balance = TEST_DEFAULT_USER_BALANCE - TEST_DEFAULT_USER_STAKE;
    let post_genesis_registry_balance = TEST_DEFAULT_USER_STAKE;

    check_sequencer_and_registry_balances(
        rollup,
        &seq_rollup_addr,
        post_genesis_sequencer_balance,
        post_genesis_registry_balance,
    )?;

    let blob = new_test_blob_from_batch(Batch { txs: value_setter }, seq_da_addr.as_ref(), [0; 32]);

    let exec_simulation =
        rollup.execution_simulation(1, init_root_hash, vec![blob.clone()], 0, None);

    assert_eq!(exec_simulation.len(), 1, "The execution simulation failed");
    assert_eq!(
        exec_simulation[0].batch_receipts.len(),
        1,
        "The batch execution failed"
    );

    let batch_receipt = &exec_simulation[0].batch_receipts[0];

    for (i, tx_receipt) in batch_receipt.tx_receipts.iter().enumerate() {
        assert!(
            matches!(tx_receipt.receipt, TxEffect::Successful(..)),
            "The tx receipt {i} was not successful"
        );
    }

    let total_gas_used =
        batch_receipt
            .tx_receipts
            .iter()
            .fold(<S as Spec>::Gas::zero(), |mut acc, tx_receipt| {
                acc.combine(&<S as Spec>::Gas::from_slice(&tx_receipt.gas_used));
                acc
            });

    let expected_reward = TEST_PRIORITY_FEE
        .apply(total_gas_used.value(&rollup.initial_base_fee_per_gas()))
        .expect("Should not overflow");

    match batch_receipt.inner.clone() {
        BatchSequencerOutcome::Rewarded(amount) => {
            assert_eq!(
                Into::<u64>::into(amount),
                expected_reward,
                "The sequencer was not rewarded the correct amount"
            );
        }
        receipt => panic!("The batch execution failed, the sequencer was slashed for {receipt:?}"),
    }

    // The sequencer registry balance should still be equal to the stake amount of the sequencer.
    // The sequencer balance should increase by the expected reward.
    check_sequencer_and_registry_balances(
        rollup,
        &seq_rollup_addr,
        post_genesis_sequencer_balance + expected_reward,
        post_genesis_registry_balance,
    )?;

    Ok(())
}

/// Checks that the sequencer gets rewarded the maximum priority fee if the base fee per gas is low enough.
/// This test checks the extreme case where the difference between the max fee and the base fee is strictly greater than the maximum priority fee.
/// Hence the sequencer should get rewarded the maximum priority fee.
#[test]
fn test_sequencer_rewarded_max_priority_fee() -> Result<(), Infallible> {
    // Build a STF blueprint with the module configurations
    let mut rollup = TestRollup::new();

    // The max fee is the same as the base fee so the sequencer should not get rewarded
    let max_fee = <S as Spec>::Gas::from_slice(&TEST_DEFAULT_GAS_LIMIT)
        .value(&rollup.initial_base_fee_per_gas());

    test_sequencer_reward_in_stf(&mut rollup, max_fee)
}

// Checks the EIP-1559 specification for the maximum priority fee. The sequencer should get rewarded the minimum
// of (the difference between the max fee and the base fee) and (the maximum priority fee). If the base fee is
// very close to the max fee, the sequencer should then get rewarded less than the maximum priority fee.
// This test checks the extreme case where the consumed base fee is the same as the max fee, hence the sequencer doesn't get rewarded at all.
//
// TODO(@theochap): the gas costs are now quite unpredictable, so this test is disabled for now. It will be re-enabled
// once we have a way to simulate transaction execution and compute the gas costs ahead of time.
//
// #[test]
// fn test_sequencer_not_rewarded_max_priority_fee() -> Result<(), Infallible> {
//     // Build a STF blueprint with the module configurations
//     let mut rollup = TestRollup::new();
//     // The max fee is the same as the base fee so the sequencer should not get rewarded
//     let max_fee =
//         <S as Spec>::Gas::from_slice(&GAS_TX_FIXED_COST).value(&rollup.initial_base_fee_per_gas());

//     test_sequencer_reward_in_stf(&mut rollup, max_fee, 0)
// }
