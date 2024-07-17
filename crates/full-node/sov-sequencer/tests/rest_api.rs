use base64::prelude::*;
use futures::stream::StreamExt;
use sov_mock_da::MockDaSpec;
use sov_modules_api::digest::Digest;
use sov_modules_api::transaction::Transaction;
use sov_modules_api::{CryptoSpec, Spec};
use sov_rollup_interface::services::batch_builder::TxHash;
use sov_sequencer_json_client::types::{PublishBatchBody, TxStatus};
use sov_test_utils::generators::bank::BankMessageGenerator;
use sov_test_utils::runtime::optimistic::TestRuntime;
use sov_test_utils::sequencer::TestSequencerSetup;
use sov_test_utils::{MessageGenerator, TestPrivateKey, TestSpec};

/// Generates a hanful of transactions and returns the hash of the first one.
fn generate_txs(admin_private_key: TestPrivateKey) -> (TxHash, Vec<Transaction<TestSpec>>) {
    let bank_generator = BankMessageGenerator::<TestSpec>::with_minter(admin_private_key);
    let messages_iter = bank_generator.create_default_messages().into_iter();
    let mut txs = Vec::default();
    for message in messages_iter {
        let tx = message.to_tx::<TestRuntime<TestSpec, MockDaSpec>>();
        txs.push(tx);
    }

    let tx_hash: TxHash = <<TestSpec as Spec>::CryptoSpec as CryptoSpec>::Hasher::digest(
        borsh::to_vec(&txs[0]).unwrap(),
    )
    .into();

    (tx_hash, txs)
}

#[tokio::test]
async fn rpc_subscribe() {
    let sequencer = TestSequencerSetup::with_real_batch_builder().await.unwrap();
    let client = sequencer.client();

    let (tx_hash, txs) = generate_txs(sequencer.admin_private_key.clone());

    let mut subscription = client
        .subscribe_to_tx_status_updates(tx_hash)
        .await
        .unwrap();

    // Before submitting a transaction, its status is unknown.
    assert_eq!(
        subscription.next().await.unwrap().unwrap().status,
        TxStatus::Unknown
    );

    for tx in txs {
        client
            .publish_batch_with_serialized_txs(&[tx])
            .await
            .unwrap();
    }

    // The transaction status should change once it enters the mempool...
    assert_eq!(
        subscription.next().await.unwrap().unwrap().status,
        TxStatus::Submitted
    );

    // ...and then change again shortly after, once it gets included in a block.
    assert_eq!(
        subscription.next().await.unwrap().unwrap().status,
        TxStatus::Published,
    );
}

#[tokio::test]
async fn axum_submit_batch_ok() {
    let sequencer = TestSequencerSetup::with_real_batch_builder().await.unwrap();
    let client = sequencer.client();

    let txs = generate_txs(sequencer.admin_private_key.clone()).1;

    let response_result = client
        .publish_batch(&PublishBatchBody {
            transactions: txs
                .iter()
                .map(|tx| BASE64_STANDARD.encode(borsh::to_vec(tx).unwrap()))
                .collect(),
        })
        .await;

    let response = response_result.unwrap();
    let response_data = response.data.as_ref().unwrap();

    assert_eq!(response_data.da_height, 0);
    assert_eq!(response_data.num_txs, txs.len() as i32);
}
