use anyhow::Result;
use serde::{Deserialize, Serialize};
use sov_bank::Amount;
use sov_modules_api::GenesisState;

use crate::SequencerRegistry;

/// Genesis configuration for the [`SequencerRegistry`] module.
///
/// This `struct` must be passed as an argument to
/// [`Module::genesis`](sov_modules_api::Module::genesis).
///
// TODO: Allow multiple sequencers: https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/278
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(
        bound = "S: ::sov_modules_api::Spec, Da::Address: ::schemars::JsonSchema",
        rename = "SequencerConfig"
    )
)]
#[serde(bound = "S::Address: serde::Serialize + serde::de::DeserializeOwned")]
pub struct SequencerConfig<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> {
    /// The rollup address of the sequencer.
    pub seq_rollup_address: S::Address,
    /// The Data Availability (DA) address of the sequencer.
    pub seq_da_address: Da::Address,
    /// The minimum bond required for a sequencer to send transactions.
    pub minimum_bond: Amount,
    /// Determines whether this sequencer is *regular* or *preferred*.
    ///
    /// Batches from the preferred sequencer are always processed first in
    /// block, which means the preferred sequencer can guarantee soft
    /// confirmation time for transactions.
    pub is_preferred_sequencer: bool,
}

impl<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> SequencerRegistry<S, Da> {
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::Module>::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<()> {
        tracing::info!(
            sequencer_rollup_address = %config.seq_rollup_address,
            sequencer_da_address = %config.seq_da_address,
            is_preferred_sequencer = config.is_preferred_sequencer,
            minimum_bond = config.minimum_bond,
            "Starting sequencer registry genesis..."
        );
        self.minimum_bond.set(&config.minimum_bond, state)?;

        self.register_sequencer(
            &config.seq_da_address,
            &config.seq_rollup_address,
            config.minimum_bond,
            state,
        )?;

        if config.is_preferred_sequencer {
            self.preferred_sequencer
                .set(&config.seq_da_address, state)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sov_mock_da::{MockAddress, MockDaSpec};
    use sov_modules_api::prelude::*;
    use sov_modules_api::AddressBech32;
    use sov_test_utils::TestSpec;

    use crate::SequencerConfig;

    #[test]
    fn test_config_serialization() {
        let seq_rollup_address: <TestSpec as Spec>::Address = AddressBech32::from_str(
            "sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94",
        )
        .unwrap()
        .into();

        let seq_da_addreess = MockAddress::from_str(
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();

        let config = SequencerConfig::<TestSpec, MockDaSpec> {
            seq_rollup_address,
            seq_da_address: seq_da_addreess,
            minimum_bond: 50,
            is_preferred_sequencer: true,
        };

        let data = r#"
        {
            "seq_rollup_address":"sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94",
            "seq_da_address":"0000000000000000000000000000000000000000000000000000000000000000",
            "minimum_bond":50,
            "is_preferred_sequencer":true
        }"#;

        let parsed_config: SequencerConfig<TestSpec, MockDaSpec> =
            serde_json::from_str(data).unwrap();
        assert_eq!(config, parsed_config);
    }
}
