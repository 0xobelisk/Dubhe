use std::cmp::max;

use serde::{Deserialize, Serialize};
use sov_modules_api::macros::config_value;
use sov_modules_api::{DaSpec, Gas, GasArray, GasPrice, GasUnit, Spec};
use thiserror::Error;

use crate::{BlockGasInfo, ChainState};

/// A non-zero `u8` ratio, useful for defining ratios and multiplicative constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NonZeroRatio(u8);

/// An error that is returned if attempting to create a [`NonZeroRatio`] from `0_u8`.
#[derive(Error, Debug)]
#[error("This value cannot be set to 0")]
pub struct NonZeroRatioConversionError;

impl NonZeroRatio {
    /// Creates a new [`NonZeroRatio`] from a [`u8`]. This method should be used to build constants of type [`NonZeroRatio`].
    /// To build a [`NonZeroRatio`] at runtime, use the [`TryFrom<u8>`] trait.
    ///
    /// # Safety
    /// This method panics if the provided value is `0`.
    pub const fn from_u8_unwrap(value: u8) -> Self {
        if value == 0 {
            panic!("This value cannot be set to 0");
        }
        Self(value)
    }

    /// Divides the provided value by the ratio.
    pub fn apply_div(&self, value: u64) -> u64 {
        value
            .checked_div(self.0.into())
            .expect("The ratio cannot be zero")
    }
}

impl TryFrom<u8> for NonZeroRatio {
    type Error = NonZeroRatioConversionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value == 0 {
            Err(NonZeroRatioConversionError)
        } else {
            Ok(Self(value))
        }
    }
}

impl From<NonZeroRatio> for u8 {
    fn from(value: NonZeroRatio) -> Self {
        value.0
    }
}

/// Defines constants used by the chain state module for gas price computation
impl<S: Spec, Da: DaSpec> ChainState<S, Da> {
    /// A constant used to control the variations of the base fee updates.
    /// This method is used as a constructor for [`ChainState::BASE_FEE_MAX_CHANGE_DENOMINATOR`].
    /// This value is retrieved from the config file and is then converted to a [`NonZeroRatio`] at constant
    /// time for type safety.
    /// Since this value is then cached in the [`ChainState::BASE_FEE_MAX_CHANGE_DENOMINATOR`] constant, one should prefer to
    /// use the constant directly instead of this function.
    const fn base_fee_change_denominator() -> NonZeroRatio {
        NonZeroRatio::from_u8_unwrap(config_value!("BASE_FEE_MAX_CHANGE_DENOMINATOR"))
    }

    /// Constant used to control the range of variation of the gas price elasticity.
    /// This method is used as a constructor for [`ChainState::ELASTICITY_MULTIPLIER`].
    /// The constant value is retrieved from the config file and is then converted to a [`NonZeroRatio`] at constant
    /// time for type safety.
    /// Since this value is then cached in the [`ChainState::ELASTICITY_MULTIPLIER`] constant, one should prefer to
    /// use the constant directly instead of this function.
    const fn elasticity_multiplier() -> NonZeroRatio {
        NonZeroRatio::from_u8_unwrap(config_value!("ELASTICITY_MULTIPLIER"))
    }

    /// Constant used to control the variations of the base fee updates.
    /// The higher this value is, the less the base fee will change. This value is an [`u8`] greater or equal than 1
    /// (setting this value to 0 doesn't make much sense).
    ///
    /// # Note
    /// This constant is the same as the `BASE_FEE_MAX_CHANGE_DENOMINATOR` constant
    /// defined in the EIP-1559 specification its default value is 8.
    pub const BASE_FEE_MAX_CHANGE_DENOMINATOR: NonZeroRatio = Self::base_fee_change_denominator();

    /// Constant used to control the range of variation of the gas price elasticity. This value is equal to the
    /// ratio of the maximum gas price and the target gas price. The higher this value is, the higher the maximum gas price
    /// can be. This value is an [`u8`] greater or equal than 1 (setting this value to 0 doesn't make much sense).
    ///
    /// # Note
    /// This constant is the same as the `ELASTICITY_MULTIPLIER` constant
    /// defined in the EIP-1559 specification its default value is 2.
    pub const ELASTICITY_MULTIPLIER: NonZeroRatio = Self::elasticity_multiplier();

    /// Specifies the initial base fee per gas for the genesis block.
    ///
    /// # TODO
    /// This method should be converted in a constant time constructor. The current implementation of the
    /// [`config_value!`] macro cannot be used to define [`sov_modules_api::GasPrice`] constants, so this will probably
    /// require a new proc-macro, see `<https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/475>`.
    ///
    /// # Note
    /// This constant is the same as the `INITIAL_BASE_FEE_PER_GAS` constant
    /// defined in the EIP-1559 specification. Its default value is `[100, 100]`.
    ///
    /// # Safety
    /// This method panics if the initial gas price is not set at genesis
    pub fn initial_base_fee_per_gas() -> <S::Gas as Gas>::Price {
        const INITIAL_BASE_FEE_PER_GAS: &[u64] = &config_value!("INITIAL_BASE_FEE_PER_GAS");

        <<S as Spec>::Gas as Gas>::Price::from_slice(INITIAL_BASE_FEE_PER_GAS)
    }

    /// Specifies the initial gas limit for the genesis block.
    /// This value is retrieved from the config file and is then converted to a [`sov_modules_api::GasUnit`] at runtime
    ///
    /// # TODO
    /// This method should be converted in a constant time constructor. The current implementation of the
    /// [`config_value!`] macro cannot be used to define [`sov_modules_api::GasUnit`] constants, so this will probably
    /// require a new proc-macro `<https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/475>`.
    ///
    /// # Note
    /// This constant is the same as the `INITIAL_BASE_FEE_PER_GAS` constant
    /// defined in the EIP-1559 specification its default value is `[1, 1]`.
    pub fn initial_gas_limit() -> S::Gas {
        const INITIAL_GAS_LIMIT: &[u64] = &config_value!("INITIAL_GAS_LIMIT");

        S::Gas::from_slice(INITIAL_GAS_LIMIT)
    }

