#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

/// Contains the call methods used by the module
mod call;
mod gas;
#[cfg(test)]
mod tests;
use sov_modules_api::{
    ModuleId, Spec, StateAccessor, StateReader, StateReaderAndWriter, StateWriter, Zkvm,
};

mod genesis;
pub use genesis::*;
use serde::de::DeserializeOwned;

/// Hook implementation for the module
pub mod hooks;

/// The query interface with the module
#[cfg(feature = "native")]
mod query;
use borsh::{BorshDeserialize, BorshSerialize};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use sov_modules_api::da::Time;
pub use sov_modules_api::hooks::TransitionHeight;
use sov_modules_api::{
    DaSpec, Error, Gas, KernelModule, KernelModuleInfo, KernelWorkingSet, ValidityConditionChecker,
    VersionReader,
};
use sov_state::codec::BcsCodec;
use sov_state::namespaces::Kernel;
use sov_state::{Storage, User};

/// Type alias that contains the height of a given transition
pub type VirtualSlotNumber = u64;

/// A structure that contains block gas information.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BorshSerialize, BorshDeserialize)]
#[serde(bound = "GU: DeserializeOwned")]
pub struct BlockGasInfo<GU: Gas> {
    /// The gas limit of the block execution.
    /// This value is dynamically adjusted over time to account for the increase
    /// in proving/execution performance.
    gas_limit: GU,
    /// The gas used by the block execution.
    /// This value is set to zero at the beginning of the block execution (in the [`ChainState::begin_slot_hook`] hook),
    /// and is populated once the block execution is complete.
    gas_used: GU,
    /// The base fee per gas used for the block execution. This value combined with the `gas_used`
    /// can be used to compute the total base fee (expressed in gas tokens) paid by the block execution.
    base_fee_per_gas: GU::Price,
}

impl<GU: Gas> BlockGasInfo<GU> {
    /// Creates a new [`BlockGasInfo`] with the provided gas limit and base fee per gas.
    /// The `gas_used` is set to zero. This method is meant to be called from the [`ChainState::begin_slot_hook`] hook.
    pub fn new(gas_limit: GU, base_fee_per_gas: GU::Price) -> Self {
        Self {
            gas_limit,
            gas_used: GU::zero(),
            base_fee_per_gas,
        }
    }

    /// Updates the gas used by the block execution.
    /// This method is meant to be called from the [`ChainState::end_slot_hook`] hook.
    pub fn update_gas_used(&mut self, gas_used: GU) {
        self.gas_used = gas_used;
    }

    /// Returns the gas limit of the block execution.
    pub fn gas_limit(&self) -> &GU {
        &self.gas_limit
    }

    /// Returns the gas used by the block execution.
    pub fn gas_used(&self) -> &GU {
        &self.gas_used
    }

    /// Returns the base fee per gas used for the block execution.
    pub fn base_fee_per_gas(&self) -> &GU::Price {
        &self.base_fee_per_gas
    }
}

#[derive(Derivative, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
// We need to use derivative here because `Storage` doesn't implement `Eq` and `PartialEq`
#[derivative(
    PartialEq(bound = "S: Spec, Da: DaSpec"),
    Eq(bound = "S: Spec, Da: DaSpec")
)]
/// Structure that contains the information needed to represent a single state transition.
pub struct StateTransition<S: Spec, Da: DaSpec> {
    slot_hash: Da::SlotHash,
    post_state_root: <S::Storage as Storage>::Root,
    validity_condition: Da::ValidityCondition,
    gas_info: BlockGasInfo<S::Gas>,
}

impl<S: Spec, Da: DaSpec> StateTransition<S, Da> {
    /// Creates a new state transition. Only available for testing as we only want to create
    /// new state transitions from existing [`TransitionInProgress`].
    pub fn new(
        slot_hash: Da::SlotHash,
        post_state_root: <S::Storage as Storage>::Root,
        validity_condition: Da::ValidityCondition,
        gas_info: BlockGasInfo<S::Gas>,
    ) -> Self {
        Self {
            slot_hash,
            post_state_root,
            validity_condition,
            gas_info,
        }
    }
}

impl<S: Spec, Da: DaSpec> StateTransition<S, Da> {
    /// Compare the transition block hash and state root with the provided input couple. If
    /// the pairs are equal, return [`true`].
    pub fn compare_hashes(
        &self,
        slot_hash: &Da::SlotHash,
        post_state_root: &<S::Storage as Storage>::Root,
    ) -> bool {
        self.slot_hash == *slot_hash && self.post_state_root == *post_state_root
    }

    /// Returns the post state root of a state transition
    pub fn post_state_root(&self) -> &<S::Storage as Storage>::Root {
        &self.post_state_root
    }

