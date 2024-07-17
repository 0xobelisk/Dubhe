//! The da module defines traits used by the full node to interact with the DA layer.

use alloc::vec::Vec;
use core::fmt::{Debug, Display};

#[cfg(feature = "native")]
use backon::Retryable;
use backon::{BackoffBuilder, ExponentialBuilder};
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
#[cfg(feature = "native")]
use tracing::error;

use crate::da::{BlockHeaderTrait, RelevantBlobs, RelevantProofs};
#[cfg(feature = "native")]
use crate::da::{DaSpec, DaVerifier};
use crate::zk::ValidityCondition;

/// Perform a checked arithmetic, returning None if the result is invalid.
pub trait CheckedMath<Rhs = Self> {
    /// The output type of arithmetic operations
    type Output;

    /// Performs checked multiplication, returning None if the result would overflow.
    fn checked_mul(&self, rhs: Rhs) -> Option<Self::Output>;
    /// Performs checked division, returning None if the caller attempts divide by zero or if the calculation overflows.
    fn checked_div(&self, rhs: Rhs) -> Option<Self::Output>;
    /// Performs checked subtraction, returning None if the result would underflow.
    fn checked_sub(&self, rhs: Rhs) -> Option<Self::Output>;
    /// Performs checked addition, returning None if the result would overflow.
    fn checked_add(&self, rhs: Rhs) -> Option<Self::Output>;
}

macro_rules! impl_checked_math_primitive {
    ($($t:ty),*) => {
        $(impl CheckedMath for $t {
            type Output = Self;

            fn checked_mul(&self, rhs: Self) -> Option<Self::Output> {
                <$t>::checked_mul(*self, rhs)
            }

            fn checked_div(&self, rhs: Self) -> Option<Self::Output> {
                <$t>::checked_div(*self, rhs)
            }

            fn checked_sub(&self, rhs: Self) -> Option<Self::Output> {
                <$t>::checked_sub(*self, rhs)
            }

            fn checked_add(&self, rhs: Self) -> Option<Self::Output> {
                <$t>::checked_add(*self, rhs)
            }
        })*
    };
}

impl_checked_math_primitive!(u8, u16, u32, u64, u128, usize);
impl_checked_math_primitive!(i8, i16, i32, i64, i128, isize);

/// The fee on a blockchain. This is usually expressed as a combination of a gas limit
/// and a fee rate (tokens per gas).
pub trait Fee: Copy + Send {
    /// The price per unit of gas.
    type FeeRate: CheckedMath + CheckedMath<u64> + Clone + Send + Sync;

    /// Returns the price per unit of gas.
    fn fee_rate(&self) -> Self::FeeRate;

    /// Updates the price per unit of gas.
    fn set_fee_rate(&mut self, rate: Self::FeeRate);

    /// The amount of gas that the transaction is expected to consume.
    /// Multiplying this quantity by the fee rate gives the total fee.
    /// for the transaction
    fn gas_estimate(&self) -> u64;
}

/// The [`MaybeRetryable`] enum can be returned from a fallible function to
/// determine whether it can re-attempted or not.
#[derive(Debug, thiserror::Error)]
pub enum MaybeRetryable<E> {
    /// This error is a permanent one and thus the function that
    /// raised it should not be retried.
    #[error("{0}")]
    Permanent(E),
    /// This error is transient and thus the function that raised
    /// ought to be retried.
    #[error("{0}")]
    Transient(E),
}

impl<E: std::fmt::Display> MaybeRetryable<E> {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient(_))
    }

    fn into_err(self) -> E {
        match self {
            Self::Permanent(e) | Self::Transient(e) => e,
        }
    }
}

/// The default error for `MaybeRetryable` is assumed to be permanent
/// rather than something that can be retried.
impl<E> From<E> for MaybeRetryable<E> {
    fn from(e: E) -> MaybeRetryable<E> {
        Self::Permanent(e)
    }
}

