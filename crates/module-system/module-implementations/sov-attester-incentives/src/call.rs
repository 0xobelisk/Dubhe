use core::result::Result::Ok;
use std::fmt::Debug;

use anyhow::{ensure, Context as AnyhowContext};
use borsh::{BorshDeserialize, BorshSerialize};
use derivative::Derivative;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sov_bank::{Amount, BurnRate, Coins, IntoPayable, GAS_TOKEN_ID};
use sov_modules_api::hooks::TransitionHeight;
use sov_modules_api::macros::config_value;
use sov_modules_api::optimistic::Attestation;
use sov_modules_api::{
    CallResponse, Context, DaSpec, EventEmitter, Gas, StateAccessor, StateAccessorError,
    StateTransitionPublicData, StateWriter, TxState, Zkvm,
};
use sov_state::storage::{SlotKey, SlotValue, Storage, StorageProof};
use sov_state::{EventContainer, User};
use thiserror::Error;
use tracing::{debug, error};

use crate::{AttesterIncentives, Event, UnbondingInfo};

/// A wrapper for attestations which implements `borsh` serialization. This is necessary since
/// Attestations are treated as `CallMessage`s, and we only support borsh encoding for transactions.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedAttestation<Da: DaSpec, StorageProof, Root> {
    #[serde(
        bound = "Da::SlotHash: Serialize + DeserializeOwned, StorageProof: Serialize + DeserializeOwned, Root: Serialize + DeserializeOwned"
    )]
    /// The inner attestation
    pub inner: Attestation<Da, StorageProof, Root>,
}

impl<Da: DaSpec, StorageProof: Debug, Root: Debug> Debug
    for WrappedAttestation<Da, StorageProof, Root>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrappedAttestation")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<Da: DaSpec, StorageProof, Root> From<Attestation<Da, StorageProof, Root>>
    for WrappedAttestation<Da, StorageProof, Root>
{
    fn from(value: Attestation<Da, StorageProof, Root>) -> Self {
        Self { inner: value }
    }
}

impl<Da: DaSpec, StorageProof: Serialize, Root: Serialize> BorshSerialize
    for WrappedAttestation<Da, StorageProof, Root>
{
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // TODO: Implement bcs `to_writer`
        let value = bcs::to_bytes(&self.inner).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to serialize attestation")
        })?;
        writer.write_all(&value)?;
        Ok(())
    }
}

impl<
        Da: DaSpec,
        StorageProof: Serialize + DeserializeOwned,
        Root: Serialize + DeserializeOwned,
    > BorshDeserialize for WrappedAttestation<Da, StorageProof, Root>
{
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        bcs::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }

    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        bcs::from_bytes(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

/// This enumeration represents the available call messages for interacting with the `AttesterIncentives` module.
#[derive(Derivative, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
#[derivative(
    PartialEq(bound = "<S::Storage as Storage>::Proof: PartialEq + Eq"),
    Eq(bound = "<S::Storage as Storage>::Proof: PartialEq + Eq")
)]
pub enum CallMessage<S: sov_modules_api::Spec, Da: DaSpec> {
    /// Bonds an attester, the parameter is the bond amount
    BondAttester(Amount),
    /// Start the first phase of the two-phase unbonding process
    BeginUnbondingAttester,
    /// Finish the two phase unbonding
    EndUnbondingAttester,
    /// Bonds a challenger, the parameter is the bond amount
    BondChallenger(Amount),
    /// Unbonds a challenger
    UnbondChallenger,
    /// Processes an attestation.
    ProcessAttestation(
        #[allow(clippy::type_complexity)]
        WrappedAttestation<
            Da,
            StorageProof<<S::Storage as Storage>::Proof>,
            <S::Storage as Storage>::Root,
        >,
    ),
    /// Processes a challenge. The challenge is encoded as a [`Vec<u8>`]. The second parameter is the transition number
    ProcessChallenge(Vec<u8>, TransitionHeight),
}

