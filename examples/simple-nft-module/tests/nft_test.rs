use std::convert::Infallible;

use simple_nft_module::{
    CallMessage, Event, NonFungibleToken, NonFungibleTokenConfig, OwnerResponse,
};
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::utils::generate_address as gen_addr_generic;
use sov_modules_api::{Context, Module, Spec, StateCheckpoint};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::ProverStorage;
use sov_test_utils::TestStorageSpec;

pub type S = sov_test_utils::TestSpec;
pub type Storage = ProverStorage<TestStorageSpec>;
fn generate_address(name: &str) -> <S as Spec>::Address {
    gen_addr_generic::<S>(name)
}

#[test]
fn genesis_and_mint() -> Result<(), Infallible> {
    // Preparation
    let admin = generate_address("admin");
    let owner1 = generate_address("owner2");
    let owner2 = generate_address("owner2");
    let sequencer = generate_address("sequencer");
    let config: NonFungibleTokenConfig<S> = NonFungibleTokenConfig {
        admin,
        owners: vec![(0, owner1)],
    };

    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::<S>::new(new_orphan_storage(tmpdir.path()).unwrap());
    let nft = NonFungibleToken::default();

    let mut genesis_state = state.to_genesis_state_accessor::<NonFungibleToken<S>>(&config);

    // Genesis
    let genesis_result = nft.genesis(&config, &mut genesis_state);
    assert!(genesis_result.is_ok());

    let query1: OwnerResponse<S> = nft.get_owner(0, &mut genesis_state)?;
    assert_eq!(query1.owner, Some(owner1));

    let query2: OwnerResponse<S> = nft.get_owner(1, &mut genesis_state)?;
    assert!(query2.owner.is_none());

    let checkpoint = genesis_state.checkpoint();
    let mut state = checkpoint.to_working_set_unmetered();

    // Mint, anybody can mint
    let mint_message = CallMessage::Mint { id: 1 };
    let owner2_context = Context::<S>::new(owner2, Default::default(), sequencer, 1);
    nft.call(mint_message.clone(), &owner2_context, &mut state)
        .expect("Minting failed");

    let typed_event = state.take_event(0).unwrap();

    assert_eq!(
        typed_event.downcast::<Event>().unwrap(),
        Event::Mint { id: 1 }
    );

    let (mut checkpoint, _, _) = state.checkpoint();
    let query3: OwnerResponse<S> = nft.get_owner(1, &mut checkpoint)?;
    assert_eq!(query3.owner, Some(owner2));

    let mut state = checkpoint.to_working_set_unmetered();
    // Try to mint again same token, should fail
    let mint_attempt = nft.call(mint_message, &owner2_context, &mut state);

    assert!(mint_attempt.is_err());
    let error_message = mint_attempt.err().unwrap().to_string();
    assert_eq!("Token with id 1 already exists", error_message);

    Ok(())
}

