use sov_bank::{Amount, Coins, IntoPayable, GAS_TOKEN_ID};
#[cfg(feature = "native")]
use sov_modules_api::macros::CliWalletArg;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{
    CallResponse, Context, DaSpec, EventEmitter, ModuleInfo, Spec, StateAccessor,
    StateAccessorError, StateCheckpoint, StateWriter, TxState,
};
use sov_state::User;
use thiserror::Error;

use crate::event::Event;
use crate::{AllowedSequencer, SequencerRegistry};

/// This enumeration represents the available call messages for interacting with
/// the `sov-sequencer-registry` module.
#[cfg_attr(feature = "native", derive(schemars::JsonSchema), derive(CliWalletArg))]
#[cfg_attr(
    feature = "arbitrary",
    derive(arbitrary::Arbitrary, proptest_derive::Arbitrary)
)]
#[derive(
    Debug,
    PartialEq,
    Clone,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum CallMessage {
    /// Add a new sequencer to the sequencer registry.
    Register {
        /// The raw Da address of the sequencer you're registering.
        da_address: Vec<u8>,
        /// The initial balance of the sequencer.
        amount: Amount,
    },
    /// Increases the balance of the sequencer, transferring the funds from the sequencer account
    /// to the rollup.
    Deposit {
        /// The raw Da address of the sequencer.
        da_address: Vec<u8>,
        /// The amount to increase.
        amount: Amount,
    },
    /// Remove a sequencer from the sequencer registry.
    Exit {
        /// The raw Da address of the sequencer you're removing.
        da_address: Vec<u8>,
    },
}

/// Errors that can be raised by the `SequencerRegistry` module
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SequencerRegistryError<S: Spec, Da: DaSpec> {
    #[error("The provided address is not an allowed sequencer")]
    /// The provided address is not an allowed sequencer.
    IsNotRegisteredSequencer(Da::Address),

    /// The sequencer tried to unregister itself during the execution of its own batch.
    #[error("Sequencers may not unregister during execution of their own batch")]
    CannotUnregisterDuringOwnBatch(Da::Address),

    #[error("The address provided as a parameter to the `exit` method does not match the transaction sender")]
    /// The address provided as a parameter to the `exit` method does not match the transaction sender.
    SuppliedAddressDoesNotMatchTxSender {
        /// The address provided as a parameter to the `exit` method.
        parameter: S::Address,
        /// The address of the transaction sender.
        sender: S::Address,
    },

    #[error("The module account does not have enough funds to refund the sequencer's staked amount. This is a bug")]
    /// The module account does not have enough funds to refund the sequencer's staked amount.
    InsufficientFundsToRefundStakedAmount(
        // The amount of gas tokens to refund
        u64,
    ),

    #[error("The provided amount makes the balance of the sequencer's account overflow.")]
    /// The provided amount makes the balance of the sequencer's account overflow.
    ToppingAccountMakesBalanceOverflow {
        /// The address of the sequencer's account.
        address: S::Address,
        /// The existing staked balance of the sequencer's account.
        existing_balance: u64,
        /// The amount to add to the balance of the sequencer's account.
        amount_to_add: u64,
    },

    #[error("Insufficient funds on the sender's account to top up it's staked balance")]
    /// Insufficient funds on the sender's account to top up it's staked balance
    InsufficientFundsToTopUpAccount {
        /// The address of the sequencer's account.
        address: S::Address,
        /// The amount to add to the balance of the sequencer's account.
        amount_to_add: u64,
    },

    #[error("The sequencer is already registered")]
    /// The sequencer is already registered.
    SequencerAlreadyRegistered(S::Address),

    #[error("Stake amount below the minimum needed to register a sequencer")]
    /// Stake amount below the minimum needed to register a sequencer.
    InsufficientStakeAmount {
        /// The amount of gas tokens the sender is trying to stake.
        bond_amount: u64,
        /// The minimum amount of gas tokens to stake.
        minimum_bond_amount: u64,
    },

    #[error(
        "The minimum bond is not set. This is a bug - the minimum bond should be set at genesis"
    )]
    /// The minimum bond is not set. This is a bug - the minimum bond should be set at genesis
    NoMinimumBondSet,

    #[error("The sender's account does not have enough funds to register itself as a sequencer")]
    /// The sender's account does not have enough funds to register itself as a sequencer.
    InsufficientFundsToRegister(
        // The amount of gas tokens to stake
        u64,
    ),

    /// An error occurred when accessing the state
    #[error("An error occurred when accessing the state, error: {0}")]
    StateAccessorError(String),
}

impl<S: Spec, Da: DaSpec> From<StateAccessorError<S::Gas>> for SequencerRegistryError<S, Da> {
    fn from(value: StateAccessorError<S::Gas>) -> Self {
        SequencerRegistryError::StateAccessorError(value.to_string())
    }
}

