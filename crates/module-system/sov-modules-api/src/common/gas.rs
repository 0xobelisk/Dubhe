//! Gas unit definitions and implementations.

use core::fmt::{self, Debug, Display};

use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

/// A multi-dimensional gas unit represented as an array of `u64`.`
pub trait GasArray:
    'static
    + fmt::Debug
    + Display
    + Clone
    + Send
    + Sync
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + core::hash::Hash
    + Serialize
    + DeserializeOwned
    + BorshSerialize
    + BorshDeserialize
{
    /// A zeroed instance of the unit.
    const ZEROED: Self;

    /// Creates a unit from a multi-dimensional unit with arbitrary dimension.
    fn from_slice(dimensions: &[u64]) -> Self;

    /// Returns a multi-dimensional representation of the unit.
    fn as_slice(&self) -> &[u64];

    /// Returns a mutable reference to the multi-dimensional representation of the unit.
    fn as_slice_mut(&mut self) -> &mut [u64];

    /// Creates a multi-dimensional representation of the unit.
    fn to_vec(&self) -> Vec<u64>;

    /// In-place combination of gas units, resulting in an addition.
    fn combine(&mut self, rhs: &Self) -> &mut Self;

    /// Out-of-place substraction of gas units.
    ///
    /// # Output
    /// Returns [`None`] if the substraction in any gas dimension underflows.
    fn checked_sub(&self, rhs: &Self) -> Option<Self>;

    /// In-place division of gas units.
    fn scalar_division(&mut self, scalar: u64) -> &mut Self;

    /// In-place product of gas units, resulting in a multiplication.
    fn scalar_product(&mut self, scalar: u64) -> &mut Self;

    /// In-place addition of gas units with a scalar.
    fn scalar_add(&mut self, scalar: u64) -> &mut Self;

    /// In-place substraction of gas units with a scalar.
    fn scalar_sub(&mut self, scalar: u64) -> &mut Self;
}

/// A unit of gas
pub trait Gas: GasArray {
    /// The price of the gas, expressed in tokens per unit.
    type Price: GasArray;

    /// Calculates the value of the given amount of gas at the given price.
    fn value(&self, price: &Self::Price) -> u64;

