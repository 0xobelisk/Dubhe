//! The `sov-sequencer-registry` module is responsible for sequencer
//! registration, slashing, and rewards. At the moment, only a centralized
//! sequencer is supported. The sequencer's address and bond are registered
//! during the rollup deployment.
//!
//! The module implements the [`sov_modules_api::hooks::ApplyBatchHooks`] trait.
#![deny(missing_docs)]
mod call;
mod capabilities;
mod event;
mod genesis;
mod hooks;

#[cfg(test)]
mod tests;

use sov_modules_api::prelude::UnwrapInfallible;
#[cfg(feature = "native")]
mod query;
use borsh::{BorshDeserialize, BorshSerialize};
pub use call::*;
pub use capabilities::SequencerStakeMeter;
pub use genesis::*;
#[cfg(feature = "native")]
pub use query::*;
use serde::{Deserialize, Serialize};
use sov_bank::{Amount, Coins, IntoPayable, GAS_TOKEN_ID};
use sov_modules_api::capabilities::FatalError;
use sov_modules_api::transaction::SequencerReward;
use sov_modules_api::{
    CallResponse, Context, Error, EventEmitter, GenesisState, InfallibleStateAccessor, ModuleId,
    ModuleInfo, Spec, StateAccessor, StateCheckpoint, StateMap, StateReader, StateValue, TxState,
};
use sov_state::codec::BcsCodec;
use sov_state::{EventContainer, User};
use thiserror::Error;

use crate::event::Event;

/// An allowed sequencer for a rollup.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Eq, PartialEq)]
#[serde(bound = "S::Address: serde::Serialize + serde::de::DeserializeOwned")]
pub struct AllowedSequencer<S: Spec> {
    /// The rollup address of the sequencer.
    pub address: S::Address,
    /// The staked balance of the sequencer.
    pub balance: Amount,
}

/// Errors that can be raised by the [`SequencerRegistry`] module during hooks execution.
#[derive(
    Debug, Clone, Error, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum AllowedSequencerError {
    /// The amount of gas tokens that the sender is has staken is too low.
    #[error("The amount staked by the sequencer is less than the minimum bond. Amount currently staked: {bond_amount}, minimum bond amount: {minimum_bond_amount}")]
    InsufficientStakeAmount {
        /// The amount of gas tokens that the sender is has staken.
        bond_amount: Amount,
        /// The minimum amount of gas tokens that the sequencer must stake.
        minimum_bond_amount: Amount,
    },
    /// The sequencer is not registered.
    #[error("The sequencer is not registered")]
    NotRegistered,
}

/// Represents the different outcomes that can occur for a sequencer after batch processing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BatchSequencerOutcome {
    /// Sequencer receives reward amount in defined token and can withdraw its deposit. The amount is net of any penalties.
    Rewarded(SequencerReward),
    /// Sequencer loses its deposit and receives no reward.
    Slashed(
        /// Reason why sequencer was slashed.
        FatalError,
    ),
    /// Batch was ignored, sequencer deposit left untouched.
    Ignored(
        /// Reason why the batch was ignored.
        String,
    ),
    /// The sequencer is not rewardable for the submitted batch.
    /// This occurs when an unregistered sequencer submits a batch directly to the DA.
    /// The batch might be applied but there is nobody to reward.
    NotRewardable,
}

/// Reason why sequencer was slashed.
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SlashingReason {
    /// This status indicates problem with batch deserialization.
    InvalidBatchEncoding,
    /// Stateless verification failed, for example deserialized transactions have invalid signatures.
    StatelessVerificationFailed,
    /// This status indicates problem with transaction deserialization.
    InvalidTransactionEncoding,
}

/// The `sov-sequencer-registry` module `struct`.
#[derive(Clone, ModuleInfo, sov_modules_api::macros::ModuleRestApi)]
pub struct SequencerRegistry<S: Spec, Da: sov_modules_api::DaSpec> {
    /// The ID of the `sov_sequencer_registry` module.
    #[id]
    pub(crate) id: ModuleId,

    /// Reference to the Bank module.
    #[module]
    pub(crate) bank: sov_bank::Bank<S>,

