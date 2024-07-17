#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod call;
mod evm;
mod genesis;
mod hooks;

pub use call::*;
pub use evm::*;
pub use genesis::*;

#[cfg(feature = "native")]
mod rpc;

#[cfg(feature = "native")]
pub use rpc::*;

#[cfg(test)]
mod tests;

mod authenticate;
mod event;
#[cfg(feature = "native")]
mod helpers;

pub use authenticate::authenticate;
use revm::primitives::Address;
pub use revm::primitives::SpecId;
use revm_primitives::BlockEnv;
use sov_modules_api::{
    Context, Error, GenesisState, ModuleId, ModuleInfo, StateAccessor, TxState,
    UnmeteredStateWrapper,
};
use sov_state::codec::BcsCodec;

use crate::event::Event;
use crate::evm::db::EvmDb;
use crate::evm::primitive_types::{Block, Receipt, SealedBlock, TransactionSignedAndRecovered};

// Gas per transaction not creating a contract.
#[cfg(feature = "native")]
pub(crate) const MIN_TRANSACTION_GAS: u64 = 21_000u64;
#[cfg(feature = "native")]
pub(crate) const MIN_CREATE_GAS: u64 = 53_000u64;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingTransaction {
    pub(crate) transaction: TransactionSignedAndRecovered,
    pub(crate) receipt: Receipt,
}

/// The sov-evm module provides compatibility with the EVM.
#[allow(dead_code)]
#[derive(Clone, ModuleInfo)]
pub struct Evm<S: sov_modules_api::Spec> {
    /// The ID of the evm module.
    #[id]
    pub(crate) id: ModuleId,

    /// Mapping from account address to account state.
    #[state]
    pub(crate) accounts: sov_modules_api::StateMap<Address, DbAccount, BcsCodec>,

    /// Mapping from code hash to code. Used for lazy-loading code into a contract account.
    #[state]
    pub(crate) code:
        sov_modules_api::StateMap<revm::primitives::B256, reth_primitives::Bytes, BcsCodec>,

    /// Chain configuration. This field is set in genesis.
    #[state]
    pub(crate) cfg: sov_modules_api::StateValue<EvmChainConfig, BcsCodec>,

    /// Block environment used by the evm. This field is set in `begin_slot_hook`.
    #[state]
    pub(crate) block_env: sov_modules_api::StateValue<BlockEnv, BcsCodec>,

    /// Transactions that will be added to the current block.
    /// A valid transaction is added to the vec on every call message.
    #[state]
    pub(crate) pending_transactions: sov_modules_api::StateVec<PendingTransaction, BcsCodec>,

    /// Head of the chain. The new head is set in `end_slot_hook` but without the inclusion of the `state_root` field.
    /// The `state_root` is added in `begin_slot_hook` of the next block because its calculation occurs after the `end_slot_hook`.
    #[state]
    pub(crate) head: sov_modules_api::StateValue<Block, BcsCodec>,

    /// Used only by the RPC: This represents the head of the chain and is set in two distinct stages:
    /// 1. `end_slot_hook`: the pending head is populated with data from pending_transactions.
    /// 2. `finalize_hook` the `root_hash` is populated.
    /// Since this value is not authenticated, it can be modified in the `finalize_hook` with the correct `state_root`.
    #[state]
    pub(crate) pending_head: sov_modules_api::AccessoryStateValue<Block, BcsCodec>,

    /// Used only by the RPC: The vec is extended with `pending_head` in `finalize_hook`.
    #[state]
    pub(crate) blocks: sov_modules_api::AccessoryStateVec<SealedBlock, BcsCodec>,

    /// Used only by the RPC: block_hash => block_number mapping.
    #[state]
    pub(crate) block_hashes:
        sov_modules_api::AccessoryStateMap<revm::primitives::B256, u64, BcsCodec>,

    /// Used only by the RPC: List of processed transactions.
    #[state]
    pub(crate) transactions:
        sov_modules_api::AccessoryStateVec<TransactionSignedAndRecovered, BcsCodec>,

    /// Used only by the RPC: transaction_hash => transaction_index mapping.
    #[state]
    pub(crate) transaction_hashes:
        sov_modules_api::AccessoryStateMap<revm::primitives::B256, u64, BcsCodec>,

    /// Used only by the RPC: Receipts.
    #[state]
    pub(crate) receipts: sov_modules_api::AccessoryStateVec<Receipt, BcsCodec>,

    #[phantom]
    phantom: core::marker::PhantomData<S>,
}

impl<S: sov_modules_api::Spec> sov_modules_api::Module for Evm<S> {
    type Spec = S;

    type Config = EvmConfig;

    type CallMessage = CallMessage;

    type Event = Event;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        Ok(self.init_module(config, state)?)
    }

    fn call(
        &self,
        msg: Self::CallMessage,
        context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,
    ) -> Result<sov_modules_api::CallResponse, Error> {
        Ok(self.execute_call(msg, context, state)?)
    }
}

impl<S: sov_modules_api::Spec> Evm<S> {
    pub(crate) fn get_db<'a, Ws: StateAccessor>(
        &self,
        state: &'a mut Ws,
    ) -> EvmDb<UnmeteredStateWrapper<'a, Ws>> {
        let infallible_state_accessor = state.to_unmetered();
        EvmDb::new(
            self.accounts.clone(),
            self.code.clone(),
            infallible_state_accessor,
        )
    }
}
