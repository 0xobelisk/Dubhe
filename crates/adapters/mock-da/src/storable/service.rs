//! Data Availability service is a controller of [`StorableMockDaLayer`].
use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use sov_rollup_interface::da::{
    BlobReaderTrait, BlockHeaderTrait, DaSpec, RelevantBlobs, RelevantProofs,
};
use sov_rollup_interface::services::da::{DaService, MaybeRetryable, SlotData};
use tokio::sync::RwLock;
use tokio::time::{interval, sleep};

use crate::storable::layer::StorableMockDaLayer;
use crate::types::WAIT_ATTEMPT_PAUSE;
use crate::{
    MockAddress, MockBlock, MockBlockHeader, MockDaConfig, MockDaSpec, MockDaVerifier, MockFee,
};

const DEFAULT_BLOCK_WAITING_TIME: Duration = Duration::from_secs(120);
// Time to accommodate rare cases of lock waiting time or latency to the database.
const EXTRA_TIME_FOR_MAX_BLOCK: Duration = Duration::from_secs(10);

/// Defines how StorableMockService should produce new blocks.
#[derive(Debug, Clone)]
pub enum BlockProducing {
    /// Produced new block at every time. Not guaranteed to be precise.
    Periodic(Duration),
    /// Produces new block at every submission of a batch/proof.
    /// Means single blob per block.
    /// Inner duration is a timeout for new block to be submitted.
    OnSubmit(Duration),
    /// Block producing is controlled externally.
    Manual,
}

impl BlockProducing {
    fn get_max_waiting_time_for_block(&self) -> Duration {
        match self {
            BlockProducing::OnSubmit(duration) | BlockProducing::Periodic(duration) => {
                *duration + EXTRA_TIME_FOR_MAX_BLOCK
            }
            // Use a large number to prevent infinite blocking.
            BlockProducing::Manual => DEFAULT_BLOCK_WAITING_TIME * 1000,
        }
    }
    /// Spawns periodic block producing. Useful for testing or custom setup.
    fn spawn_block_producing_if_needed(&self, da_layer: Arc<RwLock<StorableMockDaLayer>>) {
        if let BlockProducing::Periodic(duration) = self {
            let duration = *duration;
            tokio::spawn(async move {
                tracing::debug!(interval = ?duration, "Spawning a task for periodic producing");
                loop {
                    {
                        let mut da_layer = da_layer.write().await;
                        let res = da_layer.produce_block().await;
                        match res {
                            Ok(_) => {}
                            Err(err) => {
                                tracing::warn!(error = ?err, "Error producing new block. Will try next time.");
                            }
                        }
                    }
                    tokio::time::sleep(duration).await;
                }
            });
        }
    }
}

/// DaService that works on top of [`StorableMockDaLayer`].
#[derive(Clone)]
pub struct StorableMockDaService {
    sequencer_da_address: MockAddress,
    da_layer: Arc<RwLock<StorableMockDaLayer>>,
    block_producing: BlockProducing,
}

impl StorableMockDaService {
    /// Create new [`StorableMockDaService`] with given address.
    pub fn new(
        sequencer_da_address: MockAddress,
        da_layer: Arc<RwLock<StorableMockDaLayer>>,
        block_producing: BlockProducing,
    ) -> Self {
        Self {
            sequencer_da_address,
            da_layer,
            block_producing,
        }
    }

    /// Creates new [`StorableMockDaService`] with given address.
    /// Block producing happens on blob submission.
    /// Data is stored only in memory.
    /// It is very similar to [`crate::MockDaService`] parameters.
    pub async fn new_in_memory(sequencer_da_address: MockAddress, blocks_to_finality: u32) -> Self {
        let da_layer = StorableMockDaLayer::new_in_memory(blocks_to_finality)
            .await
            .expect("Failed to initialize StorableMockDaLayer");
        let producing = BlockProducing::OnSubmit(DEFAULT_BLOCK_WAITING_TIME);
        Self::new(
            sequencer_da_address,
            Arc::new(RwLock::new(da_layer)),
            producing,
        )
    }

    /// Creates new in memory [`StorableMockDaService`] from [`MockDaConfig`].
    pub async fn from_config(config: MockDaConfig) -> Self {
        let da_layer = StorableMockDaLayer::new_from_connection(
            &config.connection_string,
            config.finalization_blocks,
        )
        .await
        .expect("Failed to initialize StorableMockDaLayer");
        let block_producing = config.block_producing();
        let da_layer = Arc::new(RwLock::new(da_layer));
        block_producing.spawn_block_producing_if_needed(da_layer.clone());
        Self::new(config.sender_address, da_layer, block_producing)
    }

