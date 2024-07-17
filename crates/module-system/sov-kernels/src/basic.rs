//! The basic kernel provides censorship resistance by processing all blobs immediately in the order they appear on DA
use std::path::PathBuf;

use sov_blob_storage::BlobStorage;
use sov_chain_state::ChainState;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::runtime::capabilities::{BlobSelector, Kernel, KernelSlotHooks};
use sov_modules_api::{
    BlobDataWithId, BootstrapWorkingSet, DaSpec, Gas, KernelModule, KernelWorkingSet, Spec,
};
use sov_state::Storage;

/// The simplest imaginable kernel. It does not do any batching or reordering of blobs.
#[derive(Clone)]
pub struct BasicKernel<S: Spec, Da: DaSpec> {
    pub(crate) chain_state: ChainState<S, Da>,
    pub(crate) blob_storage: BlobStorage<S, Da>,
}

impl<S: Spec, Da: DaSpec> BasicKernel<S, Da> {
    /// Gets a reference to the kernel's ChainState module.
    pub fn chain_state(&self) -> &sov_chain_state::ChainState<S, Da> {
        &self.chain_state
    }

    /// Gets a reference to the kernel's BlobStorage module.
    pub fn blob_storage(&self) -> &sov_blob_storage::BlobStorage<S, Da> {
        &self.blob_storage
    }
}

impl<S: Spec, Da: DaSpec> Default for BasicKernel<S, Da> {
    fn default() -> Self {
        Self {
            chain_state: Default::default(),
            blob_storage: Default::default(),
        }
    }
}

/// Path information required to initialize a basic kernel from files
pub struct BasicKernelGenesisPaths {
    /// The path to the chain_state genesis config
    pub chain_state: PathBuf,
}

/// The genesis configuration for the basic kernel
pub struct BasicKernelGenesisConfig<S: Spec, Da: DaSpec> {
    /// The chain state genesis config
    pub chain_state: <ChainState<S, Da> as KernelModule>::Config,
}

impl<S: Spec, Da: DaSpec> Kernel<S, Da> for BasicKernel<S, Da> {
    type GenesisConfig = BasicKernelGenesisConfig<S, Da>;

    #[cfg(feature = "native")]
    type GenesisPaths = BasicKernelGenesisPaths;

    fn true_slot_number(&self, state: &mut BootstrapWorkingSet<'_, S>) -> u64 {
        self.chain_state.true_slot_number(state).unwrap_infallible()
    }
    fn visible_slot_number(&self, state: &mut BootstrapWorkingSet<'_, S>) -> u64 {
        self.chain_state.true_slot_number(state).unwrap_infallible()
    }

    fn genesis(
        &self,
        config: &Self::GenesisConfig,
        state: &mut KernelWorkingSet<'_, S>,
    ) -> Result<(), anyhow::Error> {
        self.chain_state
            .genesis_unchecked(&config.chain_state, state)?;
        self.blob_storage.genesis_unchecked(&(), state)?;
        Ok(())
    }
}

impl<S: Spec, Da: DaSpec> BlobSelector<Da> for BasicKernel<S, Da> {
    type Spec = S;

    type BlobType = BlobDataWithId;

    fn get_blobs_for_this_slot<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut sov_modules_api::KernelWorkingSet<'k, Self::Spec>,
    ) -> anyhow::Result<Vec<(Self::BlobType, Da::Address)>>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        Ok(self
            .blob_storage
            .select_blobs_as_based_sequencer(current_blobs, state))
    }
}

impl<S: Spec, Da: DaSpec> KernelSlotHooks<S, Da> for BasicKernel<S, Da> {
    fn begin_slot_hook(
        &self,
        slot_header: &<Da as DaSpec>::BlockHeader,
        validity_condition: &<Da as DaSpec>::ValidityCondition,
        pre_state_root: &<<Self::Spec as sov_modules_api::Spec>::Storage as Storage>::Root,
        state_checkpoint: &mut sov_modules_api::StateCheckpoint<Self::Spec>,
    ) -> <S::Gas as Gas>::Price {
        let mut ws = sov_modules_api::KernelWorkingSet::from_kernel(self, state_checkpoint);
        self.chain_state
            .begin_slot_hook(slot_header, validity_condition, pre_state_root, &mut ws)
    }

    fn end_slot_hook(
        &self,
        gas_used: &S::Gas,
        state_checkpoint: &mut sov_modules_api::StateCheckpoint<Self::Spec>,
    ) {
        let mut ws = sov_modules_api::KernelWorkingSet::from_kernel(self, state_checkpoint);
        self.chain_state.end_slot_hook(gas_used, &mut ws);
    }
}
