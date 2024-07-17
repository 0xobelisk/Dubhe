use std::convert::Infallible;

use reth_primitives::{Bytes, TransactionKind};
use revm::primitives::{
    Address, BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, ExecutionResult, Output, KECCAK_EMPTY, U256,
};
use revm::{Database, DatabaseCommit};
use sov_modules_api::macros::config_value;
use sov_modules_api::WorkingSet;
use sov_prover_storage_manager::new_orphan_storage;
use sov_test_utils::SimpleStorageContract;

use super::db_init::InitEvmDb;
use super::executor;
use crate::tests::test_signer::TestSigner;
use crate::{Evm, SpecId};

type S = sov_test_utils::TestSpec;

#[test]
fn simple_contract_execution_sov_state() {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut working_set: WorkingSet<S> =
        WorkingSet::new_deprecated(new_orphan_storage(tmpdir.path()).unwrap());

    let evm = Evm::<S>::default();
    let evm_db = evm.get_db(&mut working_set);

    simple_contract_execution(evm_db);
}

fn simple_contract_execution<DB: Database<Error = Infallible> + DatabaseCommit + InitEvmDb>(
    mut evm_db: DB,
) {
    let dev_signer = TestSigner::new_random();
    let caller = dev_signer.address();
    evm_db.insert_account_info(
        caller,
        revm::primitives::AccountInfo {
            balance: U256::from(1000000000),
            code_hash: KECCAK_EMPTY,
            nonce: 1,
            code: None,
        },
    );

    let contract = SimpleStorageContract::default();

    // We are not supporting CANCUN yet
    // https://github.com/Sovereign-Labs/sovereign-sdk/issues/912
    let mut cfg_env_with_handler = CfgEnvWithHandlerCfg::new(
        CfgEnv::default(),
        revm_primitives::HandlerCfg {
            spec_id: SpecId::SHANGHAI,
        },
    );
    cfg_env_with_handler.chain_id = config_value!("CHAIN_ID");

    let contract_address: Address = {
        let (tx, signer) = dev_signer
            .sign_default_transaction(TransactionKind::Create, contract.byte_code().to_vec(), 1)
            .unwrap();

        let tx = &tx.try_into().unwrap();
        let block_env = BlockEnv {
            gas_limit: U256::from(reth_primitives::constants::ETHEREUM_BLOCK_GAS_LIMIT),
            ..Default::default()
        };

        let result = executor::execute_tx(
            &mut evm_db,
            &block_env,
            tx,
            signer,
            cfg_env_with_handler.clone(),
        )
        .unwrap();
        contract_address(&result).expect("Expected successful contract creation")
    };

    let set_arg = 21989;

    {
        let call_data = contract.set_call_data(set_arg);

        let (tx, signer) = dev_signer
            .sign_default_transaction(
                TransactionKind::Call(contract_address),
                hex::decode(hex::encode(&call_data)).unwrap(),
                2,
            )
            .unwrap();

        let tx = &tx.try_into().unwrap();
        executor::execute_tx(
            &mut evm_db,
            &BlockEnv::default(),
            tx,
            signer,
            cfg_env_with_handler.clone(),
        )
        .unwrap();
    }

    let get_res = {
        let call_data = contract.get_call_data();

        let (tx, signer) = dev_signer
            .sign_default_transaction(
                TransactionKind::Call(contract_address),
                hex::decode(hex::encode(&call_data)).unwrap(),
                3,
            )
            .unwrap();

        let tx = &tx.try_into().unwrap();
        let result = executor::execute_tx(
            &mut evm_db,
            &BlockEnv::default(),
            tx,
            signer,
            cfg_env_with_handler.clone(),
        )
        .unwrap();

        let out = output(result);
        U256::from_be_slice(out.as_ref())
    };

    assert_eq!(set_arg, get_res.to::<u32>());

    {
        let failing_call_data = contract.failing_function_call_data();

        let (tx, signer) = dev_signer
            .sign_default_transaction(
                TransactionKind::Call(contract_address),
                hex::decode(hex::encode(&failing_call_data)).unwrap(),
                4,
            )
            .unwrap();

        let tx = &tx.try_into().unwrap();
        let result = executor::execute_tx(
            &mut evm_db,
            &BlockEnv::default(),
            tx,
            signer,
            cfg_env_with_handler.clone(),
        )
        .unwrap();

        assert!(matches!(result, ExecutionResult::Revert { .. }));
    }
}

fn contract_address(result: &ExecutionResult) -> Option<Address> {
    match result {
        ExecutionResult::Success {
            output: Output::Create(_, Some(addr)),
            ..
        } => Some(Address(**addr)),
        _ => None,
    }
}

fn output(result: ExecutionResult) -> Bytes {
    match result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Call(out) => out,
            Output::Create(out, _) => out,
        },
        _ => panic!("Expected successful ExecutionResult"),
    }
}
