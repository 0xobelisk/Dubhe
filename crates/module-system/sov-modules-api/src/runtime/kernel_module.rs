//! Traits to allow modular development of kernels. These traits are closely related but to the traits
//! for normal modules.

use crate::{KernelWorkingSet, ModuleError, Spec};

/// All the methods have a default implementation that can't be invoked (because they take `NonInstantiable` parameter).
/// This allows developers to override only some of the methods in their implementation and safely ignore the others.
pub trait KernelModule {
    /// Execution context.
    type Spec: Spec;

    /// Configuration for the genesis method.
    type Config;

    /// # Warning
    /// This function runs *before* the runtime containing normal modules is initialized.
    /// If you try to read or write a value from a normal module during genesis, you might encounter
    /// unexpected behavior!
    ///
    /// This function is called once, when the rollup is created. It initializes the state of the kernel without
    /// checking that any "normal" modules that this kernel module may depend on have been initialized
    fn genesis_unchecked(
        &self,
        _config: &Self::Config,
        _state: &mut KernelWorkingSet<Self::Spec>,
    ) -> Result<(), ModuleError> {
        Ok(())
    }
}
