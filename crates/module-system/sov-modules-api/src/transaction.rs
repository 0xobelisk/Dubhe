use std::cmp::min;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use borsh::{BorshDeserialize, BorshSerialize};
#[cfg(all(target_os = "zkvm", feature = "bench"))]
use risc0_cycle_macros::cycle_tracker;
use serde::{Deserialize, Serialize};
use sov_modules_macros::config_value;
#[cfg(feature = "native")]
pub use sov_rollup_interface::crypto::PrivateKey;
use sov_rollup_interface::crypto::SigVerificationError;
use sov_rollup_interface::zk::CryptoSpec;
use thiserror::Error;

use crate::{
    Gas, GasArray, GasMeter, GasMeteringError, MeteredSigVerificationError, MeteredSignature, Spec,
};

/// A type wrapper around a u64 which represents the priority fee.
/// Since the priority fee is expressed as a basis point, we should use this wrapper for
/// improved type safety.
///
/// # Note
/// The priority fee is expressed as a basis point. Ie, `1%` is represented as `10_000`.
#[derive(
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub struct PriorityFeeBips(pub u64);

impl PriorityFeeBips {
    pub const ZERO: Self = Self(0);

    /// Constant function to create a priority fee from a percentage.
    /// The priority fee is expressed as a basis point, ie `PriorityFeeBips(100)` is equivalent to a 1% fee -
    /// hence calling this `from_percentage(1)` will return `PriorityFeeBips(100)`.
    pub const fn from_percentage(value: u64) -> Self {
        Self(value * 100)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Applying the priority fee to this quantity causes an overflow")]
pub struct PriorityFeeApplyOverflowError;

impl From<u64> for PriorityFeeBips {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<PriorityFeeBips> for u64 {
    fn from(value: PriorityFeeBips) -> Self {
        value.0
    }
}

impl PriorityFeeBips {
    /// Applies the priority fee to a given quantity
    /// We make sure to cast the intermediate result to u128 to avoid overflowing.
    pub fn apply(&self, quantity: u64) -> Result<u64, PriorityFeeApplyOverflowError> {
        // We need to cast to u128 to avoid overflowing.
        let quantity_u128 = quantity as u128;
        let fee_u128 = self.0 as u128;
        let result = (quantity_u128 * fee_u128) / (10_000);
        result.try_into().map_err(|_| PriorityFeeApplyOverflowError)
    }
}

/// Contains details related to fees and gas handling.
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct TxDetails<S: Spec> {
    /// The maximum priority fee that can be paid for this transaction expressed as a basis point percentage of the gas consumed by the transaction.
    /// Ie if the transaction has consumed `100` gas tokens, and the priority fee is set to `100_000` (10%), the
    /// gas tip will be `10` tokens.
    pub max_priority_fee_bips: PriorityFeeBips,
    /// The maximum fee that can be paid for this transaction expressed as a the gas token amount
    pub max_fee: u64,
    /// The gas limit of the transaction.
    /// This is an optional field that can be used to provide a limit of the gas usage of the transaction
    /// across the different gas dimensions. If provided, this quantity will be used along
    /// with the current gas price (`gas_limit *_scalar gas_price`) to compute the transaction fee and compare it to the `max_fee`.
    /// If the scalar product of the gas limit and the gas price is greater than the `max_fee`, the transaction will be rejected.
    /// Then up to `gas_limit *_scalar gas_price` gas tokens can be spent on gas execution in the transaction execution - if the
    /// transaction spends more than that amount, it will run out of gas and be reverted.
    pub gas_limit: Option<S::Gas>,
    /// The ID of the target chain.
    pub chain_id: u64,
}

/// A Transaction object that is compatible with the module-system/sov-default-stf.
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Transaction<S: Spec> {
    pub signature: <S::CryptoSpec as CryptoSpec>::Signature,
    pub pub_key: <S::CryptoSpec as CryptoSpec>::PublicKey,
    pub runtime_msg: Vec<u8>,
    pub nonce: u64,
    pub details: TxDetails<S>,
}

/// Errors that can be raised by the [`Transaction::verify`] method.
#[derive(Error, Debug)]
pub enum TransactionVerificationError<GU: Gas> {
    #[error("Impossible to deserialize transaction: {0}")]
    TransactionDeserializationError(String),
    /// The signature check failed.
    #[error("Signature verification error: {0}")]
    BadSignature(SigVerificationError),
    /// There is not enough gas to verify the signature.
    #[error("A gas error was raised when trying to verify the signature, {0}")]
    GasError(GasMeteringError<GU>),
}

impl<GU: Gas> From<MeteredSigVerificationError<GU>> for TransactionVerificationError<GU> {
    fn from(value: MeteredSigVerificationError<GU>) -> TransactionVerificationError<GU> {
        match value {
            MeteredSigVerificationError::BadSignature(err) => {
                TransactionVerificationError::BadSignature(err)
            }
            MeteredSigVerificationError::GasError(err) => {
                TransactionVerificationError::GasError(err)
            }
        }
    }
}

impl<S: Spec> Transaction<S> {
    pub fn signature(&self) -> &<S::CryptoSpec as CryptoSpec>::Signature {
        &self.signature
    }

    pub fn pub_key(&self) -> &<S::CryptoSpec as CryptoSpec>::PublicKey {
        &self.pub_key
    }

    pub fn runtime_msg(&self) -> &[u8] {
        &self.runtime_msg
    }

    /// Check whether the transaction has been signed correctly.
    #[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
    pub fn verify(
        &self,
        meter: &mut impl GasMeter<S::Gas>,
    ) -> Result<(), TransactionVerificationError<S::Gas>> {
        let serialized_tx = borsh::to_vec(&self.to_unsigned_transaction()).map_err(|e| {
            TransactionVerificationError::TransactionDeserializationError(e.to_string())
        })?;
        MeteredSignature::<S::Gas, _>::new(self.signature.clone())
            .verify(&self.pub_key, &serialized_tx, meter)
            .map_err(TransactionVerificationError::from)?;

        Ok(())
    }

    pub fn new_with_details(
        pub_key: <S::CryptoSpec as CryptoSpec>::PublicKey,
        message: Vec<u8>,
        signature: <S::CryptoSpec as CryptoSpec>::Signature,
        nonce: u64,
        details: TxDetails<S>,
    ) -> Self {
        Self {
            signature,
            runtime_msg: message,
            pub_key,
            nonce,
            details,
        }
    }

    fn to_unsigned_transaction(&self) -> UnsignedTransaction<S> {
        UnsignedTransaction::new_with_details(
            self.runtime_msg.clone(),
            self.nonce,
            self.details.clone(),
        )
    }
}

#[cfg(feature = "native")]
impl<S: Spec> Transaction<S> {
    /// New signed transaction.
    pub fn new_signed_tx(
        priv_key: &<S::CryptoSpec as CryptoSpec>::PrivateKey,
        unsigned_tx: UnsignedTransaction<S>,
    ) -> Self {
        let mut utx_bytes: Vec<u8> = Vec::new();
        BorshSerialize::serialize(&unsigned_tx, &mut utx_bytes).unwrap();

        let pub_key = priv_key.pub_key();
        let signature = priv_key.sign(&utx_bytes);

        unsigned_tx.to_signed_tx(pub_key, signature)
    }
}

/// An unsent transaction with the required data to be submitted to the DA layer
#[derive(Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct UnsignedTransaction<S: Spec> {
    // The runtime message
    runtime_msg: Vec<u8>,
    // The nonce
    nonce: u64,
    // Data related to fees and gas handling.
    details: TxDetails<S>,
}

impl<S: Spec> UnsignedTransaction<S> {
    /// Creates a new [`UnsignedTransaction`] with the given arguments.
    pub const fn new(
        runtime_msg: Vec<u8>,
        chain_id: u64,
        max_priority_fee_bips: PriorityFeeBips,
        max_fee: u64,
        nonce: u64,
        gas_limit: Option<S::Gas>,
    ) -> Self {
        Self {
            runtime_msg,
            nonce,
            details: TxDetails {
                max_priority_fee_bips,
                max_fee,
                gas_limit,
                chain_id,
            },
        }
    }

    pub const fn new_with_details(runtime_msg: Vec<u8>, nonce: u64, details: TxDetails<S>) -> Self {
        Self {
            runtime_msg,
            nonce,
            details,
        }
    }

    /// Creates a new [`Transaction`] from this [`UnsignedTransaction`] when given a signature
    /// and a public key.
    pub fn to_signed_tx(
        self,
        pub_key: <S::CryptoSpec as CryptoSpec>::PublicKey,
        signature: <S::CryptoSpec as CryptoSpec>::Signature,
    ) -> Transaction<S> {
        Transaction::new_with_details(
            pub_key,
            self.runtime_msg,
            signature,
            self.nonce,
            self.details,
        )
    }
}

type RawTxHash = [u8; 32];

impl<S: Spec> From<Transaction<S>> for AuthenticatedTransactionData<S> {
    fn from(tx: Transaction<S>) -> Self {
        Self {
            chain_id: tx.details.chain_id,
            max_priority_fee_bips: tx.details.max_priority_fee_bips,
            max_fee: tx.details.max_fee,
            gas_limit: tx.details.gas_limit,
        }
    }
}

/// Holds the original credentials to authenticate the transaction.
/// For example, this could be a public key of the sender of the transaction.
#[derive(Clone, Debug, Default)]
pub struct Credentials {
    credentials: Rc<BTreeMap<core::any::TypeId, Rc<dyn core::any::Any>>>,
}

impl Credentials {
    /// Creates a new [`Credentials`] from the provided credential.
    pub fn new<T>(credential: T) -> Self
    where
        T: core::any::Any,
    {
        let mut map: BTreeMap<std::any::TypeId, Rc<dyn core::any::Any>> = BTreeMap::new();
        map.insert(core::any::TypeId::of::<T>(), Rc::new(credential));
        Self {
            credentials: Rc::new(map),
        }
    }

    /// Returns the relevant credential.
    pub fn get<T>(&self) -> Option<&T>
    where
        T: core::any::Any,
    {
        self.credentials
            .get(&core::any::TypeId::of::<T>())
            .and_then(|v| v.downcast_ref())
    }
}

/// Transaction data that has been authenticated.
/// This is the output of the `RuntimeAuthenticator`.
pub struct AuthenticatedTransactionData<S: Spec> {
    /// The chain ID.
    pub chain_id: u64,
    /// The maximum priority fee that can be paid for this transaction expressed in bips.
    /// This priority fee is computed as a percentage of the total gas consumed by the transaction
    pub max_priority_fee_bips: PriorityFeeBips,
    /// The maximum fee that can be paid for this transaction expressed as a the gas token amount
    pub max_fee: u64,
    /// The estimated gas usage of the transaction
    pub gas_limit: Option<S::Gas>,
}

impl<S: Spec> AuthenticatedTransactionData<S> {
    /// Creates a new [`TxGasMeter`] from the transaction data.
    pub(crate) fn gas_meter(&self, gas_price: &<S::Gas as Gas>::Price) -> TxGasMeter<S::Gas> {
        // We compute the gas amount that the transaction should consume.
        let gas_to_consume = match &self.gas_limit {
            // If the user has provided a gas limit, we use the `gas_limit * gas_price` as the amount to consume (EIP-1559).
            Some(gas_limit) => {
                // We need to check the gas price in case the user has provided a gas limit.
                gas_limit.value(gas_price)
            }
            // If the user has not provided a gas limit, we use the `max_fee` as the amount to consume.
            None => self.max_fee,
        };

        TxGasMeter {
            remaining_funds: gas_to_consume,
            gas_price: gas_price.clone(),
            gas_used: S::Gas::zero(),
        }
    }
}

pub struct AuthenticatedTransactionAndRawHash<S: Spec> {
    /// Hash of raw bytes.
    pub raw_tx_hash: RawTxHash,
    pub authenticated_tx: AuthenticatedTransactionData<S>,
}

/// A gas meter for transaction execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TxGasMeter<GU>
where
    GU: Gas,
{
    remaining_funds: u64,
    gas_price: GU::Price,
    gas_used: GU,
}

impl<GU> GasMeter<GU> for TxGasMeter<GU>
where
    GU: Gas,
{
    /// Returns the total gas incurred.
    fn gas_used(&self) -> &GU {
        &self.gas_used
    }

    fn refund_gas(&mut self, gas: &GU) -> Result<(), GasMeteringError<GU>> {
        self.gas_used = self.gas_used.checked_sub(gas).ok_or_else(|| {
            GasMeteringError::ImpossibleToRefundGas {
                gas_to_refund: gas.clone(),
                gas_used: self.gas_used.clone(),
            }
        })?;

        self.remaining_funds = self
            .remaining_funds
            .saturating_add(gas.value(&self.gas_price));

        Ok(())
    }

    /// Deducts the provided gas unit from the remaining funds, computing the scalar value of the
    /// funds from the price of the instance.
    fn charge_gas(&mut self, gas: &GU) -> Result<(), GasMeteringError<GU>> {
        // Check that there's enough gas to cover the cost before mutating the gas_used counter.
        // This ensures that in the corner case where...
        //  - User wants to do expensive operation
        //  - User does not have enough gas left
        // ... the check fails and the user does not lose any gas - which is what we want
        // since the operation won't be performed.
        //
        // This also ensures that the `gas_used` stays in sync with the `remaining_funds` counter.
        let funds_to_charge = gas.value(&self.gas_price);
        let remaining_funds = self.remaining_funds;
        self.remaining_funds = remaining_funds
            .checked_sub(funds_to_charge)
            .ok_or_else(|| GasMeteringError::OutOfGas {
                gas_to_charge: gas.clone(),
                gas_price: self.gas_price.clone(),
                remaining_funds: self.remaining_funds,
                total_gas_consumed: self.gas_used.clone(),
            })?;

        self.gas_used.combine(gas);

        Ok(())
    }

    /// Returns the gas price.
    fn gas_price(&self) -> &GU::Price {
        &self.gas_price
    }

    fn remaining_funds(&self) -> u64 {
        self.remaining_funds
    }
}

#[cfg(feature = "test-utils")]
impl<GU> TxGasMeter<GU>
where
    GU: Gas,
{
    /// Returns a gas meter which does not charge for gas.
    pub(crate) fn unmetered() -> Self {
        Self {
            remaining_funds: u64::MAX,
            gas_price: GU::Price::ZEROED,
            gas_used: GU::ZEROED,
        }
    }
}

#[cfg(test)]
impl<GU: Gas> TxGasMeter<GU> {
    pub fn new(remaining_funds: u64, gas_price: GU::Price) -> Self {
        Self {
            remaining_funds,
            gas_price,
            gas_used: GU::ZEROED,
        }
    }
}

/// The format of the resources consumed by the transaction. The base fee and the priority fee are expressed as gas token amounts.
/// The [`TransactionConsumption`] data structure can only be built from the [`crate::WorkingSet`] data structure.
///
/// ## Type safety
/// To build this data structure outside of `sov-modules-api`, one would need to call [`crate::WorkingSet::finalize`] or [`crate::WorkingSet::checkpoint`]
#[derive(PartialEq, Eq, Debug)]
pub struct TransactionConsumption<GU: Gas> {
    /// The amount of funds locked in the transaction that remains after transaction is executed and tip is processed.
    /// This amount includes the `base_fee` and the `priority_fee` gas token consumption
    pub(crate) remaining_funds: u64,
    /// The base fee reward of the transaction expressed as a gas token amount.
    pub(crate) base_fee: GU,
    /// The priority fee reward of the transaction expressed as a gas token amount.
    pub(crate) priority_fee: u64,
    /// The gas price of the transaction.
    pub(crate) gas_price: GU::Price,
}

impl<GU: Gas> TransactionConsumption<GU> {
    /// A zero consumption. Happens when the transaction is ignored (like in the case of a revert for the speculative execution mode).
    pub const ZERO: Self = Self {
        remaining_funds: 0,
        base_fee: GU::ZEROED,
        priority_fee: 0,
        gas_price: GU::Price::ZEROED,
    };

    /// The base fee reward of the transaction expressed as a gas token amount.
    pub const fn base_fee(&self) -> &GU {
        &self.base_fee
    }

    pub fn base_fee_value(&self) -> u64 {
        self.base_fee.value(&self.gas_price)
    }

    /// The priority fee reward of the transaction expressed as a gas token amount.
    pub const fn priority_fee(&self) -> u64 {
        self.priority_fee
    }

    /// If the total consumption overflows, we saturate, because we know that this amount will always be lower than the max fee.
    pub fn total_consumption(&self) -> u64 {
        self.base_fee
            .value(&self.gas_price)
            .saturating_add(self.priority_fee)
    }

    pub fn remaining_funds(&self) -> u64 {
        self.remaining_funds
    }
}

impl<GU: Gas> Display for TransactionConsumption<GU> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TransactionConsumption {{ remaining_funds: {}, base_fee: {}, priority_fee: {}, gas_price: {} }}",
            self.remaining_funds, self.base_fee, self.priority_fee, self.gas_price
        )
    }
}

