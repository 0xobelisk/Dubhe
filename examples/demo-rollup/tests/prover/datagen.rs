use std::env;

use demo_stf::authentication::ModAuth;
use sov_cli::wallet_state::PrivateKeyAndAddress;
use sov_demo_rollup::MockDemoRollup;
use sov_mock_da::{MockAddress, MockBlock, MockDaService};
use sov_modules_api::execution_mode::{Native, WitnessGeneration};
use sov_rollup_interface::services::da::DaService;
use sov_test_utils::generators::bank::BankMessageGenerator;
use sov_test_utils::MessageGenerator;

use crate::prover::MockDaSpec;
use crate::test_helpers::read_private_keys;
type S = sov_modules_api::default_spec::DefaultSpec<
    sov_risc0_adapter::Risc0Verifier,
    sov_mock_zkvm::MockZkVerifier,
    WitnessGeneration,
>;

const DEFAULT_BLOCKS: u64 = 10;
const DEFAULT_TXNS_PER_BLOCK: u64 = 100;

pub async fn get_blocks_from_da() -> anyhow::Result<Vec<MockBlock>> {
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
        read_private_keys::<S>("minter_private_key.json");

    let (create_token_message_gen, transfer_message_gen) =
        BankMessageGenerator::generate_token_and_random_transfers(
            txns_per_block,
            private_key_and_address.private_key,
        );
    let blob = create_token_message_gen.create_blobs::<<MockDemoRollup<Native> as sov_modules_rollup_blueprint::RollupBlueprint<Native>>::Runtime, ModAuth<S, MockDaSpec>>();
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
