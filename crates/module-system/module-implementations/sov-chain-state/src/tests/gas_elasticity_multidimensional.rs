use sov_mock_da::MockDaSpec;
use sov_modules_api::{Gas, GasArray, GasPrice, Spec};
use sov_test_utils::TestSpec;

use crate::{BlockGasInfo, ChainState};

const INITIAL_BASE_FEE_PER_GAS: [u64; 2] = [100, 100];

// The scalar constant by which the gas used is increased/decreased for each dimension.
// This constant is expressed as a fraction of the gas target. Hence, here if
// `GAS_DELTA_FRACTION = 2` that means the gas used is increased/decreased by `gas_target / 2` per dimension.
const GAS_DELTA_FRACTION: u64 = 2;

/// Helper function that initializes the gas elasticity tests for the multidimensional case. It computes the new base fee per gas
/// given an amount of gas used, the initial gas limit and the initial base fee per gas.
fn test_helper(gas_used: &<TestSpec as Spec>::Gas) -> <<TestSpec as Spec>::Gas as Gas>::Price {
    let mut parent_gas_info = BlockGasInfo::new(
        ChainState::<TestSpec, MockDaSpec>::initial_gas_limit(),
        INITIAL_BASE_FEE_PER_GAS.into(),
    );

    parent_gas_info.update_gas_used(gas_used.clone());

    ChainState::<TestSpec, MockDaSpec>::compute_base_fee_per_gas(&parent_gas_info)
}

/// Checks that the `base_fee_per_gas` does not change when the gas used is the same as the gas target.
#[test]
fn test_base_fee_does_not_change_if_target_is_reached() {
    let computed_base_fee_per_gas =
        test_helper(&ChainState::<TestSpec, MockDaSpec>::initial_gas_target());

    assert_eq!(
        computed_base_fee_per_gas,
        INITIAL_BASE_FEE_PER_GAS.into(),
        "The base fee per gas should not updated when the gas used is the same as the gas target"
    );
}

/// Checks that the `base_fee_per_gas` increases correctly when the gas used is above the gas target.
#[test]
fn test_base_fee_increases_if_above_target() {
    let gas_target = ChainState::<TestSpec, MockDaSpec>::initial_gas_target();
    let gas_increase_amount: u64 = gas_target.as_slice().iter().sum::<u64>()
        / (gas_target.as_slice().len() as u64)
        / GAS_DELTA_FRACTION;

    let mut gas_used = gas_target.clone();
    gas_used.scalar_add(gas_increase_amount);

    let computed_base_fee_per_gas = test_helper(&gas_used);

    // The base fee per gas should increase above the initial base fee per gas.
    assert!(
        computed_base_fee_per_gas > INITIAL_BASE_FEE_PER_GAS.into(),
        "The base fee per gas should increase when the gas used is above the gas target"
    );

    let delta_base_fee_per_gas = computed_base_fee_per_gas
        .checked_sub(&GasPrice::from_slice(&INITIAL_BASE_FEE_PER_GAS))
        .expect("The computed base fee per gas should be above the INITIAL_BASE_FEE_PER_GAS");

    assert!(
        delta_base_fee_per_gas > GasPrice::from_slice(&[1; 2]),
        "The base fee per gas delta should increase by more than 1, actual value {:?}",
        delta_base_fee_per_gas
    );
}

/// Checks that the `base_fee_per_gas` decreases correctly when the gas used is below the gas target.
#[test]
fn test_base_fee_decreases_if_below_target() {
    let gas_target = ChainState::<TestSpec, MockDaSpec>::initial_gas_target();
    let gas_decrease_amount: u64 = gas_target.as_slice().iter().sum::<u64>()
        / (gas_target.as_slice().len() as u64)
        / GAS_DELTA_FRACTION;

    let mut gas_used = gas_target.clone();
    gas_used.scalar_sub(gas_decrease_amount);

    let computed_base_fee_per_gas = test_helper(&gas_used);

    // The base fee per gas should decrease below the initial base fee per gas. The decrease amount should be high enough for the computed base fee per gas to be strictly
    // below the initial base fee per gas.
    assert!(
        computed_base_fee_per_gas < INITIAL_BASE_FEE_PER_GAS.into(),
        "The base fee per gas should decrease when the gas used is below the gas target"
    );
}

/// Checks that the update for each dimension is independent from the others
/// We consume more gas than the target for each even dimension and less gas for each odd dimension.
#[test]
fn test_base_fee_varies_accross_each_dimension() {
    let gas_target = ChainState::<TestSpec, MockDaSpec>::initial_gas_target();
    let gas_delta_amount: u64 = gas_target.as_slice().iter().sum::<u64>()
        / (gas_target.as_slice().len() as u64)
        / GAS_DELTA_FRACTION;

    let mut gas_used = gas_target.clone();

    gas_used
        .as_slice_mut()
        .iter_mut()
        .enumerate()
        .for_each(|(i, g)| {
            if i % 2 == 0 {
                *g += gas_delta_amount;
            } else {
                *g -= gas_delta_amount;
            }
        });

    let computed_base_fee_per_gas = test_helper(&gas_used);

    let initial_base_fee_per_gas =
        <<TestSpec as Spec>::Gas as Gas>::Price::from_slice(&INITIAL_BASE_FEE_PER_GAS);

    // The base fee per gas should decrease below the initial base fee per gas for odd dimensions and increase for even dimensions.
    computed_base_fee_per_gas
        .as_slice()
        .iter()
        .zip(initial_base_fee_per_gas.as_slice())
        .enumerate()
        .for_each(|(i, (g, initial_base_fee_per_gas))| {
            if i % 2 == 0 {
                assert!(
                *g > *initial_base_fee_per_gas,
                "The base fee per gas should increase when the gas used is above the gas target. Index: {i}"
            );
            } else {
                assert!(
                *g < *initial_base_fee_per_gas,
                "The base fee per gas should decrease when the gas used is below the gas target. Index: {i}"
            );
            }
        });
}