/// The type used to represent the sequencer reward. This type should be obtained from the [`TransactionConsumption`] type.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct SequencerReward(pub u64);

impl SequencerReward {
    /// Returns a zero sequencer reward. This can be used to initialize an accumulator to build a sequencer reward.
    pub const ZERO: Self = Self(0);

    /// Adds another reward to this reward. Consumes the other reward.
    /// If the result overflows, we saturate.
    pub fn accumulate(&mut self, other: Self) {
        self.0 = self.0.saturating_add(other.0);
    }
}

impl<GU: Gas> From<TransactionConsumption<GU>> for SequencerReward {
    fn from(value: TransactionConsumption<GU>) -> Self {
        Self(value.priority_fee())
    }
}

impl From<SequencerReward> for u64 {
    fn from(val: SequencerReward) -> Self {
        val.0
    }
}

impl Display for SequencerReward {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SequencerReward({})", self.0)
    }
}

/// Computes the transaction consumption for a given transaction.
/// This function is only used by the [`crate::WorkingSet`] to build a [`TransactionConsumption`] at the end of a transaction execution.
pub(crate) fn transaction_consumption_helper<S: Spec>(
    base_fee: &S::Gas,
    gas_price: &<S::Gas as Gas>::Price,
    max_fee: u64,
    max_priority_fee_bips: PriorityFeeBips,
) -> TransactionConsumption<S::Gas> {
    let base_fee_value = base_fee.value(gas_price);

    // We compute the `max_priority_fee_bips` by applying the `priority_fee_per_gas` to the consumed gas.
    let max_priority_fee_bips = max_priority_fee_bips
        .apply(base_fee_value)
        // if the computation overflows, we return the max fee - we always have `priority_fee <= tx.max_priority_fee_bips() <= tx.max_fee()`
        .unwrap_or(max_fee);

    // The tip is the minimum of the remaining gas allocated to the transaction and the maximum priority fee per gas.
    // We transfer the tip to the tip recipient address.
    let tip = min(max_priority_fee_bips, max_fee - base_fee_value);

    // Since the tip is an amount of gas tokens consumed on top of the base fee from the gas meter, we need to take that into
    // account in the calculation.
    let remaining_funds_including_tip = max_fee.saturating_sub(base_fee_value).saturating_sub(tip);

    TransactionConsumption {
        remaining_funds: remaining_funds_including_tip,
        base_fee: base_fee.clone(),
        priority_fee: tip,
        gas_price: gas_price.clone(),
    }
}

