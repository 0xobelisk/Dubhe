use std::convert::Infallible;

use sov_bank::{get_token_id, Bank, BankConfig, CallMessage, Coins, GasTokenConfig, GAS_TOKEN_ID};
use sov_modules_api::{Context, Error, Module, Spec, StateAccessor, StateCheckpoint, WorkingSet};
use sov_prover_storage_manager::new_orphan_storage;

use crate::helpers::generate_address;

type S = sov_test_utils::TestSpec;
use sov_test_utils::TEST_DEFAULT_USER_BALANCE;

use crate::helpers::create_bank_config_with_token;

#[test]
fn burn_deployed_tokens() -> Result<(), Infallible> {
    let bank = Bank::<S>::default();
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());

    let sender_address = generate_address("just_sender");
    let sequencer_address = generate_address("sequencer");
    let sender_context =
        Context::<S>::new(sender_address, Default::default(), sequencer_address, 1);
    let minter = generate_address("minter");
    let minter_context = Context::<S>::new(minter, Default::default(), sequencer_address, 1);

    let salt = 0;
    let token_name = "Token1".to_owned();
    let initial_balance = TEST_DEFAULT_USER_BALANCE;
    let token_id = GAS_TOKEN_ID;

    let bank_config = BankConfig::<S> {
        gas_token_config: GasTokenConfig {
            token_name: token_name.clone(),
            address_and_balances: vec![(minter, initial_balance)],
            authorized_minters: vec![minter],
        },
        tokens: vec![],
    };

    let mut genesis_state = state.to_genesis_state_accessor::<Bank<S>>(&bank_config);
    bank.genesis(&bank_config, &mut genesis_state).unwrap();
    let state = genesis_state.checkpoint();
    let mut state = state.to_working_set_unmetered();

    let query_total_supply = |state: &mut WorkingSet<S>| -> Result<Option<u64>, Infallible> {
        bank.get_total_supply_of(&token_id, &mut state.to_unmetered())
    };

    let query_user_balance = |user_address: <S as Spec>::Address,
                              state: &mut WorkingSet<S>|
     -> Result<Option<u64>, Infallible> {
        bank.get_balance_of(&user_address, token_id, &mut state.to_unmetered())
    };

    let previous_total_supply = query_total_supply(&mut state)?;
    assert_eq!(Some(initial_balance), previous_total_supply);

    // -----
    // Burn
    let burn_amount = 10;
    let burn_message = CallMessage::Burn {
        coins: Coins {
            amount: burn_amount,
            token_id,
        },
    };

    bank.call(burn_message.clone(), &minter_context, &mut state)
        .expect("Failed to burn token");
    assert_eq!(state.events().len(), 1);

    let current_total_supply = query_total_supply(&mut state)?;
    assert_eq!(Some(initial_balance - burn_amount), current_total_supply);
    let minter_balance = query_user_balance(minter, &mut state)?;
    assert_eq!(Some(initial_balance - burn_amount), minter_balance);

    let previous_total_supply = current_total_supply;
    // ---
    // Burn by another user, who doesn't have tokens at all
    let failed_to_burn = bank.call(burn_message, &sender_context, &mut state);
    assert!(failed_to_burn.is_err());
    let Error::ModuleError(err) = failed_to_burn.err().unwrap();
    let mut chain = err.chain();
    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());
    assert_eq!(
        format!(
            "Failed to burn coins(token_id={} amount={}) from owner {}",
            token_id, burn_amount, sender_address
        ),
        message_1
    );
    let expected_error_part = format!(
        "Value not found for prefix: \"sov_bank/Bank/tokens/{}\" and storage key:",
        token_id
    );
    assert!(message_2.starts_with(&expected_error_part));

    let current_total_supply = query_total_supply(&mut state)?;
    assert_eq!(previous_total_supply, current_total_supply);
    let sender_balance = query_user_balance(sender_address, &mut state)?;
    assert_eq!(None, sender_balance);

    // ---
    // Allow burning zero tokens
    let burn_zero_message = CallMessage::Burn {
        coins: Coins {
            amount: 0,
            token_id,
        },
    };

    bank.call(burn_zero_message, &minter_context, &mut state)
        .expect("Failed to burn token");
    assert_eq!(state.events().len(), 2);
    let minter_balance_after = query_user_balance(minter, &mut state)?;
    assert_eq!(minter_balance, minter_balance_after);

    // ---
    // Burn more than available
    let burn_message = CallMessage::Burn {
        coins: Coins {
            amount: initial_balance + 10,
            token_id,
        },
    };

    let failed_to_burn = bank.call(burn_message, &minter_context, &mut state);
    assert!(failed_to_burn.is_err());
    let Error::ModuleError(err) = failed_to_burn.err().unwrap();
    let mut chain = err.chain();
    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());
    assert_eq!(
        format!(
            "Failed to burn coins(token_id={} amount={}) from owner {}",
            token_id,
            initial_balance + 10,
            minter
        ),
        message_1
    );
    assert_eq!(
        format!(
            "Insufficient balance from={minter}, got={}, needed={}, for token={}",
            initial_balance - burn_amount,
            initial_balance + 10,
            token_name
        ),
        message_2,
    );

    // ---
    // Try to burn non-existing token
    let token_id = get_token_id::<S>("NotRealToken2", &minter, salt);
    let burn_message = CallMessage::Burn {
        coins: Coins {
            amount: 1,
            token_id,
        },
    };

    let failed_to_burn = bank.call(burn_message, &minter_context, &mut state);
    assert!(failed_to_burn.is_err());
    let Error::ModuleError(err) = failed_to_burn.err().unwrap();
    let mut chain = err.chain();
    let message_1 = chain.next().unwrap().to_string();
    let message_2 = chain.next().unwrap().to_string();
    assert!(chain.next().is_none());
    assert_eq!(
        format!(
            "Failed to burn coins(token_id={} amount={}) from owner {}",
            token_id, 1, minter
        ),
        message_1
    );
    // Note, no token ID in root cause the message.
    let expected_error_part =
        "Value not found for prefix: \"sov_bank/Bank/tokens/\" and storage key:";
    assert!(message_2.starts_with(expected_error_part));

    Ok(())
}

