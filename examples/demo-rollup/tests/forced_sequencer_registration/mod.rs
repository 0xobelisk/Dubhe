use std::sync::Arc;

use anyhow::Context;
use demo_stf::genesis_config::GenesisPaths;
use demo_stf::runtime::RuntimeCall;
use futures::StreamExt;
use sov_kernels::basic::BasicKernelGenesisPaths;
use sov_mock_da::{MockAddress, MockDaConfig, MockDaSpec};
use sov_modules_api::transaction::{PriorityFeeBips, Transaction, UnsignedTransaction};
use sov_modules_api::{BlobData, RawTx};
use sov_modules_macros::config_value;
use sov_rollup_interface::services::da::DaService;
use sov_stf_runner::RollupProverConfig;
use sov_test_utils::{ApiClient, TestPrivateKey, TestSpec};

use crate::test_helpers::{construct_rollup, read_private_keys};

const MAX_TX_FEE: u64 = 100_000_000;
const UNREGISTERED_SENDER: MockAddress = MockAddress::new([121; 32]);
const MINIMUM_BOND: u64 = 100_000_000;

#[tokio::test(flavor = "multi_thread")]
async fn test_forced_sequencer_registration() -> Result<(), anyhow::Error> {
    let (rpc_port_tx, rpc_port_rx) = tokio::sync::oneshot::channel();
    let (rest_port_tx, rest_port_rx) = tokio::sync::oneshot::channel();
    let rollup = construct_rollup(
        GenesisPaths::from_dir("../test-data/genesis/integration-tests"),
        BasicKernelGenesisPaths {
            chain_state: "../test-data/genesis/integration-tests/chain_state.json".into(),
        },
        RollupProverConfig::Skip,
        MockDaConfig::instant_with_sender(UNREGISTERED_SENDER),
    )
    .await;
    let da_service = rollup.runner.da_service();

    let rollup_task = tokio::spawn(async move {
        rollup
            .run_and_report_addr(Some(rpc_port_tx), Some(rest_port_tx))
            .await
            .unwrap();
    });

    let rpc_port = rpc_port_rx.await.unwrap().port();
    let rest_port = rest_port_rx.await.unwrap().port();
    let client = ApiClient::new(rpc_port, rest_port).await?;

    tokio::select! {
        err = rollup_task => err?,
        res = forced_sequencer_registration_test_case(da_service, &client) => res?,
    };
    Ok(())
}

async fn forced_sequencer_registration_test_case(
    da_service: Arc<impl DaService>,
    client: &ApiClient,
) -> Result<(), anyhow::Error> {
    let key_and_address = read_private_keys::<TestSpec>("tx_signer_private_key.json");
    let key = key_and_address.private_key;

    let tx = build_register_sequencer_tx(&key, 0);
    let blob = transaction_into_blob(tx);

    let fee = da_service.estimate_fee(blob.len()).await.unwrap();
    let mut slot_subscription = client
        .ledger
        .subscribe_slots()
        .await
        .context("Failed to subscribe to slots!")
        .unwrap();

    let _ = da_service.send_transaction(&blob, fee).await.unwrap();

    slot_subscription.next().await.unwrap().unwrap();

    let sequencer_address_response = sov_sequencer_registry::SequencerRegistryRpcClient::<
        TestSpec,
        MockDaSpec,
    >::sequencer_address(&client.rpc, UNREGISTERED_SENDER)
    .await
    .unwrap();

    assert!(sequencer_address_response.address.is_some());

    Ok(())
}

fn build_register_sequencer_tx(key: &TestPrivateKey, nonce: u64) -> Transaction<TestSpec> {
    let msg = RuntimeCall::<TestSpec, MockDaSpec>::sequencer_registry(
        sov_sequencer_registry::CallMessage::Register {
            da_address: UNREGISTERED_SENDER.as_ref().to_vec(),
            amount: MINIMUM_BOND,
        },
    );
    let chain_id = config_value!("CHAIN_ID");
    let max_priority_fee_bips = PriorityFeeBips::ZERO;
    let max_fee = MAX_TX_FEE;
    let gas_limit = None;
    Transaction::<TestSpec>::new_signed_tx(
        key,
        UnsignedTransaction::new(
            borsh::to_vec(&msg).unwrap(),
            chain_id,
            max_priority_fee_bips,
            max_fee,
            nonce,
            gas_limit,
        ),
    )
}

fn transaction_into_blob(transaction: Transaction<TestSpec>) -> Vec<u8> {
    let tx_data = borsh::to_vec(&transaction).unwrap();
    let blob_data = BlobData::new_batch(vec![RawTx { data: tx_data }]);
    borsh::to_vec(&blob_data).unwrap()
}
