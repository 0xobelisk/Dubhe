//! Implementation of [`Fee`] trait for both or the [`crate::MockDaService`] and [`crate::storable::service::StorableMockDaService`].
use sov_rollup_interface::services::da::Fee;

/// A fee implementation for the [`crate::MockDaService`] and [`crate::storable::service::StorableMockDaService`].
/// Fees are currently unused.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct MockFee(u64);

impl MockFee {
    /// Creates a new [`MockFee`] with the given rate.
    pub const fn new(rate: u64) -> Self {
        Self(rate)
    }

    /// Creates a new [`MockFee`] with the zero rate.
    pub const fn zero() -> Self {
        Self(0)
    }
}

impl Fee for MockFee {
    type FeeRate = u64;

    fn fee_rate(&self) -> Self::FeeRate {
        self.0
    }

    fn set_fee_rate(&mut self, rate: Self::FeeRate) {
        self.0 = rate;
    }

    fn gas_estimate(&self) -> u64 {
        1
    }
}
