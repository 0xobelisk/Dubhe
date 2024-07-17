use std::convert::Infallible;

use sov_bank::{Bank, IntoPayable, ReserveGasError, ReserveGasErrorReason, GAS_TOKEN_ID};
use sov_modules_api::transaction::{AuthenticatedTransactionData, PriorityFeeBips, Transaction};
use sov_modules_api::{
    Address, Gas, GasArray, GasMeter, GasUnit, ModuleInfo, Spec, UnlimitedGasMeter, WorkingSet,
};
use sov_test_utils::{generate_empty_tx, simple_bank_setup, TEST_DEFAULT_USER_BALANCE};

type S = sov_test_utils::TestSpec;

/// Helper struct that gets instantiated following the `reserve_gas_helper` method. Contains useful test parameters.
struct CapabilityTestParams {
    pub bank: Bank<S>,
    pub transaction: Transaction<S>,
    pub working_set: WorkingSet<S>,
    pub sender_address: <S as Spec>::Address,
}

/// Helper function that creates a simple bank setup (one account with `initial_balance`), generates a transaction
/// with the given gas parameters, reserves some gas, checks the resulting gas meter, and returns useful test parameters.
fn reserve_gas_helper(
    initial_balance: u64,
    max_priority_fee_bips: PriorityFeeBips,
    gas_limit: Option<GasUnit<2>>,
    gas_price: &<<S as Spec>::Gas as Gas>::Price,
    // The gas consumed by pre-execution checks
    gas_for_pre_execution_checks: &<S as Spec>::Gas,
) -> CapabilityTestParams {
    let (sender_address, bank, checkpoint) = simple_bank_setup(initial_balance);

    let transaction: Transaction<S> =
        generate_empty_tx(max_priority_fee_bips, initial_balance, gas_limit.clone());

    let transaction_scratchpad = checkpoint.to_tx_scratchpad();

    let mut pre_execution_ws = transaction_scratchpad.pre_exec_ws_unmetered_with_price(gas_price);
    pre_execution_ws
        .charge_gas(gas_for_pre_execution_checks)
        .unwrap();

    // We try to reserve gas, this should succeed because we have enough balance.
    let working_set = match bank.reserve_gas(
        &transaction.clone().into(),
        &sender_address,
        pre_execution_ws,
    ) {
        Ok(ws) => ws,
        Err(ReserveGasError::<S, UnlimitedGasMeter<<S as Spec>::Gas>> {
            pre_exec_working_set: _,
            reason,
        }) => {
            panic!("Unable to reserve gas for the transaction: {:?}", reason);
        }
    };

    let gas_used = working_set.gas_used();

    assert!(
        gas_used >=
        gas_for_pre_execution_checks,
        "The gas used {gas_used} should be at least equal to the gas for pre-execution checks {gas_for_pre_execution_checks} (more gas may be used for state accesses)"
    );

    CapabilityTestParams {
        bank,
        transaction,
        working_set,
        sender_address,
    }
}

/// Tests the happy path of the `reserve_gas` method. We try to reserve gas, then consume it and refund it.
/// The priority fee is zero.
/// We use half the price for pre-execution checks, the rest for the transaction.
#[test]
fn test_honest_reserve_gas_capability_without_priority_fee() -> Result<(), Infallible> {
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    let mut params = reserve_gas_helper(
        initial_balance,
        PriorityFeeBips::ZERO,
        Some(GasUnit::from_slice(&[initial_balance / 2; 2])),
        &<<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]),
        &<S as Spec>::Gas::from_slice(&[initial_balance / 4; 2]),
    );

    // Let's consume all the gas, this should succeed because there is enough gas left in the meter.
    let remaining_gas = params.working_set.remaining_funds();
    params
        .working_set
        .charge_gas(&GasUnit::from_slice(&[remaining_gas / 2; 2]))
        .expect("The charge gas operation should not fail");

    let _auth_tx: AuthenticatedTransactionData<S> = params.transaction.into();

    let (mut tx_scratchpad, tx_consumption, _) = params.working_set.finalize();

    // We try to refund the gas, this should never fail. The gas is already consumed so the sender balance should be zero.
    params.bank.allocate_consumed_gas(
        &params.bank.id().to_payable(),
        &params.bank.id().to_payable(),
        &tx_consumption,
        &mut tx_scratchpad,
    );

    params
        .bank
        .refund_remaining_gas(&params.sender_address, &tx_consumption, &mut tx_scratchpad);

    let mut checkpoint = tx_scratchpad.commit();

    assert_eq!(
        params
            .bank
            .get_balance_of(&params.sender_address, GAS_TOKEN_ID, &mut checkpoint)?
            .expect("The sender balance should exist"),
        0
    );

    Ok(())
}

