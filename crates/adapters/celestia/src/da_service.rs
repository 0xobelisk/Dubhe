use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use celestia_rpc::prelude::*;
use celestia_types::blob::Blob as JsonBlob;
use celestia_types::consts::appconsts::{
    CONTINUATION_SPARSE_SHARE_CONTENT_SIZE, FIRST_SPARSE_SHARE_CONTENT_SIZE, SHARE_SIZE,
};
use celestia_types::nmt::Namespace;
use celestia_types::state::Uint;
use futures::stream::BoxStream;
use futures::StreamExt;
use jsonrpsee::http_client::{HeaderMap, HttpClient};
use serde::{Deserialize, Serialize};
use sov_rollup_interface::da::{DaProof, RelevantBlobs, RelevantProofs};
use sov_rollup_interface::services::da::{DaService, Fee, MaybeRetryable};
use tokio::sync::Mutex;
use tracing::{debug, info, trace};

use crate::types::{FilteredCelestiaBlock, NamespaceWithShares};
use crate::utils::BoxError;
use crate::verifier::address::CelestiaAddress;
use crate::verifier::proofs::{self};
use crate::verifier::{CelestiaSpec, CelestiaVerifier, RollupParams, TmHash, PFB_NAMESPACE};
use crate::CelestiaHeader;

// https://github.com/celestiaorg/celestia-app/blob/c90e61d5a2d0c0bd0e123df4ab416f6f0d141b7f/pkg/appconsts/initial_consts.go#L16-L18
// `DefaultGasPerBlobByte`
const DEFAULT_GAS_PER_BLOB_BYTE: usize = 8;

// const DefaultTxSizeCostPerByte from cosmos-sdk
// https://github.com/cosmos/cosmos-sdk/blob/d0f6cc6d405fbce4332b5654e60bd6514ee79649/x/auth/types/params.go#L11
const DEFAULT_TX_SIZE_COST_PER_BYTE: usize = 10;

// BytesPerBlobInfo is a rough estimation for the number of extra bytes in
// information a blob adds to the size of the underlying transaction.
// https://github.com/celestiaorg/celestia-app/blob/a92de7236e7568aa1e9032a29a68c64ef751ce0a/x/blob/types/payforblob.go#L41
const BYTES_PER_BLOB_INFO: usize = 70;

// https://github.com/celestiaorg/celestia-app/blob/a92de7236e7568aa1e9032a29a68c64ef751ce0a/x/blob/types/payforblob.go#L37
const PFB_GAS_FIXED_COST: usize = 75_000;

// Second part of summation from here:
// https://github.com/celestiaorg/celestia-app/blob/a92de7236e7568aa1e9032a29a68c64ef751ce0a/x/blob/types/payforblob.go#L172
// (txSizeCost * BytesPerBlobInfo * uint64(len(blobSizes))) + PFBGasFixedCost
// where in our case:
//  * txSizeCost = DEFAULT_TX_SIZE_COST_PER_BYTE;
//  * BytesPerBlobInfo = BYTES_PER_BLOB_INFO
//  * len(blobSizes) = 1;
//  * PFBGasFixedCost = PFB_GAS_FIXED_COST;
const DEFAULT_FIXED_COST_SINGLE_BLOB: usize =
    (DEFAULT_TX_SIZE_COST_PER_BYTE * BYTES_PER_BLOB_INFO) + PFB_GAS_FIXED_COST;

// TODO: set dynamically https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/391
/// The gas price expressed in nano tia for precision. Note that the Celestia packages expect
/// fees demoninated in micro-tia ("uTIA"), so we have to scale by a factor of 1000 before submitting.
const GAS_PRICE_NANO_TIA: u64 = 1000;

#[derive(Debug, Clone)]
pub struct CelestiaService {
    // https://github.com/celestiaorg/celestia-node/issues/3192
    client: Arc<Mutex<HttpClient>>,
    rollup_batch_namespace: Namespace,
    rollup_proof_namespace: Namespace,
}

impl CelestiaService {
    pub fn with_client(
        client: HttpClient,
        rollup_batch_namespace: Namespace,
        rollup_proof_namespace: Namespace,
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            rollup_batch_namespace,
            rollup_proof_namespace,
        }
    }

    async fn submit_blob_to_namespace(
        &self,
        blob: &[u8],
        fee: CelestiaFee,
        namespace: Namespace,
    ) -> anyhow::Result<TmHash> {
        let bytes = blob.len();
        debug!(bytes, ?fee, ?namespace, "Sending raw data to Celestia");

        let blob = JsonBlob::new(namespace, blob.to_vec())?;
        info!(
            commitment = hex::encode(blob.commitment.0),
            ?fee,
            bytes,
            "Submitting a blob"
        );

        let tx_response = self
            .client
            .lock()
            .await
            .state_submit_pay_for_blob(fee.get_fee(), fee.gas_limit, &[blob])
            .await?;

        let tx_hash = TmHash(
            celestia_tendermint::Hash::from_str(&tx_response.txhash)
                .expect("Failed to decode hash from `TxResponse`"),
        );

        info!(
            height = tx_response.height,
            tx_hash = ?tx_hash,
            gas_used = %tx_response.gas_used,
            gas_wanted = %tx_response.gas_wanted,
            code = %tx_response.code,
            "Blob has been submitted to Celestia"
        );
        Ok(tx_hash)
    }
}

