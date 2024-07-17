use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// RawTx represents a serialized rollup transaction received from the DA.
#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct RawTx {
    /// Serialized transaction.
    pub data: Vec<u8>,
}

/// Contains raw transactions obtained from the DA blob.
#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct Batch {
    /// Raw transactions.
    pub txs: Vec<RawTx>,
}

/// Batch with ID.
pub struct BatchWithId {
    /// Batch of transactions.
    pub batch: Batch,
    /// The ID of the batch, carried over from the DA layer. This is the hash of the blob which contained the batch.
    pub id: [u8; 32],
}

/// Contains blob data obtained from the DA.
#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub enum BlobData {
    /// Batch of transactions.
    Batch(Batch),
    /// Aggregated proof posted on the DA.
    Proof(Vec<u8>),
}

impl BlobData {
    /// Batch variant constructor.
    pub fn new_batch(txs: Vec<RawTx>) -> BlobData {
        BlobData::Batch(Batch { txs })
    }

    /// Proof variant constructor.
    pub fn new_proof(proof: Vec<u8>) -> BlobData {
        BlobData::Proof(proof)
    }
}

/// Contains blob data obtained from the DA blob together with the ID of the blob.
#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct BlobDataWithId {
    /// Raw transactions.
    pub data: BlobData,
    /// The blob came from a registered sequencer
    pub from_registered_sequencer: bool,
    /// The ID of the batch, carried over from the DA layer. This is the hash of the blob which contained the batch.
    pub id: [u8; 32],
}
