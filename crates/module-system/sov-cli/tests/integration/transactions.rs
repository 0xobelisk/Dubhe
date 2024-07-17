use std::path::{Path, PathBuf};

use borsh::{BorshDeserialize, BorshSerialize};
use demo_stf::runtime::{Runtime, RuntimeCall, RuntimeSubcommand};
use sov_cli::wallet_state::{KeyIdentifier, WalletState};
use sov_cli::workflows::keys::KeyWorkflow;
use sov_cli::workflows::transactions::{TransactionLoadWorkflow, TransactionWorkflow};
use sov_cli::UnsignedTransactionWithoutNonce;
use sov_mock_da::MockDaSpec;
use sov_modules_api::cli::{FileNameArg, JsonStringArg};
use sov_modules_api::transaction::{Transaction, UnsignedTransaction};
use sov_modules_api::{CryptoSpec, PrivateKey, Spec, UnlimitedGasMeter};
use sov_test_utils::{TestSpec, TEST_DEFAULT_MAX_FEE, TEST_DEFAULT_MAX_PRIORITY_FEE};
type Da = MockDaSpec;

#[test]
fn test_import_transaction_from_string() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();

    let subcommand = RuntimeSubcommand::<JsonStringArg, TestSpec, Da>::bank {
        contents: default_json_string_arg_for_test("requests/create_token.json"),
    };

    let workflow = TransactionWorkflow::Import(TransactionLoadWorkflow::<
        RuntimeSubcommand<FileNameArg, TestSpec, Da>,
        RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
    >::FromString(subcommand));
    workflow
        .run::<Runtime<TestSpec, Da>, _, _, _, _, _>(&mut wallet_state, app_dir, std::io::stdout())
        .unwrap();

    assert_eq!(wallet_state.unsent_transactions.len(), 1);
}

#[test]
fn test_import_transaction_from_file() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();

    let subcommand = RuntimeSubcommand::<FileNameArg, TestSpec, Da>::bank {
        contents: default_file_name_arg_for_test("requests/create_token.json"),
    };

    let workflow = TransactionWorkflow::Import(TransactionLoadWorkflow::<
        RuntimeSubcommand<FileNameArg, TestSpec, Da>,
        RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
    >::FromFile(subcommand));

    workflow
        .run::<Runtime<TestSpec, Da>, _, _, _, _, _>(&mut wallet_state, app_dir, std::io::stdout())
        .unwrap();

    assert_eq!(wallet_state.unsent_transactions.len(), 1);
}

#[test]
fn transaction_is_serialized_correctly() {
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();

    let runtime_call = RuntimeCall::bank(call_message_from_file("requests/create_token.json"));
    let runtime_call_bytes = borsh::to_vec(&runtime_call).unwrap();

    let chain_id = 0;
    let max_priority_fee_bips = TEST_DEFAULT_MAX_PRIORITY_FEE;
    let max_fee = TEST_DEFAULT_MAX_FEE;
    let gas_limit = None;

    let unsigned_tx = UnsignedTransactionWithoutNonce::new(
        runtime_call,
        chain_id,
        max_priority_fee_bips,
        max_fee,
        gas_limit.clone(),
    );

    wallet_state.unsent_transactions.push(unsigned_tx);

    let key = <<TestSpec as Spec>::CryptoSpec as CryptoSpec>::PrivateKey::generate();
    let initial_nonce = 15;
    let txs = wallet_state.take_signed_transactions(&key, initial_nonce);

    for (i, tx) in txs.into_iter().enumerate() {
        let tx = Transaction::<TestSpec>::try_from_slice(&tx).unwrap();
        let tx_p = Transaction::<TestSpec>::new_signed_tx(
            &key,
            UnsignedTransaction::new(
                runtime_call_bytes.clone(),
                chain_id,
                max_priority_fee_bips,
                max_fee,
                initial_nonce + i as u64,
                gas_limit.clone(),
            ),
        );

        tx.verify(&mut UnlimitedGasMeter::new())
            .expect("the computed signature is incorrect");

        assert_eq!(
            tx, tx_p,
            "the stored transaction doesn't match the expected data"
        );
    }
}

#[test]
fn transaction_not_signed_without_accounts() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();

    let subcommand = RuntimeSubcommand::<FileNameArg, TestSpec, Da>::bank {
        contents: default_file_name_arg_for_test("requests/create_token.json"),
    };
    let workflow = TransactionWorkflow::Sign {
        transaction: TransactionLoadWorkflow::<
            RuntimeSubcommand<FileNameArg, TestSpec, Da>,
            RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
        >::FromFile(subcommand),
        nonce: 11,
        key_nickname: None,
        json_output: false,
    };

    let result = workflow.run::<Runtime<TestSpec, Da>, _, _, _, _, _>(
        &mut wallet_state,
        app_dir,
        std::io::stdout(),
    );

    assert!(result.is_err());
    let err_message = result.unwrap_err().to_string();
    assert_eq!(
        "No accounts found. You can generate one with the `keys generate` subcommand",
        err_message
    );
}