#[test]
fn transfer() -> Result<(), Infallible> {
    // Preparation
    let admin = generate_address("admin");
    let sequencer = generate_address("sequencer");
    let admin_context = Context::<S>::new(admin, Default::default(), sequencer, 1);
    let owner1 = generate_address("owner2");
    let owner1_context = Context::<S>::new(owner1, Default::default(), sequencer, 1);
    let owner2 = generate_address("owner2");
    let config: NonFungibleTokenConfig<S> = NonFungibleTokenConfig {
        admin,
        owners: vec![(0, admin), (1, owner1), (2, owner2)],
    };
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let nft = NonFungibleToken::default();
    let mut genesis_state = state.to_genesis_state_accessor::<NonFungibleToken<S>>(&config);
    nft.genesis(&config, &mut genesis_state).unwrap();

    let checkpoint = genesis_state.checkpoint();
    let mut state = checkpoint.to_working_set_unmetered();

    let transfer_message = CallMessage::Transfer { id: 1, to: owner2 };

    // admin cannot transfer token of the owner1
    let transfer_attempt = nft.call(transfer_message.clone(), &admin_context, &mut state);

    assert!(transfer_attempt.is_err());
    let error_message = transfer_attempt.err().unwrap().to_string();
    assert_eq!("Only token owner can transfer token", error_message);

    let (mut checkpoint, _, _) = state.checkpoint();

    let query_token_owner =
        |token_id: u64, working_set: &mut StateCheckpoint<S>| -> Option<<S as Spec>::Address> {
            let query: OwnerResponse<S> = nft.get_owner(token_id, working_set).unwrap_infallible();
            query.owner
        };

    // Normal transfer
    let token1_owner = query_token_owner(1, &mut checkpoint);
    assert_eq!(Some(owner1), token1_owner);

    let mut state = checkpoint.to_working_set_unmetered();

    nft.call(transfer_message, &owner1_context, &mut state)
        .expect("Transfer failed");

    let typed_event = state.take_event(0).unwrap();

    assert_eq!(
        typed_event.downcast::<Event>().unwrap(),
        Event::Transfer { id: 1 }
    );

    let (mut checkpoint, _, _) = state.checkpoint();

    let token1_owner = query_token_owner(1, &mut checkpoint);
    assert_eq!(Some(owner2), token1_owner);

    // Attempt to transfer non existing token
    let transfer_message = CallMessage::Transfer { id: 3, to: admin };

    let mut state = checkpoint.to_working_set_unmetered();
    let transfer_attempt = nft.call(transfer_message, &owner1_context, &mut state);

    assert!(transfer_attempt.is_err());
    let error_message = transfer_attempt.err().unwrap().to_string();
    assert_eq!("Token with id 3 does not exist", error_message);

    Ok(())
}

#[test]
fn burn() -> Result<(), Infallible> {
    // Preparation
    let admin = generate_address("admin");
    let sequencer = generate_address("sequencer");
    let admin_context = Context::<S>::new(admin, Default::default(), sequencer, 1);
    let owner1 = generate_address("owner2");
    let owner1_context = Context::<S>::new(owner1, Default::default(), sequencer, 1);
    let config: NonFungibleTokenConfig<S> = NonFungibleTokenConfig {
        admin,
        owners: vec![(0, owner1)],
    };

    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::<S>::new(new_orphan_storage(tmpdir.path()).unwrap());
    let nft = NonFungibleToken::default();
    let mut genesis_state = state.to_genesis_state_accessor::<NonFungibleToken<S>>(&config);
    nft.genesis(&config, &mut genesis_state).unwrap();

    let checkpoint = genesis_state.checkpoint();

    let burn_message = CallMessage::Burn { id: 0 };

    let mut state = checkpoint.to_working_set_unmetered();
    // Only owner can burn token
    let burn_attempt = nft.call(burn_message.clone(), &admin_context, &mut state);

    assert!(burn_attempt.is_err());
    let error_message = burn_attempt.err().unwrap().to_string();
    assert_eq!("Only token owner can burn token", error_message);

    // Normal burn
    nft.call(burn_message.clone(), &owner1_context, &mut state)
        .expect("Burn failed");
    assert!(!state.events().is_empty());

    let typed_event = state.take_event(0).unwrap();

    assert_eq!(
        typed_event.downcast::<Event>().unwrap(),
        Event::Burn { id: 0 }
    );

    let (mut checkpoint, _, _) = state.checkpoint();

    let query: OwnerResponse<S> = nft.get_owner(0, &mut checkpoint)?;

    assert!(query.owner.is_none());

    let mut state = checkpoint.to_working_set_unmetered();

    let burn_attempt = nft.call(burn_message, &owner1_context, &mut state);
    assert!(burn_attempt.is_err());
    let error_message = burn_attempt.err().unwrap().to_string();
    assert_eq!("Token with id 0 does not exist", error_message);

    Ok(())
}