    /// Returns the slot hash of a state transition
    pub fn slot_hash(&self) -> &Da::SlotHash {
        &self.slot_hash
    }

    /// Returns the total gas used for the block execution
    pub const fn gas_used(&self) -> &S::Gas {
        &self.gas_info.gas_used
    }

    /// Returns the gas price computed for the block execution
    pub const fn gas_price(&self) -> &<S::Gas as Gas>::Price {
        &self.gas_info.base_fee_per_gas
    }

    /// Returns the gas limit of used for the block execution
    pub const fn gas_limit(&self) -> &S::Gas {
        &self.gas_info.gas_limit
    }

    /// Returns the validity condition associated with the transition
    pub fn validity_condition(&self) -> &Da::ValidityCondition {
        &self.validity_condition
    }

    /// Checks the validity condition of a state transition
    pub fn validity_condition_check<Checker: ValidityConditionChecker<Da::ValidityCondition>>(
        &self,
        checker: &mut Checker,
    ) -> Result<(), <Checker as ValidityConditionChecker<Da::ValidityCondition>>::Error> {
        checker.check(&self.validity_condition)
    }
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
/// Represents a transition in progress for the rollup.
pub struct TransitionInProgress<S: Spec, Da: DaSpec> {
    slot_hash: Da::SlotHash,
    validity_condition: Da::ValidityCondition,
    gas_info: BlockGasInfo<S::Gas>,
}

impl<S: Spec, Da: DaSpec> TransitionInProgress<S, Da> {
    /// Creates a new transition in progress
    pub fn new(
        slot_hash: Da::SlotHash,
        validity_condition: Da::ValidityCondition,
        gas_info: BlockGasInfo<S::Gas>,
    ) -> Self {
        Self {
            slot_hash,
            validity_condition,
            gas_info,
        }
    }

    /// Returns the gas price of the transition.
    pub const fn gas_price(&self) -> &<S::Gas as Gas>::Price {
        &self.gas_info.base_fee_per_gas
    }

    /// Returns the total gas used of the transition.
    pub const fn gas_used(&self) -> &S::Gas {
        &self.gas_info.gas_used
    }

    /// Returns the gas limit of the transition.
    pub const fn gas_limit(&self) -> &S::Gas {
        &self.gas_info.gas_limit
    }

    /// Returns the block hash of the transition in progress
    pub const fn block_hash(&self) -> &Da::SlotHash {
        &self.slot_hash
    }
}

/// The chain state module definition. Contains the current state of the da layer.
#[derive(Clone, KernelModuleInfo)]
pub struct ChainState<S: Spec, Da: DaSpec> {
    /// The ID of the module.
    #[id]
    id: ModuleId,

    /// The height that should be loaded as the visible set at the start of the next block
    #[state]
    next_visible_slot_number: sov_modules_api::KernelStateValue<TransitionHeight>,

    /// The real slot number of the rollup.
    /// This value is also required to create a [`sov_state::storage::KernelWorkingSet`]. See note on `visible_height` above.
    #[state]
    true_slot_number: sov_modules_api::KernelStateValue<TransitionHeight>,

    /// The current time, as reported by the DA layer
    #[state]
    time: sov_modules_api::VersionedStateValue<Time>,

    /// A record of all previous state transitions which are available to the VM.
    /// Currently, this includes *all* historical state transitions, but that may change in the future.
    /// This state map is delayed by one transition. In other words - the transition that happens in time i
    /// is stored during transition i+1. This is mainly due to the fact that this structure depends on the
    /// rollup's root hash which is only stored once the transition has completed.
    #[state]
    historical_transitions:
        sov_modules_api::StateMap<TransitionHeight, StateTransition<S, Da>, BcsCodec>,

    /// The transition that is currently processed
    #[state]
    in_progress_transition:
        sov_modules_api::VersionedStateValue<TransitionInProgress<S, Da>, BcsCodec>,

    /// The genesis root hash.
    /// Set after the first transaction of the rollup is executed, using the [`ChainState::begin_slot_hook`] hook.
    // TODO: This should be made read-only
    #[state]
    genesis_root: sov_modules_api::StateValue<<S::Storage as Storage>::Root>,

    /// The height of the first DA block.
    /// Set at the rollup genesis. Since the rollup is always delayed by a constant amount of blocks,
    /// we can use this value with the `true_slot_number` to get the current height of the DA layer,
    /// using the following formula:
    /// `current_da_height = true_slot_number + genesis_da_height`.
    /// Should be the same as the `genesis_height` field in the `RunnerConfig` (`sov-stf-runner` crate)
    #[state]
    genesis_da_height: sov_modules_api::StateValue<TransitionHeight>,

