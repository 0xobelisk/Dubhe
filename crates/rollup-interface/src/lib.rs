//! This crate defines the core traits and types used by all Sovereign SDK rollups.
//! It specifies the interfaces which allow the same "business logic" to run on different
//! DA layers and be proven with different zkVMS, all while retaining compatibility
//! with the same basic full node implementation.

#![deny(missing_docs)]

extern crate alloc;

pub mod common;
mod state_machine;
#[cfg(all(feature = "native", feature = "schemars"))]
pub use schemars;
pub use state_machine::*;

#[cfg(feature = "native")]
mod node;
#[cfg(feature = "native")]
pub use node::*;
pub use {anyhow, digest};
