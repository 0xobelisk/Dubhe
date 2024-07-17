//! Workflows for transaction management

use std::path::Path;

use anyhow::Context;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sov_modules_api::clap::{self, Subcommand};
use sov_modules_api::cli::{CliFrontEnd, CliTxImportArg};
use sov_modules_api::{CliWallet, DispatchCall, GasArray, Spec};
use sov_rollup_interface::common::HexString;

use crate::wallet_state::{sign_tx, KeyIdentifier, WalletState};
use crate::workflows::keys::load_key;
use crate::workflows::NO_ACCOUNTS_FOUND;
use crate::UnsignedTransactionWithoutNonce;

#[derive(clap::Parser, Clone)]
/// Generate, sign, list and remove transactions.
pub enum TransactionWorkflow<File: Subcommand, Json: Subcommand> {
    /// Import a transaction.
    #[clap(subcommand)]
    Import(TransactionLoadWorkflow<File, Json>),
    /// Signs input transaction and outputs signed transaction in hex
    Sign {
        #[clap(subcommand)]
        /// Transaction to sign.
        transaction: TransactionLoadWorkflow<File, Json>,
        /// Nonce to use.
        #[clap(short, long)]
        nonce: u64,
        /// Optional nickname of the imported key.
        #[clap(short, long)]
        key_nickname: Option<String>,
        /// Output result in JSON.
        #[clap(short, long)]
        json_output: bool,
    },
    /// Delete the current batch of transactions.
    Clean,
    /// Remove a single transaction from the current batch.
    Remove {
        /// The index of the transaction to remove, starting from 0
        index: usize,
    },
    /// List the current batch of transactions
    List,
}

impl<File, Json> TransactionWorkflow<File, Json>
where
    File: Subcommand,
    Json: Subcommand,
{
    /// Run the transaction workflow
    pub fn run<RT: CliWallet, S: sov_modules_api::Spec, U, E1, E2, E3>(
        self,
        wallet_state: &mut WalletState<RT::Decodable, S>,
        _app_dir: impl AsRef<Path>,
        mut out: impl std::io::Write,
    ) -> Result<(), anyhow::Error>
    where
        Json: CliFrontEnd<RT> + CliTxImportArg,
        File: CliFrontEnd<RT> + CliTxImportArg,
        Json: TryInto<RT::CliStringRepr<U>, Error = E1>,
        File: TryInto<RT::CliStringRepr<U>, Error = E2>,
        RT::CliStringRepr<U>: TryInto<RT::Decodable, Error = E3>,
        RT::Decodable: BorshSerialize + BorshDeserialize + Serialize + DeserializeOwned,
        E1: Into<anyhow::Error> + Send + Sync,
        E2: Into<anyhow::Error> + Send + Sync,
        E3: Into<anyhow::Error> + Send + Sync,
    {
        match self {
            TransactionWorkflow::Import(load_tx_workflow) => {
                let tx = load_tx_workflow.load()?;
                writeln!(&mut out, "Adding the following transaction to batch:")?;
                writeln!(&mut out, "{}", serde_json::to_string_pretty(&tx)?)?;
                wallet_state.unsent_transactions.push(tx);
                Ok(())
            }
            TransactionWorkflow::List => {
                writeln!(&mut out, "Current batch:")?;
                writeln!(
                    &mut out,
                    "{}",
                    serde_json::to_string_pretty(&wallet_state.unsent_transactions)?
                )?;
                Ok(())
            }
            TransactionWorkflow::Clean => {
                wallet_state.unsent_transactions.clear();
                Ok(())
            }
            TransactionWorkflow::Remove { index } => {
                wallet_state.unsent_transactions.remove(index);
                Ok(())
            }
            TransactionWorkflow::Sign {
                transaction,
                key_nickname,
                nonce,
                json_output,
            } => {
                let tx: UnsignedTransactionWithoutNonce<S, <RT as DispatchCall>::Decodable> =
                    transaction.load()?;
                let account = if let Some(nickname) = key_nickname {
                    let id = KeyIdentifier::<S>::ByNickname { nickname };
                    let addr = wallet_state.addresses.get_address(&id);
                    addr.ok_or_else(|| {
                        anyhow::format_err!("No account found matching identifier: {}", id)
                    })?
                } else {
                    wallet_state
                        .addresses
                        .default_address()
                        .ok_or_else(|| anyhow::format_err!(NO_ACCOUNTS_FOUND))?
                };

                let private_key = load_key::<S>(&account.location).with_context(|| {
                    format!("Unable to load key {}", account.location.display())
                })?;

                let signed_tx = HexString::new(sign_tx(&private_key, &tx, nonce)?);

                if json_output {
                    let output = SignTransactionOutput {
                        nonce,
                        input_tx: tx,
                        signed_tx,
                    };
                    let output = serde_json::to_string_pretty(&output)?;
                    writeln!(&mut out, "{}", output)?;
                } else {
                    writeln!(
                        &mut out,
                        "Signing the following transaction to batch with address {} nonce {}:",
                        account.address, nonce
                    )?;
                    writeln!(&mut out, "{}", serde_json::to_string_pretty(&tx)?)?;
                    writeln!(&mut out, "Signed Transaction (borsh encoded):")?;
                    writeln!(&mut out, "{}", signed_tx)?;
                }

                Ok(())
            }
        }
    }
}

