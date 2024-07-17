use sov_modules_macros::config_value;
use sov_rollup_interface::stf::ProofSerializer;
use sov_rollup_interface::zk::aggregated_proof::SerializedAggregatedProof;

use crate::transaction::{PriorityFeeBips, TxDetails};
use crate::{BlobData, Spec};

/// Proof with metadata need for verification.
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct SerializeProofWithDetails<S: Spec> {
    pub proof: SerializedAggregatedProof,
    pub details: TxDetails<S>,
}

/// Adds metadata about gas & fees to the proof blob.
pub struct SovApiProofSerializer<S: Spec> {
    _phantom: std::marker::PhantomData<S>,
}

impl<S: Spec> ProofSerializer for SovApiProofSerializer<S> {
    fn new() -> Self
    where
        Self: Sized,
    {
        SovApiProofSerializer {
            _phantom: Default::default(),
        }
    }

    fn serialize_proof_blob_with_metadata(
        &self,
        serialized_proof: SerializedAggregatedProof,
    ) -> anyhow::Result<Vec<u8>> {
        let details = TxDetails::<S> {
            max_priority_fee_bips: PriorityFeeBips::ZERO,
            max_fee: 10_000_000,
            gas_limit: None,
            chain_id: config_value!("CHAIN_ID"),
        };

        let proof_with_details = SerializeProofWithDetails {
            proof: serialized_proof,
            details,
        };

        let serialized_proof_with_details = borsh::to_vec(&proof_with_details)?;

        let proof = BlobData::new_proof(serialized_proof_with_details);
        let serialized_proof = borsh::to_vec(&proof)?;
        Ok(serialized_proof)
    }
}
