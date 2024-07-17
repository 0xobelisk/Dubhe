#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
/// Reasons for slashing a prover
pub enum SlashingReason {
    /// The proof is not a valid zk-proof - ie the verifier did not accept the proof.
    ProofInvalid,

    /// The genesis hash supplied is incorrect
    IncorrectGenesisHash,

    /// The initial state root contained in the [`sov_modules_api::AggregatedStateTransition`] outputs is incorrect
    IncorrectInitialStateRoot,

    /// The initial transition slot contained in the [`sov_modules_api::AggregatedStateTransition`] has no associated transition
    /// in the chain state module.
    InitialTransitionDoesNotExist,

    /// The initial slot hash contained in the [`sov_modules_api::AggregatedStateTransition`] outputs is incorrect
    IncorrectInitialSlotHash,

    /// The final transition slot contained in the [`sov_modules_api::AggregatedStateTransition`] has no associated transition
    /// in the chain state module.
    FinalTransitionDoesNotExist,

    /// The final state root contained in the [`sov_modules_api::AggregatedStateTransition`] outputs is incorrect
    IncorrectFinalStateRoot,

    /// The final slot hash contained in the [`sov_modules_api::AggregatedStateTransition`] outputs is incorrect
    IncorrectFinalSlotHash,

    /// The initial slot hash contained in the [`sov_modules_api::AggregatedStateTransition`] outputs is incorrect
    IncorrectValidityConditions,
}

#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
/// The reasons for penalizing a prover
pub enum PenalizationReason {
    /// We penalize the prover for submitting a proof for transitions that have already been processed
    ProofAlreadyProcessed,
}

#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
/// Events for prover incentives
pub enum Event<S: sov_modules_api::Spec> {
    /// The prover has been bonded. The deposit is the amount of the bond and the total balance is the total amount staked.
    BondedProver {
        prover: S::Address,
        deposit: u64,
        total_balance: u64,
    },
    /// The prover has been unbonded. The amount withdrawn is the amount of the bond that was withdrawn.
    UnBondedProver {
        prover: S::Address,
        amount_withdrawn: u64,
    },
    /// The prover has been slashed. The reason describes why the prover was slashed.
    ProverSlashed {
        prover: S::Address,
        reason: SlashingReason,
    },
    /// The prover has been penalized (fined). The reason describes why the prover was fined.
    ProverPenalized {
        prover: S::Address,
        amount: u64,
        reason: PenalizationReason,
    },
    /// Event for processing a valid proof
    ProcessedValidProof { prover: S::Address, reward: u64 },
}
