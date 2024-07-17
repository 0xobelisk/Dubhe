use crate::SlashingReason;

/// Events for attester incentives
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub enum Event<S: sov_modules_api::Spec> {
    /// Event for User Slashed
    UserSlashed {
        /// The address of the user who was slashed.
        address: S::Address,
        /// The reason the user was slashed.
        reason: SlashingReason,
    },
    /// Event for a new deposit
    BondedAttester {
        /// The amount of tokens deposited by this call.
        new_deposit: u64,
        /// The total bond of the attester after succesfully processing the call.
        total_bond: u64,
    },
    /// Event for a new deposit.
    BondedChallenger {
        /// The amount of tokens deposited by this call.
        new_deposit: u64,
        /// The total bond of the challenger after this call.
        total_bond: u64,
    },
    /// Event for a new deposit
    NewDeposit {
        /// The amount of tokens deposited by this call.
        new_deposit: u64,
        /// The total bond of the challenger after this call.
        total_bond: u64,
    },
    /// Event for Unbonding
    UnbondedChallenger {
        /// The number of tokens returned to the caller's bank balance.
        amount_withdrawn: u64,
    },
    /// Event for processing a valid attestation
    ProcessedValidAttestation {
        /// The address of the attester.
        attester: S::Address,
    },
    /// Event for processing a valid proof
    ProcessedValidProof {
        /// The address of the challenger.
        challenger: S::Address,
    },
}