/// Runtime configuration for the [`DaService`] implementation.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CelestiaConfig {
    /// The JWT used to authenticate with the Celestia RPC server
    pub celestia_rpc_auth_token: String,
    /// The address of the Celestia RPC server
    #[serde(default = "default_rpc_addr")]
    pub celestia_rpc_address: String,
    /// The maximum size of a Celestia RPC response, in bytes
    #[serde(default = "default_max_response_size")]
    pub max_celestia_response_body_size: u32,
    /// The timeout for a Celestia RPC request, in seconds
    #[serde(default = "default_request_timeout_seconds")]
    pub celestia_rpc_timeout_seconds: u64,
    /// Celestia address of connected node. Used as sequencer address in case of sequencer presented
    pub own_celestia_address: CelestiaAddress,
}

fn default_rpc_addr() -> String {
    "http://localhost:11111/".into()
}

fn default_max_response_size() -> u32 {
    1024 * 1024 * 100 // 100 MB
}

const fn default_request_timeout_seconds() -> u64 {
    60
}

impl CelestiaService {
    pub async fn new(config: CelestiaConfig, chain_params: RollupParams) -> Self {
        let client = {
            let mut headers = HeaderMap::new();
            headers.insert(
                "Authorization",
                format!("Bearer {}", config.celestia_rpc_auth_token)
                    .parse()
                    .unwrap(),
            );

            jsonrpsee::http_client::HttpClientBuilder::default()
                .set_headers(headers)
                .max_response_size(config.max_celestia_response_body_size)
                .max_request_size(config.max_celestia_response_body_size)
                .request_timeout(std::time::Duration::from_secs(
                    config.celestia_rpc_timeout_seconds,
                ))
                .build(&config.celestia_rpc_address)
        }
        .expect("Client initialization is valid");

        Self::with_client(
            client,
            chain_params.rollup_batch_namespace,
            chain_params.rollup_proof_namespace,
        )
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// The fee for submitting a transaction to Celestia.
pub struct CelestiaFee {
    /// The fee rate (in nano-tia per gas).
    fee_per_gas: u64,
    /// The gas limit for the transaction.
    gas_limit: u64,
}

impl CelestiaFee {
    pub(crate) fn estimated(blob_size: usize) -> Self {
        CelestiaFee {
            fee_per_gas: GAS_PRICE_NANO_TIA,
            gas_limit: get_gas_limit_for_bytes_as_in_golang(blob_size) as u64,
        }
    }

    /// Get full fee in uTIA
    pub(crate) fn get_fee(&self) -> Uint {
        // divide by 1000 to convert to the expected uTIA
        Uint::from((self.gas_limit.saturating_mul(self.fee_per_gas)) / GAS_PRICE_NANO_TIA)
    }
}

impl Fee for CelestiaFee {
    type FeeRate = u64;

    fn fee_rate(&self) -> Self::FeeRate {
        self.fee_per_gas
    }

    fn set_fee_rate(&mut self, rate: Self::FeeRate) {
        self.fee_per_gas = rate;
    }

    fn gas_estimate(&self) -> u64 {
        self.gas_limit
    }
}

#[async_trait]
impl DaService for CelestiaService {
    type Spec = CelestiaSpec;

    type Verifier = CelestiaVerifier;

    type FilteredBlock = FilteredCelestiaBlock;
    type HeaderStream = BoxStream<'static, Result<CelestiaHeader, Self::Error>>;
    type TransactionId = TmHash;
    type Error = MaybeRetryable<BoxError>;
    type Fee = CelestiaFee;

    async fn get_block_at(&self, height: u64) -> Result<Self::FilteredBlock, Self::Error> {
        let client = self.client.lock().await;

        // Fetch the header and relevant shares via RPC
        debug!(height, "Fetching header at height...");
        let header = client
            .header_get_by_height(height)
            .await
            .map_err(|e| MaybeRetryable::Transient(e.into()))?;
        trace!(?header, height, "Got the header");

        // Fetch the rollup namespace shares, etx data and extended data square
        debug!("Fetching rollup data...");

        let etx_rows_future = client.share_get_shares_by_namespace(&header, PFB_NAMESPACE);
        let data_square_future = client.share_get_eds(&header);

        let rollup_batch_rows_future =
            client.share_get_shares_by_namespace(&header, self.rollup_batch_namespace);

        let rollup_proof_rows_future =
            client.share_get_shares_by_namespace(&header, self.rollup_proof_namespace);

        let (batch_rows, proof_rows, etx_rows, data_square) = tokio::try_join!(
            rollup_batch_rows_future,
            rollup_proof_rows_future,
            etx_rows_future,
            data_square_future
        )
        .map_err(|e| MaybeRetryable::Transient(e.into()))?;

        let rollup_batch_shares = NamespaceWithShares {
            namespace: self.rollup_batch_namespace,
            rows: batch_rows,
        };

        let rollup_proof_shares = NamespaceWithShares {
            namespace: self.rollup_proof_namespace,
            rows: proof_rows,
        };

        FilteredCelestiaBlock::new(
            rollup_batch_shares,
            rollup_proof_shares,
            header,
            etx_rows,
            data_square,
        )
        .map_err(MaybeRetryable::Permanent)
    }

    async fn get_last_finalized_block_header(
        &self,
    ) -> Result<<Self::Spec as sov_rollup_interface::da::DaSpec>::BlockHeader, Self::Error> {
        // Tendermint has instant finality, so head block is the one that finalized
        // and network is always guaranteed to be secure,
        // it can work even if the node is still catching up
        self.get_head_block_header().await
    }

    async fn subscribe_finalized_header(&self) -> Result<Self::HeaderStream, Self::Error> {
        Ok(self
            .client
            .lock()
            .await
            .header_subscribe()
            .await
            .map_err(|e| MaybeRetryable::Transient(e.into()))?
            .map(|res| {
                res.map(CelestiaHeader::from)
                    .map_err(|e| MaybeRetryable::Permanent(e.into()))
            })
            .boxed())
    }

    async fn get_head_block_header(
        &self,
    ) -> Result<<Self::Spec as sov_rollup_interface::da::DaSpec>::BlockHeader, Self::Error> {
        let header = self
            .client
            .lock()
            .await
            .header_network_head()
            .await
            .map_err(|e| MaybeRetryable::Transient(e.into()))?;
        Ok(CelestiaHeader::from(header))
    }

    fn extract_relevant_blobs(
        &self,
        block: &Self::FilteredBlock,
    ) -> RelevantBlobs<<Self::Spec as sov_rollup_interface::da::DaSpec>::BlobTransaction> {
        let proof_blobs = block.rollup_proof_data.get_blob_with_sender();
        let batch_blobs = block.rollup_batch_data.get_blob_with_sender();
        RelevantBlobs {
            proof_blobs,
            batch_blobs,
        }
    }

    async fn get_extraction_proof(
        &self,
        block: &Self::FilteredBlock,
        blobs: &RelevantBlobs<<Self::Spec as sov_rollup_interface::da::DaSpec>::BlobTransaction>,
    ) -> RelevantProofs<
        <Self::Spec as sov_rollup_interface::da::DaSpec>::InclusionMultiProof,
        <Self::Spec as sov_rollup_interface::da::DaSpec>::CompletenessProof,
    > {
        let batch = {
            let inclusion_proof = proofs::new_inclusion_proof(
                &block.header,
                &block.pfb_rows,
                &block.rollup_batch_data,
                &blobs.batch_blobs,
            );

            DaProof {
                inclusion_proof,
                completeness_proof: block.rollup_batch_data.rows.clone(),
            }
        };

        let proof = {
            // Note: The second call to new_inclusion_proof merklizes and parse the executable transactions namespace again.
            let inclusion_proof = proofs::new_inclusion_proof(
                &block.header,
                &block.pfb_rows,
                &block.rollup_proof_data,
                &blobs.proof_blobs,
            );

            DaProof {
                inclusion_proof,
                completeness_proof: block.rollup_proof_data.rows.clone(),
            }
        };

        RelevantProofs { proof, batch }
    }

    async fn send_transaction(
        &self,
        blob: &[u8],
        fee: Self::Fee,
    ) -> Result<Self::TransactionId, Self::Error> {
        debug!("Submitting batch of transactions to Celestia");
        self.submit_blob_to_namespace(blob, fee, self.rollup_batch_namespace)
            .await
            .map_err(MaybeRetryable::Transient)
    }

    async fn send_aggregated_zk_proof(
        &self,
        aggregated_proof: &[u8],
        fee: Self::Fee,
    ) -> Result<Self::TransactionId, Self::Error> {
        debug!("Submitting aggregated proof to Celestia");
        self.submit_blob_to_namespace(aggregated_proof, fee, self.rollup_proof_namespace)
            .await
            .map_err(MaybeRetryable::Transient)
    }

    async fn get_aggregated_proofs_at(&self, height: u64) -> Result<Vec<Vec<u8>>, Self::Error> {
        let blobs = self
            .client
            .lock()
            .await
            .blob_get_all(height, &[self.rollup_proof_namespace])
            .await;

        match blobs {
            Ok(blobs) => Ok(blobs.into_iter().map(|blob| blob.data).collect()),
            // If 'Celestia' encounters a missing blob, it returns an error instead of an "empty" result.
            // Thus, we address this scenario here.
            // https://github.com/celestiaorg/celestia-node/issues/3192
            Err(e) => {
                info!("Get aggregated proof error (might happen if there is no blobs, that's expected): {}", e);
                Ok(Vec::default())
            }
        }
    }

    async fn estimate_fee(&self, blob_size: usize) -> Result<Self::Fee, Self::Error> {
        Ok(CelestiaFee::estimated(blob_size))
    }
}

// ------------------------------------------------------------------------
// ------------------------------------------------------------------------
// ------------------------------------------------------------------------
/// How many Celestia shares is needed to represent payload of this size.
/// Celestia has two types of shares:
///  1. The first has extra metadata about the size of payload
///  2. Continuation shares have namespace and info bytes.
/// Technically, we rely on constants about size,
/// and it should be good as long as there are only two types of shares.
///
fn shares_needed_for_bytes(payload_bytes: usize) -> usize {
    debug_assert_ne!(
        CONTINUATION_SPARSE_SHARE_CONTENT_SIZE, 0,
        "Something wrong with celestia lib"
    );
    debug_assert_ne!(
        FIRST_SPARSE_SHARE_CONTENT_SIZE, 0,
        "Something wrong with celestia lib"
    );
    if payload_bytes == 0 {
        return 0;
    }
    if payload_bytes <= FIRST_SPARSE_SHARE_CONTENT_SIZE {
        return 1;
    }
    // we use unchecked subtraction, as we did an explicit check 2 lines before
    let remaining_payload = payload_bytes - FIRST_SPARSE_SHARE_CONTENT_SIZE;

    let additional_shares = remaining_payload
        .saturating_add(CONTINUATION_SPARSE_SHARE_CONTENT_SIZE - 1)
        / CONTINUATION_SPARSE_SHARE_CONTENT_SIZE;

    additional_shares.saturating_add(1)
}

// // DefaultEstimateGas runs EstimateGas with the system defaults. The network may change these values
// // through governance, thus this function should predominantly be used in testing.
// func DefaultEstimateGas(blobSizes []uint32) uint64 {
// 	return EstimateGas(blobSizes, appconsts.DefaultGasPerBlobByte, auth.DefaultTxSizeCostPerByte)
// }
// func EstimateGas(blobSizes []uint32, gasPerByte uint32, txSizeCost uint64) uint64 {
// 	return GasToConsume(blobSizes, gasPerByte) + (txSizeCost * BytesPerBlobInfo * uint64(len(blobSizes))) + PFBGasFixedCost
// }
//
// // GasToConsume works out the extra gas charged to pay for a set of blobs in a PFB.
// // Note that transactions will incur other gas costs, such as the signature verification
// // and reads to the user's account.
// func GasToConsume(blobSizes []uint32, gasPerByte uint32) uint64 {
// 	var totalSharesUsed uint64
// 	for _, size := range blobSizes {
// 		totalSharesUsed += uint64(appshares.SparseSharesNeeded(size))
// 	}
//
// 	return totalSharesUsed * appconsts.ShareSize * uint64(gasPerByte)
// }
// Calculates conservatively as if blob will be the only one in whole DA slot
fn get_gas_limit_for_bytes_as_in_golang(payload_size: usize) -> usize {
    gas_to_consume_from_data(payload_size, DEFAULT_GAS_PER_BLOB_BYTE)
        .saturating_add(DEFAULT_FIXED_COST_SINGLE_BLOB)
}

// Similar to GasToConsume
#[allow(dead_code)]
fn gas_to_consume_from_data(bytes: usize, gas_per_byte: usize) -> usize {
    let shares_needed = shares_needed_for_bytes(bytes);
    shares_needed
        .saturating_mul(SHARE_SIZE)
        .saturating_mul(gas_per_byte)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::time::Duration;

    use celestia_types::nmt::Namespace;
    use celestia_types::state::Uint;
    use celestia_types::Blob as JsonBlob;
    use serde_json::json;
    use sov_rollup_interface::da::{BlockHeaderTrait, DaVerifier, RelevantBlobs};
    use sov_rollup_interface::services::da::DaService;
    use wiremock::matchers::{bearer_token, body_json, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::{
        default_request_timeout_seconds, get_gas_limit_for_bytes_as_in_golang,
        shares_needed_for_bytes,
    };
    use crate::da_service::{CelestiaConfig, CelestiaFee, CelestiaService, GAS_PRICE_NANO_TIA};
    use crate::test_helper::files::*;
    use crate::types::FilteredCelestiaBlock;
    use crate::verifier::address::CelestiaAddress;
    use crate::verifier::{CelestiaVerifier, RollupParams};

    async fn setup_test_service(
        timeout_sec: Option<u64>,
    ) -> (MockServer, CelestiaConfig, CelestiaService, RollupParams) {
        setup_service(
            timeout_sec,
            CelestiaAddress::from_str("celestia1a68m2l85zn5xh0l07clk4rfvnezhywc53g8x7s").unwrap(),
            ROLLUP_BATCH_NAMESPACE,
            ROLLUP_PROOF_NAMESPACE,
        )
        .await
    }

    // Last return value is namespace
    async fn setup_service(
        timeout_sec: Option<u64>,
        celestia_address: CelestiaAddress,
        rollup_batch_namespace: Namespace,
        rollup_proof_namespace: Namespace,
    ) -> (MockServer, CelestiaConfig, CelestiaService, RollupParams) {
        // Start a background HTTP server on a random local port
        let mock_server = MockServer::start().await;

        let timeout_sec = timeout_sec.unwrap_or_else(default_request_timeout_seconds);
        let config = CelestiaConfig {
            celestia_rpc_auth_token: "RPC_TOKEN".to_string(),
            celestia_rpc_address: mock_server.uri(),
            max_celestia_response_body_size: 120_000,
            celestia_rpc_timeout_seconds: timeout_sec,
            own_celestia_address: celestia_address,
        };

        let params = RollupParams {
            rollup_batch_namespace,
            rollup_proof_namespace,
        };

        let da_service = CelestiaService::new(config.clone(), params.clone()).await;

        (mock_server, config, da_service, params)
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct BasicJsonRpcRequest {
        jsonrpc: String,
        id: u64,
        method: String,
        params: serde_json::Value,
    }

    #[tokio::test]
    async fn test_submit_blob_correct() -> anyhow::Result<()> {
        let (mock_server, config, da_service, rollup_params) = setup_test_service(None).await;

        let blob = [1, 2, 3, 4, 5, 11, 12, 13, 14, 15];
        let gas_limit = get_gas_limit_for_bytes_as_in_golang(blob.len());

        // TODO: Fee is hardcoded for now https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/382
        let expected_body = json!({
            "id": 0,
            "jsonrpc": "2.0",
            "method": "state.SubmitPayForBlob",
            "params": [
                Uint::from(gas_limit * (GAS_PRICE_NANO_TIA / 1000) as usize),
                gas_limit,
                [JsonBlob::new(rollup_params.rollup_batch_namespace, blob.to_vec()).unwrap()],
            ]
        });
        Mock::given(method("POST"))
            .and(path("/"))
            .and(bearer_token(config.celestia_rpc_auth_token))
            .and(body_json(&expected_body))
            .respond_with(|req: &Request| {
                let request: BasicJsonRpcRequest = serde_json::from_slice(&req.body).unwrap();
                // Empty strings is what was observed with actual celestia 0.12.0
                let response_json = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": {
                        "height": 30497,
                        "txhash": "05D9016060072AA71B007A6CFB1B895623192D6616D513017964C3BFCD047282",
                        "codespace": "",
                        "code": 0,
                        "data": "12260A242F636F736D6F732E62616E6B2E763162657461312E4D736753656E64526573706F6E7365",
                        "raw_log": "[]",
                        "logs": [],
                        "info": "",
                        "gas_wanted": 10000000,
                        "gas_used": 69085,
                        "timestamp": "",
                        "events": [],
                    }
                });

                ResponseTemplate::new(200)
                    .append_header("Content-Type", "application/json")
                    .set_body_json(response_json)
            })
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;
        let fee = CelestiaFee::estimated(blob.len());
        da_service.send_transaction(&blob, fee).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_submit_blob_application_level_error() -> anyhow::Result<()> {
        // Our calculation of gas is off and the gas limit exceeded, for example
        let (mock_server, _config, da_service, _namespace) = setup_test_service(None).await;

        let blob: Vec<u8> = vec![1, 2, 3, 4, 5, 11, 12, 13, 14, 15];

        // Do not check API token or expected body here.
        // Only interested in behaviour on response
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(|req: &Request| {
                let request: BasicJsonRpcRequest = serde_json::from_slice(&req.body).unwrap();
                let response_json = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "error": {
                        "code": 1,
                        "message": ": out of gas"
                    }
                });
                ResponseTemplate::new(200)
                    .append_header("Content-Type", "application/json")
                    .set_body_json(response_json)
            })
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let fee = CelestiaFee {
            fee_per_gas: GAS_PRICE_NANO_TIA,
            gas_limit: 1,
        };
        let error = da_service
            .send_transaction(&blob, fee)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("out of gas"));
        Ok(())
    }

    #[tokio::test]
    async fn test_submit_blob_internal_server_error() -> anyhow::Result<()> {
        let (mock_server, _config, da_service, _namespace) = setup_test_service(None).await;

        let error_response = ResponseTemplate::new(500).set_body_bytes("Internal Error".as_bytes());

        let blob: Vec<u8> = vec![1, 2, 3, 4, 5, 11, 12, 13, 14, 15];

        // Do not check API token or expected body here.
        // Only interested in behaviour on response
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(error_response)
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let fee = CelestiaFee::estimated(blob.len());
        let error = da_service
            .send_transaction(&blob, fee)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains(
            "Networking or low-level protocol error: Server returned an error status code: 500"
        ));
        Ok(())
    }

    #[tokio::test]
    async fn test_submit_blob_response_timeout() -> anyhow::Result<()> {
        let timeout = 1;
        let (mock_server, _config, da_service, _namespace) =
            setup_test_service(Some(timeout)).await;

        let response_json = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "result": {
                "data": "122A0A282F365",
                "events": ["some event"],
                "gas_used": 70522,
                "gas_wanted": 133540,
                "height": 26,
                "logs":  [],
                "raw_log": "",
                "txhash": "C9FEFD6D35FCC73F9E7D5C74E1D33F0B7666936876F2AD75E5D0FB2944BFADF2"
            }
        });

        let error_response = ResponseTemplate::new(200)
            .append_header("Content-Type", "application/json")
            .set_delay(Duration::from_secs(timeout) + Duration::from_millis(100))
            .set_body_json(response_json);

        let blob: Vec<u8> = vec![1, 2, 3, 4, 5, 11, 12, 13, 14, 15];
        let fee = CelestiaFee::estimated(blob.len());

        // Do not check API token or expected body here.
        // Only interested in behaviour on response
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(error_response)
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let error = da_service
            .send_transaction(&blob, fee)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("Request timeout"));
        Ok(())
    }

    #[tokio::test]
    async fn verification_succeeds_for_correct_blocks() {
        let blocks = [
            with_rollup_batch_data::filtered_block(),
            without_rollup_batch_data::filtered_block(),
        ];

        for block in blocks {
            let (_, _, da_service, rollup_params) = setup_test_service(None).await;

            let relevant_blobs = da_service.extract_relevant_blobs(&block);
            let relevant_proofs = da_service
                .get_extraction_proof(&block, &relevant_blobs)
                .await;

            let verifier = CelestiaVerifier::new(rollup_params);

            let validity_cond = verifier
                .verify_relevant_tx_list(&block.header, &relevant_blobs, relevant_proofs)
                .unwrap();

            assert_eq!(validity_cond.prev_hash, *block.header.prev_hash().inner());
            assert_eq!(validity_cond.block_hash, *block.header.hash().inner());
        }
    }

    #[tokio::test]
    async fn verification_fails_if_tx_missing() {
        let block = with_rollup_batch_data::filtered_block();
        let (_, _, da_service, rollup_params) = setup_test_service(None).await;

        let relevant_blobs = da_service.extract_relevant_blobs(&block);
        let relevant_proofs = da_service
            .get_extraction_proof(&block, &relevant_blobs)
            .await;

        let verifier = CelestiaVerifier::new(rollup_params);

        let relevant_blobs = RelevantBlobs {
            proof_blobs: Default::default(),
            batch_blobs: Default::default(),
        };
        // give to verifier an empty transactions list
        let error = verifier
            .verify_relevant_tx_list(&block.header, &relevant_blobs, relevant_proofs)
            .unwrap_err();

        assert!(error.to_string().contains("Transaction missing"));
    }

    #[tokio::test]
    async fn verification_fails_if_not_all_etxs_are_proven() {
        let block = with_rollup_batch_data::filtered_block();
        let (_, _, da_service, rollup_params) = setup_test_service(None).await;

        let relevant_blobs = da_service.extract_relevant_blobs(&block);

        let mut relevant_proofs = da_service
            .get_extraction_proof(&block, &relevant_blobs)
            .await;
        // drop the proof for last etx
        relevant_proofs.batch.inclusion_proof.pop();

        let verifier = CelestiaVerifier::new(rollup_params);

        let error = verifier
            .verify_relevant_tx_list(&block.header, &relevant_blobs, relevant_proofs)
            .unwrap_err();

        assert!(error.to_string().contains("not all blobs proven"));
    }

    #[tokio::test]
    async fn test_blobs_from_padded_namespace() {
        let block: FilteredCelestiaBlock = with_namespace_padding::filtered_block();
        let (_, _, da_service, _) = setup_service(
            None,
            CelestiaAddress::from_str("celestia1g2hwtcldcwjnw0cy9ngs9hsewpduq4zuehqlqh").unwrap(),
            with_namespace_padding::ROLLUP_BATCH_NAMESPACE,
            with_namespace_padding::ROLLUP_PROOF_NAMESPACE,
        )
        .await;
        let relevant_blobs = da_service.extract_relevant_blobs(&block);
        assert_eq!(relevant_blobs.batch_blobs.len(), 1);
        assert_eq!(relevant_blobs.proof_blobs.len(), 0);
    }

    // TODO: https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/430
    #[tokio::test]
    #[ignore = "TODO: https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/430"]
    async fn _verification_for_padded_namespace() {
        let block: FilteredCelestiaBlock = with_namespace_padding::filtered_block();
        let (_, _, da_service, rollup_params) = setup_service(
            None,
            CelestiaAddress::from_str("celestia1g2hwtcldcwjnw0cy9ngs9hsewpduq4zuehqlqh").unwrap(),
            with_namespace_padding::ROLLUP_BATCH_NAMESPACE,
            with_namespace_padding::ROLLUP_PROOF_NAMESPACE,
        )
        .await;

        let relevant_blobs = da_service.extract_relevant_blobs(&block);
        let relevant_proofs = da_service
            .get_extraction_proof(&block, &relevant_blobs)
            .await;

        let verifier = CelestiaVerifier::new(rollup_params);

        let _validity_cond = verifier
            .verify_relevant_tx_list(&block.header, &relevant_blobs, relevant_proofs)
            .unwrap();
    }

    #[tokio::test]
    async fn verification_fails_if_there_is_less_blobs_than_proofs() {
        let block = with_rollup_batch_data::filtered_block();
        let (_, _, da_service, rollup_params) = setup_test_service(None).await;

        let relevant_blobs = da_service.extract_relevant_blobs(&block);
        let mut relevant_proofs = da_service
            .get_extraction_proof(&block, &relevant_blobs)
            .await;

        // push one extra etx proof
        relevant_proofs
            .batch
            .inclusion_proof
            .push(relevant_proofs.batch.inclusion_proof[0].clone());

        let verifier = CelestiaVerifier::new(rollup_params);

        let error = verifier
            .verify_relevant_tx_list(&block.header, &relevant_blobs, relevant_proofs)
            .unwrap_err();

        assert!(error.to_string().contains("more proofs than blobs"));
    }

    #[tokio::test]
    #[should_panic]
    async fn verification_fails_for_incorrect_namespace() {
        let block = with_rollup_proof_data::filtered_block();
        let (_, _, da_service, _) = setup_test_service(None).await;

        let relevant_blobs = da_service.extract_relevant_blobs(&block);
        let relevant_proofs = da_service
            .get_extraction_proof(&block, &relevant_blobs)
            .await;

        // create a verifier with a different namespace than the da_service
        let verifier = CelestiaVerifier::new(RollupParams {
            rollup_proof_namespace: Namespace::new_v0(b"abc").unwrap(),
            rollup_batch_namespace: Namespace::new_v0(b"xyz").unwrap(),
        });

        let _panics =
            verifier.verify_relevant_tx_list(&block.header, &relevant_blobs, relevant_proofs);
    }

    #[tokio::test]
    async fn test_submit_proof() -> anyhow::Result<()> {
        let (mock_server, config, da_service, rollup_params) = setup_test_service(None).await;

        let zk_proof: Vec<u8> = vec![1, 2, 3, 4, 5, 11, 12, 13, 14, 15];
        let gas_limit = get_gas_limit_for_bytes_as_in_golang(zk_proof.len());

        let expected_body = json!({
            "id": 0,
            "jsonrpc": "2.0",
            "method": "state.SubmitPayForBlob",
            "params": [
                Uint::from(gas_limit * (GAS_PRICE_NANO_TIA / 1000) as usize),
                gas_limit,
                [JsonBlob::new(rollup_params.rollup_proof_namespace, zk_proof.to_vec()).unwrap()],
            ]
        });

        Mock::given(method("POST"))
            .and(path("/"))
            .and(bearer_token(config.celestia_rpc_auth_token))
            .and(body_json(&expected_body))
            .respond_with(|req: &Request| {
                let request: BasicJsonRpcRequest = serde_json::from_slice(&req.body).unwrap();
                let response_json = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": {
                        "height": 30497,
                        "txhash": "05D9016060072AA71B007A6CFB1B895623192D6616D513017964C3BFCD047282",
                        "codespace": "",
                        "code": 0,
                        "data": "12260A242F636F736D6F732E62616E6B2E763162657461312E4D736753656E64526573706F6E7365",
                        "raw_log": "[]",
                        "logs": [],
                        "info": "",
                        "gas_wanted": 10000000,
                        "gas_used": 69085,
                        "timestamp": "",
                        "events": [],
                     }
                });

                ResponseTemplate::new(200)
                    .append_header("Content-Type", "application/json")
                    .set_body_json(response_json)
            })
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let fee = CelestiaFee::estimated(zk_proof.len());
        da_service.send_aggregated_zk_proof(&zk_proof, fee).await?;

        Ok(())
    }

    #[test]
    fn test_gas_limit_for_gas_from_go() {
        // On the left size of single blob in batch
        // On the right is gas limit returned by
        // [func DefaultEstimateGas(blobSizes []uint32) uint64]
        // (https://github.com/celestiaorg/celestia-app/blob/c7bef58d058899de23d2cc9d47403c3898e21f53/x/blob/types/payforblob.go#L177)
        // func TestGasLimitForSingleBlobs(t *testing.T) {
        // 	for i := 1100; i <= 1300; i++ {
        // 		blobSizes := []uint32{i}
        // 		gasLimit := types.DefaultEstimateGas(blobSizes)
        // 		fmt.Printf("(%d, %d),\n", i, gasLimit)
        // 	}
        // }
        let test_cases = [
            (1200, 87988),
            (1201, 87988),
            (1300, 87988),
            (2000, 96180),
            (3000, 104372),
            (4000, 112564),
            (5000, 120756),
            (6000, 128948),
            (7000, 137140),
            (8000, 145332),
            (9000, 153524),
            (10000, 161716),
            (11000, 169908),
            (12000, 178100),
            (13000, 186292),
            (14000, 198580),
            (15000, 206772),
            (16000, 214964),
            (17000, 223156),
            (18000, 231348),
            (19000, 239540),
            (20000, 247732),
            (21000, 255924),
            (22000, 264116),
            (23000, 272308),
            (24000, 280500),
            (25000, 288692),
            (26000, 296884),
            (27000, 309172),
            (28000, 317364),
            (29000, 325556),
            (30000, 333748),
        ];

        for (bytes, expected_gas_limit) in test_cases {
            let gas_limit = get_gas_limit_for_bytes_as_in_golang(bytes);
            assert_eq!(expected_gas_limit, gas_limit);
        }
    }

    #[test]
    fn sanity_check_fee_with_current_testnet() {
        // https://mocha.celenium.io/tx/7b8dd68a7a8542714dfbb1b655a381d71ce013b0fc406acc3a56b61a116e7253
        // {
        //   "id": 3068781,
        //   "gas_wanted": 904440,
        //   "gas_used": 395479,
        //   "hash": "7b8dd68a7a8542714dfbb1b655a381d71ce013b0fc406acc3a56b61a116e7253",
        //   "fee": "904440",
        //   "time": "2024-04-01T11:38:39.407177Z",
        // TX:
        // {
        //   "id": 3077971,
        //   "type": "MsgPayForBlobs",
        //   "data": {
        //     "BlobSizes": [
        //       38617
        //     ],
        // }

        let blob_size = 38617;
        let gas_wanted = 904440;
        let gas_used = 395479;
        let gas_used_upper_bound = (gas_wanted as f64 * 1.4) as usize;

        let gas_limit = get_gas_limit_for_bytes_as_in_golang(blob_size);

        assert!(gas_limit >= gas_used);
        assert!(gas_limit <= gas_used_upper_bound);
    }

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10000))]
        #[test]
        fn get_gas_limit_for_bytes_does_not_panic_test(
            blob_size in any::<usize>(),
        ) {
            let _ = get_gas_limit_for_bytes_as_in_golang(blob_size);
        }
    }

    #[test]
    fn test_blob_size_from_payload() {
        // This tests checked [`shares_needed_for_bytes`] against actual shares generated by
        // `Blob::new`
        let sizes: Vec<usize> = (0..100)
            .chain(400..700)
            .chain(900..1200)
            .chain(4800..5200)
            .collect();
        for payload_size in sizes {
            let payload = vec![255; payload_size];
            let namespace = Namespace::new_v0(b"test").unwrap();
            let blob = JsonBlob::new(namespace, payload).unwrap();

            let shares = blob.to_shares().unwrap();

            let shares_count = shares.len();
            // let total_size: usize = shares.iter().map(|s| s.len()).sum();

            let our_shares = shares_needed_for_bytes(payload_size);

            assert_eq!(
                shares_count, our_shares,
                "Failed for payload_size {}",
                payload_size
            );
        }

        let extreme_case = shares_needed_for_bytes(usize::MAX);
        // Doesn't make much sense, but it does not panic!
        assert!(extreme_case > 1);
    }
}
