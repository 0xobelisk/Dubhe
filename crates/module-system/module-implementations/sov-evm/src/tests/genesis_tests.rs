use std::convert::Infallible;

use lazy_static::lazy_static;
use reth_primitives::constants::{EMPTY_RECEIPTS, EMPTY_TRANSACTIONS, ETHEREUM_BLOCK_GAS_LIMIT};
use reth_primitives::hex_literal::hex;
use reth_primitives::{
    Address, BaseFeeParams, Bloom, Bytes, Header, SealedHeader, B256, EMPTY_OMMER_ROOT_HASH,
};
use revm::primitives::{SpecId, KECCAK_EMPTY, U256};
use sov_modules_api::{KernelWorkingSet, Module, StateCheckpoint};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::VisibleHash;

use crate::evm::primitive_types::{Block, SealedBlock};
use crate::evm::{DbAccount, EvmChainConfig};
use crate::{AccountData, Evm, EvmConfig};

type S = sov_test_utils::TestSpec;

lazy_static! {
    pub(crate) static ref TEST_CONFIG: EvmConfig = EvmConfig {
        data: vec![AccountData {
            address: Address::from([1u8; 20]),
            balance: U256::from(1000000000),
            code_hash: KECCAK_EMPTY,
            code: Bytes::default(),
            nonce: 0,
        }],
        spec: vec![(0, SpecId::BERLIN), (1, SpecId::SHANGHAI)]
            .into_iter()
            .collect(),
        chain_id: 1000,
        block_gas_limit: ETHEREUM_BLOCK_GAS_LIMIT,
        block_timestamp_delta: 2,
        genesis_timestamp: 50,
        coinbase: Address::from([3u8; 20]),
        limit_contract_code_size: Some(5000),
        starting_base_fee: 70,
        base_fee_params: BaseFeeParams::ethereum(),
    };
}

pub(crate) const GENESIS_HASH: B256 = B256::new(hex!(
    "3441c3084e43183a53aabbbe3e94512bb3db4aca826af8f23b38f0613811571d"
));

pub(crate) const GENESIS_STATE_ROOT: B256 = B256::new(hex!(
    "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
));

pub(crate) static BENEFICIARY: Address = Address::new([3u8; 20]);

#[test]
fn genesis_data() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();

    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);

    let account = &TEST_CONFIG.data[0];

    let db_account = evm
        .accounts
        .get(&account.address, &mut state_checkpoint)?
        .unwrap();

    let mut working_set = state_checkpoint.to_working_set_unmetered();
    let evm_db = evm.get_db(&mut working_set);

    assert_eq!(
        db_account,
        DbAccount::new_with_info(
            evm_db.accounts.prefix(),
            TEST_CONFIG.data[0].address,
            revm::primitives::AccountInfo {
                balance: account.balance,
                code_hash: account.code_hash,
                nonce: account.nonce,
                code: None,
            }
        ),
    );

    Ok(())
}

#[test]
fn genesis_cfg() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);

    let cfg = evm.cfg.get(&mut state_checkpoint)?.unwrap();
    assert_eq!(
        cfg,
        EvmChainConfig {
            spec: vec![(0, SpecId::BERLIN), (1, SpecId::SHANGHAI)],
            chain_id: 1000,
            block_gas_limit: ETHEREUM_BLOCK_GAS_LIMIT,
            block_timestamp_delta: 2,
            coinbase: Address::from([3u8; 20]),
            limit_contract_code_size: Some(5000),
            base_fee_params: BaseFeeParams::ethereum(),
        }
    );

    Ok(())
}

#[test]
#[should_panic(expected = "EVM spec must start from block 0")]
fn genesis_cfg_missing_specs() {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    setup(
        &EvmConfig {
            spec: vec![(5, SpecId::BERLIN)].into_iter().collect(),
            ..Default::default()
        },
        state_checkpoint,
    );
}

