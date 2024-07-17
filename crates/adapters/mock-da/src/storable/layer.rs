//! Data Availability layer is a single entry to all available blocks.

use sea_orm::{
    ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use sha2::Digest;
use tokio::sync::broadcast;

use crate::storable::entity;
use crate::storable::entity::blobs::Entity as Blobs;
use crate::storable::entity::block_headers::Entity as BlockHeaders;
use crate::storable::entity::{blobs, block_headers};
use crate::types::{GENESIS_BLOCK, GENESIS_HEADER};
use crate::{MockAddress, MockBlob, MockBlock, MockBlockHeader};

/// Struct that stores blobs and block headers. Controller of the sea orm entities.
pub struct StorableMockDaLayer {
    conn: DatabaseConnection,
    /// Height which is currently being built.
    pub(crate) next_height: u32,
    /// Defines how many blocks should be submitted, before block is finalized.
    /// Zero means instant finality.
    blocks_to_finality: u32,
    pub(crate) finalized_header_sender: broadcast::Sender<MockBlockHeader>,
}

impl StorableMockDaLayer {
    /// Creates new [`StorableMockDaLayer`] by passing connections string directly to [`Database`]
    pub async fn new_from_connection(
        connection_string: &str,
        blocks_to_finality: u32,
    ) -> anyhow::Result<Self> {
        let conn: DatabaseConnection = Database::connect(connection_string).await?;
        entity::setup_db(&conn).await?;
        let next_height = entity::query_last_height(&conn)
            .await?
            .checked_add(1)
            .expect("next_height overflow");
        let (finalized_header_sender, mut rx) = broadcast::channel(100);

        // Spawn a task, so the receiver is not dropped, and the channel is not
        // closed.
        // Once the sender is dropped, the receiver will receive an
        // error and the task will exit.
        tokio::spawn(async move { while rx.recv().await.is_ok() {} });

        Ok(StorableMockDaLayer {
            conn,
            next_height,
            blocks_to_finality,
            finalized_header_sender,
        })
    }

    /// Creates in-memory SQLite instance.
    pub async fn new_in_memory(blocks_to_finality: u32) -> anyhow::Result<Self> {
        Self::new_from_connection("sqlite::memory:", blocks_to_finality).await
    }

    /// Creates SQLite instance at a given path.
    pub async fn new_in_path(
        path: impl AsRef<std::path::Path>,
        blocks_to_finality: u32,
    ) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            anyhow::bail!("Path {} does no exist", path.display());
        }
        let db_path = path.join("mock_da.sqlite");
        tracing::debug!(path = %db_path.display(), "Opening StorableMockDa");
        let connection_string = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
        Self::new_from_connection(&connection_string, blocks_to_finality).await
    }

    /// Saves new block header into a database.
    pub(crate) async fn produce_block(&mut self) -> anyhow::Result<()> {
        tracing::debug!(
            next_height = self.next_height,
            "Start producing a new block at"
        );
        if self.next_height >= i32::MAX as u32 {
            anyhow::bail!("Due to database limitation cannot produce anymore blocks");
        }
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.next_height.to_be_bytes());

        let prev_block_hash = if self.next_height > 1 {
            let block = BlockHeaders::find()
                .filter(block_headers::Column::Height.eq(self.next_height - 1))
                .one(&self.conn)
                .await?
                .expect("Previous block is missing from the database");
            let hash: [u8; 32] = block.hash.try_into().map_err(|e: Vec<u8>| {
                anyhow::anyhow!(
                    "BlockHash should be 32 bytes long in database, but it is {}",
                    e.len()
                )
            })?;
            hash
        } else {
            GENESIS_HEADER.hash.0
        };

        hasher.update(prev_block_hash);

        let blobs = Blobs::find()
            .filter(blobs::Column::BlockHeight.eq(self.next_height))
            .all(&self.conn)
            .await?;
        let blobs_count = blobs.len();

        for blob in &blobs {
            hasher.update(&blob.hash[..]);
            hasher.update(&blob.sender[..]);
            hasher.update(&blob.namespace[..]);
        }

        let this_block_hash = hasher.finalize();

        let block = block_headers::ActiveModel {
            height: Set(self.next_height as i32),
            prev_hash: Set(prev_block_hash.to_vec()),
            hash: Set(this_block_hash.to_vec()),
            ..Default::default()
        };
        block.insert(&self.conn).await?;
        tracing::debug!(blobs_count, "New block has been produced");

        self.next_height += 1;

        let last_finalized_height = self.get_last_finalized_height();
        // Meaning that "chain head - blocks to finalization" has moved beyond genesis block.
        if last_finalized_height > 0 {
            tracing::debug!(
                height = last_finalized_height,
                "Submitting finalized header at"
            );
            let finalized_header = self.get_header_at(last_finalized_height).await.unwrap();
            self.finalized_header_sender.send(finalized_header).unwrap();
        }
        Ok(())
    }

    async fn get_header_at(&self, height: u32) -> anyhow::Result<MockBlockHeader> {
        if height < 1 {
            return Ok(GENESIS_HEADER);
        }
        if height >= self.next_height {
            anyhow::bail!("Block at height {} has not been produced yet", height);
        }
        let header = BlockHeaders::find()
            .filter(block_headers::Column::Height.eq(height))
            .one(&self.conn)
            .await?
            .map(MockBlockHeader::from)
            .expect("Corrupted DB, block not found");
        Ok(header)
    }

    fn get_last_finalized_height(&self) -> u32 {
        self.next_height
            .checked_sub(self.blocks_to_finality.saturating_add(1))
            .unwrap_or_default()
    }

    pub(crate) async fn submit_batch(
        &self,
        batch_data: &[u8],
        sender: &MockAddress,
    ) -> anyhow::Result<()> {
        let blob = blobs::build_batch_blob(self.next_height as i32, batch_data, sender);
        blob.insert(&self.conn).await?;
        Ok(())
    }

    pub(crate) async fn submit_proof(
        &self,
        proof_data: &[u8],
        sender: &MockAddress,
    ) -> anyhow::Result<()> {
        let blob = blobs::build_proof_blob(self.next_height as i32, proof_data, sender);
        blob.insert(&self.conn).await?;
        Ok(())
    }

    pub(crate) async fn get_head_block_header(&self) -> anyhow::Result<MockBlockHeader> {
        self.get_header_at(self.next_height.saturating_sub(1)).await
    }

    pub(crate) async fn get_last_finalized_block_header(&self) -> anyhow::Result<MockBlockHeader> {
        self.get_header_at(self.get_last_finalized_height()).await
    }

    pub(crate) async fn get_block_at(&self, height: u32) -> anyhow::Result<MockBlock> {
        if height >= self.next_height {
            anyhow::bail!("Block at height {} has not been produced yet", height);
        }
        if height == 0 {
            return Ok(GENESIS_BLOCK);
        }

        let header = self.get_header_at(height).await?;

        let blobs = Blobs::find()
            .filter(blobs::Column::BlockHeight.eq(height))
            .all(&self.conn)
            .await?;

        // Batches are submitted more often,
        // so we are willing to pay for extra allocation when only proofs were submitted.
        let mut batch_blobs = Vec::with_capacity(blobs.len());
        let mut proof_blobs = Vec::new();

        for blob in blobs {
            match blob.namespace.as_str() {
                entity::BATCH_NAMESPACE => batch_blobs.push(MockBlob::from(blob)),
                entity::PROOF_NAMESPACE => proof_blobs.push(MockBlob::from(blob)),
                namespace => {
                    panic!("Unknown namespace: {}, corrupted block", namespace)
                }
            }
        }

        Ok(MockBlock {
            header,
            validity_cond: Default::default(),
            batch_blobs,
            proof_blobs,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use sov_rollup_interface::da::{BlobReaderTrait, BlockHeaderTrait};
    use sov_rollup_interface::services::da::SlotData;
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;
    use tokio::task::JoinHandle;
    use tokio::time;

    use super::*;
    use crate::MockAddress;

    enum TestBlob {
        Batch(Vec<u8>),
        Proof(Vec<u8>),
    }

    async fn check_da_layer_consistency(da_layer: &StorableMockDaLayer) -> anyhow::Result<()> {
        let mut prev_block_hash = GENESIS_HEADER.prev_hash;

        for height in 0..da_layer.next_height {
            let block = da_layer.get_block_at(height).await?;
            assert_eq!(height, block.header().height as u32);
            assert_eq!(prev_block_hash, block.header().prev_hash);
            prev_block_hash = block.header().hash;
        }

        Ok(())
    }

    async fn check_expected_blobs(
        da_layer: &StorableMockDaLayer,
        expected_blocks: &[Vec<(TestBlob, MockAddress)>],
    ) -> anyhow::Result<()> {
        // A current height is expected to be the next of number of blocks sent.
        // Meaning da layer is "building next block".
        assert_eq!(expected_blocks.len() as u32 + 1, da_layer.next_height);
        check_da_layer_consistency(da_layer).await?;
        for (idx, expected_block) in expected_blocks.iter().enumerate() {
            let height = (idx + 1) as u32;
            let received_block = da_layer.get_block_at(height).await?;
            assert_eq!(height as u64, received_block.header().height);
            let mut batches = received_block.batch_blobs.into_iter();
            let mut proofs = received_block.proof_blobs.into_iter();

            for (blob, sender) in expected_block {
                let (mut received_blob, submitted_data) = match blob {
                    TestBlob::Batch(submitted_batch) => {
                        let received_batch =
                            batches.next().expect("Missed batch data in received block");
                        (received_batch, submitted_batch)
                    }
                    TestBlob::Proof(submitted_proof) => {
                        let received_proof =
                            proofs.next().expect("Missed proof data in received block");
                        (received_proof, submitted_proof)
                    }
                };

                assert_eq!(
                    sender, &received_blob.address,
                    "Sender mismatch in received blob"
                );
                assert_eq!(&submitted_data[..], received_blob.full_data());
            }

            // No extra more batches were in received block
            assert!(batches.next().is_none());
            assert!(proofs.next().is_none());
        }
        Ok(())
    }

    fn get_finalized_headers_collector(
        da: &StorableMockDaLayer,
        expected_num_headers: usize,
    ) -> JoinHandle<Vec<MockBlockHeader>> {
        let mut receiver = da.finalized_header_sender.subscribe();
        // All finalized headers should be pushed by that time.
        let timeout_duration = Duration::from_millis(1000);
        tokio::spawn(async move {
            let mut received = Vec::with_capacity(expected_num_headers);
            for _ in 0..=expected_num_headers {
                match time::timeout(timeout_duration, receiver.recv()).await {
                    Ok(Ok(header)) => received.push(header),
                    _ => break,
                }
            }
            received
        })
    }

    // Gets vector of blocks. Block contains Vec of blobs and sender
    async fn submit_blobs_and_restart(
        connection_string: &str,
        blocks: Vec<Vec<(TestBlob, MockAddress)>>,
    ) -> anyhow::Result<()> {
        // Iteration 1, submit and check.
        {
            let mut da_layer =
                StorableMockDaLayer::new_from_connection(connection_string, 0).await?;
            let finalized_headers_collector =
                get_finalized_headers_collector(&da_layer, blocks.len());
            let mut prev_head_block_header = GENESIS_HEADER;
            for block in &blocks {
                for (blob, sender) in block {
                    match blob {
                        TestBlob::Batch(batch) => {
                            da_layer.submit_batch(batch, sender).await?;
                        }
                        TestBlob::Proof(proof) => {
                            da_layer.submit_proof(proof, sender).await?;
                        }
                    }
                }
                da_layer.produce_block().await?;
                let head_block_header = da_layer.get_head_block_header().await.unwrap();
                assert_eq!(
                    prev_head_block_header.height() + 1,
                    head_block_header.height()
                );
                assert_eq!(prev_head_block_header.hash(), head_block_header.prev_hash());
                prev_head_block_header = head_block_header;
            }
            check_expected_blobs(&da_layer, &blocks).await?;
            let finalized_headers = finalized_headers_collector.await?;
            assert_eq!(
                blocks.len(),
                finalized_headers.len(),
                "Incorrect number of finalized headers received"
            );
            let mut prev_block_hash = GENESIS_HEADER.hash;
            for (idx, header) in finalized_headers.iter().enumerate() {
                assert_eq!(idx as u64 + 1, header.height());
                assert_eq!(prev_block_hash, header.prev_hash());
                prev_block_hash = header.hash;
            }
        }

        // Iteration 2, load from disk and check.
        {
            // Open from disk again.
            let da_layer = StorableMockDaLayer::new_from_connection(connection_string, 0).await?;
            check_expected_blobs(&da_layer, &blocks).await?;
        }

        Ok(())
    }

    fn check_block_batch(block: &mut MockBlock, idx: usize, expected: &[u8]) {
        let batch = block.batch_blobs.get_mut(idx).unwrap();
        assert_eq!(expected, batch.full_data());
    }

    fn check_block_proof(block: &mut MockBlock, idx: usize, expected: &[u8]) {
        let proof = block.proof_blobs.get_mut(idx).unwrap();
        assert_eq!(expected, proof.full_data());
    }

    #[tokio::test]
    async fn empty_layer() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;

        let da_layer = StorableMockDaLayer::new_in_path(tempdir.path(), 0).await?;

        let head_block_header = da_layer.get_head_block_header().await?;
        assert_eq!(GENESIS_HEADER, head_block_header);
        let head_block = da_layer.get_block_at(GENESIS_HEADER.height as u32).await?;
        assert_eq!(GENESIS_BLOCK, head_block);
        let last_finalized_height = da_layer.get_last_finalized_height();
        assert_eq!(0, last_finalized_height);

        // Non existing
        let response = da_layer.get_block_at(1).await;
        assert!(response.is_err());
        assert_eq!(
            "Block at height 1 has not been produced yet",
            response.unwrap_err().to_string()
        );

        let response = da_layer.get_header_at(1).await;
        assert!(response.is_err());
        assert_eq!(
            "Block at height 1 has not been produced yet",
            response.unwrap_err().to_string()
        );

        Ok(())
    }

    #[tokio::test]
    async fn submit_batches_and_restart_regular_sqlite() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let db_path = tempdir.path().join("mock_da.sqlite");
        let connection_string = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

        let sender_1 = MockAddress::new([1; 32]);
        let sender_2 = MockAddress::new([2; 32]);

        // Blobs per each block, with sender
        let expected_blocks = vec![
            // Block 1
            vec![
                (TestBlob::Batch(vec![1, 1, 1, 1]), sender_1),
                (TestBlob::Batch(vec![1, 1, 2, 2]), sender_2),
            ],
            // Block 2
            vec![
                (TestBlob::Batch(vec![2, 2, 1, 1]), sender_1),
                (TestBlob::Batch(vec![2, 2, 2, 2]), sender_2),
                (TestBlob::Batch(vec![2, 2, 3, 3]), sender_1),
            ],
        ];

        submit_blobs_and_restart(&connection_string, expected_blocks).await
    }

    #[tokio::test]
    async fn submit_batches_and_restart_with_empty_blocks() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let db_path = tempdir.path().join("mock_da.sqlite");
        let connection_string = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

        let sender_1 = MockAddress::new([1; 32]);

        let expected_blocks = vec![
            // Block 1
            vec![(TestBlob::Batch(vec![1, 1, 1, 1]), sender_1)],
            // Block 2
            Vec::new(),
            // Block 3,
            Vec::new(),
            // Block 4
            vec![
                (TestBlob::Batch(vec![4, 4, 1, 1]), sender_1),
                (TestBlob::Batch(vec![4, 4, 3, 3]), sender_1),
            ],
        ];

        submit_blobs_and_restart(&connection_string, expected_blocks).await
    }

    #[tokio::test]
    async fn submit_batches_and_proofs_and_restart_regular() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let db_path = tempdir.path().join("mock_da.sqlite");
        let connection_string = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
        let sender_1 = MockAddress::new([1; 32]);
        let sender_2 = MockAddress::new([2; 32]);
        let sender_3 = MockAddress::new([3; 32]);

        // Blobs per each block, with sender
        let expected_blocks = vec![
            // Block 1
            vec![
                (TestBlob::Batch(vec![1, 1, 1, 1]), sender_1),
                (TestBlob::Proof(vec![1, 1, 2, 2]), sender_2),
                (TestBlob::Batch(vec![1, 1, 3, 3]), sender_2),
                (TestBlob::Batch(vec![1, 1, 4, 4]), sender_3),
                (TestBlob::Proof(vec![1, 1, 5, 5]), sender_1),
            ],
            // Block 2
            vec![
                (TestBlob::Batch(vec![2, 2, 1, 1]), sender_1),
                (TestBlob::Proof(vec![2, 2, 2, 2]), sender_2),
                (TestBlob::Batch(vec![2, 2, 3, 3]), sender_2),
                (TestBlob::Proof(vec![2, 2, 4, 4]), sender_3),
                (TestBlob::Batch(vec![2, 2, 5, 5]), sender_1),
            ],
        ];

        submit_blobs_and_restart(&connection_string, expected_blocks).await
    }

    #[tokio::test]
    async fn close_before_producing_block() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let sender_1 = MockAddress::new([1; 32]);

        let batch_1 = vec![1, 1, 1, 1];
        let batch_2 = vec![1, 1, 2, 2];
        let batch_3 = vec![1, 1, 3, 3];
        let batch_4 = vec![1, 1, 4, 4];
        let proof_1 = vec![2, 2, 1, 1];
        let proof_2 = vec![2, 2, 2, 2];
        let proof_3 = vec![2, 2, 3, 3];
        let proof_4 = vec![2, 2, 4, 4];

        {
            let da_layer = StorableMockDaLayer::new_in_path(tempdir.path(), 0).await?;
            check_da_layer_consistency(&da_layer).await?;
            da_layer.submit_batch(&batch_1, &sender_1).await?;
            da_layer.submit_proof(&proof_1, &sender_1).await?;
        }
        {
            let mut da_layer = StorableMockDaLayer::new_in_path(tempdir.path(), 0).await?;
            da_layer.submit_batch(&batch_2, &sender_1).await?;
            da_layer.submit_proof(&proof_2, &sender_1).await?;
            da_layer.produce_block().await?;
            check_da_layer_consistency(&da_layer).await?;
        }
        {
            let da_layer = StorableMockDaLayer::new_in_path(tempdir.path(), 0).await?;
            check_da_layer_consistency(&da_layer).await?;
            da_layer.submit_batch(&batch_3, &sender_1).await?;
            da_layer.submit_proof(&proof_3, &sender_1).await?;
        }
        {
            let mut da_layer = StorableMockDaLayer::new_in_path(tempdir.path(), 0).await?;
            da_layer.submit_batch(&batch_4, &sender_1).await?;
            da_layer.submit_proof(&proof_4, &sender_1).await?;
            da_layer.produce_block().await?;
            check_da_layer_consistency(&da_layer).await?;
        }
        // Checking
        {
            let da_layer = StorableMockDaLayer::new_in_path(tempdir.path(), 0).await?;
            check_da_layer_consistency(&da_layer).await?;
            let head_block_header = da_layer.get_head_block_header().await?;
            assert_eq!(2, head_block_header.height());
            let mut block_1 = da_layer.get_block_at(1).await?;
            assert_eq!(2, block_1.batch_blobs.len());
            assert_eq!(2, block_1.proof_blobs.len());
            check_block_batch(&mut block_1, 0, &batch_1[..]);
            check_block_batch(&mut block_1, 1, &batch_2[..]);
            check_block_proof(&mut block_1, 0, &proof_1[..]);
            check_block_proof(&mut block_1, 1, &proof_2[..]);

            let mut block_2 = da_layer.get_block_at(2).await?;
            check_block_batch(&mut block_2, 0, &batch_3[..]);
            check_block_batch(&mut block_2, 1, &batch_4[..]);
            check_block_proof(&mut block_2, 0, &proof_3[..]);
            check_block_proof(&mut block_2, 1, &proof_4[..]);
        }

        Ok(())
    }

    fn is_docker_running() -> bool {
        Command::new("docker")
            .arg("version")
            .output()
            .map_or(false, |output| output.status.success())
    }

    #[tokio::test]
    #[cfg_attr(not(feature = "postgres"), ignore)]
    async fn test_postgresql_existing() -> anyhow::Result<()> {
        if !is_docker_running() {
            eprintln!("Docker is not running, skipping test.");
            return Ok(());
        }

        let node = Postgres::default().start().await?;

        // prepare connection string
        let connection_string = &format!(
            "postgres://postgres:postgres@127.0.0.1:{}/postgres",
            node.get_host_port_ipv4(5432).await?
        );

        let sender_1 = MockAddress::new([1; 32]);
        let sender_2 = MockAddress::new([2; 32]);

        // Blobs per each block, with sender
        let expected_blocks = vec![
            // Block 1
            vec![
                (TestBlob::Batch(vec![1, 1, 1, 1]), sender_1),
                (TestBlob::Batch(vec![1, 1, 2, 2]), sender_2),
            ],
            // Block 2
            vec![
                (TestBlob::Batch(vec![2, 2, 1, 1]), sender_1),
                (TestBlob::Batch(vec![2, 2, 2, 2]), sender_2),
                (TestBlob::Batch(vec![2, 2, 3, 3]), sender_1),
            ],
        ];

        submit_blobs_and_restart(connection_string, expected_blocks).await
    }
}
