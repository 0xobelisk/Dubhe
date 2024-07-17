use std::marker::PhantomData;
use std::sync::Arc;

use sov_modules_api::capabilities::Authenticator;
use sov_modules_api::{BlobData, RawTx};
use sov_rollup_interface::common::{HexHash, HexString};
use sov_rollup_interface::da::BlockHeaderTrait;
use sov_rollup_interface::services::batch_builder::{BatchBuilder, TxHash};
use sov_rollup_interface::services::da::DaService;
use tokio::sync::Mutex;

use super::tx_status::{TxStatus, TxStatusNotifier};
use super::{AcceptTxResponse, SubmittedBatchInfo};

/// Single data structure that manages mempool and batch producing.
pub struct Sequencer<B: BatchBuilder, Da: DaService, Auth: Authenticator>(Arc<Inner<B, Da, Auth>>);

impl<B, Da, Auth> Clone for Sequencer<B, Da, Auth>
where
    B: BatchBuilder,
    Da: DaService,
    Auth: Authenticator,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

struct Inner<B: BatchBuilder, Da: DaService, Auth: Authenticator> {
    batch_builder: Mutex<B>,
    da_service: Da,
    tx_status_notifier: Arc<TxStatusNotifier<Da>>,
    _phantom: PhantomData<Auth>,
}

impl<B, Da, Auth> Sequencer<B, Da, Auth>
where
    B: BatchBuilder + Send + Sync + 'static,
    Da: DaService,
    Da::TransactionId: Clone + Send + Sync + serde::Serialize,
    Auth: Authenticator,
{
    /// Creates new Sequencer from BatchBuilder and DaService
    pub fn new(batch_builder: B, da_service: Da) -> Self {
        Self(Arc::new(Inner {
            batch_builder: Mutex::new(batch_builder),
            da_service,
            tx_status_notifier: Arc::new(TxStatusNotifier::new()),
            _phantom: PhantomData,
        }))
    }

    async fn submit_batch(&self, txs: Vec<Vec<u8>>) -> anyhow::Result<SubmittedBatchInfo> {
        // Acquire the lock before any DA operation, to avoid out-of-order
        // batches and other potential issues.
        let mut batch_builder = self.0.batch_builder.lock().await;

        let mut accept_tx_results = vec![];
        for tx in txs {
            let result = batch_builder.accept_tx(tx.clone()).await.map(|tx_hash| {
                self.0
                    .tx_status_notifier
                    .notify(tx_hash, TxStatus::Submitted);
                AcceptTxResponse {
                    tx,
                    tx_hash: HexHash::new(tx_hash),
                }
            });
            accept_tx_results.push(result);
        }

        tracing::info!("Submit batch request has been received!");

        let da_height = self
            .0
            .da_service
            .get_head_block_header()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch current head: {}", e))?
            .height();

        let blob_txs = batch_builder.get_next_blob(da_height).await?;
        let num_txs = blob_txs.len();

        let mut txs = Vec::with_capacity(num_txs);
        let mut tx_hashes = Vec::with_capacity(num_txs);

        for tx in blob_txs {
            txs.push(RawTx { data: tx.raw_tx });
            tx_hashes.push(tx.hash);
        }

        let batch = BlobData::new_batch(txs);
        let serialized_batch = borsh::to_vec(&batch)?;

        let fee = match self.0.da_service.estimate_fee(serialized_batch.len()).await {
            Ok(fee) => fee,
            Err(e) => anyhow::bail!(
                "failed to submit batch: could not determine appropriate fee rate: {}",
                e
            ),
        };
        let da_tx_id = match self
            .0
            .da_service
            .send_transaction(&serialized_batch, fee)
            .await
        {
            Ok(id) => id,
            Err(e) => anyhow::bail!("failed to submit batch: {}", e),
        };

        for tx_hash in tx_hashes {
            self.0.tx_status_notifier.notify(
                tx_hash,
                TxStatus::Published {
                    da_transaction_id: da_tx_id.clone(),
                },
            );
        }

        Ok(SubmittedBatchInfo { da_height, num_txs })
    }

    async fn accept_tx(&self, tx: Vec<u8>) -> anyhow::Result<TxHash> {
        let mut batch_builder = self.0.batch_builder.lock().await;

        tracing::info!(tx = hex::encode(&tx), "Accepting transaction");
        let tx_hash = batch_builder.accept_tx(tx).await?;
        self.0
            .tx_status_notifier
            .notify(tx_hash, TxStatus::Submitted);

        Ok(tx_hash)
    }

    async fn tx_status(
        &self,
        tx_hash: &TxHash,
    ) -> anyhow::Result<Option<TxStatus<Da::TransactionId>>> {
        let is_in_mempool = self.0.batch_builder.lock().await.contains(tx_hash).await?;

        if is_in_mempool {
            Ok(Some(TxStatus::Submitted))
        } else {
            Ok(self.0.tx_status_notifier.get_cached(tx_hash))
        }
    }
}