    async fn wait_for_height(&self, height: u32) -> anyhow::Result<()> {
        let start_wait = Instant::now();
        let max_waiting_time = self.block_producing.get_max_waiting_time_for_block();
        let mut interval = interval(WAIT_ATTEMPT_PAUSE);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                   let current_height = self.da_layer.read().await.next_height;
                   if current_height > height {
                        return Ok(())
                    }
                }
                _ = sleep(max_waiting_time.saturating_sub(start_wait.elapsed())) => {
                    anyhow::bail!("No block at height={height} has been sent in {:?}", max_waiting_time);
                }
            }
        }
    }
}

#[async_trait]
impl DaService for StorableMockDaService {
    type Spec = MockDaSpec;
    type Verifier = MockDaVerifier;
    type FilteredBlock = MockBlock;
    type HeaderStream = BoxStream<'static, Result<MockBlockHeader, Self::Error>>;
    type TransactionId = ();
    type Error = MaybeRetryable<anyhow::Error>;
    type Fee = MockFee;

    async fn get_block_at(&self, height: u64) -> Result<Self::FilteredBlock, Self::Error> {
        tracing::trace!(%height, "Getting block at");
        if height > u32::MAX as u64 {
            return Err(MaybeRetryable::Permanent(anyhow::anyhow!(
                "Height {} is too big for StorableMockDaService. Max is {}",
                height,
                u32::MAX
            )));
        }

        let height = height as u32;

        self.wait_for_height(height).await?;

        let da_layer = self.da_layer.read().await;

        let block = da_layer
            .get_block_at(height)
            .await
            .map_err(MaybeRetryable::Transient)?;
        tracing::debug!(block_header = %block.header().display(), "Block retrieved");
        Ok(block)
    }

    async fn get_last_finalized_block_header(
        &self,
    ) -> Result<<Self::Spec as DaSpec>::BlockHeader, Self::Error> {
        self.da_layer
            .read()
            .await
            .get_last_finalized_block_header()
            .await
            .map_err(MaybeRetryable::Transient)
    }

    async fn subscribe_finalized_header(&self) -> Result<Self::HeaderStream, Self::Error> {
        let receiver = self
            .da_layer
            .read()
            .await
            .finalized_header_sender
            .subscribe();

        let stream = futures::stream::unfold(receiver, |mut receiver| async move {
            match receiver.recv().await {
                Ok(header) => Some((Ok(header), receiver)),
                Err(_) => None,
            }
        });

        Ok(stream.boxed())
    }

    async fn get_head_block_header(
        &self,
    ) -> Result<<Self::Spec as DaSpec>::BlockHeader, Self::Error> {
        self.da_layer
            .read()
            .await
            .get_head_block_header()
            .await
            .map_err(MaybeRetryable::Transient)
    }

    fn extract_relevant_blobs(
        &self,
        block: &Self::FilteredBlock,
    ) -> RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction> {
        block.as_relevant_blobs()
    }

    async fn get_extraction_proof(
        &self,
        block: &Self::FilteredBlock,
        _blobs: &RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>,
    ) -> RelevantProofs<
        <Self::Spec as DaSpec>::InclusionMultiProof,
        <Self::Spec as DaSpec>::CompletenessProof,
    > {
        block.get_relevant_proofs()
    }

    async fn send_transaction(
        &self,
        blob: &[u8],
        _fee: Self::Fee,
    ) -> Result<Self::TransactionId, Self::Error> {
        tracing::debug!(batch = hex::encode(blob), "Submitting a batch");
        {
            let da_layer = self.da_layer.read().await;
            da_layer
                .submit_batch(blob, &self.sequencer_da_address)
                .await?;
        }
        if let BlockProducing::OnSubmit(_) = &self.block_producing {
            let mut da_layer = self.da_layer.write().await;
            da_layer.produce_block().await?;
        }
        Ok(())
    }

    async fn send_aggregated_zk_proof(
        &self,
        aggregated_proof_data: &[u8],
        _fee: Self::Fee,
    ) -> Result<Self::TransactionId, Self::Error> {
        tracing::debug!(
            batch = hex::encode(aggregated_proof_data),
            "Submitting an aggregated proof"
        );
        {
            let da_layer = self.da_layer.read().await;
            da_layer
                .submit_proof(aggregated_proof_data, &self.sequencer_da_address)
                .await?;
        }
        // For compatibility with MockDa, produce blocks only on submitting a batch, not proof.
        Ok(())
    }

