use std::convert::Infallible;

use sov_bank::{
    get_token_id, Bank, BankConfig, CallMessage, Coins, GasTokenConfig, IntoPayable, Payable,
    TokenId, GAS_TOKEN_ID,
};
use sov_modules_api::utils::generate_address;
use sov_modules_api::{
    Context, Error, Module, ModuleId, Spec, StateAccessor, StateCheckpoint, WorkingSet,
};
use sov_prover_storage_manager::new_orphan_storage;

type S = sov_test_utils::TestSpec;

#[test]
fn mint_token() -> Result<(), Infallible> {
    let bank = Bank::<S>::default();
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());

    let minter = generate_address::<S>("minter");
    let sequencer_address = generate_address::<S>("sequencer");
    let minter_context = Context::<S>::new(minter, Default::default(), sequencer_address, 1);

    let token_name = "Token1".to_owned();
    let initial_balance = 100;
    let token_id = GAS_TOKEN_ID;

    let bank_config = BankConfig::<S> {
        gas_token_config: GasTokenConfig {
            token_name: token_name.clone(),
            address_and_balances: vec![(minter, initial_balance)],
            authorized_minters: vec![minter],
        },

        tokens: vec![],
    };

    let mut genesis = state.to_genesis_state_accessor::<Bank<S>>(&bank_config);
    bank.genesis(&bank_config, &mut genesis).unwrap();

    let query_total_supply =
        |token_id: TokenId, state: &mut WorkingSet<S>| -> Result<Option<u64>, Infallible> {
            bank.get_total_supply_of(&token_id, &mut state.to_unmetered())
        };

    let query_user_balance = |user_address: <S as Spec>::Address,
                              state: &mut WorkingSet<S>|
     -> Result<Option<u64>, Infallible> {
        bank.get_balance_of(&user_address, token_id, &mut state.to_unmetered())
    };

    let mut state = genesis.checkpoint().to_working_set_unmetered();

    let previous_total_supply = query_total_supply(token_id, &mut state)?;
    assert_eq!(Some(initial_balance), previous_total_supply);

    // -----
    // Mint Additional
    let mint_amount = 10;
    let new_holder = generate_address::<S>("new_holder");
    let mint_message = CallMessage::Mint {
        coins: Coins {
            amount: mint_amount,
            token_id,
        },
        mint_to_address: new_holder,
    };

    let _minted = bank
        .call(mint_message.clone(), &minter_context, &mut state)
        .expect("Failed to mint token");
    assert_eq!(state.events().len(), 1);

    let total_supply = query_total_supply(token_id, &mut state)?;
    assert_eq!(Some(initial_balance + mint_amount), total_supply);

    // check user balance after minting
    let balance = query_user_balance(new_holder, &mut state)?;
    assert_eq!(Some(10), balance);

    // check original token creation balance
    let bal = query_user_balance(minter, &mut state)?;
    assert_eq!(Some(100), bal);

    // Mint with an un-authorized user
    let unauthorized_address = generate_address::<S>("unauthorized_address");
    let sequencer_address = generate_address::<S>("sequencer");
    let unauthorized_context = Context::<S>::new(
        unauthorized_address,
        Default::default(),
        sequencer_address,
        1,
    );
    let unauthorized_mint = bank.call(mint_message, &unauthorized_context, &mut state);
    assert_eq!(state.events().len(), 1);

    assert!(unauthorized_mint.is_err());

    let Error::ModuleError(err) = unauthorized_mint.err().unwrap();
    let mut chain = err.chain();

    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());

    assert_eq!(
        format!(
            "Failed mint coins(token_id={} amount={}) to {} by authorizer {}",
            token_id, mint_amount, new_holder, unauthorized_address
        ),
        message_1
    );
    assert_eq!(
        format!(
            "Sender {} is not an authorized minter of token {}",
            unauthorized_address, token_name,
        ),
        message_2
    );

    // Authorized minter test
    let salt = 0;
    let token_name = "Token_New".to_owned();
    let initial_balance = 100;
    let token_id = get_token_id::<S>(&token_name, &minter, salt);
    let authorized_minter_address_1 = generate_address::<S>("authorized_minter_1");
    let authorized_minter_address_2 = generate_address::<S>("authorized_minter_2");
    let sequencer_address = generate_address::<S>("sequencer");
    // ---
    // Deploying token
    let mint_message = CallMessage::CreateToken {
        salt,
        token_name: token_name.clone(),
        initial_balance,
        mint_to_address: minter,
        authorized_minters: vec![authorized_minter_address_1, authorized_minter_address_2],
    };
    let _minted = bank
        .call(mint_message, &minter_context, &mut state)
        .expect("Failed to mint token");
    assert_eq!(state.events().len(), 2);

    // Try to mint new token with original token creator, in this case minter_context
    let mint_amount = 10;
    let new_holder = generate_address::<S>("new_holder_2");
    let mint_message = CallMessage::Mint {
        coins: Coins {
            amount: mint_amount,
            token_id,
        },
        mint_to_address: new_holder,
    };

    let minted = bank.call(mint_message, &minter_context, &mut state);
    assert!(minted.is_err());
    let Error::ModuleError(err) = minted.err().unwrap();
    let mut chain = err.chain();

    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());
    assert_eq!(
        format!(
            "Failed mint coins(token_id={} amount={}) to {} by authorizer {}",
            token_id, mint_amount, new_holder, minter,
        ),
        message_1
    );
    assert_eq!(
        format!(
            "Sender {} is not an authorized minter of token {}",
            minter, token_name
        ),
        message_2
    );
    // Try to mint new token with authorized sender 2
    let authorized_minter_2_context = Context::<S>::new(
        authorized_minter_address_2,
        Default::default(),
        sequencer_address,
        1,
    );
    let mint_message = CallMessage::Mint {
        coins: Coins {
            amount: mint_amount,
            token_id,
        },
        mint_to_address: new_holder,
    };

    let _minted = bank
        .call(mint_message, &authorized_minter_2_context, &mut state)
        .expect("Failed to mint token");
    let supply = query_total_supply(token_id, &mut state)?;
    assert_eq!(state.events().len(), 3);
    assert_eq!(Some(110), supply);

    // Try to mint new token with authorized sender 1
    let authorized_minter_1_context = Context::<S>::new(
        authorized_minter_address_1,
        Default::default(),
        sequencer_address,
        1,
    );
    let mint_message = CallMessage::Mint {
        coins: Coins {
            amount: mint_amount,
            token_id,
        },
        mint_to_address: new_holder,
    };

    let _minted = bank
        .call(mint_message, &authorized_minter_1_context, &mut state)
        .expect("Failed to mint token");
    let supply = query_total_supply(token_id, &mut state)?;
    assert_eq!(state.events().len(), 4);
    assert_eq!(Some(120), supply);

    // Overflow test - account balance
    let overflow_mint_message = CallMessage::Mint {
        coins: Coins {
            amount: u64::MAX,
            token_id,
        },
        mint_to_address: new_holder,
    };

    let minted = bank.call(
        overflow_mint_message,
        &authorized_minter_1_context,
        &mut state,
    );
    assert!(minted.is_err());
    let Error::ModuleError(err) = minted.err().unwrap();
    let mut chain = err.chain();
    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());
    assert_eq!(
        format!(
            "Failed mint coins(token_id={} amount={}) to {} by authorizer {}",
            token_id,
            u64::MAX,
            new_holder,
            authorized_minter_address_1,
        ),
        message_1
    );
    assert_eq!(
        "Account balance overflow in the mint method of bank module",
        message_2,
    );
    // assert that the supply is unchanged after the overflow mint
    let supply = query_total_supply(token_id, &mut state)?;
    assert_eq!(Some(120), supply);

    // Overflow test 2 - total supply
    let new_holder = generate_address::<S>("new_holder_3");
    let overflow_mint_message = CallMessage::Mint {
        coins: Coins {
            amount: u64::MAX - 1,
            token_id,
        },
        mint_to_address: new_holder,
    };

    let minted = bank.call(
        overflow_mint_message,
        &authorized_minter_1_context,
        &mut state,
    );
    assert!(minted.is_err());
    let Error::ModuleError(err) = minted.err().unwrap();
    let mut chain = err.chain();
    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());
    assert_eq!(
        format!(
            "Failed mint coins(token_id={} amount={}) to {} by authorizer {}",
            token_id,
            u64::MAX - 1,
            new_holder,
            authorized_minter_address_1,
        ),
        message_1
    );
    assert_eq!(
        "Total Supply overflow in the mint method of bank module",
        message_2,
    );

    // assert that the supply is unchanged after the overflow mint
    let supply = query_total_supply(token_id, &mut state)?;
    assert_eq!(Some(120), supply);

    Ok(())
}

