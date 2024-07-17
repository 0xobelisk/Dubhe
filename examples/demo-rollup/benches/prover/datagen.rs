use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use demo_stf::authentication::ModAuth;
use demo_stf::genesis_config::{AccountConfig, AccountData};
use sov_cli::wallet_state::PrivateKeyAndAddress;
use sov_demo_rollup::MockDemoRollup;
use sov_mock_da::{MockAddress, MockBlock, MockDaService, MockDaSpec};
use sov_modules_api::execution_mode::Native;
use sov_modules_api::{CredentialId, PrivateKey, PublicKey, Spec};
use sov_rollup_interface::services::da::DaService;
use sov_test_utils::generators::bank::BankMessageGenerator;
use sov_test_utils::{MessageGenerator, TestHasher, TestPrivateKey, TestSpec};

type S = <MockDemoRollup<Native> as sov_modules_rollup_blueprint::RollupBlueprint<Native>>::Spec;

const DEFAULT_BLOCKS: u64 = 10;
const DEFAULT_TXNS_PER_BLOCK: u64 = 100;
const DEFAULT_NUM_PUB_KEYS: u64 = 1000;

pub fn generate_genesis_config(config_dir: &str) -> anyhow::Result<()> {
    let num_pub_keys = match env::var("NUM_PUB_KEYS") {
        Ok(num_pub_keys_str) => num_pub_keys_str.parse::<u64>()?,
        Err(_) => {
            println!("NUM_PUB_KEYS not set, using default");
            DEFAULT_NUM_PUB_KEYS
        }
    };

    let mut accs = vec![];

    let priv_keys = (0..num_pub_keys).map(|_| TestPrivateKey::generate());

    for priv_key in priv_keys {
        let credential_id: CredentialId = priv_key.pub_key().credential_id::<TestHasher>();
        let account = AccountData::<<TestSpec as Spec>::Address> {
            credential_id,
            address: priv_key.to_address(),
        };
        accs.push(account);
    }

    let config = AccountConfig::<TestSpec> { accounts: accs };

    let file = File::create(Path::join(Path::new(config_dir), "accounts.json")).unwrap();
    let data_buf = BufWriter::new(file);
    Ok(serde_json::ser::to_writer(data_buf, &config)?)
}

const PRIVATE_KEYS_DIR: &str = "../test-data/keys";

fn read_and_parse_private_key<S: Spec>(suffix: &str) -> PrivateKeyAndAddress<S> {
    let data = std::fs::read_to_string(Path::new(PRIVATE_KEYS_DIR).join(suffix))
        .expect("Unable to read file to string");

    let key_and_address: PrivateKeyAndAddress<S> =
        serde_json::from_str(&data).unwrap_or_else(|_| {
            panic!("Unable to convert data {} to PrivateKeyAndAddress", &data);
        });

    assert!(
        key_and_address.is_matching_to_default(),
        "Inconsistent key data"
    );

    key_and_address
}

pub async fn get_bench_blocks() -> anyhow::Result<Vec<MockBlock>> {
    let txns_per_block = match env::var("TXNS_PER_BLOCK") {
        Ok(txns_per_block) => txns_per_block.parse::<u64>()?,
        Err(_) => {
            println!("TXNS_PER_BLOCK not set, using default");
            DEFAULT_TXNS_PER_BLOCK
        }
    };

    let block_cnt = match env::var("BLOCKS") {
        Ok(block_cnt_str) => block_cnt_str.parse::<u64>()?,
        Err(_) => {
            println!("BLOCKS not set, using default");
            DEFAULT_BLOCKS
        }
    };

    let da_service = MockDaService::new(MockAddress::default());
    let mut blocks = vec![];

    let private_key_and_address: PrivateKeyAndAddress<S> =
        read_and_parse_private_key("minter_private_key.json");

    let (create_token_message_gen, transfer_message_gen) =
        BankMessageGenerator::generate_token_and_random_transfers(
            txns_per_block,
            private_key_and_address.private_key,
        );
    let blob = create_token_message_gen
        .create_blobs::<<MockDemoRollup<Native> as sov_modules_rollup_blueprint::RollupBlueprint<
        Native,
    >>::Runtime, ModAuth<S, MockDaSpec>>();

    let fee = da_service.estimate_fee(blob.len()).await.unwrap();
    da_service.send_transaction(&blob, fee).await.unwrap();
    let block1 = da_service.get_block_at(1).await.unwrap();
    blocks.push(block1);

    for i in 0..block_cnt {
        let blob = transfer_message_gen.create_blobs::<<MockDemoRollup<Native> as sov_modules_rollup_blueprint::RollupBlueprint<Native>>::Runtime, ModAuth<S, MockDaSpec>>();
        let fee = da_service.estimate_fee(blob.len()).await.unwrap();
        da_service.send_transaction(&blob, fee).await.unwrap();
        let blocki = da_service.get_block_at(2 + i).await.unwrap();
        blocks.push(blocki);
    }

    Ok(blocks)
}