pub fn forced_sequencer_registration_cost<S: Spec>() -> S::Gas {
    const GAS_FORCED_SEQUENCER_REGISTRATION_COST: [u64; 2] =
        config_value!("GAS_FORCED_SEQUENCER_REGISTRATION_COST");

    S::Gas::from_slice(&GAS_FORCED_SEQUENCER_REGISTRATION_COST)
}

#[cfg(test)]
mod tests {
    use sov_mock_zkvm::MockZkVerifier;
    use sov_rollup_interface::execution_mode::Native;

    use super::TxGasMeter;
    use crate::default_spec::DefaultSpec;
    use crate::transaction::{
        transaction_consumption_helper, PriorityFeeBips, SequencerReward, TransactionConsumption,
    };
    use crate::{GasArray, GasMeter, GasPrice, GasUnit};

    #[test]
    fn charge_gas_should_fail_if_not_enough_funds() {
        let gas_price = GasPrice::<2>::from_slice(&[1; 2]);

        let mut gas_meter = TxGasMeter::new(0, gas_price.clone());

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

        let mut gas_meter = TxGasMeter::new(100, gas_price.clone());

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

        let mut gas_meter = TxGasMeter::new(REMAINING_FUNDS, gas_price.clone());
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

        let mut gas_meter = TxGasMeter::new(REMAINING_FUNDS, gas_price);
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

    /// Consume all the remaining gas, so the transaction reward is the same as the base fee and there is no priority fee.
    #[test]
    fn test_compute_transaction_reward_consume_all_gas() {
        const REMAINING_FUNDS: u64 = 100;

        let tx_reward =
            transaction_consumption_helper::<DefaultSpec<MockZkVerifier, MockZkVerifier, Native>>(
                &GasArray::from_slice(&[REMAINING_FUNDS / 2; 2]),
                &GasPrice::from_slice(&[1; 2]),
                REMAINING_FUNDS,
                PriorityFeeBips::from_percentage(10),
            );

        assert_eq!(
            tx_reward,
            TransactionConsumption {
                remaining_funds: 0,
                base_fee: GasArray::from_slice(&[REMAINING_FUNDS / 2; 2]),
                priority_fee: 0,
                gas_price: GasPrice::from_slice(&[1; 2])
            }
        );
    }

    /// Consume half of the remaining gas, so the transaction reward is half of the initial funds and there is a maximum priority fee (100%).
    #[test]
    fn test_compute_transaction_reward_consume_not_all_gas() {
        const REMAINING_FUNDS: u64 = 100;

        let tx_reward =
            transaction_consumption_helper::<DefaultSpec<MockZkVerifier, MockZkVerifier, Native>>(
                &GasArray::from_slice(&[REMAINING_FUNDS / 4; 2]),
                &GasPrice::from_slice(&[1; 2]),
                REMAINING_FUNDS,
                PriorityFeeBips::from_percentage(100),
            );

        assert_eq!(
            tx_reward,
            TransactionConsumption {
                remaining_funds: 0,
                base_fee: GasArray::from_slice(&[REMAINING_FUNDS / 4; 2]),
                priority_fee: 50,
                gas_price: GasPrice::from_slice(&[1; 2])
            }
        );
    }

    #[test]
    fn test_display_transaction_reward() {
        let tx_reward = TransactionConsumption::<GasUnit<2>> {
            remaining_funds: 10,
            base_fee: GasUnit::from_slice(&[100; 2]),
            priority_fee: 50,
            gas_price: GasPrice::from_slice(&[1; 2]),
        };

        assert_eq!(
            format!("{}", tx_reward),
            "TransactionConsumption { remaining_funds: 10, base_fee: GasUnit[100, 100], priority_fee: 50, gas_price: GasPrice[1, 1] }"
        );
    }

    #[test]
    fn test_display_sequencer_reward() {
        let seq_reward = SequencerReward(100);

        assert_eq!(format!("{}", seq_reward), "SequencerReward(100)");
    }
}
