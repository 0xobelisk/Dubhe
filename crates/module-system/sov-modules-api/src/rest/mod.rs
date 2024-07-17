//! Traits and utilities for writing REST(ful) APIs that expose rollup data.
//!
//! Sovereign rollup nodes can expose their data over HTTP in two ways:
//!
//! 1. A JSON-RPC interface. This is mostly intended for cross-compatbility with
//!    third-party JSON-RPC interfaces e.g. the Ethereum JSON-RPC API.
//! 2. An [`axum`]-based REST API. Axum is the most flexible and capable
//!    solution out of the two listed here, and integrates most easily with the
//!    rest of the Sovereign SDK ecosystem.
//!
//! This Rust module is exclusively concerned with the latter case, and exposes
//! all the necessary tools for composing Axum routers within the rollup node.
//! If you're looking for JSON-RPC API documentation, please refer to
//! [`crate::macros::expose_rpc`].
//!
//! Nodes expose rollup data using a combination of three traits:
//!
//! | Trait | Derivable | Implemented by |
//! | ----- | --------- | ---- |
//! | [`HasRestApi`] | With [`ModuleRestApi`] and [`RuntimeRestApi`] | Modules and runtimes |
//! | [`HasCustomRestApi`]   | ❌ | Modules and runtimes |
//!
//! Implementing or deriving *any* of these traits is optional, but types
//! implementing [`HasCustomRestApi`] ought to also derive [`HasRestApi`], or
//! no REST API will be available for them.

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::{FromRequestParts, State};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use sov_rest_utils::Query;

use crate::hooks::TxHooks;
use crate::map::NamespacedStateMap;
use crate::rest::__private::maybe_archival_accessor;
use crate::vec::NamespacedStateVec;
use crate::{ApiStateAccessor, Module, ModuleId, ModuleInfo, Spec};

/// This Rust module is **NOT** part of the public API of the crate, and can
/// change at any time.
#[doc(hidden)]
pub mod __private;

/// A [`tokio::sync::watch::Receiver`] for a [`Spec`]'s storage.
pub type StorageReceiver<S> = tokio::sync::watch::Receiver<<S as Spec>::Storage>;

pub use sov_modules_macros::{ModuleRestApi, RuntimeRestApi};

/// Utilities for building opinionated REST(ful) APIs with [`axum`].
#[doc(inline)]
pub extern crate sov_rest_utils as utils;

/// This trait is intended to be derived via
/// [`crate::macros::ModuleRestApi`] by modules and via
/// [`crate::macros::RuntimeRestApi`] by runtimes.
/// runtimes, and it provides a fair amount of paths and general information
/// about the rollup, including but not limited to:
///
/// - A list of all modules included in the runtime, and related metadata.
/// - Module state, both latest and historical; per-variable state overview;
///   pagination for structured state items like
///   [`StateVec`](crate::containers::StateVec).
pub trait HasRestApi<S: Spec> {
    fn rest_api(&self, storage: StorageReceiver<S>) -> axum::Router<()>;

