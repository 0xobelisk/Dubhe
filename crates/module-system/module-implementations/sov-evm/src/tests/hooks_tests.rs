use std::convert::Infallible;

use reth_primitives::hex_literal::hex;
use reth_primitives::{
    Address, Bloom, Bytes, Header, SealedHeader, Signature, TransactionSigned, B256,
    EMPTY_OMMER_ROOT_HASH, KECCAK_EMPTY, U256,
};
use revm::primitives::BlockEnv;
use sov_modules_api::macros::config_value;
use sov_modules_api::{KernelWorkingSet, StateCheckpoint, VersionedStateReadWriter};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::VisibleHash;

use super::genesis_tests::{setup, TEST_CONFIG};
use crate::evm::primitive_types::{Block, Receipt, SealedBlock, TransactionSignedAndRecovered};
use crate::tests::genesis_tests::{BENEFICIARY, GENESIS_HASH};
use crate::PendingTransaction;

pub(crate) const DA_ROOT_HASH: B256 = B256::new([10u8; 32]);

#[test]
fn begin_slot_hook_creates_pending_block() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);
    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1);
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);
    evm.begin_slot_hook(VisibleHash::new([10u8; 32]), &mut versioned_ws);
    let pending_block = evm.block_env.get(&mut state_checkpoint)?.unwrap();
    assert_eq!(
        pending_block,
        BlockEnv {
            number: U256::from(1),
            coinbase: BENEFICIARY,
            timestamp: U256::from(
                TEST_CONFIG.genesis_timestamp + TEST_CONFIG.block_timestamp_delta
            ),
            prevrandao: Some(DA_ROOT_HASH),
            basefee: U256::from(62),
            gas_limit: U256::from(TEST_CONFIG.block_gas_limit),
            difficulty: Default::default(),
            blob_excess_gas_and_price: None,
        }
    );
    Ok(())
}

#[test]
fn end_slot_hook_sets_head() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);
    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1);
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);
    evm.begin_slot_hook(VisibleHash::new([10u8; 32]), &mut versioned_ws);

    evm.pending_transactions.push(
        &create_pending_transaction(B256::from([1u8; 32]), 1),
        &mut state_checkpoint,
    )?;

    evm.pending_transactions.push(
        &create_pending_transaction(B256::from([2u8; 32]), 2),
        &mut state_checkpoint,
    )?;

    evm.end_slot_hook(&mut state_checkpoint);
    let head = evm.head.get(&mut state_checkpoint)?.unwrap();
    let pending_head = evm
        .pending_head
        .get(&mut state_checkpoint.accessory_state())?
        .unwrap();

    assert_eq!(head, pending_head);
    assert_eq!(
        head,
        Block {
            header: Header {
                // TODO: temp parent hash until: https://github.com/Sovereign-Labs/sovereign-sdk/issues/876
                parent_hash: GENESIS_HASH,

                ommers_hash: EMPTY_OMMER_ROOT_HASH,
                beneficiary: TEST_CONFIG.coinbase,
                state_root: KECCAK_EMPTY,
                transactions_root: B256::from(hex!(
                    "9c3857045a725a519d5328ba197188bceacc6178760c4f7eac7a423666320104"
                )),
                receipts_root: B256::from(hex!(
                    "27036187b3f5e87d4306b396cf06c806da2cc9a0fef9b07c042e3b4304e01c64"
                )),
                withdrawals_root: None,
                logs_bloom: Bloom::default(),
                difficulty: U256::ZERO,
                number: 1,
                gas_limit: TEST_CONFIG.block_gas_limit,
                gas_used: 200u64,
                timestamp: TEST_CONFIG.genesis_timestamp + TEST_CONFIG.block_timestamp_delta,
                mix_hash: DA_ROOT_HASH,
                nonce: 0,
                base_fee_per_gas: Some(62u64),
                extra_data: Bytes::default(),
                blob_gas_used: None,
                excess_blob_gas: None,
                parent_beacon_block_root: None,
            },
            transactions: 0..2
        }
    );
    Ok(())
}

#[test]
fn end_slot_hook_moves_transactions_and_receipts() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);
    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1);
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);
    evm.begin_slot_hook(VisibleHash::new([10u8; 32]), &mut versioned_ws);

    let tx1 = create_pending_transaction(B256::from([1u8; 32]), 1);
    evm.pending_transactions.push(&tx1, &mut state_checkpoint)?;

    let tx2 = create_pending_transaction(B256::from([2u8; 32]), 2);
    evm.pending_transactions.push(&tx2, &mut state_checkpoint)?;

    evm.end_slot_hook(&mut state_checkpoint);

    let tx1_hash = tx1.transaction.signed_transaction.hash;
    let tx2_hash = tx2.transaction.signed_transaction.hash;

    assert_eq!(
        evm.transactions
            .iter(&mut state_checkpoint.accessory_state())
            .collect::<Vec<_>>(),
        [tx1.transaction, tx2.transaction]
    );

    assert_eq!(
        evm.receipts
            .iter(&mut state_checkpoint.accessory_state())
            .collect::<Vec<_>>(),
        [tx1.receipt, tx2.receipt]
    );

    assert_eq!(
        evm.transaction_hashes
            .get(&tx1_hash, &mut state_checkpoint.accessory_state())?
            .unwrap(),
        0
    );

    assert_eq!(
        evm.transaction_hashes
            .get(&tx2_hash, &mut state_checkpoint.accessory_state())?
            .unwrap(),
        1
    );

    assert_eq!(evm.pending_transactions.len(&mut state_checkpoint)?, 0);

    Ok(())
}