/// Tests the happy path of the `reserve_gas` method. We try to reserve gas, then consume it and refund it.
/// The priority fee is non zero but the difference between the max fee and the maximum gas value is zero
/// hence the priority fee is not charged.
/// We use half the price for pre-execution checks, the rest for the transaction.
#[test]
fn test_honest_reserve_gas_capability_does_not_charge_priority_fee() -> Result<(), Infallible> {
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    let mut params = reserve_gas_helper(
        initial_balance,
        PriorityFeeBips::from_percentage(10),
        Some(GasUnit::from_slice(&[initial_balance / 2; 2])),
        &<<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]),
        &<S as Spec>::Gas::from_slice(&[initial_balance / 4; 2]),
    );

    // Let's consume all the gas, this should succeed because there is enough gas left in the meter.
    let remaining_gas = params.working_set.remaining_funds();
    params
        .working_set
        .charge_gas(&GasUnit::from_slice(&[remaining_gas / 2; 2]))
        .expect("The charge gas operation should not fail");

    let (mut tx_scratchpad, tx_consumption, _) = params.working_set.finalize();

    // We try to refund the gas, this should never fail. The gas is already consumed so the sender balance should be zero.
    params.bank.allocate_consumed_gas(
        &params.bank.id().to_payable(),
        &params.bank.id().to_payable(),
        &tx_consumption,
        &mut tx_scratchpad,
    );

    params
        .bank
        .refund_remaining_gas(&params.sender_address, &tx_consumption, &mut tx_scratchpad);

    let mut checkpoint = tx_scratchpad.commit();

    let _auth_tx: AuthenticatedTransactionData<S> = params.transaction.into();

    assert_eq!(
        params
            .bank
            .get_balance_of(&params.sender_address, GAS_TOKEN_ID, &mut checkpoint)?
            .expect("The sender balance should exist"),
        0
    );

    Ok(())
}

