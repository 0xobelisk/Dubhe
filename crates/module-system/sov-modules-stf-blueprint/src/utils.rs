use sov_modules_api::runtime::capabilities::KernelSlotHooks;
use sov_modules_api::{DaSpec, Spec};

use crate::{Runtime, StfBlueprint};

impl<S: Spec, Da: DaSpec, RT: Runtime<S, Da>, K: KernelSlotHooks<S, Da>>
    StfBlueprint<S, Da, RT, K>
{
    /// Returns the underlying kernel.
    pub fn kernel(&self) -> &K {
        &self.kernel
    }

    /// Returns the underlying runtime.
    pub fn runtime(&self) -> &RT {
        &self.runtime
    }
}
