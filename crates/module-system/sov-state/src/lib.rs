//! Storage and state management interfaces for Sovereign SDK modules.

#![deny(missing_docs)]

mod bytes;
mod cache;
pub mod codec;
pub mod config;
pub mod namespaces;
#[cfg(feature = "native")]
mod prover_storage;
pub mod storage;
/// Defines the data structures needed by both the zk-storage and the prover storage.
mod storage_internals;
mod witness;
mod zk_storage;

pub mod jmt {
    //! Re-export the [`jellyfish-merkle-tree`](https://github.com/penumbra-zone/jmt) crate.
    pub use jmt::{KeyHash, RootHash, Version};
}
#[cfg(feature = "native")]
pub use prover_storage::{ProverChangeSet, ProverStorage};
use sha2::digest::typenum::U32;
use sov_rollup_interface::digest::Digest;
pub use storage_internals::{SparseMerkleProof, StorageRoot, VisibleHash};
pub use zk_storage::ZkStorage;

pub use crate::bytes::*;
pub use crate::cache::*;
pub use crate::codec::*;
pub use crate::namespaces::*;
pub use crate::storage::*;
pub use crate::witness::{ArrayWitness, Witness};

/// A trait specifying the hash function and format of the witness used in
/// merkle proofs for storage access
pub trait MerkleProofSpec: Send + Sync {
    /// The structure that accumulates the witness data
    type Witness: Witness + Send + Sync;
    /// The hash function used to compute the merkle root
    type Hasher: Digest<OutputSize = sha2::digest::typenum::U32> + Send + Sync;
}

/// The default [`MerkleProofSpec`] implementation.
///
/// This type is typically found as a type parameter for [`ProverStorage`].
#[derive(Clone)]
pub struct DefaultStorageSpec<H: Digest<OutputSize = U32> + Send + Sync> {
    _marker: std::marker::PhantomData<H>,
}

impl<H: Digest<OutputSize = U32> + Send + Sync> MerkleProofSpec for DefaultStorageSpec<H> {
    type Witness = ArrayWitness;

    type Hasher = H;
}

/// Accepts events emitted by modules
pub trait EventContainer {
    /// Adds a typed event to the working set.
    fn add_event<E: 'static + core::marker::Send>(&mut self, event_key: &str, event: E);
}
