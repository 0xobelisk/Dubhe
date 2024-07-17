pub use gas_price::gas_oracle::GasPriceOracleConfig;
#[cfg(feature = "local")]
pub use sov_eth_dev_signer::DevSigner;
use sov_modules_api::BlobData;
mod batch_builder;
mod gas_price;
#[cfg(feature = "local")]
mod signer;

use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::RpcModule;
use reth_primitives::{Bytes, TransactionSignedNoHash as RethTransactionSignedNoHash, B256, U256};
use sov_evm::{EthApiError, Evm, RlpEvmTransaction};
use sov_modules_api::capabilities::Authenticator;
use sov_modules_api::utils::to_jsonrpsee_error_object;
use sov_modules_api::ApiStateAccessor;
use sov_rollup_interface::services::da::DaService;
use tokio::sync::watch;

use crate::batch_builder::EthBatchBuilder;
use crate::gas_price::gas_oracle::GasPriceOracle;

const ETH_RPC_ERROR: &str = "ETH_RPC_ERROR";

#[derive(Clone)]
pub struct EthRpcConfig {
    pub min_blob_size: Option<usize>,
    pub gas_price_oracle_config: GasPriceOracleConfig,
    #[cfg(feature = "local")]
    pub eth_signer: DevSigner,
}

pub fn get_ethereum_rpc<S: sov_modules_api::Spec, Da: DaService, Auth: Authenticator>(
    da_service: Da,
    eth_rpc_config: EthRpcConfig,
    storage: watch::Receiver<S::Storage>,
) -> RpcModule<Ethereum<S, Da, Auth>> {
    // Unpack config
    let EthRpcConfig {
        min_blob_size,
        #[cfg(feature = "local")]
        eth_signer,
        gas_price_oracle_config,
    } = eth_rpc_config;

    // Fetch nonce from storage
    let mut rpc = RpcModule::new(Ethereum::new(
        da_service,
        Arc::new(Mutex::new(EthBatchBuilder::new(min_blob_size))),
        gas_price_oracle_config,
        #[cfg(feature = "local")]
        eth_signer,
        storage,
    ));

    register_rpc_methods(&mut rpc).expect("Failed to register sequencer RPC methods");
    rpc
}

pub struct Ethereum<S: sov_modules_api::Spec, Da: DaService, Auth: Authenticator> {
    da_service: Da,
    batch_builder: Arc<Mutex<EthBatchBuilder>>,
    gas_price_oracle: GasPriceOracle<S>,
    #[cfg(feature = "local")]
    eth_signer: DevSigner,
    storage: watch::Receiver<S::Storage>,
    _phantom: PhantomData<Auth>,
}

impl<S: sov_modules_api::Spec, Da: DaService, Auth: Authenticator> Ethereum<S, Da, Auth> {
    fn new(
        da_service: Da,
        batch_builder: Arc<Mutex<EthBatchBuilder>>,
        gas_price_oracle_config: GasPriceOracleConfig,
        #[cfg(feature = "local")] eth_signer: DevSigner,
        storage: watch::Receiver<S::Storage>,
    ) -> Self {
        let evm = Evm::<S>::default();
        let gas_price_oracle = GasPriceOracle::new(evm, gas_price_oracle_config);
        Self {
            da_service,
            batch_builder,
            gas_price_oracle,
            #[cfg(feature = "local")]
            eth_signer,
            storage,
            _phantom: PhantomData,
        }
    }
}

impl<S: sov_modules_api::Spec, Da: DaService, Auth: Authenticator> Ethereum<S, Da, Auth> {
    fn make_raw_tx(&self, raw_tx: RlpEvmTransaction) -> Result<(B256, Vec<u8>), ErrorObjectOwned> {
        let signed_transaction: RethTransactionSignedNoHash =
            raw_tx.clone().try_into().map_err(EthApiError::from)?;

        let tx_hash = signed_transaction.hash();
        let message = borsh::to_vec(&raw_tx).expect("Failed to serialize raw tx");

        Ok((tx_hash, message))
    }

