mod address;

use std::fmt::{Debug, Formatter};
use std::time::Duration;

pub use address::{MockAddress, MOCK_SEQUENCER_DA_ADDRESS};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sov_rollup_interface::common::HexHash;
use sov_rollup_interface::da::{
    BlockHashTrait, BlockHeaderTrait, CountedBufReader, DaProof, RelevantBlobs, RelevantProofs,
    Time,
};
#[cfg(feature = "native")]
use sov_rollup_interface::services::da::SlotData;
use sov_rollup_interface::Bytes;

#[cfg(feature = "native")]
use crate::storable::service::BlockProducing;
use crate::utils::hash_to_array;
use crate::validity_condition::MockValidityCond;

/// Time in milliseconds to wait for the next block if it is not there yet.
/// How many times wait attempts are done depends on service configuration.
pub const WAIT_ATTEMPT_PAUSE: Duration = Duration::from_millis(10);

/// Serialized aggregated proof.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct Proof(pub(crate) Vec<u8>);

/// A mock hash digest.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    BorshDeserialize,
    BorshSerialize,
    derive_more::From,
    derive_more::Into,
)]
pub struct MockHash(pub [u8; 32]);

impl Debug for MockHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", HexHash::new(self.0))
    }
}

impl core::fmt::Display for MockHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", HexHash::new(self.0))
    }
}

impl AsRef<[u8]> for MockHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for MockHash {
    type Error = anyhow::Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let hash: [u8; 32] = value.try_into().map_err(|e: Vec<u8>| {
            anyhow::anyhow!("Vec<u8> should have length 32: but it has {}", e.len())
        })?;
        Ok(MockHash(hash))
    }
}

impl BlockHashTrait for MockHash {}

/// A mock block header used for testing.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct MockBlockHeader {
    /// The hash of the previous block.
    pub prev_hash: MockHash,
    /// The hash of this block.
    pub hash: MockHash,
    /// The height of this block.
    pub height: u64,
    /// The time at which this block was created.
    pub time: Time,
}

impl MockBlockHeader {
    /// Generates [`MockBlockHeader`] with given height, where hashes are derived from height.
    /// Can be used in tests, where a header of the following blocks will be consistent.
    pub fn from_height(height: u64) -> MockBlockHeader {
        let prev_hash = u64_to_bytes(height);
        let hash = u64_to_bytes(height + 1);
        MockBlockHeader {
            prev_hash: MockHash(prev_hash),
            hash: MockHash(hash),
            height,
            time: Time::now(),
        }
    }
}

impl Default for MockBlockHeader {
    fn default() -> Self {
        MockBlockHeader::from_height(0)
    }
}

impl std::fmt::Display for MockBlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MockBlockHeader {{ height: {}, prev_hash: {}, next_hash: {} }}",
            self.height,
            hex::encode(self.prev_hash),
            hex::encode(self.hash)
        )
    }
}

impl BlockHeaderTrait for MockBlockHeader {
    type Hash = MockHash;

    fn prev_hash(&self) -> Self::Hash {
        self.prev_hash
    }

    fn hash(&self) -> Self::Hash {
        self.hash
    }

    fn height(&self) -> u64 {
        self.height
    }

    fn time(&self) -> Time {
        self.time.clone()
    }
}

#[cfg(feature = "native")]
pub(crate) const GENESIS_HEADER: MockBlockHeader = MockBlockHeader {
    prev_hash: MockHash([0; 32]),
    hash: MockHash([1; 32]),
    height: 0,
    // 2023-01-01T00:00:00Z
    time: Time::from_secs(1672531200),
};

#[cfg(feature = "native")]
pub(crate) const GENESIS_BLOCK: MockBlock = MockBlock {
    header: GENESIS_HEADER,
    validity_cond: MockValidityCond { is_valid: true },
    batch_blobs: Vec::new(),
    proof_blobs: Vec::new(),
};

/// Configuration for block producing.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockProducingConfig {
    /// New blocks are produced periodically.
    /// This means that empty blocks can be produced.
    Periodic,
    /// New blocks are produced only when blob is submitted.
    /// This also means that block has only one blob.
    OnSubmit,
}

/// The configuration for Mock Da.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MockDaConfig {
    /// Connection string to the database for storing Da Data.
    ///   - "sqlite://demo_data/da.sqlite?mode=rwc"
    ///   - "sqlite::memory:"
    ///   - "postgresql://root:hunter2@aws.amazon.com/mock-da"
    pub connection_string: String,
    /// The address to use to "submit" blobs on the mock da layer.
    pub sender_address: MockAddress,
    /// Defines how many blocks progress to finalization.
    #[serde(default)]
    pub finalization_blocks: u32,
    /// How MockDaService should produce blocks.
    #[serde(default = "default_block_producing")]
    pub block_producing: BlockProducingConfig,
    /// Block time depends on `block_producing`:
    ///  - For [`BlockProducingConfig::Periodic`] it defines how often new blocks will be produced, approximately.
    ///  - For [`BlockProducingConfig::OnSubmit`] it defines max time service will wait for a new block to be submitted.
    #[serde(default = "default_block_time_ms")]
    pub block_time_ms: u64,
}

