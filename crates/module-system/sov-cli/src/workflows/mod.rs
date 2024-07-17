//! Workflows for the CLI wallet

pub(crate) const NO_ACCOUNTS_FOUND: &str =
    "No accounts found. You can generate one with the `keys generate` subcommand";
pub mod keys;
pub mod rpc;
pub mod transactions;
