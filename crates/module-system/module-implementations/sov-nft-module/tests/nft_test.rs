use std::convert::Infallible;

use sov_modules_api::{Context, Module, PrivateKey, Spec, WorkingSet};
use sov_nft_module::utils::get_collection_id;
use sov_nft_module::{CallMessage, NonFungibleToken, OwnerAddress, UserAddress};
use sov_prover_storage_manager::new_orphan_storage;
use sov_test_utils::{TestPrivateKey, TestSpec, TestStorageSpec as StorageSpec};

#[test]
fn mints_and_transfers() -> Result<(), Infallible> {
    let creator_pk = TestPrivateKey::generate();
    let private_key_1 = TestPrivateKey::generate();
    let private_key_2 = TestPrivateKey::generate();
    let sequencer_pk = TestPrivateKey::generate();

    let creator_address: <TestSpec as Spec>::Address = creator_pk.to_address();
    let sequencer_address = sequencer_pk.to_address();
    let collection_name = "Test Collection";
    let collection_uri = "http://foo.bar/test_collection";
    let collection_id = get_collection_id::<TestSpec>(collection_name, creator_address.as_ref());

    let tmpdir = tempfile::tempdir().unwrap();
    let storage = new_orphan_storage::<StorageSpec>(tmpdir.path()).unwrap();
    let mut working_set = WorkingSet::<TestSpec>::new_deprecated(storage);
    let nft = NonFungibleToken::default();

    let create_collection_message = CallMessage::CreateCollection {
        name: collection_name.to_string(),
        collection_uri: collection_uri.to_string(),
    };

    let creator_context =
        Context::<TestSpec>::new(creator_address, Default::default(), sequencer_address, 1);

    // Create Collection
    nft.call(
        create_collection_message,
        &creator_context,
        &mut working_set,
    )
    .expect("Creating Collection failed");

    let mut state = working_set.checkpoint().0;

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();

    assert_eq!(actual_collection.name, collection_name);
    assert_eq!(actual_collection.supply, 0);
    assert_eq!(
        actual_collection.creator.get_address().clone(),
        creator_address
    );
    assert!(!actual_collection.frozen);

    let token_id = 42;
    let token_uri = "http://foo.bar/test_collection/42";
    let owner = UserAddress::new(&private_key_1.to_address());

    let mint_nft_message = CallMessage::MintNft {
        collection_name: collection_name.to_string(),
        token_uri: token_uri.to_string(),
        token_id,
        owner: owner.clone(),
        frozen: false,
    };

    let mut working_set = state.to_working_set_unmetered();
    // Mint NFT to created collection
    nft.call(mint_nft_message, &creator_context, &mut working_set)
        .expect("Minting NFT failed");
    let mut state = working_set.checkpoint().0;

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();
    assert_eq!(actual_collection.supply, 1);
    let actual_nft = nft.nft(collection_id, token_id, &mut state)?.unwrap();
    assert_eq!(actual_nft.token_id, token_id);
    assert_eq!(actual_nft.collection_id, collection_id);
    assert_eq!(actual_nft.token_uri, token_uri.to_string());
    assert_eq!(actual_nft.owner, OwnerAddress::new(owner.get_address()));

    // Mint NFT to non-existent collection
    let ne_collection_name = "NON_EXISTENT_COLLECTION";
    let mint_nft_message = CallMessage::MintNft {
        collection_name: ne_collection_name.to_string(),
        token_uri: token_uri.to_string(),
        token_id,
        owner,
        frozen: false,
    };
    let mut working_set = state.to_working_set_unmetered();
    let mint_response = nft.call(mint_nft_message, &creator_context, &mut working_set);
    let state = working_set.checkpoint().0;

    if let Err(err) = mint_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "Collection with name: {} does not exist for creator {}",
                    ne_collection_name, creator_address
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    // Update a collection
    let new_collection_uri = "http://new/uri";
    let create_collection_message = CallMessage::UpdateCollection {
        name: collection_name.to_string(),
        collection_uri: new_collection_uri.to_string(),
    };

    let creator_context =
        Context::<TestSpec>::new(creator_address, Default::default(), sequencer_address, 1);

    let mut working_set = state.to_working_set_unmetered();
    nft.call(
        create_collection_message,
        &creator_context,
        &mut working_set,
    )
    .expect("Updating Collection failed");
    let mut state = working_set.checkpoint().0;

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();
    assert_eq!(
        actual_collection.collection_uri,
        new_collection_uri.to_string()
    );
    assert!(!actual_collection.frozen);

    // Freeze a non existent collection
    let freeze_collection_message = CallMessage::FreezeCollection {
        collection_name: ne_collection_name.to_string(),
    };

    let creator_context =
        Context::<TestSpec>::new(creator_address, Default::default(), sequencer_address, 1);

    let mut working_set = state.to_working_set_unmetered();
    let freeze_response = nft.call(
        freeze_collection_message,
        &creator_context,
        &mut working_set,
    );
    let state = working_set.checkpoint().0;

    if let Err(err) = freeze_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "Collection with name: {} does not exist for creator {}",
                    ne_collection_name, creator_address
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    // Freeze collection
    let freeze_collection_message = CallMessage::FreezeCollection {
        collection_name: collection_name.to_string(),
    };

    let creator_context =
        Context::<TestSpec>::new(creator_address, Default::default(), sequencer_address, 1);

    let mut working_set = state.to_working_set_unmetered();
    nft.call(
        freeze_collection_message,
        &creator_context,
        &mut working_set,
    )
    .expect("Freeze collection call should succeed");
    let mut state = working_set.checkpoint().0;

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();

    assert!(actual_collection.frozen);

    // Update collection uri for frozen collection
    // Update a collection
    let un_updated_collection_uri = "http://new/uri2";
    let create_collection_message = CallMessage::UpdateCollection {
        name: collection_name.to_string(),
        collection_uri: un_updated_collection_uri.to_string(),
    };

    let creator_context =
        Context::<TestSpec>::new(creator_address, Default::default(), sequencer_address, 1);

    let mut working_set = state.to_working_set_unmetered();
    let update_response = nft.call(
        create_collection_message,
        &creator_context,
        &mut working_set,
    );
    let mut state = working_set.checkpoint().0;

    if let Err(err) = update_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "Collection with name: {} , creator: {} is frozen",
                    collection_name, creator_address
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();
    assert!(actual_collection.frozen);
    // assert that the collection uri hasn't been changed
    assert_eq!(actual_collection.collection_uri, new_collection_uri);
    // assert that supply hasn't been modified
    assert_eq!(actual_collection.supply, 1);

    // mint nft to frozen collection
    let new_token_id = 23;
    let new_token_uri = "http://foo.bar/test_collection/23";
    let owner: OwnerAddress<TestSpec> = OwnerAddress::new(&private_key_1.to_address());

    let mint_nft_message = CallMessage::MintNft {
        collection_name: collection_name.to_string(),
        token_uri: new_token_uri.to_string(),
        token_id: new_token_id,
        owner: UserAddress::new(owner.get_address()),
        frozen: false,
    };

    let mut working_set = state.to_working_set_unmetered();
    let mint_response = nft.call(mint_nft_message, &creator_context, &mut working_set);
    let mut state = working_set.checkpoint().0;

    if let Err(err) = mint_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "Collection with name: {} , creator: {} is frozen",
                    collection_name, creator_address
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();
    // ensure supply hasn't changed
    assert_eq!(actual_collection.supply, 1);

    // transfer NFT with non-owner
    let target_address = private_key_2.to_address();
    let transfer_nft_message = CallMessage::TransferNft {
        collection_id,
        token_id,
        to: UserAddress::new(&target_address),
    };

    // calling with the old context first (which is the creator)
    // but the creator is not the owner, so it should fail
    let mut working_set = state.to_working_set_unmetered();
    let transfer_response = nft.call(transfer_nft_message, &creator_context, &mut working_set);

    if let Err(err) = transfer_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "user: {} does not own nft: {} from collection id: {} , owner is: {}",
                    creator_address, token_id, collection_id, owner
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    // transfer NFT with non-existent token id
    let target_address = private_key_2.to_address();
    let owner_context = Context::<TestSpec>::new(
        *owner.get_address(),
        Default::default(),
        sequencer_address,
        1,
    );
    let transfer_nft_message = CallMessage::TransferNft {
        collection_id,
        token_id: 1000,
        to: UserAddress::new(&target_address),
    };

    let transfer_response = nft.call(transfer_nft_message, &owner_context, &mut working_set);
    if let Err(err) = transfer_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "Nft with token_id: {} in collection_id: {} does not exist",
                    1000, collection_id
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    // transfer NFT by owner
    let target_address = private_key_2.to_address();
    let owner_context = Context::<TestSpec>::new(
        *owner.get_address(),
        Default::default(),
        sequencer_address,
        1,
    );
    let transfer_nft_message = CallMessage::TransferNft {
        collection_id,
        token_id,
        to: UserAddress::new(&target_address),
    };
    let transfer_response = nft.call(transfer_nft_message, &owner_context, &mut working_set);
    let mut state = working_set.checkpoint().0;
    assert!(transfer_response.is_ok());

    let actual_nft = nft.nft(collection_id, token_id, &mut state)?.unwrap();
    // ensure token_id didn't change
    assert_eq!(actual_nft.token_id, token_id);
    assert_eq!(actual_nft.collection_id, collection_id);
    assert_eq!(actual_nft.token_uri, token_uri.to_string());
    // ensure that the owner is the new owner
    assert_eq!(actual_nft.owner, OwnerAddress::new(&target_address));

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();
    // ensure supply hasn't changed with a transfer
    assert_eq!(actual_collection.supply, 1);

    // Update NFT token_uri
    let token_id = 42;
    let new_token_uri = "http://foo.bar/test_collection/new_url/42";
    let update_nft_message = CallMessage::UpdateNft {
        collection_name: collection_name.to_string(),
        token_id,
        token_uri: Some(new_token_uri.to_string()),
        frozen: None,
    };

    let mut working_set = state.to_working_set_unmetered();
    let update_response = nft.call(update_nft_message, &creator_context, &mut working_set);
    let mut state = working_set.checkpoint().0;

    assert!(update_response.is_ok());

    let actual_nft = nft.nft(collection_id, token_id, &mut state)?.unwrap();

    // ensure token_id didn't change
    assert_eq!(actual_nft.token_id, token_id);
    assert_eq!(actual_nft.collection_id, collection_id);
    // token uri should be updated
    assert_eq!(actual_nft.token_uri, new_token_uri.to_string());
    // ensure owner is unchanged (new based on previous test)
    assert_eq!(actual_nft.owner, OwnerAddress::new(&target_address));
    // ensure still unfrozen
    assert!(!actual_nft.frozen);

    // Freeze NFT
    let token_id = 42;
    let update_nft_message = CallMessage::UpdateNft {
        collection_name: collection_name.to_string(),
        token_id,
        token_uri: None,
        frozen: Some(true),
    };
    let mut working_set = state.to_working_set_unmetered();
    let update_response = nft.call(update_nft_message, &creator_context, &mut working_set);
    let mut state = working_set.checkpoint().0;

    assert!(update_response.is_ok());

    let actual_nft = nft.nft(collection_id, token_id, &mut state)?.unwrap();
    // ensure token_id didn't change
    assert_eq!(actual_nft.token_id, token_id);
    assert_eq!(actual_nft.collection_id, collection_id);
    // token uri should be updated
    assert_eq!(actual_nft.token_uri, new_token_uri.to_string());
    // ensure owner is unchanged (new based on previous test)
    assert_eq!(actual_nft.owner, OwnerAddress::new(&target_address));
    // ensure frozen is true
    assert!(actual_nft.frozen);

    // Update NFT token uri for frozen NFT
    let token_id = 42;
    let new_token_uri_fail = "http://foo.bar/test_collection/new_url_fail/42";
    let update_nft_message = CallMessage::UpdateNft {
        collection_name: collection_name.to_string(),
        token_id,
        token_uri: Some(new_token_uri_fail.to_string()),
        frozen: None,
    };
    let mut working_set = state.to_working_set_unmetered();
    let update_response = nft.call(update_nft_message, &creator_context, &mut working_set);
    let mut state = working_set.checkpoint().0;

    if let Err(err) = update_response {
        match err {
            sov_modules_api::Error::ModuleError(anyhow_err) => {
                let err_message = anyhow_err.to_string();
                let expected_message = format!(
                    "NFT with token id {} in collection id {} is frozen",
                    token_id, collection_id
                );
                assert_eq!(err_message, expected_message);
            }
        }
    } else {
        panic!("Expected an error, got Ok");
    }

    // ensure that token uri is unchanged
    let actual_nft = nft.nft(collection_id, token_id, &mut state)?.unwrap();
    // token uri should be unchanged. it should be new_token_uri, not new_token_uri_fail
    assert_eq!(actual_nft.token_uri, new_token_uri.to_string());

    // Transfer on a frozen NFT should still work
    // transfer NFT by owner
    let target_address = private_key_1.to_address();
    let owner = private_key_2.to_address();
    let owner_context = Context::<TestSpec>::new(owner, Default::default(), sequencer_address, 1);
    let transfer_nft_message = CallMessage::TransferNft {
        collection_id,
        token_id,
        to: UserAddress::new(&target_address),
    };
    let mut working_set = state.to_working_set_unmetered();
    let transfer_response = nft.call(transfer_nft_message, &owner_context, &mut working_set);
    let mut state = working_set.checkpoint().0;

    assert!(transfer_response.is_ok());

    let actual_nft = nft.nft(collection_id, token_id, &mut state)?.unwrap();
    // ensure token_id didn't change
    assert_eq!(actual_nft.token_id, token_id);
    assert_eq!(actual_nft.collection_id, collection_id);
    // token uri should be new_token_uri
    assert_eq!(actual_nft.token_uri, new_token_uri.to_string());
    // ensure that the owner is the new owner
    assert_eq!(actual_nft.owner, OwnerAddress::new(&target_address));

    let actual_collection = nft.collection(collection_id, &mut state)?.unwrap();
    // ensure supply hasn't changed with a transfer
    assert_eq!(actual_collection.supply, 1);

    Ok(())
}