/// A DaService is the local side of an RPC connection talking to a node of the DA layer
/// It is *not* part of the logic that is zk-proven.
///
/// The DaService has two responsibilities - fetching data from the DA layer, transforming the
/// data into a representation that can be efficiently verified in circuit.
#[cfg(feature = "native")]
#[async_trait::async_trait]
pub trait DaService: Send + Sync + 'static {
    /// A handle to the types used by the DA layer.
    type Spec: DaSpec;

    /// The verifier for this DA layer.
    type Verifier: DaVerifier<Spec = Self::Spec> + Clone;

    /// A DA layer block, possibly excluding some irrelevant information.
    type FilteredBlock: SlotData<
        BlockHeader = <Self::Spec as DaSpec>::BlockHeader,
        Cond = <Self::Spec as DaSpec>::ValidityCondition,
    >;

    /// Type that allow to consume [`futures::Stream`] of BlockHeaders.
    type HeaderStream: futures::Stream<Item = Result<<Self::Spec as DaSpec>::BlockHeader, Self::Error>>
        + Send;

    /// A transaction ID, used to identify the transaction in the DA layer.
    type TransactionId: PartialEq + Eq + PartialOrd + Ord + core::hash::Hash;

    /// The error type for fallible methods.
    type Error: Debug + Send + Sync + Display;

    /// The fee type for the DA layer.
    type Fee: Fee;

    /// Fetch the block at the given height, waiting for one to be mined if necessary.
    /// The returned block may not be final, and can be reverted without a consensus violation.
    /// Call it for the same height are allowed to return different results.
    /// Should always returns the block at that height on the best fork.
    async fn get_block_at(&self, height: u64) -> Result<Self::FilteredBlock, Self::Error>;

    /// Fetch the [`DaSpec::BlockHeader`] of the last finalized block.
    /// If there's no finalized block yet, it should return an error.
    async fn get_last_finalized_block_header(
        &self,
    ) -> Result<<Self::Spec as DaSpec>::BlockHeader, Self::Error>;

    /// Subscribe to finalized headers as they are finalized.
    /// Expect only to receive headers which were finalized after subscription
    /// Optimized version of `get_last_finalized_block_header`.
    async fn subscribe_finalized_header(&self) -> Result<Self::HeaderStream, Self::Error>;

    /// Fetch the head block of the most popular fork.
    ///
    /// More like utility method, to provide better user experience
    async fn get_head_block_header(
        &self,
    ) -> Result<<Self::Spec as DaSpec>::BlockHeader, Self::Error>;

    /// Extract the relevant transactions from a block. For example, this method might return
    /// all of the blob transactions from a set rollup namespaces on Celestia.
    fn extract_relevant_blobs(
        &self,
        block: &Self::FilteredBlock,
    ) -> RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>;

    /// Generate a proof that the relevant blob transactions have been extracted correctly from the DA layer
    /// block.
    async fn get_extraction_proof(
        &self,
        block: &Self::FilteredBlock,
        blobs: &RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>,
    ) -> RelevantProofs<
        <Self::Spec as DaSpec>::InclusionMultiProof,
        <Self::Spec as DaSpec>::CompletenessProof,
    >;

    /// Extract the relevant transactions from a block, along with a proof that the extraction has been done correctly.
    /// For example, this method might return all of the blob transactions in rollup's namespace on Celestia,
    /// together with a range proof against the root of the namespaced-merkle-tree, demonstrating that the entire
    /// rollup namespace has been covered.
    #[allow(clippy::type_complexity)]
    async fn extract_relevant_blobs_with_proof(
        &self,
        block: &Self::FilteredBlock,
    ) -> (
        RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>,
        RelevantProofs<
            <Self::Spec as DaSpec>::InclusionMultiProof,
            <Self::Spec as DaSpec>::CompletenessProof,
        >,
    ) {
        let relevant_blobs = self.extract_relevant_blobs(block);
        let relevant_proofs = self.get_extraction_proof(block, &relevant_blobs).await;
        (relevant_blobs, relevant_proofs)
    }

    /// Send a transaction directly to the DA layer.
    /// blob is the serialized and signed transaction.
    /// Returns transaction id if it was successfully sent.
    async fn send_transaction(
        &self,
        blob: &[u8],
        fee: Self::Fee,
    ) -> Result<Self::TransactionId, Self::Error>;

    /// Sends an aggregated ZK proofs to the DA layer.
    async fn send_aggregated_zk_proof(
        &self,
        aggregated_proof_data: &[u8],
        fee: Self::Fee,
    ) -> Result<Self::TransactionId, Self::Error>;

    /// Fetches all aggregated ZK proofs at a specified block height.
    async fn get_aggregated_proofs_at(&self, height: u64) -> Result<Vec<Vec<u8>>, Self::Error>;

    /// Estimates the appropriate fee for a blob with a given size
    async fn estimate_fee(&self, blob_size: usize) -> Result<Self::Fee, Self::Error>;
}

