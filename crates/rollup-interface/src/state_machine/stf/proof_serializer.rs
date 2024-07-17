use crate::zk::aggregated_proof::SerializedAggregatedProof;

/// Serialize proof blob and adds metadata needed for verification.
pub trait ProofSerializer: Send + Sync {
    /// New ProofSerializer
    fn new() -> Self
    where
        Self: Sized;

    /// Creates a proof blob with metadata needed for verification.
    fn serialize_proof_blob_with_metadata(
        &self,
        serialized_proof: SerializedAggregatedProof,
    ) -> anyhow::Result<Vec<u8>>;
}
