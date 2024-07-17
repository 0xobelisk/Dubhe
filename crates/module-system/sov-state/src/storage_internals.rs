//! Defines the data structures needed by both the zk-storage and the prover storage.

use std::marker::PhantomData;

use borsh::{BorshDeserialize, BorshSerialize};
use derivative::Derivative;
use jmt::{RootHash, SimpleHasher};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use sha2::Digest;

use crate::namespaces::Namespaced;
use crate::MerkleProofSpec;
/// Combined root hash of the user and kernel namespaces. The user root hash is the first 32 bytes, whereas the
/// kernel root hash is the last 32 bytes.
/// We need to store both the user and the kernel root hashes to be able to check zk-proofs against the
/// correct hash.
/// We use the generic `S: MerkleProofSpec` to specify the hash function used to compute the global root hash.
/// The global root hash is computed by hashing the user hash and the kernel hash together.
#[derive(Derivative, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[derivative(
    Debug(bound = "S: MerkleProofSpec"),
    Eq(bound = "S: MerkleProofSpec"),
    PartialEq(bound = "S: MerkleProofSpec"),
    Copy(bound = "S: MerkleProofSpec")
)]
pub struct StorageRoot<S: MerkleProofSpec> {
    #[serde(with = "BigArray")]
    root_hashes: [u8; 64],
    phantom: PhantomData<S>,
}

impl<S: MerkleProofSpec> AsRef<[u8]> for StorageRoot<S> {
    fn as_ref(&self) -> &[u8] {
        &self.root_hashes
    }
}

impl<S: MerkleProofSpec> Clone for StorageRoot<S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S: MerkleProofSpec> From<StorageRoot<S>> for Namespaced<[u8; 32]> {
    fn from(val: StorageRoot<S>) -> Self {
        Namespaced::new(val.user_hash().0, val.kernel_hash().0, [0u8; 32])
    }
}

impl<S: MerkleProofSpec> From<StorageRoot<S>> for [u8; 32] {
    fn from(value: StorageRoot<S>) -> Self {
        value.root_hash().0
    }
}

impl<S: MerkleProofSpec> StorageRoot<S> {
    /// Creates a new `[ProverStorageRoot]` instance from specified root hashes.
    /// Concretely this method builds the prover root hash by concatenating the user and
    /// the kernel root hashes.
    pub fn new(user_root_hash: jmt::RootHash, kernel_root_hash: jmt::RootHash) -> Self {
        // Concatenate the user and kernel root hashes
        let user_hash = user_root_hash.0;
        let kernel_hash = kernel_root_hash.0;
        let mut root_hashes = [0u8; 64];
        root_hashes[..32].copy_from_slice(&user_hash);
        root_hashes[32..].copy_from_slice(&kernel_hash);

        Self {
            root_hashes,
            phantom: Default::default(),
        }
    }

    /// Returns the user root hash of the prover storage.
    pub fn user_hash(&self) -> jmt::RootHash {
        let mut output = [0u8; 32];
        output.copy_from_slice(&self.root_hashes[..32]);
        jmt::RootHash(output)
    }

    /// Returns the kernel root hash of the prover storage.
    pub fn kernel_hash(&self) -> jmt::RootHash {
        let mut output = [0u8; 32];
        output.copy_from_slice(&self.root_hashes[32..]);
        jmt::RootHash(output)
    }

    /// Returns the global root hash of the prover storage.
    pub fn root_hash(&self) -> jmt::RootHash {
        let mut hasher = <S::Hasher as sha2::Digest>::new();
        Digest::update(&mut hasher, self.user_hash().0);
        Digest::update(&mut hasher, self.kernel_hash().0);
        let output: [u8; 32] = Digest::finalize(hasher).into();
        jmt::RootHash(output)
    }
}

/// The visible hash associated with the storage. This is the hash of the user namespace.
pub struct VisibleHash(RootHash);

impl VisibleHash {
    /// Creates a new visible hash from a slice
    pub fn new(root_hash: [u8; 32]) -> Self {
        VisibleHash(RootHash(root_hash))
    }
}

impl<S: MerkleProofSpec> From<StorageRoot<S>> for VisibleHash {
    fn from(root: StorageRoot<S>) -> Self {
        VisibleHash(root.user_hash())
    }
}

impl<'a, S: MerkleProofSpec> From<&'a StorageRoot<S>> for VisibleHash {
    fn from(root: &'a StorageRoot<S>) -> Self {
        VisibleHash(root.user_hash())
    }
}

impl From<VisibleHash> for [u8; 32] {
    fn from(val: VisibleHash) -> Self {
        val.0 .0
    }
}

/// A storage proof that is used to verify the existence of a key in the storage.
#[derive(Derivative, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[derivative(
    PartialEq(bound = "H: SimpleHasher"),
    Eq(bound = "H: SimpleHasher"),
    Clone(bound = "H: SimpleHasher"),
    Debug(bound = "H: SimpleHasher")
)]
pub struct SparseMerkleProof<H: SimpleHasher>(
    #[serde(bound(serialize = "", deserialize = ""))]
    #[borsh(bound(serialize = "", deserialize = ""))]
    jmt::proof::SparseMerkleProof<H>,
);

impl<H: SimpleHasher> SparseMerkleProof<H> {
    /// Returns the underlying proof.
    pub fn inner(&self) -> &jmt::proof::SparseMerkleProof<H> {
        &self.0
    }
}

impl<H: SimpleHasher> From<jmt::proof::SparseMerkleProof<H>> for SparseMerkleProof<H> {
    fn from(proof: jmt::proof::SparseMerkleProof<H>) -> Self {
        SparseMerkleProof(proof)
    }
}