impl<S: sov_modules_api::Spec, Da: sov_modules_api::DaSpec> SequencerRegistry<S, Da> {
    /// Tries to register a sequencer by staking the provided amount of gas tokens.
    /// This method uses the context's sender as the sequencer's address.
    ///
    /// # Errors
    /// Will error
    ///
    /// - If the provided amount is below the minimum required to register a sequencer.
    /// - If the minimum bond is not set.
    /// - If the sender's account does not have enough funds to register itself as a sequencer.
    /// - If the sequencer is already registered.
    pub(crate) fn register(
        &self,
        da_address: &Da::Address,
        amount: Amount,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, SequencerRegistryError<S, Da>> {
        let sequencer = context.sender();
        self.register_sequencer(da_address, sequencer, amount, state)?;
        Ok(CallResponse::default())
    }

    /// Tries to remove a sequencer by unstaking the provided amount of gas tokens.
    /// This method uses the context's sender as the sequencer's address.
    ///
    /// # Errors
    /// Will error
    ///
    /// - If the sequencer is not registered.
    /// - If the sequencer tries to unregister itself during the execution of its own batch.
    /// - If the supplied `da_address` does not match the transaction sender.
    /// - If the module balance is not high enough to refund the sequencer's staked amount (this is a bug).
    pub(crate) fn exit(
        &self,
        da_address: &Da::Address,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, SequencerRegistryError<S, Da>> {
        let sender = context.sender();

        let belongs_to = self
            .allowed_sequencers
            .get_or_err(da_address, state)?
            .map_err(|_| SequencerRegistryError::IsNotRegisteredSequencer(da_address.clone()))?
            .address;

        if &belongs_to == context.sequencer() {
            return Err(SequencerRegistryError::CannotUnregisterDuringOwnBatch(
                da_address.clone(),
            ));
        }

        if sender != &belongs_to {
            return Err(
                SequencerRegistryError::SuppliedAddressDoesNotMatchTxSender {
                    parameter: belongs_to,
                    sender: sender.clone(),
                },
            );
        }

        let sender_balance = self.get_sender_balance(da_address, state)?.unwrap_or(0);

        self.bank
            .transfer_from(
                self.id().to_payable(),
                sender,
                Coins {
                    amount: sender_balance,
                    token_id: GAS_TOKEN_ID,
                },
                state,
            )
            .map_err(|_| {
                SequencerRegistryError::InsufficientFundsToRefundStakedAmount(sender_balance)
            })?;

        // we remove the sequencer from the registry *once the sequencer has received its staked amount*
        self.delete(da_address, state)?;

        self.emit_event(
            state,
            Event::<S>::Exited {
                sequencer: sender.clone(),
            },
        );

        Ok(CallResponse::default())
    }

    pub(crate) fn delete<Accessor: StateAccessor>(
        &self,
        da_address: &Da::Address,
        state: &mut Accessor,
    ) -> Result<(), <Accessor as StateWriter<User>>::Error> {
        self.allowed_sequencers.delete(da_address, state)?;

        if let Some(preferred_sequencer) = self.preferred_sequencer.get(state)? {
            if da_address == &preferred_sequencer {
                self.preferred_sequencer.delete(state)?;
            }
        }

        Ok(())
    }

    /// Increases the balance of the provided sender, updating the state of the registry.
    ///
    /// # Errors
    ///
    /// Will error when:
    ///
    /// - The provided sender is not allowed.
    /// - The provided sender doesn't have enough funds to increase its balance.
    /// - The amount overflows.
    pub(crate) fn increase_sender_balance(
        &self,
        sender: &Da::Address,
        amount: Amount,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, SequencerRegistryError<S, Da>> {
        let AllowedSequencer { address, balance } =
            self.allowed_sequencers.get(sender, state)?.ok_or(
                SequencerRegistryError::IsNotRegisteredSequencer(sender.clone()),
            )?;

        let balance = balance.checked_add(amount).ok_or(
            SequencerRegistryError::ToppingAccountMakesBalanceOverflow {
                address: address.clone(),
                existing_balance: balance,
                amount_to_add: amount,
            },
        )?;

        let coins = Coins {
            amount,
            token_id: GAS_TOKEN_ID,
        };

        self.bank
            .transfer_from(&address, self.id().to_payable(), coins, state)
            .map_err(
                |_| SequencerRegistryError::<S, Da>::InsufficientFundsToTopUpAccount {
                    address: address.clone(),
                    amount_to_add: amount,
                },
            )?;

        self.allowed_sequencers.set(
            sender,
            &AllowedSequencer {
                address: address.clone(),
                balance,
            },
            state,
        )?;

        self.emit_event(
            state,
            Event::<S>::Deposited {
                sequencer: address,
                amount,
            },
        );

        Ok(CallResponse::default())
    }

    /// Rewards the sequencer with the `amount` of gas tokens.
    /// Transfers the reward from the module's account to the sequencer's account.
    ///
    /// # Safety note:
    /// This method panics if:
    /// - The sequencer is not registered (this should be checked in the `begin_batch_hook` which should always be called before this method).
    /// - The module account does not have enough funds to pay for the reward (the module balance should be populated in the `GasEnforcer` capability hook).
    pub(crate) fn reward_sequencer(
        &self,
        sequencer: &Da::Address,
        amount: u64,
        state: &mut StateCheckpoint<S>,
    ) {
        let AllowedSequencer {
            address: rollup_address,
            balance: _,
        } = self.allowed_sequencers.get(sequencer, state).unwrap_infallible().expect("Sequencer must be allowed. This should have been checked in the `begin_batch_hook`. This is a bug");

        self.bank
            .transfer_from(
                self.id().to_payable(),
                &rollup_address,
                Coins {
                    amount,
                    token_id: GAS_TOKEN_ID,
                },
                state,
            )
            .expect(
                "Impossible to transfer the reward from the module account to the sequencer. This is a bug",
            );
    }
}
