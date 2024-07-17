//! This module is the core of the Sovereign SDK. It defines the traits and types that
//! allow the SDK to run the "business logic" of any application generically.
//!
//! The most important trait in this module is the [`StateTransitionFunction`], which defines the
//! main event loop of the rollup.
use std::fmt::Debug;
mod events;
mod proof_serializer;
pub use events::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
mod transaction;
pub use proof_serializer::*;
pub use transaction::*;

use crate::da::{DaSpec, RelevantBlobIters};
use crate::zk::aggregated_proof::{AggregatedProofPublicData, SerializedAggregatedProof};
use crate::zk::{StateTransitionPublicData, ValidityCondition, Zkvm};

#[cfg(any(all(test, feature = "sha2"), feature = "arbitrary"))]
pub mod fuzzing;

/// The configuration of a full node of the rollup which creates zk proofs.
pub struct ProverConfig;
/// The configuration used to initialize the "Verifier" of the state transition function
/// which runs inside of the zkVM.
pub struct ZkConfig;
/// The configuration of a standard full node of the rollup which does not create zk proofs
pub struct StandardConfig;

/// A special marker trait which allows us to define different rollup configurations. There are
/// only 3 possible instantiations of this trait: [`ProverConfig`], [`ZkConfig`], and [`StandardConfig`].
pub trait StateTransitionConfig: sealed::Sealed {}
impl StateTransitionConfig for ProverConfig {}
impl StateTransitionConfig for ZkConfig {}
impl StateTransitionConfig for StandardConfig {}

// https://rust-lang.github.io/api-guidelines/future-proofing.html
mod sealed {
    use super::{ProverConfig, StandardConfig, ZkConfig};

    pub trait Sealed {}
    impl Sealed for ProverConfig {}
    impl Sealed for ZkConfig {}
    impl Sealed for StandardConfig {}
}

/// A receipt for a batch of transactions. These receipts are stored in the rollup's database
/// and may be queried via RPC. Batch receipts are generic over a type `BatchReceiptContents` which the rollup
/// can use to store arbitrary typed data, like the gas used by the batch. They are also generic over a type `TxReceiptContents`,
/// since they contain a vectors of [`TransactionReceipt`]s.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "T: TxReceiptContents, BatchReceiptContents: Serialize + DeserializeOwned + Clone")]
pub struct BatchReceipt<BatchReceiptContents, T: TxReceiptContents> {
    /// The canonical hash of this batch
    pub batch_hash: [u8; 32],
    /// The receipts of all the transactions in this batch.
    pub tx_receipts: Vec<TransactionReceipt<T>>,
    /// Computed gas price for this batch.
    pub gas_price: Vec<u64>,
    /// Any additional structured data to be saved in the database and served over RPC
    pub inner: BatchReceiptContents,
}

/// A receipt for data posted into the proof namespace
pub struct ProofReceipt<Address, Da: DaSpec, Root, Extra> {
    /// The serialized zk-proof.
    pub raw_proof: SerializedAggregatedProof,
    /// The hash of the blob which contained the proof
    pub blob_hash: [u8; 32],
    /// The outcome of the proof
    pub outcome: ProofOutcome<Address, Da, Root>,
    /// Any extra structured data to store with the proof receipt. For example, this might
    /// be the full contents of the proof (for an aggregate proof), or a proof that the sender
    /// of an attestation was bonded.
    pub extra_data: Extra,
}

#[allow(clippy::large_enum_variant)]
/// The contents of a proof receipt.
pub enum ProofReceiptContents<Address, Da: DaSpec, Root> {
    /// A receipt for an aggregate proof contains the public data form the proof.
    AggregateProof(AggregatedProofPublicData),
    /// A receipt for a block proof contains the public data from the state transition which was proven.
    BlockProof(StateTransitionPublicData<Address, Da, Root>),
    /// A receipt for an attestation contains the public data that the attestation made a claim about.
    Attestation(StateTransitionPublicData<Address, Da, Root>),
}

#[allow(clippy::large_enum_variant)]
/// The outcome of a proof
pub enum ProofOutcome<Address, Da: DaSpec, Root> {
    /// The blob was filtered out as irrelevant
    Ignored,
    /// The blob is some kind of valid proof
    Valid(ProofReceiptContents<Address, Da, Root>),
    /// The blob is some kind of invalid proof
    Invalid,
}

