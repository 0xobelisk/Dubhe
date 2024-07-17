use sov_rollup_interface::da::DaSpec;
use sov_state::Storage;

use super::BlobSelector;
use crate::{BootstrapWorkingSet, Gas, KernelWorkingSet, Spec, StateCheckpoint};

/// The kernel is responsible for managing the inputs to the `apply_blob` method.
/// A simple implementation will simply process all blobs in the order that they appear,
/// while a second will support a "preferred sequencer" with some limited power to reorder blobs
/// in order to give out soft confirmations.
pub trait Kernel<S: Spec, Da: DaSpec>: BlobSelector<Da, Spec = S> + Default + Sync + Send {
    /// GenesisConfig type.
    type GenesisConfig: Send + Sync;

    #[cfg(feature = "native")]
    /// GenesisPaths type.
    type GenesisPaths: Send + Sync;

    /// Initialize the kernel at genesis
    fn genesis(
        &self,
        config: &Self::GenesisConfig,
        state: &mut KernelWorkingSet<'_, S>,
    ) -> Result<(), anyhow::Error>;

    /// Return the current slot number
    fn true_slot_number(&self, state: &mut BootstrapWorkingSet<'_, S>) -> u64;
    /// Return the slot number at which transactions currently *appear* to be executing.
    fn visible_slot_number(&self, state: &mut BootstrapWorkingSet<'_, S>) -> u64;
}

/// Hooks allowing the kernel to get access to the DA layer state
pub trait KernelSlotHooks<S: Spec, Da: DaSpec>: Kernel<S, Da> {
    /// Called at the beginning of a slot. Computes the gas price for the slot
    fn begin_slot_hook(
        &self,
        slot_header: &Da::BlockHeader,
        validity_condition: &Da::ValidityCondition,
        pre_state_root: &<<Self::Spec as Spec>::Storage as Storage>::Root,
        state: &mut StateCheckpoint<Self::Spec>,
    ) -> <S::Gas as Gas>::Price;
    /// Called at the end of a slot
    fn end_slot_hook(&self, gas_used: &S::Gas, state: &mut StateCheckpoint<Self::Spec>);
}