    async fn get_aggregated_proofs_at(&self, height: u64) -> Result<Vec<Vec<u8>>, Self::Error> {
        let blobs = self.get_block_at(height).await?.proof_blobs;
        Ok(blobs
            .into_iter()
            .map(|mut proof_blob| proof_blob.full_data().to_vec())
            .collect())
    }

    async fn estimate_fee(&self, _blob_size: usize) -> Result<Self::Fee, Self::Error> {
        Ok(MockFee::zero())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rand::Rng;

    use super::*;
    use crate::types::GENESIS_HEADER;

    async fn check_consistency(
        da_service: &StorableMockDaService,
        expected_blobs_count: usize,
    ) -> anyhow::Result<()> {
        let mut prev_block_hash = GENESIS_HEADER.prev_hash;

        let head_block = da_service.get_head_block_header().await?;

        let mut total_blobs_count = 0;
        for height in 0..=head_block.height() {
            let block = da_service.get_block_at(height).await?;
            assert_eq!(height, block.header().height());
            assert_eq!(prev_block_hash, block.header().prev_hash());
            prev_block_hash = block.header().hash();

            total_blobs_count += block.batch_blobs.len() + block.proof_blobs.len();
        }

        assert_eq!(
            expected_blobs_count, total_blobs_count,
            "total blobs count do no match"
        );

        Ok(())
    }

    #[tokio::test]
    async fn multiple_threads_producing_reading() -> anyhow::Result<()> {
        let da_layer = Arc::new(RwLock::new(StorableMockDaLayer::new_in_memory(0).await?));
        let block_time = Duration::from_millis(50);
        let block_producing = BlockProducing::Periodic(block_time);

        block_producing.spawn_block_producing_if_needed(da_layer.clone());

        let services_count = 20;
        let blobs_per_service = 50;

        let mut services_blobs: Vec<Vec<(Duration, Vec<u8>)>> = Vec::with_capacity(services_count);

        let mut rng = rand::thread_rng();
        for i in 0..services_count {
            let mut this_service_blobs = Vec::with_capacity(blobs_per_service);
            for j in 0..blobs_per_service {
                let blob = vec![(i as u8).saturating_mul(j as u8); 8];
                let sleep_time = Duration::from_millis(rng.gen_range(5..=40));
                this_service_blobs.push((sleep_time, blob));
            }
            services_blobs.push(this_service_blobs);
        }

        let mut handlers = Vec::new();
        for (idx, this_service_blobs) in services_blobs.into_iter().enumerate() {
            let this_da_layer = da_layer.clone();
            let this_block_producing = block_producing.clone();
            let address = MockAddress::new([idx as u8; 32]);
            let fee = MockFee::zero();
            handlers.push(tokio::spawn(async move {
                let da_service =
                    StorableMockDaService::new(address, this_da_layer, this_block_producing);
                for (wait, blob) in this_service_blobs {
                    sleep(wait).await;
                    da_service.send_transaction(&blob, fee).await.unwrap();
                }
            }));
        }

        for handler in handlers {
            handler.await?;
        }
        // Sleep extra block time so all blocks are produced.
        sleep(block_time * 2).await;

        let da_service =
            StorableMockDaService::new(MockAddress::new([1; 32]), da_layer, block_producing);
        check_consistency(&da_service, services_count * blobs_per_service).await?;

        Ok(())
    }

    #[tokio::test]
    async fn querying_height_above_u32_max() -> anyhow::Result<()> {
        let producing = BlockProducing::OnSubmit(Duration::from_millis(10));
        let mut service = StorableMockDaService::new_in_memory(MockAddress::new([0; 32]), 0).await;
        service.block_producing = producing;

        let height_1 = u32::MAX as u64;
        let height_2 = u32::MAX as u64 + 1;

        let result_1 = service.get_block_at(height_1).await;
        assert!(result_1.is_err());
        let err = result_1.unwrap_err().to_string();
        assert_eq!("No block at height=4294967295 has been sent in 10.01s", err);

        let result_2 = service.get_block_at(height_2).await;
        assert!(result_2.is_err());
        let err = result_2.unwrap_err().to_string();
        assert_eq!(
            "Height 4294967296 is too big for StorableMockDaService. Max is 4294967295",
            err
        );

        Ok(())
    }
}
