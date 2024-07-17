use anyhow::Result;
use serde::{Deserialize, Serialize};
use sov_modules_api::da::Time;
use sov_modules_api::hooks::TransitionHeight;
use sov_modules_api::{KernelWorkingSet, Zkvm};

use crate::ChainState;

/// Initial configuration of the chain state
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ChainStateConfig<S: sov_modules_api::Spec> {
    /// The time at `genesis_da_height` slot according to the DA layer.
    /// So the format depends on DA layer time representation.
    /// Most probably is used for bridging purposes.
    pub current_time: Time,

    /// The code commitment to be used for verifying the rollup's execution.
    pub inner_code_commitment: <S::InnerZkvm as Zkvm>::CodeCommitment,

    /// The code commitment to be used for verifying the rollup's execution from genesis to the current slot.
    /// This value is used by the `ProverIncentives` module to verify the proofs posted on the DA layer.
    pub outer_code_commitment: <S::OuterZkvm as Zkvm>::CodeCommitment,

    /// The height of the first DA block.
    pub genesis_da_height: TransitionHeight,
}

impl<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> ChainState<S, Da> {
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::KernelModule>::Config,
        state: &mut KernelWorkingSet<S>,
    ) -> Result<()> {
        tracing::info!(
            current_time = ?config.current_time,
            genesis_da_height = config.genesis_da_height,
            inner_code_commitment = ?config.inner_code_commitment,
            outer_code_commitment = ?config.outer_code_commitment,
            "Starting chain state genesis...",
        );
        self.true_slot_number.set(&0, state)?;
        self.next_visible_slot_number.set(&1, state)?;

        self.time.set_true_current(&config.current_time, state);

        self.inner_code_commitment
            .set(&config.inner_code_commitment, state)?;

        self.outer_code_commitment
            .set(&config.outer_code_commitment, state)?;

        self.genesis_da_height
            .set(&config.genesis_da_height, state)?;

        Ok(())
    }
}
