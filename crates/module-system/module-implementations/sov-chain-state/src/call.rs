use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{DaSpec, KernelWorkingSet, Spec};

use crate::ChainState;

impl<S, Da> ChainState<S, Da>
where
    S: Spec,
    Da: DaSpec,
{
    /// Increment the current slot number
    /// This function also modifies the kernel working set to update the true height.
    pub(crate) fn increment_true_slot_number(&self, state: &mut KernelWorkingSet<S>) {
        let current_height = self
            .true_slot_number
            .get(state)
            .unwrap_infallible()
            .unwrap_or_default();
        let new_height = current_height.saturating_add(1);
        self.true_slot_number
            .set(&(new_height), state)
            .unwrap_infallible();

        state.update_true_slot_number(new_height);
    }
}