    /// The rollup's code commitment.
    /// This value is initialized at genesis and can be used to verify the rollup's execution.
    /// This value is used by the `AttesterIncentives` module to verify challenges of attestations.
    #[state]
    inner_code_commitment:
        sov_modules_api::StateValue<<S::InnerZkvm as Zkvm>::CodeCommitment, BcsCodec>,

    /// Aggregated code commitment.
    /// This value is initialized at genesis and can be used in the aggregated proving circuit to
    /// verify the rollup execution from genesis to the current slot.
    /// This value is used by the `ProverIncentives` module to verify the proofs posted on the DA layer.
    #[state]
    outer_code_commitment:
        sov_modules_api::StateValue<<S::OuterZkvm as Zkvm>::CodeCommitment, BcsCodec>,
}

impl<S: Spec, Da: DaSpec> ChainState<S, Da> {
    /// Returns transition height in the current slot
    pub fn true_slot_number<T>(
        &self,
        state: &mut T,
    ) -> Result<TransitionHeight, <T as StateReader<Kernel>>::Error>
    where
        T: StateReaderAndWriter<Kernel>,
    {
        Ok(self.true_slot_number.get(state)?.unwrap_or_default())
    }

    /// Returns transition height for the next slot to start execution
    pub fn next_visible_slot_number<T>(
        &self,
        state: &mut T,
    ) -> Result<TransitionHeight, <T as StateReader<Kernel>>::Error>
    where
        T: StateReaderAndWriter<Kernel>,
    {
        Ok(self
            .next_visible_slot_number
            .get(state)?
            .unwrap_or_default())
    }

    /// Returns transition height in the current slot
    pub fn set_next_visible_slot_number<T>(
        &self,
        value: &u64,
        state: &mut T,
    ) -> Result<(), T::Error>
    where
        T: StateWriter<Kernel>,
    {
        tracing::debug!(slot_number = value, "Setting next visible slot number");
        self.next_visible_slot_number.set(value, state)
    }

    /// Returns the current time, as reported by the DA layer
    pub fn get_time<Reader: VersionReader>(
        &self,
        state: &mut Reader,
    ) -> Result<Time, <Reader as StateReader<Kernel>>::Error> {
        Ok(self
            .time
            .get_current(state)?
            .expect("Time must be set at initialization"))
    }

    /// Return the genesis hash of the module.
    pub fn get_genesis_hash<Accessor: StateAccessor>(
        &self,
        state: &mut Accessor,
    ) -> Result<Option<<S::Storage as Storage>::Root>, <Accessor as StateReader<User>>::Error> {
        self.genesis_root.get(state)
    }

    /// Return the code commitment to be used for verifying the rollup's execution
    /// for each state transition.
    pub fn inner_code_commitment<Accessor: StateAccessor>(
        &self,
        state: &mut Accessor,
    ) -> Result<
        Option<<S::InnerZkvm as Zkvm>::CodeCommitment>,
        <Accessor as StateReader<User>>::Error,
    > {
        self.inner_code_commitment.get(state)
    }

    /// Return the code commitment to be used for verifying the rollup's execution from genesis to the current slot
    /// in the aggregated proving circuit.
    pub fn outer_code_commitment<Accessor: StateAccessor>(
        &self,
        state: &mut Accessor,
    ) -> Result<
        Option<<S::OuterZkvm as Zkvm>::CodeCommitment>,
        <Accessor as StateReader<User>>::Error,
    > {
        self.outer_code_commitment.get(state)
    }

    /// Return the initial height of the DA layer.
    pub fn genesis_da_height<Accessor: StateAccessor>(
        &self,
        state: &mut Accessor,
    ) -> Result<Option<TransitionHeight>, <Accessor as StateReader<User>>::Error> {
        self.genesis_da_height.get(state)
    }

    /// Returns the transition in progress of the module.
    pub fn get_in_progress_transition<Reader: VersionReader>(
        &self,
        state: &mut Reader,
    ) -> Result<Option<TransitionInProgress<S, Da>>, <Reader as StateReader<Kernel>>::Error> {
        self.in_progress_transition.get_current(state)
    }

    /// Returns the completed transition associated with the provided `transition_num`.
    pub fn get_historical_transitions<Accessor: StateAccessor>(
        &self,
        transition_num: TransitionHeight,
        state: &mut Accessor,
    ) -> Result<Option<StateTransition<S, Da>>, <Accessor as StateReader<User>>::Error> {
        self.historical_transitions.get(&transition_num, state)
    }
}

impl<S: Spec, Da: DaSpec> KernelModule for ChainState<S, Da> {
    type Spec = S;

    type Config = ChainStateConfig<S>;

    fn genesis_unchecked(
        &self,
        config: &Self::Config,
        state: &mut KernelWorkingSet<S>,
    ) -> Result<(), Error> {
        // The initialization logic
        Ok(self.init_module(config, state)?)
    }
}
