use sov_bank::Amount;
use sov_modules_api::capabilities::AuthorizeSequencerError;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{Gas, GasMeter, GasMeteringError, PreExecWorkingSet, Spec, TxScratchpad};

use crate::{AllowedSequencer, SequencerRegistry};

/// A struct that keeps track of the staked amount of a sequencer and the accumulated penalty amount.
/// The sequencer may get penalized for submitting invalid transactions, the penalties are accumulated
/// during execution in that struct. The remaining stake amount decreases as the penalties are accumulated.
///
/// The current amount staked by the sequencer is the sum of the
/// remaining stake amount and the accumulated penalty amount.
///
/// # Type-safety invariant
/// This struct should always ensure that the penalty amount is always below the total amount staked
/// for type safety purposes.
///
/// # Constructor
/// This struct can only be constructed by the [`SequencerRegistry::authorize_sequencer`] method.
pub struct SequencerStakeMeter<GU: Gas> {
    remaining_stake: Amount,
    penalty_accumulator: GU,
    gas_price: GU::Price,
}

impl<GU: Gas> GasMeter<GU> for SequencerStakeMeter<GU> {
    fn charge_gas(&mut self, amount: &GU) -> Result<(), GasMeteringError<GU>> {
        let amount_value = amount.value(&self.gas_price);

        if amount_value > self.remaining_stake {
            return Err(GasMeteringError::OutOfGas {
                gas_to_charge: amount.clone(),
                gas_price: self.gas_price.clone(),
                remaining_funds: self.remaining_stake,
                total_gas_consumed: self.gas_used().clone(),
            });
        }

        self.remaining_stake -= amount_value;
        self.penalty_accumulator.combine(amount);

        Ok(())
    }

    fn refund_gas(&mut self, gas: &GU) -> Result<(), GasMeteringError<GU>> {
        self.penalty_accumulator = self.penalty_accumulator.checked_sub(gas).ok_or_else(|| {
            GasMeteringError::ImpossibleToRefundGas {
                gas_to_refund: gas.clone(),
                gas_used: self.penalty_accumulator.clone(),
            }
        })?;
        self.remaining_stake = self
            .remaining_stake
            .saturating_add(gas.value(&self.gas_price));

        Ok(())
    }

    fn gas_used(&self) -> &GU {
        &self.penalty_accumulator
    }

    fn gas_price(&self) -> &GU::Price {
        &self.gas_price
    }

    fn remaining_funds(&self) -> u64 {
        self.remaining_stake
    }
}

impl<S: Spec, Da: sov_modules_api::DaSpec> SequencerRegistry<S, Da> {
    /// Checks whether `sender` is a registered sequencer with enough staked amount.
    /// If so, returns a [`SequencerStakeMeter`] which tracks the sequencer stake. Otherwise, returns a [`AuthorizeSequencerError`].
    pub fn authorize_sequencer(
        &self,
        sender: &Da::Address,
        base_fee_per_gas: &<S::Gas as Gas>::Price,
        mut scratchpad: TxScratchpad<S>,
    ) -> Result<PreExecWorkingSet<S, SequencerStakeMeter<S::Gas>>, AuthorizeSequencerError<S>> {
        let sequencer = match self.is_sender_allowed(sender, &mut scratchpad) {
            Ok(seq) => seq,
            Err(e) => {
                return Err(AuthorizeSequencerError {
                    tx_scratchpad: scratchpad,
                    reason: e.into(),
                })
            }
        };

        let seq_meter = SequencerStakeMeter::<S::Gas> {
            remaining_stake: sequencer.balance,
            penalty_accumulator: S::Gas::zero(),
            gas_price: base_fee_per_gas.clone(),
        };

        Ok(scratchpad.to_pre_exec_working_set(seq_meter))
    }

    /// Refunds some of the sequencer's staked amount.
    /// Only modifies the `remaining_stake` field of the [`SequencerStakeMeter`] to increase the remaining staked amount.
    /// The `gas_used` field of the [`SequencerStakeMeter`] is not modified.
    ///
    /// # Note
    /// Saturates if the sum of the refunded amount and remaining stake overflows.
    pub fn refund_sequencer(
        &self,
        sequencer_stake_meter: &mut SequencerStakeMeter<S::Gas>,
        refund_amount: u64,
    ) {
        sequencer_stake_meter.remaining_stake = sequencer_stake_meter
            .remaining_stake
            .saturating_add(refund_amount);
    }

