//! Prometheus metrics for Sovereign rollups.

#![deny(missing_docs)]

use std::sync::OnceLock;

use prometheus::{
    register_int_counter_with_registry, register_int_gauge_with_registry, IntCounter, IntGauge,
};

/// Applies a function to the global [`Metrics`] instance if and only if the
/// `native` feature is enabled.
pub fn update_metrics<F>(f: F)
where
    F: FnOnce(&Metrics),
{
    if cfg!(feature = "native") {
        static METRICS: OnceLock<Metrics> = OnceLock::new();

        f(OnceLock::get_or_init(&METRICS, || {
            Metrics::new(prometheus::default_registry())
                .expect("failed to create new metrics; this is a bug in the Sovereign SDK")
        }));
    }
}

/// Prometheus metrics for Sovereign rollups.
///
/// Values of this type are only accessible through the [`update_metrics`] function.
#[derive(Debug)]
pub struct Metrics {
    /// Number of DA blocks processed.
    pub da_blocks_processed: IntCounter,
    /// Number of rollup batches processed.
    pub rollup_batches_processed: IntCounter,
    /// Number of rollup transactions processed.
    pub rollup_txns_processed: IntCounter,
    /// Number of rollup transactions per DA block.
    pub rollup_txns_per_da_block: IntGauge,
    /// Current DA height for the rollup.
    pub current_da_height: IntGauge,
}

impl Metrics {
    fn new(registry: &prometheus::Registry) -> prometheus::Result<Self> {
        let da_blocks_processed = register_int_counter_with_registry!(
            "da_blocks_processed",
            "Number of DA blocks processed",
            registry,
        )?;

        let rollup_batches_processed = register_int_counter_with_registry!(
            "rollup_batches_processed",
            "Number of rollup batches processed",
            registry,
        )?;

        let rollup_txns_processed = register_int_counter_with_registry!(
            "rollup_txns_processed",
            "Number of rollup transactions processed",
            registry,
        )?;

        let rollup_txns_per_da_block = register_int_gauge_with_registry!(
            "rollup_txns_per_da_block",
            "Number of rollup transactions per DA block",
            registry,
        )?;

        let current_da_height = register_int_gauge_with_registry!(
            "current_da_height",
            "Current DA height for the rollup",
            registry,
        )?;

        Ok(Self {
            da_blocks_processed,
            rollup_batches_processed,
            rollup_txns_processed,
            rollup_txns_per_da_block,
            current_da_height,
        })
    }
}