async fn run_maybe_retryable_async_fn_with_retries<F, Fut, T, E>(
    backoff_policy: &impl BackoffBuilder,
    fxn: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, MaybeRetryable<E>>>,
    E: std::fmt::Display,
{
    fxn.retry(backoff_policy)
        .when(MaybeRetryable::is_retryable)
        .await
        .map_err(MaybeRetryable::into_err)
}

/// A wrapper around a [`DaService`] adding retry logic based on the supplied backoff policy.
#[cfg(feature = "native")]
#[derive(Clone)]
pub struct DaServiceWithRetries<D> {
    da_service: D,
    // TODO (@gskapka) Eventually we want this to be generic so that other, non
    // exponential policies may be used.
    backoff_policy: ExponentialBuilder,
}

#[cfg(feature = "native")]
impl<D: DaService> DaServiceWithRetries<D> {
    /// Creates a wrapped [`DaService`]` where methods have retry logic enabled using
    /// the supplied exponential back-off policy.
    pub fn with_exponential_backoff(da_service: D, backoff_policy: ExponentialBuilder) -> Self {
        Self {
            da_service,
            backoff_policy,
        }
    }

    /// Creates a wrapped [`DaService`] with zero retry attempts and a short max delay.
    /// Useful for tests where the retry wrapper is required for a [`DaService`] but
    /// we don't want the temporal overhead of the actual retries (eg, testing a fallible
    /// function's fail path).
    pub fn new_fast(da_service: D) -> Self {
        let backoff_policy = ExponentialBuilder::default()
            .with_max_delay(std::time::Duration::from_secs(5))
            .with_max_times(0);
        Self {
            da_service,
            backoff_policy,
        }
    }

    /// Get a reference to the underlying [`DaService`]
    pub fn da_service(&self) -> &D {
        &self.da_service
    }

    /// Get a mutable reference to the underlying [`DaService`]
    pub fn da_service_mut(&mut self) -> &mut D {
        &mut self.da_service
    }
}

#[async_trait::async_trait]
impl<D, E> DaService for DaServiceWithRetries<D>
where
    D: DaService<Error = MaybeRetryable<E>>,
    D::Fee: Sync,
    E: Debug + Send + Sync + Display,
{
    type Error = E;
    type Spec = D::Spec;
    type Verifier = D::Verifier;
    type FilteredBlock = D::FilteredBlock;

    type HeaderStream =
        BoxStream<'static, Result<<Self::Spec as DaSpec>::BlockHeader, Self::Error>>;

    type TransactionId = D::TransactionId;
    type Fee = D::Fee;

    async fn get_block_at(&self, height: u64) -> Result<Self::FilteredBlock, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::get_block_at(&self.da_service, height)
        })
        .await
    }

    async fn send_transaction(
        &self,
        blob: &[u8],
        fee: D::Fee,
    ) -> Result<Self::TransactionId, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::send_transaction(&self.da_service, blob, fee)
        })
        .await
    }

    async fn send_aggregated_zk_proof(
        &self,
        aggregated_proof_data: &[u8],
        fee: D::Fee,
    ) -> Result<Self::TransactionId, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::send_aggregated_zk_proof(&self.da_service, aggregated_proof_data, fee)
        })
        .await
    }

    async fn get_aggregated_proofs_at(&self, height: u64) -> Result<Vec<Vec<u8>>, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::get_aggregated_proofs_at(&self.da_service, height)
        })
        .await
    }

    async fn estimate_fee(&self, blob_size: usize) -> Result<D::Fee, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::estimate_fee(&self.da_service, blob_size)
        })
        .await
    }

    async fn get_last_finalized_block_header(
        &self,
    ) -> Result<<D::Spec as DaSpec>::BlockHeader, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::get_last_finalized_block_header(&self.da_service)
        })
        .await
    }

    async fn subscribe_finalized_header(&self) -> Result<Self::HeaderStream, Self::Error> {
        Ok(D::subscribe_finalized_header(&self.da_service)
            .await
            .map_err(MaybeRetryable::into_err)?
            .map(|res| res.map_err(MaybeRetryable::into_err))
            .boxed())
    }

    async fn get_head_block_header(&self) -> Result<<D::Spec as DaSpec>::BlockHeader, Self::Error> {
        run_maybe_retryable_async_fn_with_retries(&self.backoff_policy, || {
            D::get_head_block_header(&self.da_service)
        })
        .await
    }

    fn extract_relevant_blobs(
        &self,
        block: &Self::FilteredBlock,
    ) -> RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction> {
        D::extract_relevant_blobs(&self.da_service, block)
    }

    async fn get_extraction_proof(
        &self,
        block: &Self::FilteredBlock,
        blobs: &RelevantBlobs<<Self::Spec as DaSpec>::BlobTransaction>,
    ) -> RelevantProofs<
        <Self::Spec as DaSpec>::InclusionMultiProof,
        <Self::Spec as DaSpec>::CompletenessProof,
    > {
        D::get_extraction_proof(&self.da_service, block, blobs).await
    }
}