#[test]
fn genesis_empty_spec_defaults_to_shanghai() -> Result<(), Infallible> {
    let mut config = TEST_CONFIG.clone();
    config.spec.clear();
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&config, state_checkpoint);

    let cfg = evm.cfg.get(&mut state_checkpoint)?.unwrap();
    assert_eq!(cfg.spec, vec![(0, SpecId::SHANGHAI)]);

    Ok(())
}

#[test]
#[should_panic(expected = "Cancun is not supported")]
fn genesis_cfg_cancun() {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    setup(
        &EvmConfig {
            spec: vec![(0, SpecId::CANCUN)].into_iter().collect(),
            ..Default::default()
        },
        state_checkpoint,
    );
}

#[test]
fn genesis_block() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();

    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);
    let mut accessory_state = state_checkpoint.accessory_state();

    let block_number = evm
        .block_hashes
        .get(&GENESIS_HASH, &mut accessory_state)?
        .unwrap();

    let block = evm
        .blocks
        .get(block_number as usize, &mut accessory_state)?
        .unwrap();

    assert_eq!(block_number, 0);

    let expected_header = Header {
        parent_hash: B256::default(),
        state_root: B256::from_slice(&hex!(
            "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"
        )),
        transactions_root: EMPTY_TRANSACTIONS,
        receipts_root: EMPTY_RECEIPTS,
        logs_bloom: Bloom::default(),
        difficulty: U256::ZERO,
        number: 0,
        gas_limit: ETHEREUM_BLOCK_GAS_LIMIT,
        gas_used: 0,
        timestamp: 50,
        extra_data: Bytes::default(),
        mix_hash: B256::default(),
        nonce: 0,
        base_fee_per_gas: Some(70),
        ommers_hash: EMPTY_OMMER_ROOT_HASH,
        beneficiary: BENEFICIARY,
        withdrawals_root: None,
        blob_gas_used: None,
        excess_blob_gas: None,
        parent_beacon_block_root: None,
    };

    let expected_block = SealedBlock {
        header: SealedHeader::new(expected_header, GENESIS_HASH),
        transactions: 0u64..0u64,
    };

    assert_eq!(expected_block, block);

    Ok(())
}

#[test]
fn genesis_head() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&TEST_CONFIG, state_checkpoint);
    let head = evm.head.get(&mut state_checkpoint)?.unwrap();

    assert_eq!(
        head,
        Block {
            header: Header {
                parent_hash: B256::default(),
                state_root: GENESIS_STATE_ROOT,
                transactions_root: EMPTY_TRANSACTIONS,
                receipts_root: EMPTY_RECEIPTS,
                logs_bloom: Bloom::default(),
                difficulty: U256::ZERO,
                number: 0,
                gas_limit: ETHEREUM_BLOCK_GAS_LIMIT,
                gas_used: 0,
                timestamp: 50,
                extra_data: Bytes::default(),
                mix_hash: B256::default(),
                nonce: 0,
                base_fee_per_gas: Some(70),
                ommers_hash: EMPTY_OMMER_ROOT_HASH,
                beneficiary: BENEFICIARY,
                withdrawals_root: None,
                blob_gas_used: None,
                excess_blob_gas: None,
                parent_beacon_block_root: None,
            },
            transactions: 0u64..0u64,
        }
    );

    Ok(())
}

pub(crate) fn setup(
    evm_config: &EvmConfig,
    state_checkpoint: StateCheckpoint<S>,
) -> (Evm<S>, StateCheckpoint<S>) {
    let evm = Evm::<S>::default();
    let mut genesis_ws = state_checkpoint.to_genesis_state_accessor::<Evm<S>>(evm_config);
    evm.genesis(evm_config, &mut genesis_ws).unwrap();
    let mut state_checkpoint = genesis_ws.checkpoint();
    let kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    evm.finalize_hook(
        VisibleHash::new([10u8; 32]),
        &mut kernel_working_set.inner.accessory_state(),
    );

    (evm, state_checkpoint)
}
