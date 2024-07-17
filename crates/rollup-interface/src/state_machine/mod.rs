//! Defines types, traits, and helpers that are used by the core state-machine of the rollup.
//! Items in this module must be fully deterministic, since they are expected to be executed inside of zkVMs.
pub mod crypto;
pub mod da;
pub mod stf;
pub mod zk;

pub use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub mod optimistic;
pub mod storage;

/// Defines types and traits distingushing between "native" (full node) and "zk" execution.
///
/// This module uses a combination of a sealed marker trait, unit structs, and an enum to
/// emulate the behavior of a const-generic enum.
pub mod execution_mode {
    use borsh::{BorshDeserialize, BorshSerialize};
    use serde::{Deserialize, Serialize};

    /// Execution modes for the rollup.
    #[derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Hash,
        Serialize,
        Deserialize,
        BorshDeserialize,
        BorshSerialize,
    )]
    pub enum RuntimeExecutionMode {
        /// Execution inside of a [`Zkvm`](super::zk::Zkvm).
        Zk,
        /// Execution on a full node.
        Native,
        /// Execution on a full node with the ability to generate proofs.
        /// This adds some overhead on top of the [`RuntimeExecutionMode::Native`] mode.
        WitnessGeneration,
    }
    /// Marker trait for execution modes.
    pub trait ExecutionMode:
        super::sealed::Sealed + Send + Sync + Default + Serialize + serde::de::DeserializeOwned
    {
        /// An enum variant equivalent to the implementing type.
        const EXECUTION_MODE: RuntimeExecutionMode;
    }
    /// A unit struct marking that execution occurs inside of a [`Zkvm`](super::zk::Zkvm).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
    #[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
    pub struct Zk;
    impl ExecutionMode for Zk {
        const EXECUTION_MODE: RuntimeExecutionMode = RuntimeExecutionMode::Zk;
    }
    /// A unit struct marking that execution occurs on a full node.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
    #[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
    pub struct Native;
    impl ExecutionMode for Native {
        const EXECUTION_MODE: RuntimeExecutionMode = RuntimeExecutionMode::Native;
    }
    /// A unit struct marking that execution generates a witness, adding additional overhead on top of [`Native`] execution.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
    #[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
    pub struct WitnessGeneration;
    impl ExecutionMode for WitnessGeneration {
        const EXECUTION_MODE: RuntimeExecutionMode = RuntimeExecutionMode::WitnessGeneration;
    }
}

mod sealed {
    use super::execution_mode::{Native, WitnessGeneration, Zk};
    pub trait Sealed {}

    impl Sealed for Zk {}
    impl Sealed for Native {}
    impl Sealed for WitnessGeneration {}
}

/// A marker trait for general addresses.
pub trait BasicAddress:
    Eq
    + PartialEq
    + core::fmt::Debug
    + core::fmt::Display
    + Send
    + Sync
    + Clone
    + core::hash::Hash
    + AsRef<[u8]>
    + for<'a> TryFrom<&'a [u8], Error = anyhow::Error>
    + core::str::FromStr
    + Serialize
    + DeserializeOwned
    + 'static
{
}

/// An address used inside rollup
pub trait RollupAddress: BasicAddress {}