    /// Returns the OpenAPI specification for [`HasRestApi::rest_api`].
    /// [`None`] means there is no known OpenAPI spec for the API.
    fn openapi_spec(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Makes deriving [`HasRestApi`] for modules optional, with the autoref trick.
impl<M: ModuleInfo> HasRestApi<M::Spec> for &M {
    fn rest_api(&self, _state: StorageReceiver<M::Spec>) -> axum::Router<()> {
        axum::Router::new()
    }

    fn openapi_spec(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Optionally exposes hand-written, custom API routes for a module or
/// runtime.
///
/// This trait cannot be derived, and implementing it is entirely optional.
/// A module that implements this trait will be automatically exposed as part of
/// the runtime API, as [`ModuleRestApi`] will automatically merge the two.
///
/// # Example
///
/// ```
/// use sov_modules_api::prelude::*;
/// use sov_modules_api::{ModuleId, ModuleInfo, StateValue};
/// use sov_modules_api::rest::{HasCustomRestApi, ApiState};
///
/// #[derive(Clone, ModuleInfo, ModuleRestApi)]
/// struct MyModule<S: Spec> {
///     #[id]
///     id: ModuleId,
///     #[state]
///     state_item: StateValue<S::Address>,
/// }
///
/// impl<S: Spec> HasCustomRestApi<S> for MyModule<S> {
///     fn custom_rest_api(&self, state: ApiState<Self, S>) -> axum::Router<()> {
///         use axum::routing::get;
///
///         axum::Router::new()
///             .route("/", get(|| async { "Hello, world!" }))
///             .with_state(state)
///     }
/// }
/// # // BEGIN MODULE IMPL, COPY-PASTE-ME (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html#hiding-portions-of-the-example)
/// # impl<S: Spec> sov_modules_api::Module for MyModule<S> {
/// #    type Spec = S;
/// #    type Config = ();
/// #    type CallMessage = ();
/// #    type Event = ();
/// #
/// #    fn genesis(
/// #        &self,
/// #        _config: &Self::Config,
/// #        _state: &mut impl sov_modules_api::state::GenesisState<S>,
/// #    ) -> Result<(), sov_modules_api::Error> {
/// #        Ok(())
/// #    }
/// #
/// #    fn call(
/// #        &self,
/// #        _msg: Self::CallMessage,
/// #        _context: &Context<Self::Spec>,
/// #        _state: &mut impl sov_modules_api::state::TxState<S>,
/// #    ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
/// #        unimplemented!()
/// #    }
/// # }
/// # // END MODULE IMPL
/// ```
pub trait HasCustomRestApi<S: Spec>: Sized {
    fn custom_rest_api(&self, state: ApiState<Self, S>) -> axum::Router<()>;

    /// Returns the OpenAPI specification for [`HasCustomRestApi::custom_rest_api`].
    /// [`None`] means there is no known OpenAPI spec for the API.
    fn custom_openapi_spec(&self) -> Option<serde_json::Value> {
        None
    }
}

/// In case [`HasCustomRestApi`] is implemented for a [`Module`] or Runtime, an
/// empty [`axum::Router`] will be returned instead.
///
/// This "blanket" implementation uses the autoref trick.
impl<T, S: Spec> HasCustomRestApi<S> for &T {
    fn custom_rest_api(&self, _state: ApiState<Self, S>) -> axum::Router<()> {
        axum::Router::new()
    }
}

/// A wrapper around [`Spec::Storage`] that is appropriate for use as a state
/// type of module and runtime [`axum::Router`]s.
#[derive(derivative::Derivative, derive_more::Deref)]
#[derivative(Clone(bound = ""))]
pub struct ApiState<T, S: Spec> {
    #[deref]
    inner: Arc<T>,
    storage_receiver: StorageReceiver<S>,
    height: Option<u64>,
}

impl<T, S: Spec> ApiState<T, S> {
    /// Creates an [`ApiState`] that subscribes to the given
    /// [`StorageReceiver`].
    pub fn new(inner: T, storage_receiver: StorageReceiver<S>) -> Self {
        Self {
            inner: Arc::new(inner),
            storage_receiver,
            height: None,
        }
    }

    /// Returns a reference to the latest available storage.
    ///
    /// You will usually not call this method directly, as storage alone
    /// can't be used to read state.
    pub fn storage(&self) -> impl Deref<Target = S::Storage> + '_ {
        self.storage_receiver.borrow()
    }

    /// Returns a [`ApiStateAccessor`] that you can use to read state from within REST
    /// API.
    pub fn api_state_accessor(&self) -> ApiStateAccessor<S> {
        let storage = self.storage().clone();
        let state_accessor = ApiStateAccessor::new(storage);

        maybe_archival_accessor(state_accessor, self.height)
    }
}

#[axum::async_trait]
impl<T, S: Spec> FromRequestParts<ApiState<T, S>> for ApiState<T, S>
where
    T: Send + Sync,
{
    type Rejection = utils::ErrorObject;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &ApiState<T, S>,
    ) -> Result<Self, Self::Rejection> {
        let height = Query::<HeightQueryParam>::from_request_parts(parts, state)
            .await
            .ok()
            .map(|q| q.0.height);

        let mut state = state.clone();
        state.height = height;

        Ok(state)
    }
}

#[derive(Copy, Clone, Debug, Deserialize)]
pub(crate) struct HeightQueryParam {
    pub height: u64,
}