/// Tests the happy path of the `reserve_gas` method. We try to reserve gas, then consume it and refund it.
/// The priority fee is non zero and is charged as part of the transaction.
#[test]
fn test_honest_reserve_gas_capability_with_priority_fee() -> anyhow::Result<()> {
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    let max_priority_fee_bips = PriorityFeeBips::from_percentage(10);

    let mut params = reserve_gas_helper(
        initial_balance,
        PriorityFeeBips::from_percentage(10),
        Some(GasUnit::from_slice(&[initial_balance / 4; 2])),
        &<<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]),
        &<<S as Spec>::Gas>::zero(),
    );

    let gas_to_charge = GasUnit::from_slice(&[initial_balance / 20; 2]);

    // We try to charge for some of the gas
    params
        .working_set
        .charge_gas(&gas_to_charge)
        .expect("The charge gas operation should not fail");

    let (mut tx_scratchpad, tx_consumption, _) = params.working_set.finalize();

    // We try to refund the gas, this should never fail. The gas is already consumed so the sender balance should be zero.
    params.bank.allocate_consumed_gas(
        &params.bank.id().to_payable(),
        &params.bank.id().to_payable(),
        &tx_consumption,
        &mut tx_scratchpad,
    );

    params
        .bank
        .refund_remaining_gas(&params.sender_address, &tx_consumption, &mut tx_scratchpad);

    let mut checkpoint = tx_scratchpad.commit();

    let _auth_tx: AuthenticatedTransactionData<S> = params.transaction.into();

    // The sender should have been refunded:
    // initial_balance - base_fee * (1 + max_priority_fee_percentage)
    let base_fee_value = tx_consumption.base_fee_value();

    let refund_amount = initial_balance
        - base_fee_value
        - max_priority_fee_bips
            .apply(base_fee_value)
            .expect("This should not overflow");

    assert_eq!(
        params
            .bank
            .get_balance_of(&params.sender_address, GAS_TOKEN_ID, &mut checkpoint)?
            .expect("The sender balance should exist"),
        refund_amount
    );

    // We now test that the bank has locked the remaining amount
    assert_eq!(
        params
            .bank
            .get_balance_of(params.bank.id().to_payable(), GAS_TOKEN_ID, &mut checkpoint)?
            .expect("The bank balance should exist"),
        initial_balance - refund_amount
    );

    Ok(())
}

/// Tests that the `reserve_gas` method fails if the sender does not have a bank account for the gas token
#[test]
fn test_reserve_gas_no_account() {
    let (_, bank, checkpoint) = simple_bank_setup(0);

    let transaction_scratchpad = checkpoint.to_tx_scratchpad();

    let pre_exec_ws = transaction_scratchpad.pre_exec_ws_unmetered();

    // This transaction has a maximum fee of twice the initial balance.
    let transaction: Transaction<S> = generate_empty_tx(PriorityFeeBips::ZERO, 0, None);

    let payer = Address::new([0u8; 32]);

    // We try to reserve gas, this should fail because we have not enough balance.
    let reserve_gas_result = match bank.reserve_gas(&transaction.into(), &payer, pre_exec_ws) {
        Ok(_) => panic!("The reserve gas operation should fail"),
        Err(ReserveGasError::<S, UnlimitedGasMeter<<S as Spec>::Gas>> {
            pre_exec_working_set: _,
            reason,
        }) => reason,
    };

    assert_eq!(
        reserve_gas_result,
        ReserveGasErrorReason::AccountDoesNotExist {
            account: payer.to_string(),
        }
    );
}

/// Tests that the `reserve_gas` method fails if the sender balance is not high enough to pay for the gas.
#[test]
fn test_reserve_gas_not_enough_balance() {
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    let (sender_address, bank, checkpoint) = simple_bank_setup(initial_balance);

    let gas_price = <<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]);

    let transaction_scratchpad = checkpoint.to_tx_scratchpad();

    let pre_exec_ws = transaction_scratchpad.pre_exec_ws_unmetered_with_price(&gas_price);

    // This transaction has a maximum fee of twice the initial balance.
    let transaction: Transaction<S> =
        generate_empty_tx(PriorityFeeBips::ZERO, 2 * initial_balance, None);

    // We try to reserve gas, this should fail because we have not enough balance.
    let reserve_gas_result =
        match bank.reserve_gas(&transaction.into(), &sender_address, pre_exec_ws) {
            Ok(_) => panic!("The reserve gas operation should fail"),
            Err(ReserveGasError {
                pre_exec_working_set: _,
                reason,
            }) => reason,
        };

    assert_eq!(
        reserve_gas_result,
        ReserveGasErrorReason::InsufficientBalanceToReserveGas
    );
}