/// `SlotData` is the subset of a DA layer block which is stored in the rollup's database.
/// At the very least, the rollup needs access to the hashes and headers of all DA layer blocks,
/// but rollup may choose to store partial (or full) block data as well.
pub trait SlotData:
    Serialize + DeserializeOwned + PartialEq + core::fmt::Debug + Clone + Send + Sync
{
    /// The header type for a DA layer block as viewed by the rollup. This need not be identical
    /// to the underlying rollup's header type, but it must be sufficient to reconstruct the block hash.
    ///
    /// For example, most fields of a Tendermint-based DA chain like Celestia are irrelevant to the rollup.
    /// For these fields, we only ever store their *serialized* representation in memory or on disk. Only a few special
    /// fields like `data_root` are stored in decoded form in the `CelestiaHeader` struct.
    type BlockHeader: BlockHeaderTrait;

    /// The validity condition associated with the slot data.
    type Cond: ValidityCondition;

    /// The canonical hash of the DA layer block.
    fn hash(&self) -> [u8; 32];
    /// The header of the DA layer block.
    fn header(&self) -> &Self::BlockHeader;
    /// Get the validity condition set associated with the slot
    fn validity_condition(&self) -> Self::Cond;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn should_run_async_fn_with_retries() {
        let error = "some error".to_string();
        let retry_counter = tokio::sync::Mutex::new(0);
        let max_retries = 3;
        let backoff_policy = ExponentialBuilder::default().with_max_times(max_retries);

        let r = run_maybe_retryable_async_fn_with_retries(&backoff_policy, || async {
            let mut count = retry_counter.lock().await;
            *count += 1;
            Result::<(), MaybeRetryable<String>>::Err(MaybeRetryable::Transient(error.clone()))
        })
        .await;

        assert_eq!(r, Err(error));
        assert_eq!(*retry_counter.lock().await, max_retries + 1); // NOTE: Because first attempt is not a retry.
    }
}
