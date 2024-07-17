use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sov_modules_api::{DaSpec, GenesisState};

use crate::{Amount, ProverIncentives};

/// Configuration of the prover incentives module. Specifies the minimum bond, the commitment to
/// the allowed verifier method and a set of initial provers with their
/// bonding amount.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(
        bound = "S: ::sov_modules_api::Spec",
        rename = "ProverIncentivesConfig"
    )
)]
#[serde(bound = "S::Address: Serialize + DeserializeOwned")]
pub struct ProverIncentivesConfig<S: sov_modules_api::Spec> {
    /// A penalty for provers who submit a proof for transitions that were already proven
    pub proving_penalty: Amount,
    /// The minimum bond for a prover.
    pub minimum_bond: u64,
    /// A list of initial provers and their bonded amount.
    pub initial_provers: Vec<(S::Address, u64)>,
}

impl<S: sov_modules_api::Spec, Da: DaSpec> ProverIncentives<S, Da> {
    /// Init the [`ProverIncentives`] module using the provided `config`.
    /// Sets the minimum amount necessary to bond, the commitment to the verifier circuit
    /// the bonding token ID and builds the set of initial provers.
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::Module>::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<()> {
        anyhow::ensure!(
            !config.initial_provers.is_empty(),
            "At least one prover must be set at genesis!"
        );

        self.minimum_bond.set(&config.minimum_bond, state)?;
        self.proving_penalty.set(&config.proving_penalty, state)?;
        self.last_claimed_reward.set(&0, state)?;

        for (prover, bond) in config.initial_provers.iter() {
            self.bond_prover_helper(*bond, prover, state)?;
        }

        Ok(())
    }
}