/// Tests that the `reserve_gas` method fails if the current gas price is too high to cover the maximum fee for the transaction.
/// This check is only performed if the `gas_limit` is set.
#[test]
fn test_reserve_gas_price_too_high() {
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    let (sender_address, bank, checkpoint) = simple_bank_setup(initial_balance);

    // This transaction has gas limit set to [50; 2], which means the associated gas price is [1; 2].
    let transaction: Transaction<S> = generate_empty_tx(
        PriorityFeeBips::ZERO,
        initial_balance,
        Some(GasUnit::from_slice(&[initial_balance / 2; 2])),
    );

    // The gas price is [2; 2] which is higher than the one associated with the gas limit.
    let gas_price = <<S as Spec>::Gas as Gas>::Price::from_slice(&[2; 2]);

    let transaction_scratchpad = checkpoint.to_tx_scratchpad();

    let pre_exec_ws = transaction_scratchpad.pre_exec_ws_unmetered_with_price(&gas_price);

    // We try to reserve gas, this should fail because the gas price is too high.
    let reserve_gas_result =
        match bank.reserve_gas(&transaction.into(), &sender_address, pre_exec_ws) {
            Ok(_) => panic!("The reserve gas operation should fail"),
            Err(ReserveGasError {
                pre_exec_working_set: _,
                reason,
            }) => reason,
        };

    assert_eq!(
        reserve_gas_result,
        ReserveGasErrorReason::CurrentGasPriceTooHigh
    );
}

/// Tests that the `reserve_gas` method does not overflow or panic if the total gas amount to reserve is `u64::MAX`.
#[test]
fn test_reserve_gas_should_not_overflow_or_panic_zero_priority() -> anyhow::Result<()> {
    let initial_balance = u64::MAX;

    let params = reserve_gas_helper(
        initial_balance,
        PriorityFeeBips::from_percentage(0),
        None,
        &<<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]),
        &<S as Spec>::Gas::zero(),
    );

    let (mut tx_scratchpad, tx_consumption, _) = params.working_set.finalize();

    // We try to refund the gas, this should never fail. The gas is already consumed so the sender balance should be zero.
    params.bank.allocate_consumed_gas(
        &params.bank.id().to_payable(),
        &params.bank.id().to_payable(),
        &tx_consumption,
        &mut tx_scratchpad,
    );

    params
        .bank
        .refund_remaining_gas(&params.sender_address, &tx_consumption, &mut tx_scratchpad);

    let mut checkpoint = tx_scratchpad.commit();

    assert_eq!(
        params
            .bank
            .get_balance_of(&params.sender_address, GAS_TOKEN_ID, &mut checkpoint)?
            .expect("The sender balance should exist"),
        initial_balance - tx_consumption.total_consumption()
    );

    Ok(())
}

/// Tests that the `reserve_gas` method does not overflow or panic if the total gas amount to reserve is `u64::MAX` and the priority fee is not zero.
#[test]
fn test_reserve_gas_should_not_overflow_or_panic_non_zero_priority() -> anyhow::Result<()> {
    let initial_balance = u64::MAX;
    let params = reserve_gas_helper(
        initial_balance,
        PriorityFeeBips::from_percentage(10),
        None,
        &<<S as Spec>::Gas as Gas>::Price::from_slice(&[1; 2]),
        &<S as Spec>::Gas::zero(),
    );

    let (mut tx_scratchpad, tx_consumption, _) = params.working_set.finalize();

    // We try to refund the gas, this should never fail. The gas is already consumed so the sender balance should be zero.
    params.bank.allocate_consumed_gas(
        &params.bank.id().to_payable(),
        &params.bank.id().to_payable(),
        &tx_consumption,
        &mut tx_scratchpad,
    );

    params
        .bank
        .refund_remaining_gas(&params.sender_address, &tx_consumption, &mut tx_scratchpad);

    let mut checkpoint = tx_scratchpad.commit();

    assert_eq!(
        params
            .bank
            .get_balance_of(&params.sender_address, GAS_TOKEN_ID, &mut checkpoint)?
            .expect("The sender balance should exist"),
        initial_balance - tx_consumption.total_consumption()
    );

    Ok(())
}
