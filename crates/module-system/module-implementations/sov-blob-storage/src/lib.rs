#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
mod capabilities;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sov_chain_state::TransitionHeight;
use sov_modules_api::macros::config_value;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{
    BlobData, BlobDataWithId, KernelModule, KernelModuleInfo, KernelStateValue, KernelWorkingSet,
    ModuleId, StateCheckpoint, StateMap,
};
use sov_state::codec::BcsCodec;

/// For how many slots deferred blobs are stored before being executed
pub const DEFERRED_SLOTS_COUNT: u64 = config_value!("DEFERRED_SLOTS_COUNT");

/// How many blobs from unregistered sequencers we will accept per slot
/// We can't slash misbehaving senders because they aren't a registered sequencer with a stake so
/// this serves as protection against spam.
pub const UNREGISTERED_BLOBS_PER_SLOT: u64 = config_value!("UNREGISTERED_BLOBS_PER_SLOT");

/// The sequence number for a batch from the preferred sequencer.   
pub type SequenceNumber = u64;

/// Blob storage contains only address and vector of blobs
#[derive(Clone, KernelModuleInfo)]
pub struct BlobStorage<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> {
    /// The ID of blob storage module
    #[id]
    pub(crate) id: ModuleId,

    /// Actual storage of blobs
    /// DA block number => vector of blobs
    /// Caller controls the order of blobs in the vector
    #[state]
    pub(crate) deferred_blobs: StateMap<u64, Vec<(BlobDataWithId, Da::Address)>, BcsCodec>,

    /// Any preferred sequencer blobs which were received out of order. Mapped from sequence number to batch.
    #[state]
    pub(crate) deferred_preferred_sequencer_blobs:
        StateMap<SequenceNumber, PreferredBlobDataWithId>,

    /// The next sequence number for the preferred sequencer. This is used to determine if a batch is out of order.
    #[state]
    next_sequence_number: KernelStateValue<SequenceNumber>,

    #[module]
    pub(crate) sequencer_registry: sov_sequencer_registry::SequencerRegistry<S, Da>,

    #[kernel_module]
    chain_state: sov_chain_state::ChainState<S, Da>,
}

/// Non standard methods for blob storage
impl<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> BlobStorage<S, Da> {
    /// Store blobs for given block number, overwrite if already exists
    pub fn store_batches(
        &self,
        slot_number: TransitionHeight,
        batches: &Vec<(BlobDataWithId, Da::Address)>,
        state: &mut StateCheckpoint<S>,
    ) {
        self.deferred_blobs
            .set(&slot_number, batches, state)
            .unwrap_infallible();
    }

    /// Take all blobs for given block number, return empty vector if not exists
    /// Returned blobs are removed from the storage
    pub fn take_blobs_for_slot_number(
        &self,
        slot_height: TransitionHeight,
        state: &mut StateCheckpoint<S>,
    ) -> Vec<(BlobDataWithId, Da::Address)> {
        self.deferred_blobs
            .remove(&slot_height, state)
            .unwrap_infallible()
            .unwrap_or_default()
    }

    pub(crate) fn get_preferred_sequencer(
        &self,
        state: &mut StateCheckpoint<S>,
    ) -> Option<Da::Address> {
        self.sequencer_registry
            .get_preferred_sequencer(state)
            .unwrap_infallible()
    }
}

/// Empty module implementation
impl<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> KernelModule for BlobStorage<S, Da> {
    type Spec = S;
    type Config = ();

    fn genesis_unchecked(
        &self,
        _config: &Self::Config,
        _state: &mut KernelWorkingSet<'_, Self::Spec>,
    ) -> Result<(), sov_modules_api::Error> {
        Ok(())
    }
}

/// Contains data obtained from the DA blob, plus metadata required for blobs
/// from the preferred sequencer.
#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct PreferredBlobData {
    /// The sequence number of the batch. The rollup attempts to process batches in order by sequence number.
    pub sequence_number: u64,
    /// The actual data of the blob.
    pub data: BlobData,
    /// The number of virtual slots to advance after processing the batch. Minimum 1.
    pub virtual_slots_to_advance: u8,
}

/// A preferred blob and the ID (hash) of the blob that it was deserialized from
#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct PreferredBlobDataWithId {
    /// Raw transactions.
    pub inner: PreferredBlobData,
    /// The ID of the batch, carried over from the DA layer. This is the hash of the blob which contained the batch.
    pub id: [u8; 32],
}
