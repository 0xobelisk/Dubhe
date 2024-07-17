use anyhow::Context;
use demo_stf::genesis_config::GenesisPaths;
use demo_stf::runtime::RuntimeCall;
use futures::StreamExt;
use sov_bank::event::Event as BankEvent;
use sov_bank::utils::TokenHolder;
use sov_bank::{Coins, TokenId};
use sov_kernels::basic::BasicKernelGenesisPaths;
use sov_mock_da::{BlockProducingConfig, MockAddress, MockDaConfig, MockDaSpec};
use sov_mock_zkvm::{MockCodeCommitment, MockZkVerifier};
use sov_modules_api::transaction::{Transaction, UnsignedTransaction};
use sov_modules_api::{PrivateKey, Spec};
use sov_modules_macros::config_value;
use sov_rollup_interface::rpc::FinalityStatus;
use sov_rollup_interface::zk::aggregated_proof::AggregateProofVerifier;
use sov_stf_runner::RollupProverConfig;
use sov_test_utils::{
    ApiClient, TestPrivateKey, TestSpec, TEST_DEFAULT_MAX_FEE, TEST_DEFAULT_MAX_PRIORITY_FEE,
};

use crate::test_helpers::{get_appropriate_rollup_prover_config, read_private_keys, start_rollup};

const TOKEN_SALT: u64 = 0;
const TOKEN_NAME: &str = "test_token";

struct TestCase {
    wait_for_aggregated_proof: bool,
    finalization_blocks: u32,
}

impl TestCase {
    fn expected_head_finality(&self) -> FinalityStatus {
        match self.finalization_blocks {
            0 => FinalityStatus::Finalized,
            _ => FinalityStatus::Pending,
        }
    }

    fn get_latest_finalized_slot_after(&self, slot_number: u64) -> Option<u64> {
        slot_number.checked_sub(self.finalization_blocks as u64)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bank_tx_tests_instant_finality() -> Result<(), anyhow::Error> {
    let test_case = TestCase {
        wait_for_aggregated_proof: true,
        finalization_blocks: 0,
    };
    let rollup_prover_config = get_appropriate_rollup_prover_config();
    bank_tx_tests(test_case, rollup_prover_config).await
}

#[tokio::test(flavor = "multi_thread")]
async fn bank_tx_tests_non_instant_finality() -> Result<(), anyhow::Error> {
    let test_case = TestCase {
        wait_for_aggregated_proof: false,
        finalization_blocks: 2,
    };
    bank_tx_tests(test_case, RollupProverConfig::Skip).await
}

async fn bank_tx_tests(
    test_case: TestCase,
    rollup_prover_config: RollupProverConfig,
) -> anyhow::Result<()> {
    let (rpc_port_tx, rpc_port_rx) = tokio::sync::oneshot::channel();
    let (rest_port_tx, rest_port_rx) = tokio::sync::oneshot::channel();

    let rollup_task = tokio::spawn(async move {
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
                finalization_blocks: test_case.finalization_blocks,
                block_producing: BlockProducingConfig::OnSubmit,
                block_time_ms: 5_000,
            },
        )
        .await;
    });

    let rpc_port = rpc_port_rx.await.unwrap().port();
    let rest_port = rest_port_rx.await.unwrap().port();
    let client = ApiClient::new(rpc_port, rest_port).await?;

    // If the rollup throws an error, return it and stop trying to send the transaction
    tokio::select! {
        err = rollup_task => err?,
        res = send_test_bank_txs(test_case, &client) => res?,
    };
    Ok(())
}