    async fn build_and_submit_batch(
        &self,
        min_blob_size: Option<usize>,
    ) -> Result<(), jsonrpsee::core::client::Error> {
        tracing::info!(
            min_blob_size = min_blob_size,
            "Build and submit ETH batch request has been received",
        );
        let tx_batch = self.build_tx_batch(min_blob_size)?;
        tracing::debug!(transactions_count = tx_batch.len(), "Batch have been built");

        self.submit_tx_batch(tx_batch)
            .await
            .map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR))?;

        Ok(())
    }

    async fn submit_tx_batch(
        &self,
        tx_batch: Vec<Vec<u8>>,
    ) -> Result<(), jsonrpsee::core::client::Error> {
        if tx_batch.is_empty() {
            tracing::error!("Attempt to submit empty batch");
            return Err(jsonrpsee::core::client::Error::Custom(
                "Attempt to submit empty batch".to_string(),
            ));
        }

        let txs = tx_batch
            .into_iter()
            .map(|tx| Auth::encode(tx).map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR)))
            .collect::<Result<Vec<_>, _>>()?;

        let batch = BlobData::new_batch(txs);
        let serialized_batch =
            borsh::to_vec(&batch).map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR))?;

        let fee = self
            .da_service
            .estimate_fee(serialized_batch.len())
            .await
            .map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR))?;
        self.da_service
            .send_transaction(&serialized_batch, fee)
            .await
            .map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR))?;
        tracing::debug!("ETH Batch has been submitted");
        Ok(())
    }

    fn build_tx_batch(
        &self,
        min_blob_size: Option<usize>,
    ) -> Result<Vec<Vec<u8>>, jsonrpsee::core::client::Error> {
        let batch = self
            .batch_builder
            .lock()
            .unwrap()
            .get_next_blob(min_blob_size);

        Ok(batch)
    }

    fn add_messages(&self, messages: Vec<Vec<u8>>) {
        self.batch_builder.lock().unwrap().add_messages(messages);
    }
}

fn register_rpc_methods<S: sov_modules_api::Spec, Da: DaService, Auth: Authenticator>(
    rpc: &mut RpcModule<Ethereum<S, Da, Auth>>,
) -> Result<(), jsonrpsee::core::client::Error> {
    rpc.register_async_method("eth_gasPrice", |_, ethereum| async move {
        let price = {
            let mut state = ApiStateAccessor::<S>::new(ethereum.storage.borrow().clone());

            let suggested_tip = ethereum
                .gas_price_oracle
                .suggest_tip_cap(&mut state)
                .await
                .unwrap();

            let evm = Evm::<S>::default();
            let base_fee = evm
                .get_block_by_number(None, None, &mut state)
                .unwrap()
                .unwrap()
                .header
                .base_fee_per_gas
                .unwrap_or_default();

            suggested_tip + base_fee
        };

        Ok::<U256, ErrorObjectOwned>(price)
    })?;

    rpc.register_async_method("eth_publishBatch", |_params, ethereum| async move {
        ethereum
            .build_and_submit_batch(Some(1))
            .await
            .map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR))?;

        Ok::<String, ErrorObjectOwned>("Submitted transaction".to_string())
    })?;

    rpc.register_async_method(
        "eth_sendRawTransaction",
        |parameters, ethereum| async move {
            let data: Bytes = parameters.one().unwrap();

            let raw_evm_tx = RlpEvmTransaction { rlp: data.to_vec() };

            let (tx_hash, raw_message) = ethereum
                .make_raw_tx(raw_evm_tx)
                .map_err(|e| to_jsonrpsee_error_object(e, ETH_RPC_ERROR))?;

            ethereum.add_messages(vec![raw_message]);

            Ok::<_, ErrorObjectOwned>(tx_hash)
        },
    )?;

    #[cfg(feature = "local")]
    signer::register_signer_rpc_methods::<_, _, Auth>(rpc)?;

    Ok(())
}