// Manually implement Debug to remove spurious Debug bound on S::Storage
impl<S: sov_modules_api::Spec, Da: DaSpec> Debug for CallMessage<S, Da> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BondAttester(arg0) => f.debug_tuple("BondAttester").field(arg0).finish(),
            Self::BeginUnbondingAttester => write!(f, "BeginUnbondingAttester"),
            Self::EndUnbondingAttester => write!(f, "EndUnbondingAttester"),
            Self::BondChallenger(arg0) => f.debug_tuple("BondChallenger").field(arg0).finish(),
            Self::UnbondChallenger => write!(f, "UnbondChallenger"),
            Self::ProcessAttestation(arg0) => {
                f.debug_tuple("ProcessAttestation").field(arg0).finish()
            }
            Self::ProcessChallenge(arg0, arg1) => f
                .debug_tuple("ProcessChallenge")
                .field(arg0)
                .field(arg1)
                .finish(),
        }
    }
}

#[derive(
    Debug,
    Error,
    PartialEq,
    Eq,
    BorshDeserialize,
    BorshSerialize,
    Clone,
    Copy,
    Serialize,
    Deserialize,
)]
/// Error type that explains why a user is slashed
pub enum SlashingReason {
    #[error("Transition isn't found")]
    /// The specified transition does not exist
    TransitionNotFound,

    #[error("The attestation does not contain the right block hash and post-state transition")]
    /// The specified transition is invalid (block hash, post-root hash or validity condition)
    TransitionInvalid,

    #[error("The initial hash of the transition is invalid")]
    /// The initial hash of the transition is invalid.
    InvalidInitialHash,

    #[error("The proof opening raised an error")]
    /// The proof verification raised an error
    InvalidProofOutputs,

    #[error("No invalid transition to challenge")]
    /// No invalid transition to challenge.
    NoInvalidTransition,
}

/// Error raised while processing the attester incentives
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttesterIncentiveErrors {
    #[error("Attester slashed")]
    /// The user was slashed. Reason specified by [`SlashingReason`]
    UserSlashed(#[source] SlashingReason),

    #[error("Invalid bonding proof")]
    /// The bonding proof was invalid
    InvalidBondingProof,

    #[error("The sender key doesn't match the attester key provided in the proof")]
    /// The sender key doesn't match the attester key provided in the proof
    InvalidSender,

    #[error("Attester is unbonding")]
    /// The attester is in the first unbonding phase
    AttesterIsUnbonding,

    #[error("User is not trying to unbond at the time of the transaction")]
    /// User is not trying to unbond at the time of the transaction
    AttesterIsNotUnbonding,

    #[error("The first phase of unbonding has not been finalized")]
    /// The attester is trying to finish the two-phase unbonding too soon
    UnbondingNotFinalized,

    #[error("The bond is not a 64-bit number")]
    /// The bond is not a 64-bit number
    InvalidBondFormat,

    #[error("User is not bonded at the time of the transaction")]
    /// User is not bonded at the time of the transaction
    UserNotBonded,

    #[error("Transition invariant isn't respected")]
    /// Transition invariant isn't respected
    InvalidTransitionInvariant,

    #[error("Error occurred when transferred bonding funds. The user's account may not have enough funds")]
    /// An error occurred when transferred funds
    BondTransferFailure,

    #[error(
        "Error occurred when trying to reward a user. The `AttesterIncentives` module may not have enough funds. This is a bug."
    )]
    /// An error occurred when transferred funds
    RewardTransferFailure,

    /// An error occurred when accessing the state
    #[error("Error occurred when accessing the state, error: {0}")]
    StateAccessError(String),
}

impl<GU: Gas> From<StateAccessorError<GU>> for AttesterIncentiveErrors {
    fn from(value: StateAccessorError<GU>) -> Self {
        Self::StateAccessError(value.to_string())
    }
}

