#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
mod call;
mod event;
mod genesis;

#[cfg(test)]
mod tests;

#[cfg(feature = "native")]
mod query;

pub use call::*;
pub use genesis::*;
/// The response type used by RPC queries.
#[cfg(feature = "native")]
pub use query::*;
use sov_bank::Amount;
use sov_modules_api::hooks::TransitionHeight;
use sov_modules_api::{Context, DaSpec, Error, GenesisState, ModuleId, ModuleInfo, Spec, TxState};

use crate::event::Event;

/// A new module:
/// - Must derive `ModuleInfo`
/// - Must contain `[address]` field
/// - Can contain any number of ` #[state]` or `[module]` fields
#[derive(Clone, ModuleInfo, sov_modules_api::macros::ModuleRestApi)]
pub struct ProverIncentives<S: Spec, Da: DaSpec> {
    /// Id of the module.
    #[id]
    pub id: ModuleId,

    /// The set of registered provers and their bonded amount.
    #[state]
    pub bonded_provers: sov_modules_api::StateMap<S::Address, Amount>,

    /// The minimum bond for a prover to be eligible for onchain verification
    /// TODO(@theochap) `<https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/360>`: This bond should be express in gas units.
    #[state]
    #[rest_api(include)]
    pub minimum_bond: sov_modules_api::StateValue<Amount>,

    /// The highest slot height for which the reward has been claimed. The next proofs should claim the next slot height.
    #[state]
    pub last_claimed_reward: sov_modules_api::StateValue<TransitionHeight>,

    /// A penalty for provers who submit a proof for transitions that were already proven
    /// TODO(@theochap) `<https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/360>`: This should be express in gas units.
    #[state]
    pub proving_penalty: sov_modules_api::StateValue<Amount>,

    /// Reference to the Bank module.
    #[module]
    pub(crate) bank: sov_bank::Bank<S>,

    /// Reference to the Chain state module. Used to check the proof inputs
    #[kernel_module]
    pub(crate) chain_state: sov_chain_state::ChainState<S, Da>,
}

impl<S: Spec, Da: DaSpec> sov_modules_api::Module for ProverIncentives<S, Da> {
    type Spec = S;

    type Config = ProverIncentivesConfig<S>;

    type CallMessage = call::CallMessage;

    type Event = Event<S>;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        // The initialization logic
        Ok(self.init_module(config, state)?)
    }

    fn call(
        &self,
        msg: Self::CallMessage,
        context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,
    ) -> Result<sov_modules_api::CallResponse, Error> {
        match msg {
            call::CallMessage::BondProver(bond_amount) => {
                self.bond_prover(bond_amount, context.sender(), state)
            }
            call::CallMessage::UnbondProver => self.unbond_prover(context.sender(), state),
            call::CallMessage::VerifyProof(proof) => {
                self.process_proof(&proof, context.sender(), state)
            }
        }
        .map_err(|e| Error::ModuleError(e.into()))
    }
}