    /// The minimum bond for a sequencer to send transactions.
    /// TODO(@theochap): This should be expressed in gas units.
    #[state]
    pub minimum_bond: StateValue<Amount>,

    /// Only batches from sequencers from this list are going to be processed.
    /// We need to map the DA address to the rollup address because the sequencer interacts with the rollup
    /// through the DA layer.
    #[state]
    pub(crate) allowed_sequencers: StateMap<Da::Address, AllowedSequencer<S>, BcsCodec>,

    /// Optional preferred sequencer.
    /// If set, batches from this sequencer will be processed first in block,
    /// So this sequencer can guarantee soft confirmation time for transactions
    #[state]
    pub(crate) preferred_sequencer: StateValue<Da::Address, BcsCodec>,
}

impl<S: Spec, Da: sov_modules_api::DaSpec> sov_modules_api::Module for SequencerRegistry<S, Da> {
    type Spec = S;

    type Config = SequencerConfig<S, Da>;

    type CallMessage = CallMessage;

    type Event = Event<S>;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<(), Error> {
        Ok(self.init_module(config, state)?)
    }

    fn call(
        &self,
        message: Self::CallMessage,
        context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, Error> {
        Ok(match message {
            CallMessage::Register { da_address, amount } => {
                let da_address = Da::Address::try_from(&da_address)?;
                self.register(&da_address, amount, context, state)
                    .map_err(|e| Error::ModuleError(e.into()))?
            }
            CallMessage::Deposit { da_address, amount } => {
                let da_address = Da::Address::try_from(&da_address)?;
                self.increase_sender_balance(&da_address, amount, state)
                    .map_err(|e| Error::ModuleError(e.into()))?
            }
            CallMessage::Exit { da_address } => {
                let da_address = Da::Address::try_from(&da_address)?;
                self.exit(&da_address, context, state)
                    .map_err(|e| Error::ModuleError(e.into()))?
            }
        })
    }
}

impl<S: Spec, Da: sov_modules_api::DaSpec> SequencerRegistry<S, Da> {
    /// Returns the minimum amount of tokens that the sequencer must lock.
    pub fn get_coins_to_lock<Reader: StateReader<User>>(
        &self,
        state: &mut Reader,
    ) -> Result<Coins, Reader::Error> {
        let amount = self
            .minimum_bond
            .get(state)?
            .expect("The minimum bond should be set at genesis");
        Ok(Coins {
            amount,
            token_id: GAS_TOKEN_ID,
        })
    }

    /// Tries to register a sequencer by staking the provided amount of gas tokens.
    /// # Errors
    /// Will error
    ///
    /// - If the provided amount is below the minimum required to register a sequencer.
    /// - If the minimum bond is not set.
    /// - If the sender's account does not have enough funds to register itself as a sequencer.
    /// - If the sequencer is already registered.
    pub(crate) fn register_sequencer(
        &self,
        da_address: &Da::Address,
        address: &S::Address,
        amount: Amount,
        state: &mut (impl StateAccessor + EventContainer),
    ) -> Result<(), SequencerRegistryError<S, Da>> {
        if self
            .allowed_sequencers
            .get(da_address, state)
            .map_err(|e| SequencerRegistryError::StateAccessorError(e.to_string()))?
            .is_some()
        {
            return Err(SequencerRegistryError::SequencerAlreadyRegistered(
                address.clone(),
            ));
        }

        let minimum_bond = self
            .minimum_bond
            .get(state)
            .map_err(|e| SequencerRegistryError::StateAccessorError(e.to_string()))?
            .ok_or(SequencerRegistryError::NoMinimumBondSet)?;

        if amount < minimum_bond {
            return Err(SequencerRegistryError::InsufficientStakeAmount {
                bond_amount: amount,
                minimum_bond_amount: minimum_bond,
            });
        }

        let locker = &self.id;

        let coins = Coins {
            amount,
            token_id: GAS_TOKEN_ID,
        };

        self.bank
            .transfer_from(address, locker.to_payable(), coins, state)
            .map_err(|_| SequencerRegistryError::<S, Da>::InsufficientFundsToRegister(amount))?;

        self.allowed_sequencers
            .set(
                da_address,
                &AllowedSequencer {
                    address: address.clone(),
                    balance: amount,
                },
                state,
            )
            .map_err(|e| SequencerRegistryError::StateAccessorError(e.to_string()))?;

        self.emit_event(
            state,
            Event::<S>::Registered {
                sequencer: address.clone(),
                amount,
            },
        );

        Ok(())
    }

