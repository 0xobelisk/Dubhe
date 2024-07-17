use std::convert::Infallible;

use sov_bank::{IntoPayable, Payable, ReserveGasError};
use sov_modules_api::hooks::ApplyBatchHooks;
use sov_modules_api::transaction::{PriorityFeeBips, SequencerReward};
use sov_modules_api::{
    Batch, BatchWithId, Gas, GasArray, GasMeter, GasUnit, ModuleInfo, RawTx, Spec,
};
use sov_test_utils::{generate_empty_tx, TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_STAKE};

use super::helpers::{TestSequencer, S};
use crate::BatchSequencerOutcome;

/// Tests that the sequencer gets correctly rewarded when it processes a batch and:
/// - the `GasEnforcer` capability is correctly used (hence the module has enough funds to pay for the reward)
/// - the `end_batch_hook` is called with a `SequencerOutcome::Rewarded` result
#[test]
fn test_reward_sequencer() -> Result<(), Infallible> {
    // Genesis initialization.
    // We need to pass the large balance to make sure we have enough funds to pay for the tip and the sequencer registration
    let (sequencer_test, mut state) =
        TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;
    let balance_after_genesis = sequencer_test.query_sequencer_balance(&mut state)?.unwrap();
    let registry_balance_after_genesis = sequencer_test
        .query_balance(sequencer_test.registry.id().to_payable(), &mut state)?
        .unwrap();

    let seq_address = &sequencer_test.sequencer_config.seq_rollup_address;
    let seq_da_address = sequencer_test.sequencer_config.seq_da_address;
    let seq_address_as_token_holder = seq_address.as_token_holder();

    let gas_price = <<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]);

    let tx = generate_empty_tx(
        PriorityFeeBips::from_percentage(10),
        balance_after_genesis,
        None,
    );

    let txs = vec![RawTx {
        data: borsh::to_vec(&tx).unwrap(),
    }];

    // Execute the begin batch hook
    let test_batch = BatchWithId {
        batch: Batch { txs },
        id: [0u8; 32],
    };

    sequencer_test
        .registry
        .begin_batch_hook(&test_batch, &seq_da_address, &mut state)
        .expect("The begin batch hook should succeed");

    let transaction_scratchpad = state.to_tx_scratchpad();

    let pre_exec_ws = sequencer_test
        .registry
        .authorize_sequencer(&seq_da_address, &gas_price, transaction_scratchpad)
        .expect("Impossible to authorize sequencer");

    let tx = tx.into();
    // Reserves some gas for the bank
    let mut working_set = match sequencer_test
        .bank
        .reserve_gas(&tx, seq_address, pre_exec_ws)
    {
        Ok(ws) => ws,
        Err(ReserveGasError {
            pre_exec_working_set: _,
            reason,
        }) => {
            panic!("Unable to reserve gas for the transaction: {:?}", reason);
        }
    };

    // Charges the gas
    working_set
        .charge_gas(&GasUnit::from_slice(&[balance_after_genesis / 4; 2]))
        .expect("The charge gas operation should not fail");

    let (mut tx_scratchpad, tx_consumption, _) = working_set.finalize();

    // We refund the base tip to the sequencer account and send the tip to the registry
    sequencer_test.bank.allocate_consumed_gas(
        &seq_address_as_token_holder,
        &sequencer_test.registry.id().to_payable(),
        &tx_consumption,
        &mut tx_scratchpad,
    );

    sequencer_test
        .bank
        .refund_remaining_gas(seq_address, &tx_consumption, &mut tx_scratchpad);

    let mut checkpoint = tx_scratchpad.commit();

    let registry_balance_after_refund = sequencer_test
        .query_balance(sequencer_test.registry.id().to_payable(), &mut checkpoint)?
        .unwrap();

    assert_ne!(
        registry_balance_after_genesis, registry_balance_after_refund,
        "The tip has not been refunded to the sequencer registry"
    );

    // We refund the tip to the sequencer account in the end batch hook
    sequencer_test.registry.end_batch_hook(
        BatchSequencerOutcome::Rewarded(SequencerReward(
            registry_balance_after_refund - registry_balance_after_genesis,
        )),
        &seq_da_address,
        &mut checkpoint,
    );

    // The sequencer balance should be the same as the initial balance after the refunds
    assert_eq!(
        sequencer_test
            .query_sequencer_balance(&mut checkpoint)?
            .unwrap(),
        balance_after_genesis
    );

    Ok(())
}

/// Tests that the sequencer gets correctly penalized when it incorrectly processes a batch
#[test]
fn test_penalize_sequencer() -> Result<(), Infallible> {
    // Genesis initialization.
    let (sequencer_test, state) = TestSequencer::initialize_test(TEST_DEFAULT_USER_BALANCE, false)?;
    let seq_da_address = sequencer_test.sequencer_config.seq_da_address;

    let gas_price = &<<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]);
    let transaction_scratchpad = state.to_tx_scratchpad();

    let mut pre_exec_ws = sequencer_test
        .registry
        .authorize_sequencer(&seq_da_address, gas_price, transaction_scratchpad)
        .expect("The sequencer should be registered and have enough staked amount");

    pre_exec_ws
        .charge_gas(&<S as Spec>::Gas::from_slice(
            &[TEST_DEFAULT_USER_STAKE / 2; 2],
        ))
        .unwrap();

    // We penalize the sequencer by removing all its stake
    let res = sequencer_test
        .registry
        .penalize_sequencer(&seq_da_address, "no reason", pre_exec_ws);

    let mut state_checkpoint = res.commit();

    // The sequencer stake should be zero
    assert_eq!(
        sequencer_test
            .query_sender_balance(&seq_da_address, &mut state_checkpoint)?
            .unwrap(),
        0
    );

    Ok(())
}
