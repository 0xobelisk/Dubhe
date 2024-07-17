use core::marker::PhantomData;

use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sov_bank::Amount;
use sov_modules_api::hooks::TransitionHeight;
use sov_modules_api::{DaSpec, GenesisState, Spec};
use sov_state::Storage;

use crate::{AttesterIncentives, Role};

/// Configuration of the attester incentives module
#[derive(Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct AttesterIncentivesConfig<S, Da>
where
    S: Spec,
    Da: DaSpec,
{
    /// The minimum bond for an attester.
    pub minimum_attester_bond: Amount,
    /// The minimum bond for a challenger.
    pub minimum_challenger_bond: Amount,
    /// A list of initial attesters and their bonded amount.
    pub initial_attesters: Vec<(S::Address, Amount)>,
    /// The finality period of the rollup (constant) in the number of DA layer slots processed.
    pub rollup_finality_period: TransitionHeight,
    /// The current maximum attested height
    pub maximum_attested_height: TransitionHeight,
    /// The light client finalized height
    pub light_client_finalized_height: TransitionHeight,
    /// Phantom data that contains the validity condition
    pub phantom_data: PhantomData<Da::ValidityCondition>,
}

impl<S, Store, P, Da> AttesterIncentives<S, Da>
where
    S: sov_modules_api::Spec<Storage = Store>,
    Store: Storage<Proof = P>,
    P: BorshDeserialize + BorshSerialize,
    Da: sov_modules_api::DaSpec,
{
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::Module>::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<()> {
        anyhow::ensure!(
            !config.initial_attesters.is_empty(),
            "At least one prover must be set at genesis!"
        );

        self.minimum_attester_bond
            .set(&config.minimum_attester_bond, state)?;
        self.minimum_challenger_bond
            .set(&config.minimum_challenger_bond, state)?;

        self.rollup_finality_period
            .set(&config.rollup_finality_period, state)?;

        for (attester, bond) in config.initial_attesters.iter() {
            self.bond_user_helper(*bond, attester, Role::Attester, state)?;
        }

        self.maximum_attested_height
            .set(&config.maximum_attested_height, state)?;

        self.light_client_finalized_height
            .set(&config.light_client_finalized_height, state)?;

        Ok(())
    }
}