#[test]
fn burn_initial_tokens() -> Result<(), Infallible> {
    let initial_balance = 100;
    let bank_config = create_bank_config_with_token(2, initial_balance);
    let tmpdir = tempfile::tempdir().unwrap();
    let bank = Bank::default();

    let state = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let mut genesis_state = state.to_genesis_state_accessor::<Bank<S>>(&bank_config);

    bank.genesis(&bank_config, &mut genesis_state).unwrap();

    let token_id = sov_bank::GAS_TOKEN_ID;
    let sender_address = bank_config.gas_token_config.address_and_balances[0].0;
    let sequencer_address = bank_config.gas_token_config.address_and_balances[1].0;

    let mut state = genesis_state.checkpoint().to_working_set_unmetered();

    let query_user_balance = |user_address: <S as Spec>::Address,
                              state: &mut WorkingSet<S>|
     -> Result<Option<u64>, Infallible> {
        bank.get_balance_of(&user_address, token_id, &mut state.to_unmetered())
    };

    let balance_before = query_user_balance(sender_address, &mut state)?;
    assert_eq!(Some(initial_balance), balance_before);

    let burn_amount = 10;
    let burn_message = CallMessage::Burn {
        coins: Coins {
            amount: burn_amount,
            token_id,
        },
    };

    let context = Context::<S>::new(sender_address, Default::default(), sequencer_address, 1);
    bank.call(burn_message, &context, &mut state)
        .expect("Failed to burn token");
    assert_eq!(state.events().len(), 1);

    let balance_after = query_user_balance(sender_address, &mut state)?;
    assert_eq!(Some(initial_balance - burn_amount), balance_after);

    // Assume that the rest of edge cases are similar to deployed tokens
    Ok(())
}