    /// Returns a gas unit which is zero in all dimensions.
    fn zero() -> Self {
        Self::ZEROED
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
/// A multi-dimensional gas unit.
pub struct GasUnit<const N: usize>([u64; N]);
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
/// A gas price for multi-dimensional gas.
pub struct GasPrice<const N: usize>([u64; N]);

macro_rules! impl_gas_dimensions {
    ($t: ty, $n: expr) => {
        impl ::serde::Serialize for $t {
            fn serialize<__S>(&self, serializer: __S) -> Result<__S::Ok, __S::Error>
            where
                __S: serde::Serializer,
            {
                <[u64; $n] as serde::Serialize>::serialize(&self.0, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $t {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                <[u64; $n] as serde::Deserialize>::deserialize(deserializer).map(Self)
            }
        }

        impl From<[u64; $n]> for $t {
            fn from(array: [u64; $n]) -> Self {
                Self(array)
            }
        }

        impl GasArray for $t {
            const ZEROED: Self = Self([0; $n]);

            fn from_slice(dimensions: &[u64]) -> Self {
                // as demonstrated on the link below, the compiler can easily optimize the conversion as if
                // it is a transparent type.
                //
                // https://rust.godbolt.org/z/rPhaxnPEY
                let mut unit = Self::ZEROED;
                unit.0
                    .iter_mut()
                    .zip(dimensions.iter().copied())
                    .for_each(|(a, b)| *a = b);
                unit
            }

            fn as_slice(&self) -> &[u64] {
                &self.0[..]
            }

            fn as_slice_mut(&mut self) -> &mut [u64] {
                &mut self.0[..]
            }

            fn to_vec(&self) -> Vec<u64> {
                self.0.to_vec()
            }

            fn checked_sub(&self, rhs: &Self) -> Option<Self> {
                let res: Option<Vec<u64>> = self
                    .0
                    .as_slice()
                    .iter()
                    .zip(rhs.0.as_slice())
                    .map(|(l, r)| l.checked_sub(*r))
                    .collect();

                res.map(|v| Self::from_slice(&v))
            }

            fn scalar_division(&mut self, scalar: u64) -> &mut Self {
                self.0
                    .iter_mut()
                    .for_each(|s| *s = s.checked_div(scalar).unwrap_or(0));
                self
            }

            fn scalar_product(&mut self, scalar: u64) -> &mut Self {
                self.0
                    .iter_mut()
                    .for_each(|s| *s = s.saturating_mul(scalar));
                self
            }

            fn scalar_add(&mut self, scalar: u64) -> &mut Self {
                self.0
                    .iter_mut()
                    .for_each(|s| *s = s.saturating_add(scalar));
                self
            }

            fn scalar_sub(&mut self, scalar: u64) -> &mut Self {
                self.0
                    .iter_mut()
                    .for_each(|s| *s = s.saturating_sub(scalar));
                self
            }

            fn combine(&mut self, rhs: &Self) -> &mut Self {
                self.0
                    .iter_mut()
                    .zip(rhs.0.iter())
                    .for_each(|(l, r)| *l = l.saturating_add(*r));
                self
            }
        }
    };
}

macro_rules! impl_gas_unit {
    ($n:expr) => {
        impl Gas for GasUnit<$n> {
            type Price = GasPrice<$n>;

            fn value(&self, price: &Self::Price) -> u64 {
                self.0
                    .iter()
                    .zip(price.as_slice().iter().copied())
                    .map(|(a, b)| a.saturating_mul(b))
                    .fold(0, |a, b| a.saturating_add(b))
            }
        }

        impl_gas_dimensions!(GasUnit<$n>, $n);
        impl_gas_dimensions!(GasPrice<$n>, $n);

        impl fmt::Display for GasUnit<$n> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "GasUnit[{}]",
                    self.0
                        .iter()
                        .map(|g| g.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }

        impl fmt::Display for GasPrice<$n> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "GasPrice[{}]",
                    self.as_slice()
                        .iter()
                        .map(|g| g.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    };
}

impl_gas_unit!(1);
impl_gas_unit!(2);
impl_gas_unit!(3);
impl_gas_unit!(4);
impl_gas_unit!(5);
impl_gas_unit!(6);
impl_gas_unit!(7);
impl_gas_unit!(8);
impl_gas_unit!(9);
impl_gas_unit!(10);
impl_gas_unit!(11);
impl_gas_unit!(12);
impl_gas_unit!(13);
impl_gas_unit!(14);
impl_gas_unit!(15);
impl_gas_unit!(16);
impl_gas_unit!(17);
impl_gas_unit!(18);
impl_gas_unit!(19);
impl_gas_unit!(20);
impl_gas_unit!(21);
impl_gas_unit!(22);
impl_gas_unit!(23);
impl_gas_unit!(24);
impl_gas_unit!(25);
impl_gas_unit!(26);
impl_gas_unit!(27);
impl_gas_unit!(28);
impl_gas_unit!(29);
impl_gas_unit!(30);
impl_gas_unit!(31);
impl_gas_unit!(32);

impl<'a> From<&'a GasPrice<1>> for u64 {
    fn from(value: &'a GasPrice<1>) -> Self {
        value.0[0]
    }
}

impl From<GasPrice<1>> for u64 {
    fn from(value: GasPrice<1>) -> Self {
        value.0[0]
    }
}

impl From<u64> for GasPrice<1> {
    fn from(value: u64) -> Self {
        GasPrice::from_slice(&[value])
    }
}

impl<'a> From<&'a GasUnit<1>> for u64 {
    fn from(value: &'a GasUnit<1>) -> Self {
        value.0[0]
    }
}

impl From<GasUnit<1>> for u64 {
    fn from(value: GasUnit<1>) -> Self {
        value.0[0]
    }
}

impl From<u64> for GasUnit<1> {
    fn from(value: u64) -> Self {
        GasUnit::from_slice(&[value])
    }
}

/// Error type that can be raised by the `GasMeter` trait.
/// Errors can be raised either when the meter runs out of gas or when the refund operation fails.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GasMeteringError<GU: Gas> {
    /// The gas meter has ran out of gas.
    #[error("The gas to charge is greater than the funds available in the meter. Gas to charge {gas_to_charge}, gas price {gas_price}, remaining funds {remaining_funds}, total gas consumed {total_gas_consumed}")]
    OutOfGas {
        gas_to_charge: GU,
        gas_price: GU::Price,
        remaining_funds: u64,
        total_gas_consumed: GU,
    },
    /// The refund operation failed for the gas meter.
    #[error("The gas to refund is greater than the gas used. Gas to refund {gas_to_refund}, gas used {gas_used}")]
    ImpossibleToRefundGas { gas_to_refund: GU, gas_used: GU },
}

/// A type-safe trait that should track the gas consumed by a finite ressource over time.
pub trait GasMeter<GU: Gas> {
    /// Charges some gas in the gas meter.
    ///
    /// # Error
    /// May raises an error if the gas to charge is greater than the funds available
    fn charge_gas(&mut self, amount: &GU) -> Result<(), GasMeteringError<GU>>;

    /// Refunds some gas to the gas meter.
    ///
    /// ## Note
    /// This method may fail if the gas to refund is greater than the funds charged to the gas meter.
    /// In that case, the gas meter won't be updated and the refund will fail.
    fn refund_gas(&mut self, gas: &GU) -> Result<(), GasMeteringError<GU>>;

    /// Returns the current gas used accumulated by the stake meter.
    fn gas_used(&self) -> &GU;

    /// Returns the current gas price.
    fn gas_price(&self) -> &GU::Price;

    /// Returns the gas used as a token amount.
    fn gas_used_value(&self) -> u64 {
        self.gas_used().value(self.gas_price())
    }

    /// The remaining amount of tokens locked in the meter
    fn remaining_funds(&self) -> u64;
}

/// An unlimited gas meter. Only tracks the amount of gas consumed.
/// The [`UnlimitedGasMeter::charge_gas`] method will always succeed.
/// Only use this if you are certain that the gas meter will never run out of funds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnlimitedGasMeter<GU: Gas> {
    gas_used: GU,
    gas_price: GU::Price,
}

impl<GU: Gas> Default for UnlimitedGasMeter<GU> {
    fn default() -> Self {
        Self::new()
    }
}

impl<GU: Gas> UnlimitedGasMeter<GU> {
    /// Creates a new unlimited gas meter with the provided gas price.
    pub const fn new_with_price(gas_price: GU::Price) -> Self {
        Self {
            gas_used: GU::ZEROED,
            gas_price,
        }
    }
    /// Creates a new unlimited gas meter with a zeroed price.
    pub const fn new() -> Self {
        Self {
            gas_used: GU::ZEROED,
            gas_price: GU::Price::ZEROED,
        }
    }
}

impl<GU: Gas> GasMeter<GU> for UnlimitedGasMeter<GU> {
    fn charge_gas(&mut self, gas: &GU) -> Result<(), GasMeteringError<GU>> {
        self.gas_used.combine(gas);
        std::result::Result::Ok(())
    }

    fn refund_gas(&mut self, gas: &GU) -> Result<(), GasMeteringError<GU>> {
        self.gas_used = self.gas_used.checked_sub(gas).ok_or_else(|| {
            GasMeteringError::ImpossibleToRefundGas {
                gas_to_refund: gas.clone(),
                gas_used: self.gas_used.clone(),
            }
        })?;

        Ok(())
    }

    fn gas_used(&self) -> &GU {
        &self.gas_used
    }

    /// Returns the gas price.
    fn gas_price(&self) -> &GU::Price {
        &self.gas_price
    }

    fn remaining_funds(&self) -> u64 {
        u64::MAX
    }
}

#[cfg(test)]
mod tests {
    use crate::{GasArray, GasMeter, GasPrice, GasUnit, UnlimitedGasMeter};

    #[test]
    fn charge_gas_should_always_succeed() {
        let gas_price = GasPrice::<2>::from_slice(&[1; 2]);

        let mut gas_meter = UnlimitedGasMeter::new_with_price(gas_price.clone());

        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[u64::MAX; 2]))
                .is_ok(),
            "The unlimited gas meter should never run out of gas"
        );
    }