#[test]
fn mint_token_from_module_and_address() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut state = WorkingSet::<S>::new_deprecated(new_orphan_storage(tmpdir.path()).unwrap());

    let bank = Bank::<S>::default();

    let sender_context = {
        let sender_address = generate_address::<S>("sender");
        let sequencer_address = generate_address::<S>("sequencer");
        Context::<S>::new(sender_address, Default::default(), sequencer_address, 1)
    };

    let module_id = ModuleId::from([0; 32]);
    let mod_minter = module_id.to_payable();

    let addr = &generate_address::<S>("addr_minter");
    let addr_minter = addr.as_token_holder();

    let initial_balance = 500;

    // Create token.
    let token_id = bank
        .create_token(
            "Token1".to_owned(),
            1,
            initial_balance,
            mod_minter,
            vec![mod_minter, addr_minter],
            sender_context.sender(),
            &mut state,
        )
        .unwrap();

    let mut state = state.checkpoint().0;

    // Test token creation.
    {
        let minter_balance = bank.get_balance_of(mod_minter, token_id, &mut state)?;
        assert_eq!(Some(initial_balance), minter_balance);

        let total_supply = bank.get_total_supply_of(&token_id, &mut state)?.unwrap();

        assert_eq!(initial_balance, total_supply);
    }

    // Mint coins.
    let coins = Coins {
        amount: 1000,
        token_id,
    };

    // Test token minting from module.
    {
        let mut working_set = state.to_working_set_unmetered();
        bank.mint(&coins, mod_minter, mod_minter, &mut working_set)
            .unwrap();
        state = working_set.checkpoint().0;

        let minter_balance = bank.get_balance_of(mod_minter, token_id, &mut state)?;
        assert_eq!(Some(initial_balance + coins.amount), minter_balance);

        let total_supply = bank.get_total_supply_of(&token_id, &mut state)?.unwrap();

        assert_eq!(initial_balance + coins.amount, total_supply);
    }

    // Test token minting from address.
    {
        let mut working_set = state.to_working_set_unmetered();
        bank.mint(&coins, addr_minter, addr_minter, &mut working_set)
            .unwrap();
        state = working_set.checkpoint().0;

        let minter_balance = bank.get_balance_of(addr_minter, token_id, &mut state)?;
        assert_eq!(Some(coins.amount), minter_balance);

        let total_supply = bank.get_total_supply_of(&token_id, &mut state)?.unwrap();

        assert_eq!(initial_balance + 2 * coins.amount, total_supply);
    }

    Ok(())
}

#[test]
fn create_token_from_module() -> Result<(), Infallible> {
    let tmpdir = tempfile::tempdir().unwrap();
    let mut state = WorkingSet::<S>::new_deprecated(new_orphan_storage(tmpdir.path()).unwrap());

    let bank = Bank::<S>::default();

    let module_id = ModuleId::from([0; 32]);
    let mod_originator = module_id.to_payable();

    let addr = &generate_address::<S>("addr_minter");
    let addr_minter = addr.as_token_holder();

    let initial_balance = 500;

    // Create token.
    let token_id = bank
        .create_token(
            "Token1".to_owned(),
            1,
            initial_balance,
            addr_minter,
            vec![addr_minter],
            mod_originator,
            &mut state,
        )
        .unwrap();

    let mut state = state.checkpoint().0;

    let minter_balance = bank.get_balance_of(addr_minter, token_id, &mut state)?;
    assert_eq!(Some(initial_balance), minter_balance);

    let total_supply = bank.get_total_supply_of(&token_id, &mut state)?.unwrap();

    assert_eq!(initial_balance, total_supply);

    Ok(())
}