pub(crate) fn default_block_producing() -> BlockProducingConfig {
    BlockProducingConfig::OnSubmit
}

pub(crate) fn default_block_time_ms() -> u64 {
    120_000
}

impl MockDaConfig {
    /// Create [`MockDaConfig`] with instant finality.
    pub fn instant_with_sender(sender: MockAddress) -> Self {
        MockDaConfig {
            connection_string: "sqlite::memory:".to_string(),
            sender_address: sender,
            finalization_blocks: 0,
            block_producing: default_block_producing(),
            block_time_ms: default_block_time_ms(),
        }
    }

    #[cfg(feature = "native")]
    pub(crate) fn block_producing(&self) -> BlockProducing {
        match self.block_producing {
            BlockProducingConfig::Periodic => {
                BlockProducing::Periodic(Duration::from_millis(self.block_time_ms))
            }
            BlockProducingConfig::OnSubmit => {
                BlockProducing::OnSubmit(Duration::from_millis(self.block_time_ms))
            }
        }
    }
}

#[derive(Clone, Default)]
/// DaVerifier used in tests.
pub struct MockDaVerifier {}

#[derive(
    Debug,
    Clone,
    PartialEq,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
/// A mock BlobTransaction from a DA layer used for testing.
pub struct MockBlob {
    pub(crate) address: MockAddress,
    pub(crate) hash: [u8; 32],
    pub(crate) blob: CountedBufReader<Bytes>,
}

impl MockBlob {
    /// Creates a new mock blob with the given data, claiming to have been published by the provided address.
    pub fn new(tx_blob: Vec<u8>, address: MockAddress, hash: [u8; 32]) -> Self {
        Self {
            address,
            blob: CountedBufReader::new(Bytes::from(tx_blob)),
            hash,
        }
    }

    /// Build new blob, but calculates hash from input data
    pub fn new_with_hash(blob: Vec<u8>, address: MockAddress) -> Self {
        let data_hash = hash_to_array(&blob).to_vec();
        let blob_hash = hash_to_array(&data_hash);
        Self {
            address,
            blob: CountedBufReader::new(Bytes::from(blob)),
            hash: blob_hash,
        }
    }

    /// Creates blob of transactions.
    pub fn advance(&mut self) {
        self.blob.advance(self.blob.total_len());
    }
}

/// A mock block type used for testing.
#[derive(Serialize, Deserialize, Default, PartialEq, Debug, Clone)]
pub struct MockBlock {
    /// The header of this block.
    pub header: MockBlockHeader,
    /// Validity condition
    pub validity_cond: MockValidityCond,
    /// Rollup's batch namespace.
    pub batch_blobs: Vec<MockBlob>,
    /// Rollup's proof namespace.
    pub proof_blobs: Vec<MockBlob>,
}

#[cfg(feature = "native")]
impl SlotData for MockBlock {
    type BlockHeader = MockBlockHeader;
    type Cond = MockValidityCond;

    fn hash(&self) -> [u8; 32] {
        self.header.hash.0
    }

    fn header(&self) -> &Self::BlockHeader {
        &self.header
    }

    fn validity_condition(&self) -> MockValidityCond {
        self.validity_cond
    }
}

impl MockBlock {
    /// Creates empty block, which is following of the current
    pub fn next_mock(&self) -> MockBlock {
        let mut next_block = MockBlock::default();
        let h = self.header.height + 1;
        next_block.header = MockBlockHeader::from_height(h);
        next_block
    }

    /// Creates [`RelevantBlobs`] data from this block.
    /// Where all batches and proofs are relevant.
    pub fn as_relevant_blobs(&self) -> RelevantBlobs<MockBlob> {
        RelevantBlobs {
            proof_blobs: self.proof_blobs.clone(),
            batch_blobs: self.batch_blobs.clone(),
        }
    }

    /// Creates [`RelevantProofs`] with default values for inclusion and completeness proofs.
    pub fn get_relevant_proofs(&self) -> RelevantProofs<[u8; 32], ()> {
        RelevantProofs {
            batch: DaProof {
                inclusion_proof: Default::default(),
                completeness_proof: Default::default(),
            },
            proof: DaProof {
                inclusion_proof: Default::default(),
                completeness_proof: Default::default(),
            },
        }
    }
}

fn u64_to_bytes(value: u64) -> [u8; 32] {
    let value = value.to_be_bytes();
    let mut result = [0u8; 32];
    result[..value.len()].copy_from_slice(&value);
    result
}
