use std::sync::Arc;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::de::DeserializeOwned;
use sov_rollup_interface::rpc::{BatchResponse, TxResponse};
use sov_rollup_interface::stf::{StoredEvent, TransactionReceipt, TxReceiptContents};

/// A cheaply cloneable bytes abstraction for use within the trust boundary of the node
/// (i.e. when interfacing with the database). Serializes and deserializes more efficiently,
/// than most bytes abstractions, but is vulnerable to out-of-memory attacks
/// when read from an untrusted source.
///
/// # Warning
/// Do not use this type when deserializing data from an untrusted source!!
#[derive(
    Clone, PartialEq, PartialOrd, Eq, Ord, Debug, Default, BorshDeserialize, BorshSerialize,
)]
#[cfg_attr(
    feature = "arbitrary",
    derive(proptest_derive::Arbitrary, arbitrary::Arbitrary)
)]
pub struct DbBytes(Arc<Vec<u8>>);

impl DbBytes {
    /// Create `DbBytes` from a `Vec<u8>`
    pub fn new(contents: Vec<u8>) -> Self {
        Self(Arc::new(contents))
    }
}

impl From<Vec<u8>> for DbBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(Arc::new(value))
    }
}

impl AsRef<[u8]> for DbBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// The "key" half of a key/value pair from accessory state.
///
/// See [`AccessoryDb`](crate::accessory_db::AccessoryDb) for more information.
pub type AccessoryKey = Vec<u8>;
/// The "value" half of a key/value pair from accessory state.
///
/// See [`AccessoryDb`](crate::accessory_db::AccessoryDb) for more information.
pub type AccessoryStateValue = Option<Vec<u8>>;

/// A hash stored in the database
pub type DbHash = [u8; 32];
/// The "value" half of a key/value pair from the JMT
pub type JmtValue = Option<Vec<u8>>;

/// The on-disk format of a slot. Specifies the batches contained in the slot
/// and the hash of the da block. TODO(@preston-evans98): add any additional data
/// required to reconstruct the da block proof.
#[derive(Debug, PartialEq, BorshDeserialize, BorshSerialize)]
#[cfg_attr(
    feature = "arbitrary",
    derive(proptest_derive::Arbitrary, ::arbitrary::Arbitrary)
)]
pub struct StoredSlot {
    /// The slot's hash, as reported by the DA layer.
    pub hash: DbHash,
    /// The root hash of the slot's JMT state.
    pub state_root: DbBytes,
    /// Any extra data which the rollup decides to store relating to this slot.
    pub extra_data: DbBytes,
    /// The range of batches which occurred in this slot.
    pub batches: std::ops::Range<BatchNumber>,
}

/// The on-disk format for a batch. Stores the hash and identifies the range of transactions
/// included in the batch.
#[derive(Debug, PartialEq, BorshDeserialize, BorshSerialize)]
#[cfg_attr(
    feature = "arbitrary",
    derive(proptest_derive::Arbitrary, arbitrary::Arbitrary)
)]
pub struct StoredBatch {
    /// The hash of the batch, as reported by the DA layer.
    pub hash: DbHash,
    /// The range of transactions which occurred in this batch.
    pub txs: std::ops::Range<TxNumber>,
    /// A customer "receipt" for this batch defined by the rollup.
    pub receipt: DbBytes,
}

impl<B: DeserializeOwned, T: TxReceiptContents> TryFrom<StoredBatch> for BatchResponse<B, T> {
    type Error = anyhow::Error;
    fn try_from(value: StoredBatch) -> Result<Self, Self::Error> {
        Ok(Self {
            hash: value.hash,
            receipt: bincode::deserialize(&value.receipt.0)?,
            tx_range: value.txs.start.into()..value.txs.end.into(),
            txs: None,
        })
    }
}

/// The on-disk format of a transaction. Includes the txhash, the serialized tx data,
/// and identifies the events emitted by this transaction
#[derive(Debug, PartialEq, BorshSerialize, BorshDeserialize, Clone)]
#[cfg_attr(
    feature = "arbitrary",
    derive(proptest_derive::Arbitrary, arbitrary::Arbitrary)
)]
pub struct StoredTransaction {
    /// The hash of the transaction.
    pub hash: DbHash,
    /// The range of event-numbers emitted by this transaction.
    pub events: std::ops::Range<EventNumber>,
    /// The serialized transaction data, if the rollup decides to store it.
    pub body: Option<Vec<u8>>,
    /// A customer "receipt" for this transaction defined by the rollup.
    pub receipt: DbBytes,
}

impl<R: TxReceiptContents> TryFrom<StoredTransaction> for TxResponse<R> {
    type Error = anyhow::Error;
    fn try_from(value: StoredTransaction) -> Result<Self, Self::Error> {
        Ok(Self {
            hash: value.hash,
            event_range: value.events.start.into()..value.events.end.into(),
            body: value.body,
            receipt: bincode::deserialize(&value.receipt.0)?,
        })
    }
}

/// Split a `TransactionReceipt` into a `StoredTransaction` and a list of `Event`s for storage in the database.
pub fn split_tx_for_storage<T: TxReceiptContents>(
    tx: TransactionReceipt<T>,
    event_offset: u64,
) -> (StoredTransaction, Vec<StoredEvent>) {
    let event_range =
        EventNumber(event_offset)..EventNumber(event_offset.saturating_add(tx.events.len() as u64));
    let tx_for_storage = StoredTransaction {
        hash: tx.tx_hash,
        events: event_range,
        body: tx.body_to_save,
        receipt: DbBytes::new(
            bincode::serialize(&tx.receipt).expect("Serialization to vec is infallible"),
        ),
    };
    (tx_for_storage, tx.events)
}

/// A singleton key for the latest finalized slot
#[derive(
    Clone,
    Copy,
    ::core::fmt::Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ::serde::Serialize,
    ::serde::Deserialize,
)]
pub struct LatestFinalizedSlotSingleton;

macro_rules! u64_wrapper {
    ($name:ident) => {
        /// A typed wrapper around u64 implementing `Encode` and `Decode`
        #[derive(
            Clone,
            Copy,
            ::core::fmt::Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            ::borsh::BorshDeserialize,
            ::borsh::BorshSerialize,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[cfg_attr(
            feature = "arbitrary",
            derive(proptest_derive::Arbitrary, arbitrary::Arbitrary)
        )]
        pub struct $name(pub u64);

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl ::core::ops::Add<u64> for $name {
            type Output = Self;

            fn add(self, rhs: u64) -> Self {
                Self(self.0 + rhs)
            }
        }

        impl ::core::ops::AddAssign<u64> for $name {
            fn add_assign(&mut self, rhs: u64) {
                self.0 += rhs;
            }
        }

        impl ::core::ops::Sub<u64> for $name {
            type Output = Self;

            fn sub(self, rhs: u64) -> Self {
                Self(self.0 - rhs)
            }
        }

        impl ::core::ops::SubAssign<u64> for $name {
            fn sub_assign(&mut self, rhs: u64) {
                self.0 -= rhs;
            }
        }
    };
}

u64_wrapper!(TxIncrId);
u64_wrapper!(SlotNumber);
u64_wrapper!(BatchNumber);
u64_wrapper!(TxNumber);
u64_wrapper!(EventNumber);
u64_wrapper!(ProofUniqueId);