mod axum_router {
    use std::sync::OnceLock;

    use axum::extract::ws::WebSocket;
    use axum::extract::{ws, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;
    use serde_with::base64::Base64;
    use serde_with::serde_as;
    use sov_rest_utils::{
        errors, json_obj, preconfigured_router_layers, ApiResult, ErrorObject, Path,
    };
    use tracing::debug;
    use utoipa_swagger_ui::{Config, SwaggerUi};

    use super::*;

    /// This function does a pretty expensive clone of the entire OpenAPI
    /// specification object, so it might be slow.
    pub(crate) fn openapi_spec() -> serde_json::Value {
        static OPENAPI_SPEC: OnceLock<serde_json::Value> = OnceLock::new();

        OPENAPI_SPEC
            .get_or_init(|| {
                let openapi_spec_raw_yaml_contents = include_str!("../openapi-v3.yaml");
                serde_yaml::from_str::<serde_json::Value>(openapi_spec_raw_yaml_contents).unwrap()
            })
            .clone()
    }

    #[serde_as]
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(transparent)]
    pub struct Base64Blob {
        #[serde_as(as = "Base64")]
        blob: Vec<u8>,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct AcceptTx {
        pub body: Base64Blob,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct SubmitBatch {
        pub transactions: Vec<Base64Blob>,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct TxInfo<DaTxId> {
        id: HexString<TxHash>,
        #[serde(flatten)]
        status: TxStatus<DaTxId>,
    }

    // Web server and Axum-related methods.
    impl<B, Da, Auth> Sequencer<B, Da, Auth>
    where
        B: BatchBuilder + Send + Sync + 'static,
        Da: DaService,
        Da::TransactionId: Clone + Send + Sync + serde::Serialize,
        Auth: Authenticator + Send + Sync + 'static,
    {
        /// Creates an Axum router for the sequencer.
        pub fn axum_router(&self, path_prefix: &str) -> axum::Router<Self> {
            preconfigured_router_layers(
                axum::Router::new()
                    // See:
                    // - https://github.com/juhaku/utoipa/issues/599
                    // - https://github.com/juhaku/utoipa/issues/734
                    .merge(
                        SwaggerUi::new("/swagger-ui")
                            .external_url_unchecked("/openapi-v3.yaml", openapi_spec())
                            .config(Config::from(format!("{}/openapi-v3.yaml", path_prefix))),
                    )
                    .route("/txs", axum::routing::post(Self::axum_accept_tx))
                    .route("/txs/:tx_hash", axum::routing::get(Self::axum_get_tx))
                    .route("/txs/:tx_hash/ws", axum::routing::get(Self::axum_get_tx_ws))
                    .route("/batches", axum::routing::post(Self::axum_submit_batch)),
            )
        }

        async fn send_initial_status_to_ws(
            &self,
            tx_hash: TxHash,
            socket: &mut WebSocket,
        ) -> anyhow::Result<()> {
            // Send a messge with the initial status of the transaction,
            // without waiting for it to change for the first time.
            let initial_status = self.tx_status(&tx_hash).await?.unwrap_or(TxStatus::Unknown);
            let ws_msg = ws::Message::Text(serde_json::to_string(&TxInfo {
                id: HexString(tx_hash),
                status: initial_status,
            })?);
            dbg!(&ws_msg);
            socket.send(ws_msg).await?;

            Ok(())
        }

        async fn axum_get_tx_ws(
            sequencer: State<Self>,
            tx_hash: Path<HexString<TxHash>>,
            ws: ws::WebSocketUpgrade,
        ) -> impl IntoResponse {
            let notifier = sequencer.0 .0.tx_status_notifier.clone();
            let mut tx_status_recv = notifier.subscribe(tx_hash.0 .0);

            ws.on_upgrade(move |mut socket| async move {
                sequencer
                    .send_initial_status_to_ws(tx_hash.0 .0, &mut socket)
                    .await
                    .ok();

                while let Ok(tx_status) = tx_status_recv.recv.recv().await {
                    let resource_obj = TxInfo {
                        id: HexString(tx_hash.0 .0),
                        status: tx_status,
                    };
                    let ws_msg = ws::Message::Text(serde_json::to_string(&resource_obj).unwrap());
                    dbg!(&ws_msg);

                    if let Err(error) = socket.send(ws_msg).await {
                        debug!(?error, "WebSocket connection closed (or errored)");
                        break;
                    }
                }
            })
        }

        async fn axum_get_tx(
            sequencer: State<Self>,
            tx_hash: Path<HexString<TxHash>>,
        ) -> ApiResult<TxInfo<Da::TransactionId>> {
            let tx_status = sequencer.0 .0.tx_status_notifier.get_cached(&tx_hash.0 .0);

            if let Some(tx_status) = tx_status {
                Ok(TxInfo {
                    id: HexString(tx_hash.0 .0),
                    status: tx_status,
                }
                .into())
            } else {
                Err(errors::not_found_404("Transaction", tx_hash.0))
            }
        }

        async fn axum_accept_tx(
            sequencer: State<Self>,
            tx: Json<AcceptTx>,
        ) -> ApiResult<TxInfo<Da::TransactionId>> {
            let tx = tx.0.body.blob;
            let authed_tx = Auth::encode(tx)
                .map_err(|e| errors::bad_request_400("Failed to encode transaction", e))?;

            let tx_hash = match sequencer.accept_tx(authed_tx.data).await {
                Ok(tx_hash) => tx_hash,
                Err(err) => {
                    return Err(ErrorObject {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        title: "Failed to submit transaction".to_string(),
                        details: json_obj!({
                            "message": err.to_string(),
                        }),
                    }
                    .into_response());
                }
            };

            Ok(TxInfo {
                id: HexString(tx_hash),
                status: TxStatus::Submitted,
            }
            .into())
        }

        async fn axum_submit_batch(
            sequencer: State<Self>,
            batch: Json<SubmitBatch>,
        ) -> ApiResult<SubmittedBatchInfo> {
            let batch = batch
                .0
                .transactions
                .into_iter()
                .map(|tx| Ok(Auth::encode(tx.blob)?.data))
                .collect::<anyhow::Result<Vec<_>>>()
                .map_err(|e| errors::bad_request_400("Failed to encode transaction(s)", e))?;

            match sequencer.submit_batch(batch).await {
                Ok(info) => Ok(info.into()),
                Err(err) => Err(ErrorObject {
                    status: StatusCode::CONFLICT,
                    title: "Failed to submit batch".to_string(),
                    details: json_obj!({
                        "message": err.to_string(),
                    }),
                }
                .into_response()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use base64::prelude::*;
    use borsh::BorshDeserialize;
    use sov_mock_da::{MockAddress, MockDaService};
    use sov_rollup_interface::da::BlobReaderTrait;
    use sov_rollup_interface::services::batch_builder::TxWithHash;
    use sov_sequencer_json_client::types;
    use sov_test_utils::sequencer::TestSequencerSetup;

    use self::axum_router::openapi_spec;
    use super::*;

    async fn new_sequencer(
        batch_builder: MockBatchBuilder,
    ) -> TestSequencerSetup<MockBatchBuilder> {
        let dir = tempfile::tempdir().unwrap();
        let da_service = MockDaService::new(MockAddress::default());

        TestSequencerSetup::new(dir, da_service, batch_builder)
            .await
            .unwrap()
    }

    /// BatchBuilder used in tests.
    #[derive(Default)]
    pub struct MockBatchBuilder {
        /// Mempool with transactions.
        pub mempool: Vec<Vec<u8>>,
    }

    // It only takes the first byte of the tx, when submits it.
    // This allows to show an effect of batch builder
    #[async_trait]
    impl BatchBuilder for MockBatchBuilder {
        async fn accept_tx(&mut self, tx: Vec<u8>) -> anyhow::Result<TxHash> {
            self.mempool.push(tx);
            Ok([0; 32])
        }

        async fn contains(&self, _tx_hash: &TxHash) -> anyhow::Result<bool> {
            unimplemented!("MockBatchBuilder::contains is not implemented")
        }

        async fn get_next_blob(&mut self, _height: u64) -> anyhow::Result<Vec<TxWithHash>> {
            if self.mempool.is_empty() {
                anyhow::bail!("Mock mempool is empty");
            }
            let txs = std::mem::take(&mut self.mempool)
                .into_iter()
                .map(|raw_tx| TxWithHash {
                    raw_tx,
                    hash: [0; 32],
                })
                .collect();
            Ok(txs)
        }
    }

    #[test]
    fn openapi_spec_is_valid() {
        let _spec = openapi_spec();
    }

    #[tokio::test]
    async fn test_submit_on_empty_mempool() {
        let sequencer = new_sequencer(MockBatchBuilder::default()).await;
        let client = sequencer.client();

        let error_response = client
            .publish_batch(&types::PublishBatchBody {
                transactions: vec![],
            })
            .await
            .unwrap_err();

        dbg!(&error_response);
        assert_eq!(error_response.status().map(|s| s.as_u16()), Some(409));
    }

    #[tokio::test]
    async fn test_submit_happy_path() {
        let tx1 = vec![1, 2, 3];
        let tx2 = vec![3, 4, 5];

        let batch_builder = MockBatchBuilder {
            mempool: vec![tx1.clone(), tx2.clone()],
        };
        let sequencer = new_sequencer(batch_builder).await;

        sequencer
            .client()
            .publish_batch(&types::PublishBatchBody {
                transactions: vec![],
            })
            .await
            .unwrap();

        let mut submitted_block = sequencer.da_service.get_block_at(1).await.unwrap();
        let block_data = submitted_block.batch_blobs[0].full_data();

        let proof_or_batch = BlobData::try_from_slice(block_data).unwrap();

        match proof_or_batch {
            BlobData::Batch(batch) => {
                assert_eq!(batch.txs.len(), 2);
                assert_eq!(tx1, batch.txs[0].data);
                assert_eq!(tx2, batch.txs[1].data);
            }
            BlobData::Proof(_) => panic!("Expected a batch, but got a proof"),
        }
    }

    #[tokio::test]
    async fn test_accept_tx() {
        let batch_builder = MockBatchBuilder { mempool: vec![] };
        let da_service = MockDaService::new(MockAddress::default());

        let sequencer = TestSequencerSetup::new(
            tempfile::tempdir().unwrap(),
            da_service.clone(),
            batch_builder,
        )
        .await
        .unwrap();

        let client = sequencer.client();

        let tx: Vec<u8> = vec![1, 2, 3, 4, 5];

        client
            .accept_tx(&types::AcceptTxBody {
                body: BASE64_STANDARD.encode(&tx),
            })
            .await
            .unwrap();

        client
            .publish_batch(&types::PublishBatchBody {
                transactions: vec![],
            })
            .await
            .unwrap();

        let mut submitted_block = da_service.get_block_at(1).await.unwrap();
        let block_data = submitted_block.batch_blobs[0].full_data();

        let proof_or_batch = BlobData::try_from_slice(block_data).unwrap();

        match proof_or_batch {
            BlobData::Batch(batch) => {
                assert_eq!(tx, batch.txs[0].data);
            }
            BlobData::Proof(_) => panic!("Expected a batch, but got a proof"),
        }
    }
}
