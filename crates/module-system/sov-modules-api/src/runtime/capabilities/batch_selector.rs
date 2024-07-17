use sov_rollup_interface::da::DaSpec;

use crate::{KernelWorkingSet, Spec};

/// BlobSelector decides which blobs to process in a current slot.
pub trait BlobSelector<Da: DaSpec> {
    /// Spec type
    type Spec: Spec;

    /// The type of batch returned by the selector
    type BlobType;

    /// Returns a vector of blobs that should be processed in the current slot.
    fn get_blobs_for_this_slot<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut KernelWorkingSet<'k, Self::Spec>,
    ) -> anyhow::Result<Vec<(Self::BlobType, Da::Address)>>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>;
}
