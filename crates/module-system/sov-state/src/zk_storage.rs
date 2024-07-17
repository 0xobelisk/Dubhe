use std::marker::PhantomData;

#[cfg(all(target_os = "zkvm", feature = "bench"))]
use risc0_cycle_macros::cycle_tracker;

use crate::cache::{OrderedReadsAndWrites, StateAccesses};
use crate::jmt::KeyHash;
#[cfg(feature = "native")]
use crate::namespaces::ProvableCompileTimeNamespace;
use crate::namespaces::{CompileTimeNamespace, ProvableNamespace};
use crate::storage::{SlotKey, SlotValue, Storage, StorageProof};
use crate::storage_internals::SparseMerkleProof;
use crate::{MerkleProofSpec, StorageRoot, Witness};

/// A [`Storage`] implementation designed to be used inside the zkVM.
#[derive(Default, derivative::Derivative)]
#[derivative(Clone(bound = "S: MerkleProofSpec"))]
pub struct ZkStorage<S: MerkleProofSpec> {
    _phantom_hasher: PhantomData<S::Hasher>,
}

impl<S: MerkleProofSpec> ZkStorage<S> {
    /// Creates a new [`ZkStorage`] instance. Identical to [`Default::default`].
    pub fn new() -> Self {
        Self {
            _phantom_hasher: Default::default(),
        }
    }
}

#[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
fn jmt_verify_existence<S: MerkleProofSpec>(
    prev_state_root: [u8; 32],
    state_accesses: &OrderedReadsAndWrites,
    witness: &S::Witness,
) -> Result<(), anyhow::Error> {
    // For each value that's been read from the tree, verify the provided smt proof
    for (key, read_value) in &state_accesses.ordered_reads {
        let key_hash = KeyHash::with::<S::Hasher>(key.key().as_ref());
        // TODO: Switch to the batch read API once it becomes available
        let proof: jmt::proof::SparseMerkleProof<S::Hasher> = witness.get_hint();

        match read_value {
            Some(val) => proof.verify_existence(
                jmt::RootHash(prev_state_root),
                key_hash,
                val.value().as_ref(),
            )?,
            None => proof.verify_nonexistence(jmt::RootHash(prev_state_root), key_hash)?,
        }
    }

    Ok(())
}

#[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
fn jmt_verify_update<S: MerkleProofSpec>(
    prev_state_root: [u8; 32],
    state_accesses: OrderedReadsAndWrites,
    witness: &S::Witness,
) -> [u8; 32] {
    // Compute the jmt update from the write batch
    let batch = state_accesses
        .ordered_writes
        .into_iter()
        .map(|(key, value)| {
            let key_hash = KeyHash::with::<S::Hasher>(key.key().as_ref());
            (key_hash, value.as_ref().map(|v| v.value().to_vec()))
        })
        .collect::<Vec<_>>();

    let update_proof: jmt::proof::UpdateMerkleProof<S::Hasher> = witness.get_hint();
    let new_root: [u8; 32] = witness.get_hint();
    update_proof
        .verify_update(
            jmt::RootHash(prev_state_root),
            jmt::RootHash(new_root),
            batch,
        )
        .expect("Updates must be valid");

    new_root
}

impl<S: MerkleProofSpec> ZkStorage<S> {
    fn compute_state_update_namespace(
        &self,
        state_accesses: OrderedReadsAndWrites,
        witness: &S::Witness,
    ) -> Result<jmt::RootHash, anyhow::Error> {
        let prev_state_root = witness.get_hint();

        // For each value that's been read from the tree, verify the provided smt proof
        jmt_verify_existence::<S>(prev_state_root, &state_accesses, witness)?;

        let new_root = jmt_verify_update::<S>(prev_state_root, state_accesses, witness);

        Ok(jmt::RootHash(new_root))
    }
}

impl<S: MerkleProofSpec> Storage for ZkStorage<S> {
    type Witness = S::Witness;
    type RuntimeConfig = ();
    type Proof = SparseMerkleProof<S::Hasher>;
    type Root = StorageRoot<S>;
    type StateUpdate = ();
    type ChangeSet = ();

    fn get<N: CompileTimeNamespace>(
        &self,
        _key: &SlotKey,
        _version: Option<u64>,
        witness: &Self::Witness,
    ) -> Option<SlotValue> {
        witness.get_hint()
    }

    fn compute_state_update(
        &self,
        state_accesses: StateAccesses,
        witness: &Self::Witness,
    ) -> Result<(Self::Root, Self::StateUpdate), anyhow::Error> {
        let user_root = self.compute_state_update_namespace(state_accesses.user, witness)?;
        let kernel_root = self.compute_state_update_namespace(state_accesses.kernel, witness)?;

        Ok((StorageRoot::<S>::new(user_root, kernel_root), ()))
    }

    #[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
    fn materialize_changes(&self, _node_batch: &Self::StateUpdate) {}

    fn open_proof(
        state_root: Self::Root,
        state_proof: StorageProof<Self::Proof>,
    ) -> Result<(SlotKey, Option<SlotValue>), anyhow::Error> {
        let StorageProof {
            key,
            value,
            proof,
            namespace,
        } = state_proof;
        let key_hash = KeyHash::with::<S::Hasher>(key.as_ref());

        // We need to verify the proof against the correct root hash,
        // Hence we match the key against its namespace
        match namespace {
            ProvableNamespace::User => proof.inner().verify(
                state_root.user_hash(),
                key_hash,
                value.as_ref().map(|v| v.value()),
            )?,
            ProvableNamespace::Kernel => proof.inner().verify(
                state_root.kernel_hash(),
                key_hash,
                value.as_ref().map(|v| v.value()),
            )?,
        }

        Ok((key, value))
    }
}

#[cfg(feature = "native")]
impl<S: MerkleProofSpec> crate::storage::NativeStorage for ZkStorage<S> {
    fn get_with_proof<N: ProvableCompileTimeNamespace>(
        &self,
        _key: SlotKey,
        _version: Option<u64>,
    ) -> StorageProof<Self::Proof> {
        unimplemented!("The ZkStorage should not be used to generate merkle proofs! The NativeStorage trait is only implemented to allow for the use of the ZkStorage in tests.");
    }

    fn get_root_hash(&self, _version: jmt::Version) -> anyhow::Result<Self::Root> {
        unimplemented!("The ZkStorage should not be used to generate merkle proofs! The NativeStorage trait is only implemented to allow for the use of the ZkStorage in tests.");
    }
}