    /// Computes the gas target for the provided gas limit.
    /// Basically, divides each dimension of the gas limit by the [`ChainState::ELASTICITY_MULTIPLIER`].
    pub fn gas_target(gas_limit: &S::Gas) -> S::Gas {
        S::Gas::from_slice(
            &gas_limit
                .as_slice()
                .iter()
                .map(|g| Self::ELASTICITY_MULTIPLIER.apply_div(*g))
                .collect::<Vec<u64>>(),
        )
    }

    /// Computes the initial gas target (genesis block) by calling [`ChainState::gas_target`] on the initial gas limit.
    pub fn initial_gas_target() -> S::Gas {
        Self::gas_target(&Self::initial_gas_limit())
    }
}

impl<S: Spec, Da: DaSpec> ChainState<S, Da> {
    /// Computes the updated gas price following a block execution for a single dimension.
    /// This reproduces the logic of the EIP-1559 specification to compute the updated `base_fee_per_gas` (`<https://eips.ethereum.org/EIPS/eip-1559>`).
    /// Note that here we drop the `parent` prefix and call the state variables `gas_limit`, `gas_used` and `base_fee_per_gas`.
    pub(crate) fn compute_base_fee_per_gas_unidimensional(
        parent_gas_info: &BlockGasInfo<GasUnit<1>>,
    ) -> GasPrice<1> {
        let BlockGasInfo {
            gas_limit,
            gas_used,
            mut base_fee_per_gas,
        } = parent_gas_info.clone();

        // The gas target is equal to `gas_limit // ELASTICITY_MULTIPLIER`
        let gas_target =
            GasUnit::<1>::from(Self::ELASTICITY_MULTIPLIER.apply_div(gas_limit.into()));

        if gas_used == gas_target {
            // We reached the gas target, so we don't need to update the base fee
            base_fee_per_gas
        } else {
            // We need to update the base fee because we didn't reach the gas target.

            // Compute the difference in absolute value between the gas target and the gas used.
            // This value is the delta between the gas target and the gas used. We need to then apply the `base_fee_per_gas` to compute its value in tokens.
            let gas_used_delta_as_u64 =
                u64::from(gas_target.clone()).abs_diff(gas_used.clone().into());
            let gas_used_delta = GasUnit::<1>::from(gas_used_delta_as_u64);
            let gas_used_delta_value = gas_used_delta.value(&base_fee_per_gas);

            // This division expresses the `base_fee_per_gas` delta as the ration (gas_used_delta_value / gas_target).
            // If the division underflows, the delta is set to zero
            //
            // Note here that this operation gives a value that can be expressed as a `GasPrice<1>` because we do
            // `base_fee_per_gas * (gas_used_delta / gas_target)`.
            let base_fee_per_gas_delta_u64 = gas_used_delta_value
                .checked_div(gas_target.clone().into())
                .unwrap_or_default();

            // We normalize the result, the same way as in the EIP-1559 specification (`<https://eips.ethereum.org/EIPS/eip-1559>`)
            let base_fee_per_gas_delta_normalized =
                Self::BASE_FEE_MAX_CHANGE_DENOMINATOR.apply_div(base_fee_per_gas_delta_u64);

            if gas_used > gas_target {
                // In that case, we take the maximum with `1` to make sure the `base_fee_per_gas` is always increased
                let base_fee_per_gas_delta_normalized =
                    GasPrice::<1>::from(max(base_fee_per_gas_delta_normalized, 1));

                base_fee_per_gas.combine(&base_fee_per_gas_delta_normalized);

                base_fee_per_gas
            } else {
                // Although unlikely, the `base_fee_per_gas` can reach zero. We cannot have a negative value for gas price
                // so we saturate at zero.
                base_fee_per_gas
                    .checked_sub(&GasPrice::<1>::from(base_fee_per_gas_delta_normalized))
                    .unwrap_or(GasPrice::<1>::ZEROED)
            }
        }
    }

    /// Computes the updated gas price following a block execution, provided the arguments.
    ///
    /// The computation of the base price for the current block is determined
    /// by the value of the `base_fee_per_gas`, `gas_limit` of the parent block as well as constant parameters
    /// such as the [`ChainState::ELASTICITY_MULTIPLIER`] and the amount of gas used in the parent block.
    /// The computation follows the one described in the EIP-1559 specification, where each dimension
    /// of the multi-dimensional gas price is independently updated following EIP-1559.
    pub fn compute_base_fee_per_gas(
        parent_gas_info: &BlockGasInfo<S::Gas>,
    ) -> <S::Gas as Gas>::Price {
        let res: Vec<u64> = parent_gas_info
            .base_fee_per_gas
            .as_slice()
            .iter()
            .zip(parent_gas_info.gas_limit.as_slice())
            .zip(parent_gas_info.gas_used.as_slice())
            .map(|((base_fee_per_gas, gas_limit), gas_used)| {
                Self::compute_base_fee_per_gas_unidimensional(&BlockGasInfo {
                    gas_limit: GasUnit::<1>::from(*gas_limit),
                    gas_used: GasUnit::<1>::from(*gas_used),
                    base_fee_per_gas: GasPrice::<1>::from(*base_fee_per_gas),
                })
                .into()
            })
            .collect();

        <S::Gas as Gas>::Price::from_slice(res.as_slice())
    }
}