#[test]
fn transaction_signed_properly_from_file() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();
    import_key(&mut wallet_state, &app_dir);

    let bank_create_token_path = "requests/create_token.json";
    let subcommand = RuntimeSubcommand::<FileNameArg, TestSpec, Da>::bank {
        contents: default_file_name_arg_for_test(bank_create_token_path),
    };
    let runtime_call =
        RuntimeCall::<TestSpec, MockDaSpec>::bank(call_message_from_file(bank_create_token_path));
    let runtime_call_bytes = borsh::to_vec(&runtime_call).unwrap();

    let nonce = 11;
    let workflow = TransactionWorkflow::Sign {
        transaction: TransactionLoadWorkflow::<
            RuntimeSubcommand<FileNameArg, TestSpec, Da>,
            RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
        >::FromFile(subcommand),
        nonce,
        key_nickname: None,
        json_output: false,
    };

    let mut output = Vec::new();
    workflow
        .run::<Runtime<TestSpec, Da>, _, _, _, _, _>(&mut wallet_state, app_dir, &mut output)
        .unwrap();
    let output = String::from_utf8(output).expect("Not UTF-8");

    assert!(
        wallet_state.unsent_transactions.is_empty(),
        "Signed transaction should be imported"
    );

    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2);

    assert_eq!(
        &"Signed Transaction (borsh encoded):",
        &lines[lines.len() - 2]
    );
    let last_line = lines.last().unwrap();
    assert!(last_line.starts_with("0x"));
    let raw_signed_tx = hex::decode(&last_line[2..]).unwrap();

    let signed_tx: Transaction<TestSpec> = Transaction::try_from_slice(&raw_signed_tx).unwrap();
    signed_tx.verify(&mut UnlimitedGasMeter::new()).unwrap();

    let default_pubkey = &wallet_state.addresses.default_address().unwrap().pub_key;

    assert_eq!(default_pubkey, &signed_tx.pub_key);
    assert_eq!(nonce, signed_tx.nonce);

    assert_eq!(&runtime_call_bytes, &signed_tx.runtime_msg);
}

#[test]
fn transaction_signed_properly_from_json_string() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();
    import_key(&mut wallet_state, &app_dir);

    let create_token_path = "requests/create_token.json";
    let subcommand = RuntimeSubcommand::<JsonStringArg, TestSpec, Da>::bank {
        contents: default_json_string_arg_for_test(create_token_path),
    };
    let runtime_call =
        RuntimeCall::<TestSpec, MockDaSpec>::bank(call_message_from_file(create_token_path));
    let runtime_call_bytes = borsh::to_vec(&runtime_call).unwrap();

    let workflow = TransactionWorkflow::Sign {
        transaction: TransactionLoadWorkflow::<
            RuntimeSubcommand<FileNameArg, TestSpec, Da>,
            RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
        >::FromString(subcommand),
        nonce: 13,
        key_nickname: None,
        json_output: false,
    };

    let mut output = Vec::new();
    workflow
        .run::<Runtime<TestSpec, Da>, _, _, _, _, _>(&mut wallet_state, app_dir, &mut output)
        .unwrap();
    let output = String::from_utf8(output).expect("Not UTF-8");

    let last_line: &str = output.lines().last().unwrap();

    let raw_signed_tx = hex::decode(&last_line[2..]).unwrap();
    let signed_tx: Transaction<TestSpec> = Transaction::try_from_slice(&raw_signed_tx).unwrap();
    signed_tx.verify(&mut UnlimitedGasMeter::new()).unwrap();
    assert_eq!(&runtime_call_bytes, &signed_tx.runtime_msg);
}