#[derive(clap::Subcommand, Clone)]
/// Import a pre-formatted transaction from a JSON file or as a JSON string
pub enum TransactionLoadWorkflow<File: Subcommand, Json: Subcommand> {
    /// Import a transaction from a JSON file at the provided path
    #[clap(subcommand)]
    FromFile(File),
    /// Provide a JSON serialized transaction directly as input
    #[clap(subcommand)]
    FromString(
        /// The JSON serialized transaction as a string.
        /// The expected format is: {"module_name": {"call_name": {"field_name": "field_value"}}}
        Json,
    ),
}

impl<Json, File> TransactionLoadWorkflow<Json, File>
where
    Json: Subcommand,
    File: Subcommand,
{
    /// Parse from a file or a json string
    pub fn load<RT: CliWallet, S: sov_modules_api::Spec, U, E1, E2, E3>(
        self,
    ) -> Result<UnsignedTransactionWithoutNonce<S, RT::Decodable>, anyhow::Error>
    where
        Json: CliFrontEnd<RT> + CliTxImportArg,
        File: CliFrontEnd<RT> + CliTxImportArg,
        Json: TryInto<RT::CliStringRepr<U>, Error = E1>,
        File: TryInto<RT::CliStringRepr<U>, Error = E2>,
        RT::CliStringRepr<U>: TryInto<RT::Decodable, Error = E3>,
        RT::Decodable: BorshSerialize + BorshDeserialize + Serialize + DeserializeOwned,
        E1: Into<anyhow::Error> + Send + Sync,
        E2: Into<anyhow::Error> + Send + Sync,
        E3: Into<anyhow::Error> + Send + Sync,
    {
        let chain_id;
        let max_priority_fee_bips;
        let max_fee;
        let gas_limit;

        let intermediate_repr: RT::CliStringRepr<U> = match self {
            TransactionLoadWorkflow::FromFile(file) => {
                chain_id = file.chain_id();
                max_priority_fee_bips = file.max_priority_fee_bips();
                max_fee = file.max_fee();
                gas_limit = file.gas_limit().map(|m| m.to_vec());
                file.try_into().map_err(Into::<anyhow::Error>::into)?
            }
            TransactionLoadWorkflow::FromString(json) => {
                chain_id = json.chain_id();
                max_priority_fee_bips = json.max_priority_fee_bips();
                max_fee = json.max_fee();
                gas_limit = json.gas_limit().map(|m| m.to_vec());
                json.try_into().map_err(Into::<anyhow::Error>::into)?
            }
        };

        let tx: RT::Decodable = intermediate_repr
            .try_into()
            .map_err(Into::<anyhow::Error>::into)?;

        let gas_limit = gas_limit.map(|m| GasArray::from_slice(&m));

        Ok(UnsignedTransactionWithoutNonce::new(
            tx,
            chain_id,
            max_priority_fee_bips.into(),
            max_fee,
            gas_limit,
        ))
    }
}

#[derive(serde::Serialize)]
#[serde(bound = "Tx: serde::Serialize + serde::de::DeserializeOwned")]
struct SignTransactionOutput<S: Spec, Tx: BorshSerialize + BorshDeserialize> {
    nonce: u64,
    input_tx: UnsignedTransactionWithoutNonce<S, Tx>,
    signed_tx: HexString,
}
