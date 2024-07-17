use std::convert::Infallible;

use sov_modules_api::prelude::*;
use sov_modules_api::{
    Address, Context, CredentialId, Module, PrivateKey, PublicKey, StateCheckpoint,
};
use sov_prover_storage_manager::new_orphan_storage;
use sov_test_utils::{TestHasher, TestPrivateKey};

use crate::query::Response;
use crate::{call, Account, AccountConfig, AccountData, Accounts};

type S = sov_test_utils::TestSpec;

#[test]
fn test_config_account() -> Result<(), Infallible> {
    let priv_key = TestPrivateKey::generate();
    let init_pub_key = &priv_key.pub_key();
    let init_addr = init_pub_key.to_address::<<S as Spec>::Address>();
    let init_credential_id: CredentialId = init_pub_key.credential_id::<TestHasher>();

    let account_config = AccountConfig {
        accounts: vec![AccountData {
            credential_id: init_credential_id,
            address: init_pub_key.into(),
        }],
    };

    let accounts = &mut Accounts::<S>::default();
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::<S>::new(new_orphan_storage(tmpdir.path()).unwrap());
    let mut genesis = state.to_genesis_state_accessor::<Accounts<S>>(&account_config);

    accounts.init_module(&account_config, &mut genesis).unwrap();

    let query_response = accounts
        .accounts
        .get(&init_credential_id, &mut genesis)?
        .map(|Account { addr: a }| a);

    assert_eq!(query_response, Some(init_addr));

    Ok(())
}

#[test]
fn test_update_account() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let accounts = &mut Accounts::<S>::default();

    let priv_key = TestPrivateKey::generate();
    let sequencer_priv_key = TestPrivateKey::generate();

    let sender = priv_key.pub_key();
    let sequencer = sequencer_priv_key.pub_key();
    let sender_addr = sender.to_address::<<S as Spec>::Address>();
    let sender_credential_id: CredentialId = sender.credential_id::<TestHasher>();

    let sequencer_addr = sequencer.to_address::<<S as Spec>::Address>();
    let sender_context = Context::<S>::new(sender_addr, Default::default(), sequencer_addr, 1);

    let config = AccountConfig {
        accounts: vec![AccountData {
            credential_id: sender_credential_id,
            address: sender_addr,
        }],
    };

    let state = StateCheckpoint::<S>::new(new_orphan_storage(tmpdir.path()).unwrap());
    let mut genesis = state.to_genesis_state_accessor::<Accounts<S>>(&config);

    accounts.init_module(&config, &mut genesis).unwrap();
    // Test new account creation
    {
        let query_response = accounts
            .accounts
            .get(&sender_credential_id, &mut genesis)?
            .map(|Account { addr: a }| a);

        assert_eq!(query_response, Some(sender_addr));
    }

    let state = genesis.checkpoint();

    // Test credentials id update
    {
        let mut working_set = state.to_working_set_unmetered();
        let priv_key = TestPrivateKey::generate();
        let new_pub_key = priv_key.pub_key();
        let new_credential_id: CredentialId = new_pub_key.credential_id::<TestHasher>();
        accounts
            .call(
                call::CallMessage::InsertCredentialId(new_credential_id),
                &sender_context,
                &mut working_set,
            )
            .unwrap();

        let mut state = working_set.checkpoint().0;

        // Account corresponding to the old credential still exists.
        let query_response = accounts
            .accounts
            .get(&sender_credential_id, &mut state)?
            .map(|Account { addr: a }| a);

        assert_eq!(query_response, Some(sender_addr));

        // New account with the new public key and an old address is created.
        let query_response = accounts
            .accounts
            .get(&new_credential_id, &mut state)?
            .map(|Account { addr: a }| a);

        assert_eq!(query_response, Some(sender_addr));
    }

    Ok(())
}

#[test]
fn test_update_account_fails() {
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::<S>::new(new_orphan_storage(tmpdir.path()).unwrap());
    let accounts = &mut Accounts::<S>::default();

    let sender_1 = TestPrivateKey::generate().pub_key();
    let sender_1_addr = sender_1.to_address::<<S as Spec>::Address>();
    let sender_1_credential_id: CredentialId = sender_1.credential_id::<TestHasher>();

    let sequencer = TestPrivateKey::generate().pub_key();
    let sender_context_1 = Context::<S>::new(
        sender_1.to_address(),
        Default::default(),
        sequencer.to_address(),
        1,
    );

    let sender_2 = TestPrivateKey::generate().pub_key();
    let sender_2_addr = sender_2.to_address::<<S as Spec>::Address>();
    let sender_2_credential_id: CredentialId = sender_2.credential_id::<TestHasher>();

    let config = AccountConfig {
        accounts: vec![
            AccountData {
                credential_id: sender_1_credential_id,
                address: sender_1_addr,
            },
            AccountData {
                credential_id: sender_2_credential_id,
                address: sender_2_addr,
            },
        ],
    };

    let mut genesis = state.to_genesis_state_accessor::<Accounts<S>>(&config);
    accounts.init_module(&config, &mut genesis).unwrap();
    let state = genesis.checkpoint();

    let mut state = state.to_working_set_unmetered();
    // The new credential already exists and the call fails.
    assert!(accounts
        .call(
            call::CallMessage::InsertCredentialId(sender_2_credential_id),
            &sender_context_1,
            &mut state
        )
        .is_err());
}

