//! Defines types that are related to the `AggregatedProof`.
use alloc::vec::Vec;
use core::marker::PhantomData;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use super::Zkvm;

type SerializedValidityCondition = Vec<u8>;
type SerializedAddress = Vec<u8>;

/// Aggregated proof code commitment.
#[derive(
    Debug, Eq, PartialEq, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Default,
)]
pub struct CodeCommitment(pub Vec<u8>);

impl core::fmt::Display for CodeCommitment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0.is_empty() {
            return write!(f, "CodeCommitment([])");
        }
        write!(f, "CodeCommitment(0x{})", hex::encode(&self.0))
    }
}

/// Public data of an aggregated proof.
#[derive(Debug, Eq, PartialEq, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
pub struct AggregatedProofPublicData {
    /// Contains the validity conditions for each block in the aggregated proof.
    pub validity_conditions: Vec<SerializedValidityCondition>,
    /// Initial slot number.
    pub initial_slot_number: u64,
    /// Final slot number.
    pub final_slot_number: u64,
    /// The genesis state root of the aggregated proof.
    pub genesis_state_root: Vec<u8>,
    /// The initial state root of the aggregated proof.
    pub initial_state_root: Vec<u8>,
    /// The final state root of the aggregated proof.
    pub final_state_root: Vec<u8>,
    /// The initial slot hash of the aggregated proof.
    pub initial_slot_hash: Vec<u8>,
    /// The final slot hash of the aggregated proof.
    pub final_slot_hash: Vec<u8>,
    /// Code Commitment of the aggregated proof circuit.
    pub code_commitment: CodeCommitment,
    /// These are the addresses of the provers who proved individual blocks.
    pub rewarded_addresses: Vec<SerializedAddress>,
}

impl core::fmt::Display for AggregatedProofPublicData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "AggregatedProofPublicData(initial_slot_number: {}, final_slot_number: {}, genesis_state_root: {}, initial_state_root: 0x{}, final_state_root: 0x{}, initial_slot_hash: 0x{}, final_slot_hash: 0x{}, code_commitment: {})",
            self.initial_slot_number,
            self.final_slot_number,
            hex::encode(&self.genesis_state_root),
            hex::encode(&self.initial_state_root),
            hex::encode(&self.final_state_root),
            hex::encode(&self.initial_slot_hash),
            hex::encode(&self.final_slot_hash),
            self.code_commitment
        )
    }
}

/// Represents an aggregated proof with the public input.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone, BorshDeserialize, BorshSerialize)]
pub struct AggregatedProof {
    pub(crate) serialized_proof: SerializedAggregatedProof,
    pub(crate) public_data: AggregatedProofPublicData,
}

impl AggregatedProof {
    /// Creates AggregatedProofData
    pub fn new(
        serialized_proof: SerializedAggregatedProof,
        public_data: AggregatedProofPublicData,
    ) -> Self {
        Self {
            serialized_proof,
            public_data,
        }
    }

    /// Public input of the aggregated proof.
    pub fn public_data(&self) -> &AggregatedProofPublicData {
        &self.public_data
    }

    /// The serialized proof raw bytes.
    pub fn serialized_proof(&self) -> &[u8] {
        &self.serialized_proof.raw_aggregated_proof
    }
}

/// Represents a serialized aggregated proof.
#[derive(Debug, Eq, PartialEq, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
pub struct SerializedAggregatedProof {
    /// Serialized proof.
    pub raw_aggregated_proof: Vec<u8>,
}

/// Validates an Aggregated Proof.
pub struct AggregateProofVerifier<Vm: Zkvm> {
    _vm: PhantomData<Vm>,
    outer_proof_code_commitment: Vm::CodeCommitment,
}

impl<Vm: Zkvm> AggregateProofVerifier<Vm> {
    /// Creates a new `AggregateProofVerifier`.
    pub fn new(outer_proof_code_commitment: Vm::CodeCommitment) -> Self {
        Self {
            _vm: PhantomData,
            outer_proof_code_commitment,
        }
    }

    /// Verifies whether an [`AggregatedProof`] contains a valid proof.
    pub fn verify(&self, proof_data: &AggregatedProof) -> Result<(), Vm::Error> {
        let public_data = Vm::verify::<AggregatedProofPublicData>(
            proof_data.serialized_proof.raw_aggregated_proof.as_slice(),
            &self.outer_proof_code_commitment,
        )?;

        assert_eq!(public_data, proof_data.public_data);
        Ok(())
    }
}
