use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use jsonrpsee::http_client::HttpClientBuilder;
use sov_bank::{Amount, Bank, BankRpcClient, CallMessage, GAS_TOKEN_ID};
use sov_modules_api::prelude::tokio;
use sov_modules_api::Spec;
use sov_rollup_interface::services::da::DaService;
use tokio::sync::mpsc::Sender;

use super::{MessageSender, MessageSenderT};
use crate::account_pool::AccountPool;
use crate::args::Args;
use crate::call_messages::{PreparedCallMessage, SerializedPreparedCallMessage};
use crate::constants::DEFAULT_MAX_FEE;

// How much funds account should have to be considered a "whale".
const MINIMAL_WHALE_BALANCE: u64 = 5_000_000;

fn get_bank_config<S: Spec>(
    genesis_dir: impl AsRef<Path>,
) -> anyhow::Result<sov_bank::BankConfig<S>> {
    let path = genesis_dir.as_ref().join("bank.json");

    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let bank_config = serde_json::from_reader(reader)?;

    Ok(bank_config)
}

async fn get_gas_funding_txs<S: Spec>(
    config: &Args,
    account_pool: &AccountPool<S>,
) -> anyhow::Result<Vec<PreparedCallMessage<S, Bank<S>>>> {
    let genesis_dir = &config.genesis_dir;
    let client = HttpClientBuilder::default().build(&config.rpc_url)?;

    let bank_config = get_bank_config::<S>(genesis_dir)?;
    tracing::info!(?bank_config, "Bank config");

    let gas_whale_account_pool_indices = {
        let mut map = HashMap::<S::Address, u64>::new();
        bank_config
            .gas_token_config
            .address_and_balances
            .into_iter()
            .for_each(|(address, balance)| {
                if balance < MINIMAL_WHALE_BALANCE {
                    if let Some(index) = account_pool.get_index(&address) {
                        map.insert(address, *index);
                    }
                }
            });
        map
    };

    let num_gas_whales = gas_whale_account_pool_indices.keys().len();

    if num_gas_whales == 0 {
        anyhow::bail!("No whales found!");
    }

    let total_supply = BankRpcClient::<S>::supply_of(&client, None, GAS_TOKEN_ID)
        .await?
        .amount
        .expect("Gas token should exist");

    let mut txs = Vec::new();

    if total_supply < (Amount::MAX / 2) {
        let gas_token_minter = bank_config
            .gas_token_config
            .authorized_minters
            .iter()
            .find(|address| account_pool.contains_address(address))
            .expect("Haven't found gas token minter in available keys. Cannot proceed");

        let gas_token_minter_account_pool_index = account_pool
            .addresses()
            .enumerate()
            .find(|(_, address)| address == &gas_token_minter)
            .expect("gas token minter should have an index")
            .0 as u64;
        tracing::info!(gas_token_minter = %gas_token_minter, account_pool_index = %gas_token_minter_account_pool_index, "Gas token minter");

        tracing::info!("Total supply of gas token is not large enough, need to mint!");

        let to_mint = Amount::MAX - 100 - total_supply;
        let to_mint_per_whale = to_mint / num_gas_whales as u64;
        for whale_address in gas_whale_account_pool_indices.keys() {
            tracing::info!(amount = to_mint_per_whale, to = %whale_address, from = %gas_token_minter, account_pool_index = gas_token_minter_account_pool_index, "Mint call message");
            let call_message = CallMessage::<S>::Mint {
                coins: sov_bank::Coins {
                    amount: to_mint_per_whale,
                    token_id: GAS_TOKEN_ID,
                },
                mint_to_address: whale_address.clone(),
            };
            txs.push(PreparedCallMessage::<S, Bank<S>>::new(
                call_message,
                gas_token_minter_account_pool_index,
                DEFAULT_MAX_FEE,
            ));
        }
    }
    tracing::info!("Filling gas balance for all non-whale accounts in account pool");

    let accounts_to_fill = account_pool
        .addresses()
        .filter(|addr| !gas_whale_account_pool_indices.contains_key(addr));

    for (idx, account) in accounts_to_fill.enumerate() {
        let whales: Vec<S::Address> = gas_whale_account_pool_indices.keys().cloned().collect();
        let whale_idx = idx % num_gas_whales;
        let whale = &whales[whale_idx];
        let whale_account_pool_index = *gas_whale_account_pool_indices
            .get(whale)
            .expect("gas whale should exist in account pool");

        // TODO: Better calculation on how much gas will be needed for account.
        let amount = 100_000_000;

        tracing::debug!(whale = %whale, whale_index = whale_idx, whale_acount_pool_index = whale_account_pool_index, "whale info");
        let call_message = CallMessage::<S>::Transfer {
            to: account.clone(),
            coins: sov_bank::Coins {
                amount,
                token_id: GAS_TOKEN_ID,
            },
        };

        txs.push(PreparedCallMessage::<S, Bank<S>>::new(
            call_message,
            whale_account_pool_index,
            DEFAULT_MAX_FEE,
        ));
    }

    Ok(txs)
}

pub(crate) async fn get_gas_funding_message_sender<S: Spec, Da: DaService>(
    config: &Args,
    account_pool: AccountPool<S>,
    serialized_messages_tx: Sender<SerializedPreparedCallMessage>,
    should_stop: Arc<AtomicBool>,
) -> anyhow::Result<Box<dyn MessageSenderT>> {
    let gas_funding_txs = get_gas_funding_txs(config, &account_pool).await?;

    let message_sender: MessageSender<
        demo_stf::runtime::Runtime<S, <Da as DaService>::Spec>,
        <Da as DaService>::Spec,
        S,
        Bank<S>,
    > = MessageSender::new(
        "gas funding",
        should_stop.clone(),
        Box::new(gas_funding_txs.into_iter()),
        serialized_messages_tx.clone(),
    );

    Ok(Box::new(message_sender))
}
