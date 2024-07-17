#![deny(missing_docs)]
//! The rollup capabilities module defines "capabilities" that rollup must
//! provide if they wish to use the standard app template.
//! If you don't want to provide these capabilities,
//! you can bypass the Sovereign module-system completely
//! and write a state transition function from scratch.
//! [See here for docs](https://github.com/Sovereign-Labs/sovereign-sdk/blob/nightly/examples/demo-stf/README.md)
pub mod auth;
mod batch_selector;
mod kernel;
mod proof;
pub use auth::*;
pub use batch_selector::*;
pub use kernel::*;
mod gas;
pub use gas::*;
pub use proof::ProofProcessor;
use sov_rollup_interface::da::DaSpec;
mod sequencer;
pub use sequencer::*;

use crate::{GasMeter, Spec};

/// Indicates that a type provides the necessary capabilities for a runtime.
pub trait HasCapabilities<S: Spec, Da: DaSpec> {
    /// The concrete implementation of the capabilities.
    type Capabilities<'a>: GasEnforcer<S, Da>
        + SequencerAuthorization<S, Da, SequencerStakeMeter = Self::SequencerStakeMeter>
        + RuntimeAuthorization<
            S,
            Da,
            SequencerStakeMeter = Self::SequencerStakeMeter,
            AuthorizationData = Self::AuthorizationData,
        > + ProofProcessor<S, Da>
    where
        Self: 'a;

    /// The type used to meter gas for operations invoked by the sequencer
    /// (e.g. transaction deserialization, failing nonce checks)
    // Note: We require an extra associated type here because `Capabilities` has
    // a lifetime and rustc isn't smart enough to know that he lifetime of `SequencerAuthorization::SequencerStakeMeter`
    // doesn't depend on the lifetime of capabilities.
    type SequencerStakeMeter: GasMeter<S::Gas>;

    /// The type that is passed to the authorizer.
    type AuthorizationData;

    /// Fetches the capabilities from the runtime.
    fn capabilities(&self) -> Self::Capabilities<'_>;
}

#[cfg(feature = "test-utils")]
pub mod mocks {
    //! Mocks for the rollup capabilities module

    use sov_rollup_interface::da::DaSpec;

    use super::{BlobSelector, Kernel, Spec};
    use crate::{BootstrapWorkingSet, KernelWorkingSet, StateCheckpoint};

    /// A mock kernel for use in tests
    #[derive(Debug, Clone, Default)]
    pub struct MockKernel<S, Da> {
        /// The current slot number
        pub true_slot_number: u64,
        /// The slot number at which transactions appear to be executing
        pub visible_slot_number: u64,
        phantom: core::marker::PhantomData<(S, Da)>,
    }

    impl<S: Spec, Da: DaSpec> MockKernel<S, Da> {
        /// Create a new mock kernel with the given slot number
        pub fn new(true_slot_number: u64, visible_height: u64) -> Self {
            Self {
                true_slot_number,
                visible_slot_number: visible_height,
                phantom: core::marker::PhantomData,
            }
        }

        /// The genesis working set
        pub fn genesis_ws(state_checkpoint: &mut StateCheckpoint<S>) -> KernelWorkingSet<'_, S> {
            let kernel = Self::new(0, 0);
            KernelWorkingSet::from_kernel(&kernel, state_checkpoint)
        }
    }

    impl<S: Spec, Da: DaSpec> Kernel<S, Da> for MockKernel<S, Da> {
        fn true_slot_number(&self, _ws: &mut BootstrapWorkingSet<'_, S>) -> u64 {
            self.true_slot_number
        }
        fn visible_slot_number(&self, _ws: &mut BootstrapWorkingSet<'_, S>) -> u64 {
            self.visible_slot_number
        }

        type GenesisConfig = ();

        #[cfg(feature = "native")]
        type GenesisPaths = ();

        fn genesis(
            &self,
            _config: &Self::GenesisConfig,
            _state: &mut KernelWorkingSet<'_, S>,
        ) -> Result<(), anyhow::Error> {
            Ok(())
        }
    }

    impl<S: Spec, Da: DaSpec> BlobSelector<Da> for MockKernel<S, Da> {
        type Spec = S;

        type BlobType = Da::BlobTransaction;

        fn get_blobs_for_this_slot<'a, 'k, I>(
            &self,
            _current_blobs: I,
            _state: &mut crate::KernelWorkingSet<'k, Self::Spec>,
        ) -> anyhow::Result<Vec<(Self::BlobType, Da::Address)>>
        where
            I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
        {
            // Ok(current_blobs
            //     .into_iter()
            //     .map(|blob| {
            //         blob.full_data();
            //         blob.clone()
            //     })
            //     .collect())
            todo!()
        }
    }
}