type ProofReceipts<Address, Da, StateRoot, Extra> =
    Vec<ProofReceipt<Address, Da, StateRoot, Extra>>;

/// The result of applying a slot to current state.
pub struct ApplySlotOutput<
    InnerVm: Zkvm,
    OuterVm: Zkvm,
    Da: DaSpec,
    Stf: StateTransitionFunction<InnerVm, OuterVm, Da>,
> {
    /// Final state root after all blobs were applied
    pub state_root: Stf::StateRoot,
    /// Container for all state alterations that happened during slot execution
    pub change_set: Stf::ChangeSet,
    /// Receipt for each applied proof transaction
    pub proof_receipts: ProofReceipts<Stf::Address, Da, Stf::StateRoot, Stf::ProofReceiptContents>,
    /// Receipt for each applied batch
    pub batch_receipts: Vec<BatchReceipt<Stf::BatchReceiptContents, Stf::TxReceiptContents>>,
    /// Witness after applying the whole block
    pub witness: Stf::Witness,
}

// TODO(@preston-evans98): update spec with simplified API
/// State transition function defines business logic that responsible for changing state.
/// Terminology:
///  - state root: root hash of state merkle tree
///  - block: DA layer block
///  - batch: Set of transactions grouped together, or block on L2
///  - blob: Non serialised batch or anything else that can be posted on DA layer, like attestation or proof.
///
/// The STF is generic over a DA layer and two `Zkvm`s. The `InnerVm` is used to prove individual slots,
/// while the `OuterVm` is used to generate recursive proofs over multiple slots. The two VMs *may* be set to be
/// the  same type.
pub trait StateTransitionFunction<InnerVm: Zkvm, OuterVm: Zkvm, Da: DaSpec>: Sized {
    /// Root hash of state merkle tree
    type StateRoot: Serialize + DeserializeOwned + Clone + AsRef<[u8]>;

    /// The address of the prover.
    type Address: Serialize + DeserializeOwned + Clone;

    /// The initial params of the rollup.
    type GenesisParams;

    /// State of the rollup before transition.
    type PreState;

    /// State of the rollup after transition.
    type ChangeSet;

    /// The contents of a proof receipt. This is the data that is persisted in the database
    type ProofReceiptContents: Serialize + DeserializeOwned + Clone;

    /// The contents of a transaction for a successful transaction.
    type TxReceiptContents: TxReceiptContents;

    /// The contents of a batch receipt. This is the data that is persisted in the database
    type BatchReceiptContents: Serialize + DeserializeOwned + Clone;

    /// Witness is a data that is produced during actual batch execution
    /// or validated together with proof during verification
    type Witness: Default + Serialize + DeserializeOwned;

    /// The validity condition that must be verified outside of the Vm
    type Condition: ValidityCondition;

    /// Perform one-time initialization for the genesis block and
    /// returns the resulting root hash and changeset.
    /// If the init chain fails we panic.
    fn init_chain(
        &self,
        genesis_state: Self::PreState,
        params: Self::GenesisParams,
    ) -> (Self::StateRoot, Self::ChangeSet);

    /// Called at each **DA-layer block** - whether or not that block contains any
    /// data relevant to the rollup.
    /// If slot is started in Full Node mode, default witness should be provided.
    /// If slot is started in Zero Knowledge mode, witness from execution should be provided.
    ///
    /// Applies batches of transactions to the rollup,
    /// slashing the sequencer who proposed the blob on failure.
    /// The blobs are contained into a slot whose data is contained within the `slot_data` parameter,
    /// this parameter is mainly used within the begin_slot hook.
    /// The concrete blob type is defined by the DA layer implementation,
    /// which is why we use a generic here instead of an associated type.
    ///
    /// Commits state changes to the database
    #[allow(clippy::type_complexity)]
    fn apply_slot<'a, I>(
        &self,
        pre_state_root: &Self::StateRoot,
        pre_state: Self::PreState,
        witness: Self::Witness,
        slot_header: &Da::BlockHeader,
        validity_condition: &Da::ValidityCondition,
        relevant_blobs: RelevantBlobIters<I>,
    ) -> ApplySlotOutput<InnerVm, OuterVm, Da, Self>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>;
}