    /// Penalizes the sequencer. In practice, sets its stake to the remaining stake tracked by the [`SequencerStakeMeter`].
    pub fn penalize_sequencer(
        &self,
        sender: &Da::Address,
        reason: impl std::fmt::Display,
        pre_exec_working_set: PreExecWorkingSet<S, SequencerStakeMeter<S::Gas>>,
    ) -> TxScratchpad<S> {
        let penalty_amount = pre_exec_working_set.gas_used_value();
        let remaining_stake = pre_exec_working_set.remaining_funds();

        let mut scratchpad = pre_exec_working_set.into();

        if let Some(AllowedSequencer {
            address,
            balance: _,
        }) = self
            .allowed_sequencers
            .get(sender, &mut scratchpad)
            .unwrap_infallible()
        {
            tracing::info!(
                sequencer = %address,
                penalty_amount = ?penalty_amount,
                remaining_stake = %remaining_stake,
                reason = %reason,
                "The sequencer was penalized",
            );

            self.allowed_sequencers
                .set(
                    sender,
                    &AllowedSequencer {
                        address,
                        balance: remaining_stake,
                    },
                    &mut scratchpad,
                )
                .unwrap_infallible();
        }

        scratchpad
    }
}

#[cfg(test)]
mod tests {
    use sov_modules_api::{Gas, GasArray, GasMeter, GasPrice, GasUnit};

    use crate::{Amount, SequencerStakeMeter};

    impl<GU: Gas> SequencerStakeMeter<GU> {
        fn new(remaining_stake: Amount, gas_price: GU::Price) -> Self {
            Self {
                remaining_stake,
                penalty_accumulator: GU::ZEROED,
                gas_price,
            }
        }
    }

    #[test]
    fn charge_gas_should_fail_if_not_enough_funds() {
        let gas_price = GasPrice::<2>::from_slice(&[1; 2]);

        let mut gas_meter = SequencerStakeMeter::new(0, gas_price.clone());

        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[100; 2]))
                .is_err(),
            "The gas meter should not be able to charge gas if there is not enough funds"
        );
    }

    #[test]
    fn refund_gas_should_fail_if_not_enough_funds_consumed() {
        let gas_price = GasPrice::<2>::from_slice(&[1; 2]);

        let mut gas_meter = SequencerStakeMeter::new(100, gas_price.clone());

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

        let mut gas_meter = SequencerStakeMeter::new(REMAINING_FUNDS, gas_price.clone());
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
        assert_eq!(
            gas_meter.remaining_funds(),
            0,
            "There should be no more gas left in the meter"
        );

        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[1; 2]))
                .is_err(),
            "There should be no more gas left in the meter, hence charging more gas should fail"
        );
    }

    #[test]
    fn try_refund_gas() {
        const REMAINING_FUNDS: u64 = 100;
        let gas_price = GasPrice::from_slice(&[1; 2]);

        let mut gas_meter = SequencerStakeMeter::new(REMAINING_FUNDS, gas_price);
        assert!(
            gas_meter
                .charge_gas(&GasUnit::<2>::from_slice(&[REMAINING_FUNDS / 2; 2]))
                .is_ok(),
            "There should be enough gas left in the meter to charge"
        );
        assert_eq!(
            gas_meter.remaining_funds(),
            0,
            "There should be no more gas left in the meter"
        );

        assert!(
            gas_meter
                .refund_gas(&GasUnit::from_slice(&[REMAINING_FUNDS / 4; 2]))
                .is_ok(),
            "Enough gas should have been consumed to be refunded",
        );

        assert_eq!(
            gas_meter.gas_used(),
            &GasUnit::from_slice(&[REMAINING_FUNDS / 4; 2],),
            "The gas used amount should have decreased"
        );

        assert_eq!(
            gas_meter.remaining_funds(),
            REMAINING_FUNDS / 2,
            "Half of the gas should be refunded"
        );
    }
}
