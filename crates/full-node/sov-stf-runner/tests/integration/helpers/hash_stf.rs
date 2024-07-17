use borsh::BorshDeserialize;
use sha2::Digest;
use sov_modules_api::{BlobData, ProofOutcome, ProofReceipt};
use sov_rollup_interface::da::{BlobReaderTrait, BlockHeaderTrait, DaSpec, RelevantBlobIters};
use sov_rollup_interface::stf::{ApplySlotOutput, StateTransitionFunction};
use sov_rollup_interface::zk::aggregated_proof::SerializedAggregatedProof;
use sov_rollup_interface::zk::{ValidityCondition, Zkvm};
use sov_state::namespaces::User;
use sov_state::storage::{NativeStorage, SlotKey, SlotValue};
use sov_state::{
    ArrayWitness, DefaultStorageSpec, OrderedReadsAndWrites, Prefix, ProverChangeSet,
    ProverStorage, StateAccesses, Storage,
};

pub type S = DefaultStorageSpec<sha2::Sha256>;

#[derive(Default, Clone)]
pub struct HashStf<Cond> {
    phantom_data: std::marker::PhantomData<Cond>,
}

impl<Cond> HashStf<Cond> {
    pub fn new() -> Self {
        Self {
            phantom_data: std::marker::PhantomData,
        }
    }

    fn hash_key() -> SlotKey {
        let prefix = Prefix::new(b"root".to_vec());
        SlotKey::singleton(&prefix)
    }

    fn save_from_hasher(
        hasher: sha2::Sha256,
        storage: ProverStorage<S>,
        witness: &ArrayWitness,
    ) -> ([u8; 32], ProverChangeSet) {
        let result = hasher.finalize();

        let hash_key = HashStf::<Cond>::hash_key();
        let hash_value = SlotValue::from(result.as_slice().to_vec());

        let ordered_reads_writes = OrderedReadsAndWrites {
            ordered_reads: Vec::default(),
            ordered_writes: vec![(hash_key, Some(hash_value))],
        };
        let state_accesses = StateAccesses {
            user: ordered_reads_writes,
            kernel: Default::default(),
        };

        let (jmt_root_hash, state_update) = storage
            .compute_state_update(state_accesses, witness)
            .unwrap();

        let change_set = storage.materialize_changes(&state_update);

        (jmt_root_hash.into(), change_set)
    }
}

impl<InnerVm: Zkvm, OuterVm: Zkvm, Cond: ValidityCondition, Da: DaSpec>
    StateTransitionFunction<InnerVm, OuterVm, Da> for HashStf<Cond>
{
    type Address = [u8; 32];
    type StateRoot = [u8; 32];
    type GenesisParams = Vec<u8>;
    type PreState = ProverStorage<S>;
    type ChangeSet = ProverChangeSet;
    type TxReceiptContents = ();
    type ProofReceiptContents = ();
    type BatchReceiptContents = [u8; 32];
    type Witness = ArrayWitness;
    type Condition = Cond;

    fn init_chain(
        &self,
        genesis_state: Self::PreState,
        params: Self::GenesisParams,
    ) -> (Self::StateRoot, Self::ChangeSet) {
        let mut hasher = sha2::Sha256::new();
        hasher.update(params);

        HashStf::<Cond>::save_from_hasher(hasher, genesis_state, &ArrayWitness::default())
    }

    #[tracing::instrument(name = "HashStf::apply_slot", skip_all)]
    fn apply_slot<'a, I>(
        &self,
        pre_state_root: &Self::StateRoot,
        pre_state: Self::PreState,
        witness: Self::Witness,
        slot_header: &Da::BlockHeader,
        _validity_condition: &Da::ValidityCondition,
        relevant_blobs: RelevantBlobIters<I>,
    ) -> ApplySlotOutput<InnerVm, OuterVm, Da, Self>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        let all_blobs = relevant_blobs
            .batch_blobs
            .into_iter()
            .chain(relevant_blobs.proof_blobs);

        // Note: Uses native code, so won't work in ZK
        let storage_root_hash = pre_state.get_root_hash(slot_header.height()).unwrap();

        tracing::debug!(
            header = %slot_header.display(),
            passed = hex::encode(pre_state_root),
            from_storage = hex::encode(storage_root_hash.root_hash().0),
            "HashStf, starting apply slot",
        );

        assert_eq!(
            pre_state_root,
            &storage_root_hash.root_hash().0,
            "Incorrect pre_state_root has been passed"
        );

        let mut hasher = sha2::Sha256::new();

        let hash_key = HashStf::<Cond>::hash_key();
        let existing_cache = pre_state.get::<User>(&hash_key, None, &witness).unwrap();
        tracing::debug!(
            pre_state_root = hex::encode(pre_state_root),
            existing_cache = hex::encode(existing_cache.value()),
            "Fetched existing cache value from pre_state"
        );
        hasher.update(existing_cache.value());

        let mut proof_receipts = Vec::new();
        for blob in all_blobs {
            let data = blob.full_data();

            if !data.is_empty() {
                match BlobData::try_from_slice(data).unwrap() {
                    BlobData::Batch(_) => hasher.update(data),
                    BlobData::Proof(raw_proof) => proof_receipts.push(ProofReceipt {
                        raw_proof: SerializedAggregatedProof {
                            raw_aggregated_proof: raw_proof,
                        },
                        blob_hash: [0u8; 32],
                        outcome: ProofOutcome::<Self::Address, Da, Self::StateRoot>::Ignored,
                        extra_data: (),
                    }),
                };
            }
        }

        let (state_root, change_set) =
            HashStf::<Cond>::save_from_hasher(hasher, pre_state, &witness);

        tracing::debug!(
            from = hex::encode(pre_state_root),
            to = hex::encode(state_root),
            "Post apply slot root hashes",
        );

        ApplySlotOutput {
            state_root,
            change_set,
            proof_receipts,
            // TODO: Add batch receipts to inspection
            batch_receipts: vec![],
            witness,
        }
    }
}
