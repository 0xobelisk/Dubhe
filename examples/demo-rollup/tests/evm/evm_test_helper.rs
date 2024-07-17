use std::net::SocketAddr;

use demo_stf::genesis_config::GenesisPaths;
use ethers_core::abi::Address;
use ethers_providers::ProviderError;
use ethers_signers::{LocalWallet, Signer};
use futures::future::join_all;
use futures::stream::BoxStream;
use futures::StreamExt;
use sov_kernels::basic::BasicKernelGenesisPaths;
use sov_mock_da::{BlockProducingConfig, MockAddress, MockDaConfig};
use sov_stf_runner::RollupProverConfig;
use sov_test_utils::SimpleStorageContract;
use tokio::task::JoinHandle;

use super::test_client::TestClient;
use crate::test_helpers::start_rollup;

/// Starts test rollup node.  
pub(crate) async fn start_node(
    rollup_prover_config: RollupProverConfig,
    finalization_blocks: u32,
) -> (JoinHandle<()>, SocketAddr, SocketAddr) {
    let (rpc_port_tx, rpc_port_rx) = tokio::sync::oneshot::channel();
    let (rest_port_tx, rest_port_rx) = tokio::sync::oneshot::channel();

    let rollup_task = tokio::spawn(async move {
        // Don't provide a prover since the EVM is not currently provable
        start_rollup(
            rpc_port_tx,
            rest_port_tx,
            GenesisPaths::from_dir("../test-data/genesis/integration-tests"),
            BasicKernelGenesisPaths {
                chain_state: "../test-data/genesis/integration-tests/chain_state.json".into(),
            },
            rollup_prover_config,
            MockDaConfig {
                connection_string: "sqlite::memory:".to_string(),
                // This value is important and should match ../test-data/genesis/integration-tests /sequencer_registry.json
                // Otherwise batches are going to be rejected
                sender_address: MockAddress::new([0; 32]),
                finalization_blocks,
                block_producing: BlockProducingConfig::OnSubmit,
                // This parameter is important!
                block_time_ms: 30_000,
            },
        )
        .await;
    });

    let rpc_port = rpc_port_rx.await.unwrap();
    let rest_port = rest_port_rx.await.unwrap();

    (rollup_task, rpc_port, rest_port)
}

/// Creates a test client to communicate with the rollup node.
pub(crate) async fn create_test_client(
    rpc_port: SocketAddr,
    rest_port: SocketAddr,
    chain_id: u64,
    private_key: &str,
) -> (TestClient, Address) {
    let key = private_key
        .parse::<LocalWallet>()
        .unwrap()
        .with_chain_id(chain_id);

    let contract = SimpleStorageContract::default();
    let from_addr = key.address();

    let test_client =
        TestClient::new(chain_id, key, from_addr, contract, rpc_port, rest_port).await;

    let eth_chain_id = test_client.eth_chain_id().await;
    assert_eq!(chain_id, eth_chain_id);

    (test_client, from_addr)
}

/// Deploys a test contract on the test rollup.
pub(crate) async fn deploy_contract_check(
    client: &TestClient,
    slot_subscription: &mut BoxStream<'static, anyhow::Result<u64>>,
) -> Result<Address, Box<dyn std::error::Error>> {
    let runtime_code = client.deploy_contract_call().await?;

    let deploy_contract_req = client.deploy_contract().await?;
    client.send_publish_batch_request().await;
    let _ = slot_subscription.next().await.unwrap().unwrap();

    let contract_address = deploy_contract_req
        .await?
        .unwrap()
        .contract_address
        .unwrap();

    // Assert contract deployed correctly
    let code = client.eth_get_code(contract_address).await;
    // code has natural following 0x00 bytes, so we need to trim it
    assert_eq!(code.to_vec()[..runtime_code.len()], runtime_code.to_vec());

    Ok(contract_address)
}

/// Calls `set_value` on the test contract.
pub(crate) async fn set_value_check(
    client: &TestClient,
    slot_subscription: &mut BoxStream<'static, anyhow::Result<u64>>,
    contract_address: Address,
    set_arg: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let tx_hash = {
        let set_value_req = client
            .set_value(contract_address, set_arg, None, None)
            .await;
        client.send_publish_batch_request().await;
        let _ = slot_subscription.next().await.unwrap().unwrap();
        set_value_req.await.unwrap().unwrap().transaction_hash
    };

    let get_arg = client.query_contract(contract_address).await?;
    assert_eq!(set_arg, get_arg.as_u32());

    // Assert storage slot is set
    let storage_slot = 0x0;
    let storage_value = client
        .eth_get_storage_at(contract_address, storage_slot.into())
        .await;
    assert_eq!(storage_value, ethereum_types::U256::from(set_arg));

    let latest_block = client.eth_get_block_by_number(None).await;
    assert_eq!(latest_block.transactions.len(), 1);
    assert_eq!(latest_block.transactions[0], tx_hash);

    Ok(())
}

/// Calls `set_value` on the test contract with unsigned transaction.
pub(crate) async fn set_value_unsigned_check(
    client: &TestClient,
    slot_subscription: &mut BoxStream<'static, anyhow::Result<u64>>,
    contract_address: Address,
    set_arg: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let set_value_req = client.set_value_unsigned(contract_address, set_arg).await;
    client.send_publish_batch_request().await;
    let _ = slot_subscription.next().await.unwrap().unwrap();
    set_value_req.await.unwrap().unwrap();

    let get_arg = client.query_contract(contract_address).await?;
    assert_eq!(set_arg, get_arg.as_u32());

    Ok(())
}

/// Calls `set_values` on the test contract.
pub(crate) async fn set_multiple_values_check(
    client: &TestClient,
    slot_subscription: &mut BoxStream<'static, anyhow::Result<u64>>,
    contract_address: Address,
    values: Vec<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let requests = client
        .set_values(contract_address, values, None, None)
        .await;

    client.send_publish_batch_request().await;
    let _ = slot_subscription.next().await.unwrap().unwrap();

    let receipts: Vec<Result<Option<_>, ProviderError>> = join_all(requests).await;
    assert!(receipts
        .into_iter()
        .all(|x| x.is_ok() && x.unwrap().is_some()));

    {
        let get_arg = client.query_contract(contract_address).await?.as_u32();
        // should be one of three values sent in a single block. 150, 151, or 152
        assert!((150..=152).contains(&get_arg));
    }

    Ok(())
}

/// Checks evm gas evolution.
pub(crate) async fn gas_check(
    client: &TestClient,
    slot_subscription: &mut BoxStream<'static, anyhow::Result<u64>>,
    contract_address: Address,
) -> Result<(), Box<dyn std::error::Error>> {
    // get initial gas price
    let initial_base_fee_per_gas = client.eth_gas_price().await;

    // send 10 "set" transactions with high gas fee in 2 batches to increase gas price
    for _ in 0..2 {
        let values: Vec<u32> = (0..5).collect();
        let _requests = client
            .set_values(contract_address, values, Some(20u64), Some(21u64))
            .await;
        client.send_publish_batch_request().await;
        slot_subscription.next().await;
    }
    // get gas price
    let latest_gas_price = client.eth_gas_price().await;

    // assert gas price is higher
    // TODO: emulate gas price oracle here to have exact value
    assert!(latest_gas_price > initial_base_fee_per_gas);
    Ok(())
}