#[test]
fn test_get_account_after_pub_key_update() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::<S>::new(new_orphan_storage(tmpdir.path()).unwrap());
    let accounts = &mut Accounts::<S>::default();

    let sender = TestPrivateKey::generate().pub_key();
    let sender_addr = sender.to_address::<<S as Spec>::Address>();
    let sender_credential_id: CredentialId = sender.credential_id::<TestHasher>();

    let sequencer = TestPrivateKey::generate().pub_key();
    let sequencer_addr = sequencer.to_address::<<S as Spec>::Address>();
    let sender_context = Context::<S>::new(sender_addr, Default::default(), sequencer_addr, 1);

    let config = AccountConfig {
        accounts: vec![AccountData {
            credential_id: sender_credential_id,
            address: sender_addr,
        }],
    };

    let mut genesis = state.to_genesis_state_accessor::<Accounts<S>>(&config);

    accounts.init_module(&config, &mut genesis).unwrap();

    let mut state = genesis.checkpoint().to_working_set_unmetered();

    let priv_key = TestPrivateKey::generate();
    let new_pub_key = priv_key.pub_key();
    let new_credential_id: CredentialId = new_pub_key.credential_id::<TestHasher>();
    accounts
        .call(
            call::CallMessage::InsertCredentialId(new_credential_id),
            &sender_context,
            &mut state,
        )
        .unwrap();

    let mut state = state.checkpoint().0;

    let acc = accounts
        .accounts
        .get(&new_credential_id, &mut state)?
        .unwrap();

    assert_eq!(acc.addr, sender_addr);

    Ok(())
}

#[test]
fn test_resolve_sender_address() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let state: WorkingSet<S> =
        WorkingSet::new_deprecated(new_orphan_storage(tmpdir.path()).unwrap());
    let (mut checkpoint, _, _) = state.checkpoint();
    let accounts = &mut Accounts::<S>::default();

    let priv_key = TestPrivateKey::generate();
    let sender = priv_key.pub_key();
    let sender_addr = sender.to_address::<<S as Spec>::Address>();
    let sender_credential_id: CredentialId = sender.credential_id::<TestHasher>();

    let maybe_address =
        accounts.resolve_sender_address(&None, &sender_credential_id, &mut checkpoint);
    assert_eq!(
        maybe_address.unwrap_err().to_string(),
        format!("No default address found for {}", sender_credential_id)
    );

    accounts
        .resolve_sender_address(&Some(sender_addr), &sender_credential_id, &mut checkpoint)
        .unwrap();

    let acc = accounts
        .accounts
        .get(&sender_credential_id, &mut checkpoint)?
        .unwrap();

    assert_eq!(acc.addr, sender_addr);

    Ok(())
}

#[test]
fn test_response_serialization() {
    let addr: Vec<u8> = (1..=32).collect();
    let mut addr_array = [0u8; 32];
    addr_array.copy_from_slice(&addr);
    let response = Response::AccountExists::<<S as Spec>::Address> {
        addr: Address::from(addr_array),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert_eq!(
        json,
        r#"{"AccountExists":{"addr":"sov1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5z5tpwxqergd3c8g7rusqqsn6hm"}}"#
    );
}

#[test]
fn test_response_deserialization() {
    let json = r#"{"AccountExists":{"addr":"sov1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5z5tpwxqergd3c8g7rusqqsn6hm"}}"#;
    let response: Response<<S as Spec>::Address> = serde_json::from_str(json).unwrap();

    let expected_addr: Vec<u8> = (1..=32).collect();
    let mut addr_array = [0u8; 32];
    addr_array.copy_from_slice(&expected_addr);
    let expected_response = Response::AccountExists::<<S as Spec>::Address> {
        addr: Address::from(addr_array),
    };

    assert_eq!(response, expected_response);
}

#[test]
fn test_response_deserialization_on_wrong_hrp() {
    let json = r#"{"AccountExists":{"addr":"hax1qypqx68ju0l"}}"#;
    let response: Result<Response<<S as Spec>::Address>, serde_json::Error> =
        serde_json::from_str(json);
    match response {
        Ok(response) => panic!("Expected error, got {:?}", response),
        Err(err) => {
            assert_eq!(err.to_string(), "Wrong HRP: hax at line 1 column 43");
        }
    }
}