fn create_pending_transaction(hash: B256, index: u64) -> PendingTransaction {
    PendingTransaction {
        transaction: TransactionSignedAndRecovered {
            signer: Address::from([1u8; 20]),
            signed_transaction: TransactionSigned {
                hash,
                signature: Signature::default(),
                transaction: reth_primitives::Transaction::Eip1559(reth_primitives::TxEip1559 {
                    chain_id: config_value!("CHAIN_ID"),
                    nonce: 1u64,
                    gas_limit: 1000u64,
                    max_fee_per_gas: 2000u64 as u128,
                    max_priority_fee_per_gas: 3000u64 as u128,
                    to: reth_primitives::TransactionKind::Call(Address::from([3u8; 20])),
                    value: U256::from(4000u64),
                    access_list: reth_primitives::AccessList::default(),
                    input: Bytes::from([4u8; 20]),
                }),
            },
            block_number: 1,
        },
        receipt: Receipt {
            receipt: reth_primitives::Receipt {
                tx_type: reth_primitives::TxType::Eip1559,
                success: true,
                cumulative_gas_used: 100u64 * index,
                logs: vec![],
            },
            gas_used: 100u64,
            log_index_start: 0,
            error: None,
        },
    }
}

#[test]
fn finalize_hook_creates_final_block() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);
    let p = [10u8; 32];
    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1);
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);
    evm.begin_slot_hook(VisibleHash::new(p), &mut versioned_ws);
    evm.pending_transactions.push(
        &create_pending_transaction(B256::from([1u8; 32]), 1),
        &mut state_checkpoint,
    )?;
    evm.pending_transactions.push(
        &create_pending_transaction(B256::from([2u8; 32]), 2),
        &mut state_checkpoint,
    )?;
    evm.end_slot_hook(&mut state_checkpoint);

    let root_hash = [99u8; 32];

    let mut accessory_state = state_checkpoint.accessory_state();
    evm.finalize_hook(VisibleHash::new(root_hash), &mut accessory_state);
    assert_eq!(evm.blocks.len(&mut accessory_state)?, 2);

    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1); // Because we haven't invoked the chain-state hooks, re-use the block number
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);

    evm.begin_slot_hook(VisibleHash::new(root_hash), &mut versioned_ws);

    let mut accessory_state = state_checkpoint.accessory_state();

    let parent_block = evm.blocks.get(0usize, &mut accessory_state)?.unwrap();
    let parent_hash = parent_block.header.hash();
    let block = evm.blocks.get(1usize, &mut accessory_state)?.unwrap();

    assert_eq!(
        block,
        SealedBlock {
            header: SealedHeader::new(
                Header {
                    parent_hash,
                    ommers_hash: EMPTY_OMMER_ROOT_HASH,
                    beneficiary: TEST_CONFIG.coinbase,
                    state_root: B256::from(root_hash),
                    transactions_root: B256::from(hex!(
                        "9c3857045a725a519d5328ba197188bceacc6178760c4f7eac7a423666320104"
                    )),
                    receipts_root: B256::from(hex!(
                        "27036187b3f5e87d4306b396cf06c806da2cc9a0fef9b07c042e3b4304e01c64"
                    )),
                    withdrawals_root: None,
                    logs_bloom: Bloom::default(),
                    difficulty: U256::ZERO,
                    number: 1,
                    gas_limit: 30000000,
                    gas_used: 200,
                    timestamp: 52,
                    mix_hash: DA_ROOT_HASH,
                    nonce: 0,
                    base_fee_per_gas: Some(62),
                    extra_data: Bytes::default(),
                    blob_gas_used: None,
                    excess_blob_gas: None,
                    parent_beacon_block_root: None,
                },
                B256::from(hex!(
                    // This hash changes because the header is different
                    "2dc597cf3b00b4af7c30e0cfd4f26340557543d0e2f168f96b8dfe1546b0699c"
                )),
            ),
            transactions: 0..2
        }
    );

    assert_eq!(
        evm.block_hashes
            .get(&block.header.hash(), &mut accessory_state)?
            .unwrap(),
        1u64
    );

    assert_eq!(evm.pending_head.get(&mut accessory_state)?, None);

    Ok(())
}
