use std::convert::Infallible;

use reth_primitives::{Address, Bytes, TransactionKind};
use revm::primitives::{SpecId, KECCAK_EMPTY, U256};
use sov_modules_api::transaction::Credentials;
use sov_modules_api::utils::generate_address;
use sov_modules_api::{
    Context, KernelWorkingSet, Module, StateAccessor, StateCheckpoint, VersionedStateReadWriter,
};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::VisibleHash;
use sov_test_utils::SimpleStorageContract;

use crate::call::CallMessage;
use crate::evm::primitive_types::Receipt;
use crate::tests::genesis_tests::setup;
use crate::tests::test_signer::TestSigner;
use crate::{AccountData, EvmConfig};

type S = sov_test_utils::TestSpec;

#[test]
fn call_test() -> Result<(), Infallible> {
    let dev_signer: TestSigner = TestSigner::new_random();
    let evm_config = EvmConfig {
        data: vec![AccountData {
            address: dev_signer.address(),
            balance: U256::from(1000000000),
            code_hash: KECCAK_EMPTY,
            code: Bytes::default(),
            nonce: 0,
        }],
        // SHANGHAI instead of LATEST
        // https://github.com/Sovereign-Labs/sovereign-sdk/issues/912
        spec: vec![(0, SpecId::SHANGHAI)].into_iter().collect(),
        ..Default::default()
    };

    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&evm_config, state_checkpoint);

    let contract_addr: Address = Address::from_slice(
        hex::decode("819c5497b157177315e1204f52e588b393771719")
            .unwrap()
            .as_slice(),
    );

    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1);
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);
    evm.begin_slot_hook(VisibleHash::new([10u8; 32]), &mut versioned_ws);

    let set_arg = 999;
    let mut working_set = state_checkpoint.to_working_set_unmetered();
    {
        let sender_address = generate_address::<S>("sender");
        let sequencer_address = generate_address::<S>("sequencer");

        let messages = vec![
            create_contract_message(&dev_signer, 0),
            set_arg_message(contract_addr, &dev_signer, 1, set_arg),
        ];
        for (tx, signer) in messages {
            let context = Context::<S>::new(
                sender_address,
                Credentials::new(signer),
                sequencer_address,
                1,
            );
            evm.call(tx, &context, &mut working_set).unwrap();
        }
    }
    let mut state_checkpoint = working_set.checkpoint().0;
    evm.end_slot_hook(&mut state_checkpoint);

    let db_account = evm
        .accounts
        .get(&contract_addr, &mut state_checkpoint)?
        .unwrap();
    let storage_value = db_account
        .storage
        .get(&U256::ZERO, &mut state_checkpoint)?
        .unwrap();

    assert_eq!(U256::from(set_arg), storage_value);
    assert_eq!(
        evm.receipts
            .iter(&mut state_checkpoint.accessory_state())
            .collect::<Vec<_>>(),
        [
            Receipt {
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 132943,
                    logs: vec![]
                },
                gas_used: 132943,
                log_index_start: 0,
                error: None
            },
            Receipt {
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 176673,
                    logs: vec![]
                },
                gas_used: 43730,
                log_index_start: 0,
                error: None
            }
        ]
    );

    Ok(())
}

#[test]
fn failed_transaction_test() -> Result<(), Infallible> {
    let dev_signer: TestSigner = TestSigner::new_random();
    let binding = EvmConfig::default();
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (evm, mut state_checkpoint) = setup(&binding, state_checkpoint);
    let mut temp_kernel = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    temp_kernel.update_virtual_height(1);
    let mut versioned_ws = VersionedStateReadWriter::from_kernel_ws_virtual(temp_kernel);

    evm.begin_slot_hook(VisibleHash::new([10u8; 32]), &mut versioned_ws);
    let mut working_set = state_checkpoint.to_working_set_unmetered();
    {
        let sender_address = generate_address::<S>("sender");
        let sequencer_address = generate_address::<S>("sequencer");
        let (message, signer) = create_contract_message(&dev_signer, 0);
        let context = Context::<S>::new(
            sender_address,
            Credentials::new(signer),
            sequencer_address,
            1,
        );

        evm.call(message, &context, &mut working_set).unwrap();
    }

    // assert no pending transaction
    let mut unmetered_ws = working_set.to_unmetered();
    let pending_txs = evm.pending_transactions.iter(&mut unmetered_ws);
    assert_eq!(pending_txs.len(), 0);

    let state_checkpoint = &mut working_set.checkpoint().0;
    evm.end_slot_hook(state_checkpoint);

    // Assert block does not have any transaction
    let block = evm
        .pending_head
        .get(&mut state_checkpoint.accessory_state())?
        .unwrap();
    assert_eq!(block.transactions.start, 0);
    assert_eq!(block.transactions.end, 0);

    Ok(())
}

fn create_contract_message(dev_signer: &TestSigner, nonce: u64) -> (CallMessage, Address) {
    let contract = SimpleStorageContract::default();
    let (signed_tx, signer) = dev_signer
        .sign_default_transaction(
            TransactionKind::Create,
            contract.byte_code().to_vec(),
            nonce,
        )
        .unwrap();
    (CallMessage { rlp: signed_tx }, signer)
}

fn set_arg_message(
    contract_addr: Address,
    dev_signer: &TestSigner,
    nonce: u64,
    set_arg: u32,
) -> (CallMessage, Address) {
    let contract = SimpleStorageContract::default();
    let (signed_tx, signer) = dev_signer
        .sign_default_transaction(
            TransactionKind::Call(contract_addr),
            hex::decode(hex::encode(&contract.set_call_data(set_arg))).unwrap(),
            nonce,
        )
        .unwrap();

    (CallMessage { rlp: signed_tx }, signer)
}