    /// Returns the preferred sequencer, or [`None`] it wasn't set.
    ///
    /// Read about [`SequencerConfig::is_preferred_sequencer`] to learn about
    /// preferred sequencers.
    pub fn get_preferred_sequencer<Reader: StateReader<User>>(
        &self,
        state: &mut Reader,
    ) -> Result<Option<Da::Address>, Reader::Error> {
        self.preferred_sequencer.get(state)
    }

    /// Resolve a DA address to a rollup address.
    pub fn resolve_da_address<Reader: StateReader<User>>(
        &self,
        address: &Da::Address,
        state: &mut Reader,
    ) -> Result<Option<S::Address>, Reader::Error> {
        self.allowed_sequencers
            .get(address, state)
            .map(|s| s.map(|s| s.address))
    }

    /// Returns the rollup address of the preferred sequencer, or [`None`] it wasn't set.
    ///
    /// Read about [`SequencerConfig::is_preferred_sequencer`] to learn about
    /// preferred sequencers.
    pub fn get_preferred_sequencer_rollup_address<Reader: StateReader<User>>(
        &self,
        state: &mut Reader,
    ) -> Result<Option<S::Address>, Reader::Error> {
        Ok(match self.preferred_sequencer.get(state)? {
            Some(da_addr) => Some(
                self.allowed_sequencers
                    .get(&da_addr, state)?
                    .expect("Preferred Sequencer must have known address on rollup")
                    .address,
            ),
            None => None,
        })
    }

    /// Checks whether `sender` is a registered sequencer with enough staked amount.
    /// If so, returns the allowed sequencer in a [`AllowedSequencer`] object.
    /// Otherwise, returns a [`AllowedSequencerError`].
    pub fn is_sender_allowed(
        &self,
        sender: &Da::Address,
        state: &mut impl InfallibleStateAccessor,
    ) -> Result<AllowedSequencer<S>, AllowedSequencerError> {
        if let Some(sequencer) = self
            .allowed_sequencers
            .get(sender, state)
            .unwrap_infallible()
        {
            let min_bond = self
                .minimum_bond
                .get(state)
                .unwrap_infallible()
                .expect("The minimum bond should be set at genesis");

            if sequencer.balance < min_bond {
                return Err(AllowedSequencerError::InsufficientStakeAmount {
                    bond_amount: sequencer.balance,
                    minimum_bond_amount: min_bond,
                });
            }

            return Ok(sequencer);
        }

        Err(AllowedSequencerError::NotRegistered)
    }

    /// Returns the balance of the provided sender, if present.
    pub fn get_sender_balance<Reader: StateReader<User>>(
        &self,
        sender: &Da::Address,
        state: &mut Reader,
    ) -> Result<Option<Amount>, Reader::Error> {
        Ok(self
            .allowed_sequencers
            .get(sender, state)?
            .map(|s| s.balance))
    }

    /// Returns the rollup address of the sequencer with the given DA address.
    pub fn get_sequencer_address<Reader: StateReader<User>>(
        &self,
        da_address: Da::Address,
        state_accessor: &mut Reader,
    ) -> Result<Option<S::Address>, Reader::Error> {
        Ok(self
            .allowed_sequencers
            .get(&da_address, state_accessor)?
            .map(|s| s.address))
    }

    /// Slash the sequencer with the given address.
    pub fn slash_sequencer(&self, da_address: &Da::Address, state: &mut StateCheckpoint<S>) {
        self.delete(da_address, state).unwrap_infallible();
    }

    /// Check if the provided `Da::Address` belongs to a registered sequencer.
    pub fn is_registered_sequencer<Reader: StateReader<User>>(
        &self,
        da_address: &Da::Address,
        state: &mut Reader,
    ) -> Result<bool, Reader::Error> {
        Ok(self.allowed_sequencers.get(da_address, state)?.is_some())
    }
}