impl From<SlashingReason> for AttesterIncentiveErrors {
    fn from(value: SlashingReason) -> Self {
        Self::UserSlashed(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A role in the attestation process
pub enum Role {
    /// A user who attests to new state transitions
    Attester,
    /// A user who challenges attestations
    Challenger,
}

impl<S, Da> AttesterIncentives<S, Da>
where
    S: sov_modules_api::Spec,
    Da: sov_modules_api::DaSpec,
{
    /// Returns the burn rate for the reward
    pub fn burn_rate(&self) -> BurnRate {
        const PERCENT_BASE_FEE_TO_BURN: u8 = config_value!("PERCENT_BASE_FEE_TO_BURN");

        BurnRate::new_unchecked(PERCENT_BASE_FEE_TO_BURN)
    }

    /// Verifies the provided proof, returning its underlying storage value, if present.
    pub fn verify_proof(
        &self,
        state_root: <S::Storage as Storage>::Root,
        proof: StorageProof<<S::Storage as Storage>::Proof>,
        expected_key: &S::Address,
    ) -> Result<Option<SlotValue>, anyhow::Error> {
        let (storage_key, storage_value) = S::Storage::open_proof(state_root, proof)?;
        let prefix = self.bonded_attesters.prefix();
        let codec = self.bonded_attesters.codec();

        // We have to check that the storage key is the same as the external key
        ensure!(
            storage_key == SlotKey::new(prefix, expected_key, codec),
            "The storage key from the proof doesn't match the expected storage key."
        );

        Ok(storage_value)
    }

    /// A helper function that simply slashes an attester and returns a reward value
    fn slash_user<TxStateAccessor: TxState<S>>(
        &self,
        user: &S::Address,
        role: Role,
        reason: SlashingReason,
        state: &mut TxStateAccessor,
    ) -> Result<u64, <TxStateAccessor as StateWriter<User>>::Error> {
        let bonded_set = match role {
            Role::Attester => {
                // We have to remove the attester from the unbonding set
                // to prevent him from skipping the first phase
                // unbonding if he bonds himself again.
                self.unbonding_attesters.remove(user, state)?;

                &self.bonded_attesters
            }
            Role::Challenger => &self.bonded_challengers,
        };

        // We have to deplete the attester's bonded account, it amounts to removing the attester from the bonded set
        let reward = bonded_set.get(user, state)?.unwrap_or_default();
        bonded_set.remove(user, state)?;

        // We raise an event
        self.emit_event(
            state,
            Event::<S>::UserSlashed {
                address: user.clone(),
                reason,
            },
        );

        Ok(reward)
    }

    fn slash_burn_reward(
        &self,
        user: &S::Address,
        role: Role,
        reason: SlashingReason,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        if let Err(e) = self.slash_user(user, role, reason, state) {
            error!(
                error = ?e,
                "Error raised when trying to slash the attester. Attester not slashed and transaction reverted"
            );
            return Err(e.into());
        };

        self.emit_event(
            state,
            Event::UserSlashed {
                address: user.clone(),
                reason,
            },
        );

        error!(
            error = ?reason,
            ?role,
            "The user was slashed");

        Ok(CallResponse::default())
    }

    /// A helper function that is used to slash an attester, and put the associated attestation in the slashed pool
    fn slash_and_invalidate_attestation<TxStateAccessor: TxState<S>>(
        &self,
        attester: &S::Address,
        height: TransitionHeight,
        reason: SlashingReason,
        state: &mut TxStateAccessor,
    ) -> Result<AttesterIncentiveErrors, <TxStateAccessor as StateWriter<User>>::Error> {
        let reward = self.slash_user(attester, Role::Attester, reason, state)?;

        let curr_reward_value = self
            .bad_transition_pool
            .get(&height, state)?
            .unwrap_or_default();

        let new_value = curr_reward_value.saturating_add(reward);
        self.bad_transition_pool.set(&height, &new_value, state)?;

        Ok(AttesterIncentiveErrors::UserSlashed(reason))
    }

    /// A helper function that rewards the sender with a given amount of tokens
    /// Some of the tokens need to be burnt to avoid the system participants to be incentivized to prove and submit empty blocks.
    fn reward_sender(
        &self,
        context: &Context<S>,
        amount: u64,
        state: &mut impl StateAccessor,
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        self.transfer_tokens_to_sender(
            context,
            // Note: if we have an empty block, the attester will pay more than the reward (because of the transaction cost)
            self.burn_rate().apply(amount),
            state,
        )
    }

    fn transfer_tokens_to_sender(
        &self,
        context: &Context<S>,
        amount: u64,
        state: &mut impl StateAccessor,
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        let coins = Coins {
            token_id: GAS_TOKEN_ID,
            amount,
        };

        // The reward tokens are unlocked from the module's id.
        self.bank
            .transfer_from(self.id.to_payable(), context.sender(), coins, state)
            .map_err(|_err| AttesterIncentiveErrors::RewardTransferFailure)?;

        Ok(CallResponse::default())
    }

    /// A helper function for the `bond_challenger/attester` call. Also used to bond challengers/attesters
    /// during genesis when no context is available.
    pub(super) fn bond_user_helper(
        &self,
        bond_amount: u64,
        user_address: &S::Address,
        role: Role,
        state: &mut (impl StateAccessor + EventContainer),
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        // If the user is an attester, we have to check that he's not trying to unbond
        if role == Role::Attester
            && self
                .unbonding_attesters
                .get(user_address, state)
                .map_err(|e| AttesterIncentiveErrors::StateAccessError(e.to_string()))?
                .is_some()
        {
            return Err(AttesterIncentiveErrors::AttesterIsUnbonding);
        }

        // Transfer the bond amount from the sender to the module's id.
        // On failure, no state is changed
        let coins = Coins {
            token_id: GAS_TOKEN_ID,
            amount: bond_amount,
        };

        self.bank
            .transfer_from(user_address, self.id.to_payable(), coins, state)
            .map_err(|_err| AttesterIncentiveErrors::BondTransferFailure)?;

        let balances = match role {
            Role::Attester => &self.bonded_attesters,
            Role::Challenger => &self.bonded_challengers,
        };

        // Update our record of the total bonded amount for the sender.
        // This update is infallible, so no value can be destroyed.
        let old_balance = balances
            .get(user_address, state)
            .map_err(|e| AttesterIncentiveErrors::StateAccessError(e.to_string()))?
            .unwrap_or_default();
        let total_balance = old_balance
            .checked_add(bond_amount)
            .with_context(|| {
                anyhow::anyhow!("The total balance overflows with the given operation")
            })
            .map_err(|_| AttesterIncentiveErrors::BondTransferFailure)?;
        balances
            .set(user_address, &total_balance, state)
            .map_err(|e| AttesterIncentiveErrors::StateAccessError(e.to_string()))?;

        // Emit the bonding event
        match role {
            Role::Attester => self.emit_event(
                state,
                Event::<S>::BondedAttester {
                    new_deposit: bond_amount,
                    total_bond: total_balance,
                },
            ),
            Role::Challenger => self.emit_event(
                state,
                Event::<S>::BondedChallenger {
                    new_deposit: bond_amount,
                    total_bond: total_balance,
                },
            ),
        }

        Ok(CallResponse::default())
    }

    /// Try to unbond the requested amount of coins with context.sender() as the beneficiary.
    pub(crate) fn unbond_challenger(
        &self,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> anyhow::Result<CallResponse> {
        // Get the user's old balance.
        if let Some(old_balance) = self.bonded_challengers.get(context.sender(), state)? {
            // Transfer the bond amount from the sender to the module's id.
            // On failure, no state is changed
            self.transfer_tokens_to_sender(context, old_balance, state)?;

            // Emit the unbonding event
            self.emit_event(
                state,
                Event::<S>::UnbondedChallenger {
                    amount_withdrawn: old_balance,
                },
            );
        }

        Ok(CallResponse::default())
    }

    /// The attester starts the first phase of the two-phase unbonding.
    /// We put the current max finalized height with the attester address
    /// in the set of unbonding attesters if the attester
    /// is already present in the unbonding set
    pub(crate) fn begin_unbond_attester(
        &self,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        // First get the bonded attester
        if let Some(bond) = self.bonded_attesters.get(context.sender(), state)? {
            let finalized_height = self
                .light_client_finalized_height
                .get(state)?
                .expect("Must be set at genesis");

            // Remove the attester from the bonding set
            self.bonded_attesters.remove(context.sender(), state)?;

            // Then add the bonded attester to the unbonding set, with the current finalized height
            self.unbonding_attesters.set(
                context.sender(),
                &UnbondingInfo {
                    unbonding_initiated_height: finalized_height,
                    amount: bond,
                },
                state,
            )?;
        }

        Ok(CallResponse::default())
    }

    pub(crate) fn end_unbond_attester(
        &self,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        // We have to ensure that the attester is unbonding, and that the unbonding transaction
        // occurred at least `finality_period` blocks ago to let the attester unbond
        if let Some(unbonding_info) = self
            .unbonding_attesters
            .get(context.sender(), state)
            .map_err(|e| AttesterIncentiveErrors::StateAccessError(e.to_string()))?
        {
            // These two constants should always be set beforehand, hence we can panic if they're not set
            let curr_height = self
                .light_client_finalized_height
                .get(state)
                .map_err(|e| AttesterIncentiveErrors::StateAccessError(e.to_string()))?
                .expect("Should be defined at genesis");
            let finality_period = self
                .rollup_finality_period
                .get(state)
                .map_err(|e| AttesterIncentiveErrors::StateAccessError(e.to_string()))?
                .expect("Should be defined at genesis");

            if unbonding_info
                .unbonding_initiated_height
                .saturating_add(finality_period)
                > curr_height
            {
                return Err(AttesterIncentiveErrors::UnbondingNotFinalized);
            }

            // Get the user's old balance.
            // Transfer the bond amount from the sender to the module's id.
            // On failure, no state is changed
            self.transfer_tokens_to_sender(context, unbonding_info.amount, state)?;

            // Update our internal tracking of the total bonded amount for the sender.
            self.bonded_attesters.remove(context.sender(), state)?;
            self.unbonding_attesters.remove(context.sender(), state)?;

            self.emit_event(
                state,
                Event::<S>::UnbondedChallenger {
                    amount_withdrawn: unbonding_info.amount,
                },
            );
        } else {
            return Err(AttesterIncentiveErrors::AttesterIsNotUnbonding);
        }
        Ok(CallResponse::default())
    }

    /// The bonding proof is now a proof that an attester was bonded during the last `finality_period` range.
    /// The proof must refer to a valid state of the rollup. The initial root hash must represent a state between
    /// the bonding proof one and the current state.
    #[allow(clippy::type_complexity)]
    fn check_bonding_proof(
        &self,
        context: &Context<S>,
        attestation: &Attestation<
            Da,
            StorageProof<<S::Storage as Storage>::Proof>,
            <S::Storage as Storage>::Root,
        >,
        state: &mut impl TxState<S>,
    ) -> Result<(), AttesterIncentiveErrors> {
        let bonding_root = {
            // If we cannot get the transition before the current one, it means that we are trying
            // to get the genesis state root
            let transition_height = TransitionHeight::from(
                attestation
                    .proof_of_bond
                    .claimed_transition_num
                    .checked_sub(1)
                    .expect("The transition height should be greater than 1"),
            );

            if let Some(transition) = self
                .chain_state
                .get_historical_transitions(transition_height, state)?
            {
                transition.post_state_root().clone()
            } else {
                self.chain_state
                    .get_genesis_hash(state)?
                    .expect("The genesis hash should be set at genesis")
            }
        };

        // This proof checks that the attester was bonded at the given transition num
        let bond_opt = self
            .verify_proof(
                bonding_root,
                attestation.proof_of_bond.proof.clone(),
                context.sender(),
            )
            .map_err(|_err| AttesterIncentiveErrors::InvalidBondingProof)?;

        let bond = bond_opt.ok_or(AttesterIncentiveErrors::UserNotBonded)?;
        let bond: u64 = BorshDeserialize::deserialize(&mut bond.value())
            .map_err(|_err| AttesterIncentiveErrors::InvalidBondFormat)?;

        let minimum_bond = self
            .minimum_attester_bond
            .get_or_err(state)?
            .expect("The minimum bond should be set at genesis");

        // We then have to check that the bond was greater than the minimum bond
        if bond < minimum_bond {
            return Err(AttesterIncentiveErrors::UserNotBonded);
        }

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn check_transition(
        &self,
        claimed_transition_height: TransitionHeight,
        attester: &S::Address,
        attestation: &Attestation<
            Da,
            StorageProof<<S::Storage as Storage>::Proof>,
            <S::Storage as Storage>::Root,
        >,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse, AttesterIncentiveErrors> {
        if let Some(curr_tx) = self
            .chain_state
            .get_historical_transitions(claimed_transition_height, state)?
        {
            // We first need to compare the initial block hash to the previous post state root
            if !curr_tx.compare_hashes(&attestation.slot_hash, &attestation.post_state_root) {
                debug!(
                    claimed_transition_height,
                    attestation_slot_hash = ?attestation.slot_hash,
                    attestation_post_state = ?attestation.post_state_root,
                    curr_tx_slot_hash = ?curr_tx.slot_hash(),
                    curr_tx_state_root = ?curr_tx.post_state_root(),
                    "The attestation has an invalid block hash or post state root");
                // Check if the attestation has the same slot_hash and post_state_root as the actual transition
                // that we found in state. If not, slash the attester.
                // If so, the attestation is valid, so return Ok
                match self.slash_and_invalidate_attestation(
                    attester,
                    claimed_transition_height,
                    SlashingReason::TransitionInvalid,
                    state,
                ) {
                    Err(e) => {
                        error!(
                            error = ?e,
                            "An error occurred while slashing the attester. Attester not slashed and transaction reverted");

                        return Err(e.into());
                    }

                    Ok(e) => {
                        self.emit_event(
                            state,
                            Event::UserSlashed {
                                address: attester.clone(),
                                reason: SlashingReason::TransitionInvalid,
                            },
                        );

                        return Err(e);
                    }
                }
            }
            Ok(CallResponse::default())
        } else {
            // Case where we cannot get the transition from the chain state historical transitions.
            self.slash_burn_reward(
                attester,
                Role::Attester,
                SlashingReason::TransitionNotFound,
                state,
            )
        }
    }

    #[allow(clippy::type_complexity)]
    fn check_initial_hash(
        &self,
        claimed_transition_height: TransitionHeight,
        attester: &S::Address,
        attestation: &Attestation<
            Da,
            StorageProof<<S::Storage as Storage>::Proof>,
            <S::Storage as Storage>::Root,
        >,
        state: &mut impl TxState<S>,
    ) -> anyhow::Result<CallResponse, AttesterIncentiveErrors> {
        // Normal state
        if let Some(transition) = self
            .chain_state
            .get_historical_transitions(claimed_transition_height.saturating_sub(1), state)?
        {
            if transition.post_state_root() != &attestation.initial_state_root {
                // The initial root hashes don't match, just slash the attester
                return self.slash_burn_reward(
                    attester,
                    Role::Attester,
                    SlashingReason::InvalidInitialHash,
                    state,
                );
            }
        } else {
            // Genesis state
            // We can assume that the genesis hash is always set, otherwise we need to panic.
            // We don't need to prove that the attester was bonded, simply need to check that the current bond is higher than the
            // minimal bond and that the attester is not unbonding

            // We add a check here that the claimed transition height is the same as the genesis height.
            let genesis_height = 0;
            let previous = claimed_transition_height
                .checked_sub(1)
                .expect("Transition height must be > 0");
            if genesis_height != previous {
                return self.slash_burn_reward(
                    attester,
                    Role::Attester,
                    SlashingReason::TransitionNotFound,
                    state,
                );
            }

            if self
                .chain_state
                .get_genesis_hash(state)?
                .expect("The initial hash should be set")
                != attestation.initial_state_root
            {
                // Slash the attester, and burn the fees
                return self.slash_burn_reward(
                    attester,
                    Role::Attester,
                    SlashingReason::InvalidInitialHash,
                    state,
                );
            }

            // Normal state
        }

        Ok(CallResponse::default())
    }

    /// Try to process an attestation if the attester is bonded.
    /// This function returns an error (hence ignores the transaction) when the attester is not bonded
    /// or when the module is unable to verify the bonding proof.
    #[allow(clippy::type_complexity)]
    pub(crate) fn process_attestation(
        &self,
        context: &Context<S>,
        attestation: WrappedAttestation<
            Da,
            StorageProof<<S::Storage as Storage>::Proof>,
            <S::Storage as Storage>::Root,
        >,
        state: &mut impl TxState<S>,
    ) -> anyhow::Result<CallResponse, AttesterIncentiveErrors> {
        let attestation = attestation.inner;
        // We first need to check that the attester is still in the bonding set
        if self
            .bonded_attesters
            .get(context.sender(), state)?
            .is_none()
        {
            return Err(AttesterIncentiveErrors::UserNotBonded);
        }

        // If the bonding proof in the attestation is invalid, light clients will ignore the attestation. In that case, we should too.
        self.check_bonding_proof(context, &attestation, state)?;

        // We suppose that these values are always defined, otherwise we panic
        let last_attested_height = self
            .maximum_attested_height
            .get(state)?
            .expect("The maximum attested height should be set at genesis");
        let current_finalized_height = self
            .light_client_finalized_height
            .get(state)?
            .expect("The light client finalized height should be set at genesis");
        let finality = self
            .rollup_finality_period
            .get(state)?
            .expect("The rollup finality period should be set at genesis");

        assert!(
            current_finalized_height <= last_attested_height,
            "The last attested height should always be below the current finalized height."
        );

        // Update the max_attested_height in case the blocks have already been finalized
        let new_height_to_attest = last_attested_height
            .checked_add(1)
            .expect("reached end of the chain");

        // Minimum height at which the proof of bond can be valid
        let min_height = new_height_to_attest.saturating_sub(finality);

        // We have to check the following order invariant is respected:
        // (height to attest - finality) <= bonding_proof.transition_num <= height to attest
        //
        // Which with our variable gives:
        // min_height <= bonding_proof.transition_num <= new_height_to_attest
        // If this invariant is respected, we can be sure that the attester was bonded at new_height_to_attest.
        if !(min_height <= attestation.proof_of_bond.claimed_transition_num
            && attestation.proof_of_bond.claimed_transition_num <= new_height_to_attest)
        {
            return Err(AttesterIncentiveErrors::InvalidTransitionInvariant);
        }

        // From this point below, the attester has been correctly authenticated -
        // any error constitutes a slashable offense which *needs to be reflected in the state*.
        // Hence we don't want to return an error after this point, but rather slash the attester and exit gracefully.

        // First compare the initial hashes
        if let Err(err) = self.check_initial_hash(
            attestation.proof_of_bond.claimed_transition_num,
            context.sender(),
            &attestation,
            state,
        ) {
            error!(
                error = ?err,
                ?attestation,
                "Error raised when checking initial hashes for attestation");
            return Ok(CallResponse::default());
        }

        // Then compare the transition
        if let Err(err) = self.check_transition(
            attestation.proof_of_bond.claimed_transition_num,
            context.sender(),
            &attestation,
            state,
        ) {
            error!(
                error = ?err,
                ?attestation,
                "Error raised when checking the transition for attestation");
            return Ok(CallResponse::default());
        }

        self.emit_event(
            state,
            Event::<S>::ProcessedValidAttestation {
                attester: context.sender().clone(),
            },
        );

        // Now we have to check whether the claimed_transition_num is the max_attested_height.
        // If so, update the maximum attested height and reward the sender
        if attestation.proof_of_bond.claimed_transition_num == new_height_to_attest {
            // We reward the attester with the amount of gas used for the transition.
            let transition = self
                .chain_state
                .get_historical_transitions(new_height_to_attest, state)?
                .expect("The transition should exist. The check has been done above");

            let reward = transition.gas_used().value(transition.gas_price());

            // Update the maximum attested height
            self.maximum_attested_height
                .set(&(new_height_to_attest), state)?;

            self.reward_sender(context, reward, state)?;
        }

        // Then we can optimistically process the transaction
        Ok(CallResponse::default())
    }

    fn check_challenge_outputs_against_transition(
        &self,
        public_outputs: StateTransitionPublicData<S::Address, Da, <S::Storage as Storage>::Root>,
        height: &TransitionHeight,
        state: &mut impl TxState<S>,
    ) -> anyhow::Result<(), AttesterIncentiveErrors> {
        let transition = self
            .chain_state
            .get_historical_transitions(*height, state)?
            .ok_or(SlashingReason::TransitionInvalid)?;

        let initial_hash = {
            if let Some(prev_transition) = self
                .chain_state
                .get_historical_transitions(height.saturating_sub(1), state)?
            {
                prev_transition.post_state_root().clone()
            } else {
                self.chain_state
                    .get_genesis_hash(state)?
                    .expect("The genesis hash should be set")
            }
        };

        if public_outputs.initial_state_root != initial_hash {
            return Err(SlashingReason::InvalidInitialHash.into());
        }

        if &public_outputs.slot_hash != transition.slot_hash() {
            return Err(SlashingReason::TransitionInvalid.into());
        }

        if public_outputs.validity_condition != *transition.validity_condition() {
            return Err(SlashingReason::TransitionInvalid.into());
        }

        Ok(())
    }

    /// Try to process a zk proof if the challenger is bonded.
    /// Same comment as above for the [`AttesterIncentives::process_attestation`] method: if we have a slashable
    /// offense, we want to be able to exit gracefully.
    pub(crate) fn process_challenge(
        &self,
        context: &Context<S>,
        proof: &[u8],
        transition_num: &TransitionHeight,
        state: &mut impl TxState<S>,
    ) -> anyhow::Result<CallResponse, AttesterIncentiveErrors> {
        // Get the challenger's old balance.
        // Revert if they aren't bonded
        let old_balance = self
            .bonded_challengers
            .get_or_err(context.sender(), state)?
            .map_err(|_| AttesterIncentiveErrors::UserNotBonded)?;

        // Check that the challenger has enough balance to process the proof.
        let minimum_bond = self
            .minimum_challenger_bond
            .get(state)?
            .expect("Should be set at genesis");

        if old_balance < minimum_bond {
            return Err(AttesterIncentiveErrors::UserNotBonded);
        }

        let code_commitment = self
            .chain_state
            .inner_code_commitment(state)?
            .expect("Should be set at genesis");

        // Find the faulty attestation pool and get the associated reward
        let attestation_reward: u64 =
            match self.bad_transition_pool.get_or_err(transition_num, state)? {
                Ok(reward) => reward,
                Err(_err) => {
                    return self.slash_burn_reward(
                        context.sender(),
                        Role::Challenger,
                        SlashingReason::NoInvalidTransition,
                        state,
                    );
                }
            };

        let public_outputs_opt = <S::InnerZkvm as Zkvm>::verify::<
            StateTransitionPublicData<S::Address, Da, <S::Storage as Storage>::Root>,
        >(proof, &code_commitment)
        .map_err(|e| anyhow::format_err!("{:?}", e));

        // Don't return an error for invalid proofs - those are expected and shouldn't cause reverts.
        match public_outputs_opt {
            Ok(public_output) => {
                // We have to perform the checks to ensure that the challenge is valid while the attestation isn't.
                if let Err(err) = self.check_challenge_outputs_against_transition(
                    public_output,
                    transition_num,
                    state,
                ) {
                    if let AttesterIncentiveErrors::UserSlashed(err) = err {
                        return self.slash_burn_reward(
                            context.sender(),
                            Role::Challenger,
                            err,
                            state,
                        );
                    }

                    return Err(err);
                };

                // Reward the sender
                self.reward_sender(context, attestation_reward, state)?;

                // Now remove the bad transition from the pool
                self.bad_transition_pool.remove(transition_num, state)?;

                self.emit_event(
                    state,
                    Event::<S>::ProcessedValidProof {
                        challenger: context.sender().clone(),
                    },
                );
            }
            Err(_err) => {
                // Slash the challenger
                return self.slash_burn_reward(
                    context.sender(),
                    Role::Challenger,
                    SlashingReason::InvalidProofOutputs,
                    state,
                );
            }
        }

        Ok(CallResponse::default())
    }
}
