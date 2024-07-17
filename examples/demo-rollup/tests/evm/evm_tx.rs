use sov_modules_macros::config_value;
use sov_stf_runner::RollupProverConfig;

use super::evm_test_helper;
use super::test_client::TestClient;
use crate::test_helpers::get_appropriate_rollup_prover_config;

#[tokio::test(flavor = "multi_thread")]
async fn evm_tx_tests_instant_finality() -> anyhow::Result<()> {
    let rollup_prover_config = get_appropriate_rollup_prover_config();
    evm_tx_test(0, rollup_prover_config).await
}

#[tokio::test(flavor = "multi_thread")]
async fn evm_tx_tests_non_instant_finality() -> anyhow::Result<()> {
    evm_tx_test(3, RollupProverConfig::Skip).await
}

async fn evm_tx_test(
    finalization_blocks: u32,
    rollup_prover_config: RollupProverConfig,
) -> anyhow::Result<()> {
    let chain_id = config_value!("CHAIN_ID");
    let (rollup_task, rpc_port, rest_port) =
        evm_test_helper::start_node(rollup_prover_config, finalization_blocks).await;

    let (test_client, _) = evm_test_helper::create_test_client(
        rpc_port,
        rest_port,
        chain_id,
        // This will produce an evm key exist in rollup accounts-genesis.
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )
    .await;

    send_tx_test_to_eth(&test_client).await.unwrap();
    rollup_task.abort();
    Ok(())
}

async fn send_tx_test_to_eth(test_client: &TestClient) -> Result<(), Box<dyn std::error::Error>> {
    let etc_accounts = test_client.eth_accounts().await;
    assert_eq!(vec![test_client.from_addr], etc_accounts);

    let eth_chain_id = test_client.eth_chain_id().await;
    assert_eq!(test_client.chain_id, eth_chain_id);

    // No block exists yet
    let latest_block = test_client
        .eth_get_block_by_number(Some("latest".to_owned()))
        .await;
    let earliest_block = test_client
        .eth_get_block_by_number(Some("earliest".to_owned()))
        .await;

    assert_eq!(latest_block, earliest_block);
    assert_eq!(latest_block.number.unwrap().as_u64(), 0);

    execute_evm_tests(test_client).await
}

async fn execute_evm_tests(client: &TestClient) -> Result<(), Box<dyn std::error::Error>> {
    // Nonce should be 0 in genesis
    let nonce = client.eth_get_transaction_count(client.from_addr).await;
    assert_eq!(0, nonce);

    // Balance should be > 0 in genesis
    let balance = client.eth_get_balance(client.from_addr).await;
    assert!(balance > ethereum_types::U256::zero());

    let mut slot_subscription = client.subscribe_for_slots().await?;

    let contract_address =
        evm_test_helper::deploy_contract_check(client, &mut slot_subscription).await?;

    // Nonce should be 1 after the deploy
    let nonce = client.eth_get_transaction_count(client.from_addr).await;
    assert_eq!(1, nonce);

    // Check that the first block has published
    // It should have a single transaction, deploying the contract
    let first_block = client.eth_get_block_by_number(Some("1".to_owned())).await;
    assert_eq!(first_block.number.unwrap().as_u64(), 1);
    assert_eq!(first_block.transactions.len(), 1);

    let set_arg = 923;
    evm_test_helper::set_value_check(client, &mut slot_subscription, contract_address, set_arg)
        .await?;

    // Check that the second block has published
    // None should return the latest block
    // It should have a single transaction, setting the value
    let latest_block = client.eth_get_block_by_number_with_detail(None).await;
    assert_eq!(latest_block.number.unwrap().as_u64(), 2);

    // This should just pass without error
    client
        .set_value_call(contract_address, set_arg)
        .await
        .unwrap();

    // This call should fail because function does not exist
    let failing_call = client.failing_call(contract_address).await;
    assert!(failing_call.is_err());

    // Create a blob with multiple transactions.
    let values: Vec<u32> = (150..153).collect();
    // Create a blob with multiple transactions.
    evm_test_helper::set_multiple_values_check(
        client,
        &mut slot_subscription,
        contract_address,
        values,
    )
    .await?;

    let value = 103;
    evm_test_helper::set_value_unsigned_check(
        client,
        &mut slot_subscription,
        contract_address,
        value,
    )
    .await?;

    evm_test_helper::gas_check(client, &mut slot_subscription, contract_address).await?;

    let first_block = client.eth_get_block_by_number(Some("0".to_owned())).await;
    let second_block = client.eth_get_block_by_number(Some("1".to_owned())).await;

    // assert parent hash works correctly
    assert_eq!(
        first_block.hash.unwrap(),
        second_block.parent_hash,
        "Parent hash should be the hash of the previous block"
    );

    Ok(())
}
