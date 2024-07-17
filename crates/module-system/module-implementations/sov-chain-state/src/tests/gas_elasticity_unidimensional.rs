use sov_mock_da::MockDaSpec;
use sov_modules_api::{GasArray, GasPrice, GasUnit};
use sov_test_utils::TestSpec;

use crate::{BlockGasInfo, ChainState};

/// These tests ensure that the unidimensional base fee per gas update function has the correct behavior
fn assert_correct_gas_update(
    prev_block_gas_info: &BlockGasInfo<GasUnit<1>>,
    expected_base_fee_per_gas: &GasPrice<1>,
    assert_error_message: &str,
) {
    let computed_base_fee_per_gas =
        ChainState::<TestSpec, MockDaSpec>::compute_base_fee_per_gas_unidimensional(
            prev_block_gas_info,
        );
    assert_eq!(
        *expected_base_fee_per_gas,
        computed_base_fee_per_gas,
        "The result of `compute_base_fee_per_gas_unidimensional` does not match the expected value. 
        Expected: {:?}, actual: {:?}, test message: {}",
        expected_base_fee_per_gas,
        computed_base_fee_per_gas,
        assert_error_message
    );
}

/// The base fee per gas should remain to zero if the gas used is below the target
#[test]
fn test_zero_base_fee_gas_below_target() {
    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::ZEROED,
            gas_used: GasUnit::ZEROED,
            base_fee_per_gas: GasPrice::ZEROED,
        },
        &GasPrice::ZEROED,
        "When the base fee per gas is zero, it should remain to zero if the gas used is below the target",
    );
}

/// The base fee per gas should increase from zero to 1 if the gas used is above the target
#[test]
fn test_zero_base_fee_gas_above_target() {
    const GAS_LIMIT: u64 = 100;
    let gas_target: u64 =
        ChainState::<TestSpec, MockDaSpec>::ELASTICITY_MULTIPLIER.apply_div(GAS_LIMIT);

    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::<1>::from(GAS_LIMIT),
            gas_used: GasUnit::<1>::from(gas_target + 1),
            base_fee_per_gas: GasPrice::ZEROED,
        },
        &GasPrice::<1>::from(1),
        "When the base fee per gas is zero, it should be equal to one if the gas used is above the target",
    );
}

/// The base fee per gas should not change if the gas used is the same as the target
#[test]
fn test_base_fee_target_reached() {
    const INITIAL_BASE_FEE_PER_GAS: u64 = 10000;
    const GAS_LIMIT: u64 = 100;
    let gas_target: u64 =
        ChainState::<TestSpec, MockDaSpec>::ELASTICITY_MULTIPLIER.apply_div(GAS_LIMIT);

    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::<1>::from(GAS_LIMIT),
            gas_used: GasUnit::<1>::from(gas_target),
            base_fee_per_gas: GasPrice::<1>::from(INITIAL_BASE_FEE_PER_GAS),
        },
        &GasPrice::<1>::from(INITIAL_BASE_FEE_PER_GAS),
        "When the gas target is met, the base fee per gas shouldn't change",
    );
}

/// The base fee per gas should increase by `base_fee_per_gas * 1/BASE_FEE_MAX_CHANGE_DENOMINATOR` if the gas used is twice as much as the target
#[test]
fn test_base_fee_increases_if_gas_used_is_twice_target() {
    const INITIAL_BASE_FEE_PER_GAS: u64 = 100;

    let expected_base_fee_per_gas: u64 = INITIAL_BASE_FEE_PER_GAS
        + ChainState::<TestSpec, MockDaSpec>::BASE_FEE_MAX_CHANGE_DENOMINATOR
            .apply_div(INITIAL_BASE_FEE_PER_GAS);

    let gas_limit: u64 = 100;
    let gas_target: u64 =
        ChainState::<TestSpec, MockDaSpec>::ELASTICITY_MULTIPLIER.apply_div(gas_limit);

    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::<1>::from(gas_limit),
            gas_used: GasUnit::<1>::from(2 * gas_target),
            base_fee_per_gas: GasPrice::<1>::from(INITIAL_BASE_FEE_PER_GAS),
        },
        &GasPrice::<1>::from(expected_base_fee_per_gas),
        "The base fee per gas should increase by `base_fee_per_gas * (1/BASE_FEE_MAX_CHANGE_DENOMINATOR)` if the gas used is twice as much as the target",
    );

    // The new value of the base fee should not depend on the value of the gas used, as long as it is twice as much as the target
    let new_gas_limit: u64 = 100 * gas_limit;
    let new_gas_target: u64 =
        ChainState::<TestSpec, MockDaSpec>::ELASTICITY_MULTIPLIER.apply_div(new_gas_limit);

    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::<1>::from(new_gas_limit),
            gas_used: GasUnit::<1>::from(2 * new_gas_target),
            base_fee_per_gas: GasPrice::<1>::from(INITIAL_BASE_FEE_PER_GAS),
        },
        &GasPrice::<1>::from(expected_base_fee_per_gas),
        "The base fee per gas should increase by `base_fee_per_gas * (1/BASE_FEE_MAX_CHANGE_DENOMINATOR)` if the gas used is twice as much as the target. The new base fee should not depend on the value of the gas used, as long as it is twice as much as the target",
    );
}

/// The base fee per gas should decrease by `base_fee_per_gas * 1/(2*BASE_FEE_MAX_CHANGE_DENOMINATOR)` if the gas used is half as much as the target
#[test]
fn base_fee_decreases_if_gas_used_is_half_target() {
    const INITIAL_BASE_FEE_PER_GAS: u64 = 10000;

    let expected_base_fee_per_gas: u64 = INITIAL_BASE_FEE_PER_GAS
        - ChainState::<TestSpec, MockDaSpec>::BASE_FEE_MAX_CHANGE_DENOMINATOR
            .apply_div(INITIAL_BASE_FEE_PER_GAS)
            / 2;

    let gas_limit: u64 = 100;
    let gas_target: u64 =
        ChainState::<TestSpec, MockDaSpec>::ELASTICITY_MULTIPLIER.apply_div(gas_limit);

    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::<1>::from(gas_limit),
            gas_used: GasUnit::<1>::from(gas_target/2),
            base_fee_per_gas: GasPrice::<1>::from(INITIAL_BASE_FEE_PER_GAS),
        },
        &GasPrice::<1>::from(expected_base_fee_per_gas),
        "The base fee per gas should decrease by `base_fee_per_gas * 1/(2*BASE_FEE_MAX_CHANGE_DENOMINATOR)` if the gas used is half the target",
    );

    // The new value of the base fee should not depend on the value of the gas used, as long as it is half of the target
    let new_gas_limit: u64 = 100 * gas_limit;
    let new_gas_target: u64 =
        ChainState::<TestSpec, MockDaSpec>::ELASTICITY_MULTIPLIER.apply_div(new_gas_limit);

    assert_correct_gas_update(
        &BlockGasInfo {
            gas_limit: GasUnit::<1>::from(new_gas_limit),
            gas_used: GasUnit::<1>::from(new_gas_target/2),
            base_fee_per_gas: GasPrice::<1>::from(INITIAL_BASE_FEE_PER_GAS),
        },
        &GasPrice::<1>::from(expected_base_fee_per_gas),
        "The base fee per gas should increase by `base_fee_per_gas * 1/(2*BASE_FEE_MAX_CHANGE_DENOMINATOR)` if the gas used is half the target. The new base fee should not depend on the value of the gas used",
    );
}