#[test]
fn transaction_signed_by_account_nickname() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();

    let key1 = "key1";
    let key2 = "key2";
    let import_key = KeyWorkflow::Generate {
        nickname: Some(key1.to_string()),
    };
    import_key.run(&mut wallet_state, &app_dir).unwrap();
    let import_key = KeyWorkflow::Generate {
        nickname: Some(key2.to_string()),
    };
    import_key.run(&mut wallet_state, &app_dir).unwrap();

    let default_key = wallet_state
        .addresses
        .default_address()
        .unwrap()
        .nickname
        .clone()
        .unwrap();
    // Just a check which key is "default", in case if logic changes.
    assert_eq!(key1, default_key);

    let subcommand = RuntimeSubcommand::<FileNameArg, TestSpec, Da>::bank {
        contents: default_file_name_arg_for_test("requests/create_token.json"),
    };

    let nonce = 11;
    let workflow = TransactionWorkflow::Sign {
        transaction: TransactionLoadWorkflow::<
            RuntimeSubcommand<FileNameArg, TestSpec, Da>,
            RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
        >::FromFile(subcommand),
        nonce,
        key_nickname: Some(key2.to_string()),
        json_output: false,
    };

    let mut output = Vec::new();
    workflow
        .run::<Runtime<TestSpec, Da>, _, _, _, _, _>(&mut wallet_state, app_dir, &mut output)
        .unwrap();
    let output = String::from_utf8(output).expect("Not UTF-8");

    let last_line: &str = output.lines().last().unwrap();

    let raw_signed_tx = hex::decode(&last_line[2..]).unwrap();
    let signed_tx: Transaction<TestSpec> = Transaction::try_from_slice(&raw_signed_tx).unwrap();
    signed_tx.verify(&mut UnlimitedGasMeter::new()).unwrap();

    // the key
    let key2 = wallet_state
        .addresses
        .get_address(&KeyIdentifier::ByNickname {
            nickname: key2.to_string(),
        })
        .unwrap();

    assert_eq!(&key2.pub_key, &signed_tx.pub_key);
}

#[test]
fn transaction_outputs_json() {
    let app_dir = tempfile::tempdir().unwrap();
    let mut wallet_state = WalletState::<RuntimeCall<TestSpec, Da>, TestSpec>::default();
    import_key(&mut wallet_state, &app_dir);

    let subcommand = RuntimeSubcommand::<FileNameArg, TestSpec, Da>::bank {
        contents: default_file_name_arg_for_test("requests/create_token.json"),
    };

    let workflow = TransactionWorkflow::Sign {
        transaction: TransactionLoadWorkflow::<
            RuntimeSubcommand<FileNameArg, TestSpec, Da>,
            RuntimeSubcommand<JsonStringArg, TestSpec, Da>,
        >::FromFile(subcommand),
        nonce: 12,
        key_nickname: None,
        json_output: true,
    };

    let mut output = Vec::new();
    workflow
        .run::<Runtime<TestSpec, Da>, _, _, _, _, _>(&mut wallet_state, app_dir, &mut output)
        .unwrap();
    let output = String::from_utf8(output).expect("Not UTF-8");
    let output: serde_json::Value = serde_json::from_str(&output).unwrap();

    let hex_tx = match &output["signed_tx"] {
        serde_json::Value::String(s) => s,
        _ => panic!("Should be string at signed_tx"),
    };
    let raw_signed_tx = hex::decode(&hex_tx[2..]).unwrap();
    let signed_tx: Transaction<TestSpec> = Transaction::try_from_slice(&raw_signed_tx).unwrap();
    signed_tx.verify(&mut UnlimitedGasMeter::new()).unwrap();
}

fn make_test_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut sender_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    sender_path.push("test-data");

    sender_path.push(path);

    sender_path
}

fn default_file_name_arg_for_test(path: &str) -> FileNameArg {
    let test_path = make_test_path(path);
    FileNameArg {
        path: test_path.to_str().unwrap().into(),
        chain_id: 0,
        max_priority_fee_bips: 0,
        max_fee: 0,
        gas_limit: None,
    }
}

fn default_json_string_arg_for_test(path: impl AsRef<Path>) -> JsonStringArg {
    let test_path = make_test_path(path);
    JsonStringArg {
        json: std::fs::read_to_string(test_path).unwrap(),
        chain_id: 0,
        max_priority_fee_bips: 0,
        max_fee: 0,
        gas_limit: None,
    }
}

fn import_key<Tx, S>(wallet_state: &mut WalletState<Tx, S>, app_dir: impl AsRef<Path>)
where
    Tx: BorshSerialize + BorshDeserialize + serde::Serialize + serde::de::DeserializeOwned,
    S: Spec,
{
    let workflow = KeyWorkflow::Generate {
        nickname: Some("key1".into()),
    };
    workflow.run(wallet_state, &app_dir).unwrap();
}

fn call_message_from_file<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    let runtime_call_path = make_test_path(path);
    let runtime_call_json = std::fs::read_to_string(runtime_call_path).unwrap();
    serde_json::from_str::<T>(&runtime_call_json).unwrap()
}