    #[test]
    fn refund_gas_should_fail_if_not_enough_funds_consumed() {
        let gas_price = GasPrice::<2>::from_slice(&[1; 2]);

        let mut gas_meter = UnlimitedGasMeter::new_with_price(gas_price.clone());

        assert!(
            gas_meter
                .refund_gas(&GasUnit::<2>::from_slice(&[100; 2]))
                .is_err(),
            "The gas meter should not be able to refund gas if there is not enough gas consumed"
        );
    }

    #[test]
    fn try_charge_gas() {
        const REMAINING_FUNDS: u64 = 100;
        let gas_price = GasPrice::<2>::from_slice(&[1; 2]);

        let mut gas_meter = UnlimitedGasMeter::new_with_price(gas_price.clone());
        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[REMAINING_FUNDS / 2; 2]))
                .is_ok(),
            "It should be possible to charge gas"
        );
        assert_eq!(
            gas_meter.gas_used(),
            &GasUnit::from_slice(&[REMAINING_FUNDS / 2; 2]),
            "The gas used should be the same as the gas charged"
        );
        assert_eq!(gas_meter.gas_price(), &gas_price);

        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[1; 2]))
                .is_ok(),
            "The unlimited gas meter should never run out of gas"
        );
    }

    #[test]
    fn try_refund_gas() {
        const FUNDS_TO_CONSUME: u64 = 100;
        let gas_price = GasPrice::from_slice(&[1; 2]);

        let mut gas_meter = UnlimitedGasMeter::new_with_price(gas_price);
        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[FUNDS_TO_CONSUME / 2; 2]))
                .is_ok(),
            "There should be enough gas left in the meter to charge"
        );

        assert!(
            gas_meter
                .refund_gas(&GasUnit::from_slice(&[FUNDS_TO_CONSUME / 4; 2]))
                .is_ok(),
            "Enough gas should have been consumed to be refunded",
        );

        assert_eq!(
            gas_meter.gas_used(),
            &GasUnit::from_slice(&[FUNDS_TO_CONSUME / 4; 2],),
            "The gas used amount should have decreased"
        );
    }

    #[test]
    fn test_gas_display_unidimensional() {
        let gas_unit = GasUnit::<1>::from(100);
        assert_eq!(
            "GasUnit[100]",
            gas_unit.to_string(),
            "The gas unit should be displayed correctly"
        );

        let gas_price = GasPrice::<1>::from(100);
        assert_eq!(
            "GasPrice[100]",
            gas_price.to_string(),
            "The gas unit should be displayed correctly"
        );
    }

    #[test]
    fn test_gas_display_multidimensional() {
        let gas_unit = GasUnit::<2>::from_slice(&[100, 50]);
        assert_eq!(
            "GasUnit[100, 50]",
            gas_unit.to_string(),
            "The gas unit should be displayed correctly"
        );

        let gas_price = GasPrice::<2>::from_slice(&[100, 50]);
        assert_eq!(
            "GasPrice[100, 50]",
            gas_price.to_string(),
            "The gas unit should be displayed correctly"
        );
    }
}