fn build_create_token_tx(key: &TestPrivateKey, nonce: u64) -> Transaction<TestSpec> {
    let user_address: <TestSpec as Spec>::Address = key.to_address();
    let msg =
        RuntimeCall::<TestSpec, MockDaSpec>::bank(sov_bank::CallMessage::<TestSpec>::CreateToken {
            salt: TOKEN_SALT,
            token_name: TOKEN_NAME.to_string(),
            initial_balance: 1000,
            mint_to_address: user_address,
            authorized_minters: vec![],
        });
    let chain_id = config_value!("CHAIN_ID");
    let max_priority_fee_bips = TEST_DEFAULT_MAX_PRIORITY_FEE;
    let max_fee = TEST_DEFAULT_MAX_FEE;
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

fn build_transfer_token_tx(
    key: &TestPrivateKey,
    token_id: TokenId,
    recipient: <TestSpec as Spec>::Address,
    amount: u64,
    nonce: u64,
) -> Transaction<TestSpec> {
    let msg =
        RuntimeCall::<TestSpec, MockDaSpec>::bank(sov_bank::CallMessage::<TestSpec>::Transfer {
            to: recipient,
            coins: Coins { amount, token_id },
        });
    let chain_id = config_value!("CHAIN_ID");
    let max_priority_fee_bips = TEST_DEFAULT_MAX_PRIORITY_FEE;
    let max_fee = TEST_DEFAULT_MAX_FEE;
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

fn build_multiple_transfers(
    amounts: &[u64],
    signer_key: &TestPrivateKey,
    token_id: TokenId,
    recipient: <TestSpec as Spec>::Address,
    start_nonce: u64,
) -> Vec<Transaction<TestSpec>> {
    let mut txs = vec![];
    let mut nonce = start_nonce;
    for amt in amounts {
        txs.push(build_transfer_token_tx(
            signer_key, token_id, recipient, *amt, nonce,
        ));
        nonce += 1;
    }
    txs
}

async fn send_transactions_and_wait_slot(
    client: &ApiClient,
    transactions: &[Transaction<TestSpec>],
) -> anyhow::Result<u64> {
    let mut slot_subscription = client
        .ledger
        .subscribe_slots()
        .await
        .context("Failed to subscribe to slots!")?;

    client
        .sequencer
        .publish_batch_with_serialized_txs(transactions)
        .await?;

    let slot_number = slot_subscription
        .next()
        .await
        .transpose()?
        .map(|slot| slot.number)
        .unwrap_or_default();

    Ok(slot_number)
}

async fn assert_balance(
    client: &ApiClient,
    assert_amount: u64,
    token_id: TokenId,
    user_address: <TestSpec as Spec>::Address,
    version: Option<u64>,
) -> Result<(), anyhow::Error> {
    let balance_response = sov_bank::BankRpcClient::<TestSpec>::balance_of(
        &client.rpc,
        version,
        user_address,
        token_id,
    )
    .await?;
    assert_eq!(balance_response.amount.unwrap_or_default(), assert_amount);
    Ok(())
}

async fn assert_aggregated_proof(
    initial_slot: u64,
    final_slot: u64,
    client: &ApiClient,
) -> anyhow::Result<()> {
    let proof_response = client.ledger.get_latest_aggregated_proof().await?;

    let verifier = AggregateProofVerifier::<MockZkVerifier>::new(MockCodeCommitment::default());
    verifier.verify(&proof_response.data.clone().try_into()?)?;

    let proof_pub_data = &proof_response.data.public_data;
    // We test inequality because proofs are saved asynchronously in the db.
    assert!(initial_slot <= proof_pub_data.initial_slot_number);
    assert!(final_slot <= proof_pub_data.final_slot_number);

    let proof_data_info_response = client.ledger.get_latest_aggregated_proof().await?;

    assert!(
        initial_slot
            <= proof_data_info_response
                .data
                .public_data
                .initial_slot_number
    );
    assert!(final_slot <= proof_data_info_response.data.public_data.final_slot_number);

    Ok(())
}

fn assert_aggregated_proof_public_data(
    expected_initial_slot_number: u64,
    expected_final_slot_number: u64,
    pub_data: &sov_ledger_json_client::types::AggregatedProofPublicData,
) {
    assert_eq!(expected_initial_slot_number, pub_data.initial_slot_number);
    assert_eq!(expected_final_slot_number, pub_data.final_slot_number);
}

async fn assert_slot_finality(
    client: &ApiClient,
    slot_number: u64,
    expected_finality: FinalityStatus,
) {
    let slot = client
        .ledger
        .get_slot_by_id(
            &sov_ledger_json_client::types::IntOrHash::Variant0(slot_number),
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        expected_finality,
        slot.data.finality_status.into(),
        "Wrong finality status for slot number {slot_number}"
    );
}

async fn assert_bank_event<S: Spec>(
    client: &ApiClient,
    event_number: u64,
    expected_event: BankEvent<S>,
) -> anyhow::Result<()> {
    let event_response = client.ledger.get_event_by_id(event_number).await?;

    // Ensure "bank" is present in response json
    assert_eq!(event_response.data.module.name, "bank");

    let event_value = serde_json::Value::Object(event_response.data.value.clone());

    // Attempt to deserialize the "body" of the bank key in the response to the Event type
    let bank_event_contents = serde_json::from_value::<BankEvent<S>>(event_value)?;

    // Ensure the event generated is a TokenCreated event with the correct token_id
    assert_eq!(bank_event_contents, expected_event);

    Ok(())
}

async fn send_test_bank_txs(test_case: TestCase, client: &ApiClient) -> Result<(), anyhow::Error> {
    let key_and_address = read_private_keys::<TestSpec>("tx_signer_private_key.json");
    let key = key_and_address.private_key;
    let user_address: <TestSpec as Spec>::Address = key_and_address.address;

    let token_id = sov_bank::get_token_id::<TestSpec>(TOKEN_NAME, &user_address, TOKEN_SALT);

    let recipient_key = TestPrivateKey::generate();
    let recipient_address: <TestSpec as Spec>::Address = recipient_key.to_address();

    let token_id_response = sov_bank::BankRpcClient::<TestSpec>::token_id(
        &client.rpc,
        TOKEN_NAME.to_owned(),
        user_address,
        TOKEN_SALT,
    )
    .await?;

    let mut aggregated_proof_subscription = client
        .ledger
        .subscribe_aggregated_proof()
        .await
        .context("Failed to subscribe to aggregated proof")?;

    assert_eq!(token_id, token_id_response);

    // create token. height 2
    let tx = build_create_token_tx(&key, 0);
    let slot_number = send_transactions_and_wait_slot(client, &[tx]).await?;
    assert_eq!(1, slot_number);
    assert_slot_finality(client, slot_number, test_case.expected_head_finality()).await;
    assert_balance(client, 1000, token_id, user_address, None).await?;

    // transfer 100 tokens. assert sender balance. height 3
    let tx = build_transfer_token_tx(&key, token_id, recipient_address, 100, 1);
    let slot_number = send_transactions_and_wait_slot(client, &[tx]).await?;
    assert_eq!(2, slot_number);
    assert_slot_finality(client, slot_number, test_case.expected_head_finality()).await;
    assert_balance(client, 900, token_id, user_address, None).await?;

    // transfer 200 tokens. assert sender balance. height 4
    let tx = build_transfer_token_tx(&key, token_id, recipient_address, 200, 2);
    let slot_number = send_transactions_and_wait_slot(client, &[tx]).await?;
    assert_eq!(3, slot_number);
    assert_slot_finality(client, slot_number, test_case.expected_head_finality()).await;
    assert_balance(client, 700, token_id, user_address, None).await?;

    // assert sender balance at height 2.
    assert_balance(client, 1000, token_id, user_address, Some(2)).await?;

    // assert sender balance at height 3.
    assert_balance(client, 900, token_id, user_address, Some(3)).await?;

    // assert sender balance at height 4.
    assert_balance(client, 700, token_id, user_address, Some(4)).await?;

    // 10 transfers of 10,11..20
    let transfer_amounts: Vec<u64> = (10u64..20).collect();
    let txs = build_multiple_transfers(&transfer_amounts, &key, token_id, recipient_address, 3);
    let slot_number = send_transactions_and_wait_slot(client, &txs).await?;
    assert_eq!(4, slot_number);
    assert_slot_finality(client, slot_number, test_case.expected_head_finality()).await;

    assert_bank_event::<TestSpec>(
        client,
        0,
        BankEvent::TokenCreated {
            token_name: TOKEN_NAME.to_owned(),
            coins: Coins {
                amount: 1000,
                token_id,
            },
            minter: TokenHolder::User(user_address),
            authorized_minters: vec![],
        },
    )
    .await?;
    assert_bank_event::<TestSpec>(
        client,
        1,
        BankEvent::TokenTransferred {
            from: TokenHolder::User(user_address),
            to: TokenHolder::User(recipient_address),
            coins: Coins {
                amount: 100,
                token_id,
            },
        },
    )
    .await?;
    assert_bank_event::<TestSpec>(
        client,
        2,
        BankEvent::TokenTransferred {
            from: TokenHolder::User(user_address),
            to: TokenHolder::User(recipient_address),
            coins: Coins {
                amount: 200,
                token_id,
            },
        },
    )
    .await?;

    if test_case.wait_for_aggregated_proof {
        let aggregated_proof_resp = aggregated_proof_subscription.next().await.unwrap()?;
        let pub_data = aggregated_proof_resp.public_data;
        assert_aggregated_proof_public_data(1, 1, &pub_data);
        assert_aggregated_proof(1, 1, client).await?;
    }

    if let Some(finalized_slot_number) = test_case.get_latest_finalized_slot_after(slot_number) {
        assert_slot_finality(client, finalized_slot_number, FinalityStatus::Finalized).await;
    }

    Ok(())
}
