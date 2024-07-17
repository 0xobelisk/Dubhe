// NOTE: this crate can be used as a standalone crate, but most rollup
// developers will interface with it through `sov_modules_api::rest`. So, keep
// that in mind when writing docs.

//! Utilities for building opinionated REST(ful) APIs with [`axum`].
//!
//! # Design choices
//! - Response and request formats are *occasionally* inspired by
//!   [JSON:API](https://jsonapi.org/format/). This crate does *NOT* aim to be
//!   JSON:API compliant. More specifically, we completely disregard any parts
//!   of the spec that we find unnecessary or problematic for most use cases
//!   (e.g. "link objects" and "relationships", which only make sense when
//!   designing truly  HATEOAS-driven APIs).
//! - Query string parameters follow the bracket notation `foo[bar]` that was
//!   popularized by [`qs`](https://github.com/ljharb/qs).
//! - Pagination is cursor-based.
//!
//! # Missing features
//! - Multi-column sorting (see <https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/449>).

#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod axum_extractors;
mod pagination;
mod sorting;

pub mod errors;

#[doc(hidden)]
pub mod test_utils;

use std::fmt::Debug;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
pub use axum_extractors::{Path, Query};
pub use pagination::{PageSelection, Pagination};
pub use sorting::{Sorting, SortingOrder};
use tower_http::compression::CompressionLayer;
use tower_http::propagate_header::PropagateHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_request_id::{RequestId, RequestIdLayer};
use tracing::error_span;

/// The standard response type used by the utilities in this crate.
pub type ApiResult<T, E = Response> = Result<ResponseObject<T>, E>;

/// Top-level response object to be used for all responses.
///
/// Every [`ResponseObject`] has at least of:
/// - A `data` field, which can be any JSON value.
/// - An `errors` field with one or more errors in it.
///
/// These two cases are usually but not always exclusive, notably in the case of
/// partial success.
#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseObject<T> {
    /// Core response data when successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// A list of errors that occurred during the request. If the list is empty,
    /// the request was successful.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ErrorObject>,
    /// Metadata about the response, if present or needed (e.g. remaining
    /// requests available in the current rate limit window). This will be empty
    /// in most cases.
    pub meta: JsonObject,
}

impl<T> From<T> for ResponseObject<T> {
    fn from(data: T) -> Self {
        Self {
            data: Some(data),
            errors: Vec::new(),
            meta: JsonObject::default(),
        }
    }
}

impl<T> IntoResponse for ResponseObject<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> Response {
        // If there are no errors, we return a 200 OK response.
        let status = self
            .errors
            .first()
            .map(|err| err.status)
            .unwrap_or(StatusCode::OK);

        (status, Json(self)).into_response()
    }
}

impl IntoResponse for ErrorObject {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ResponseObject::<()> {
                data: None,
                errors: vec![self],
                meta: JsonObject::default(),
            }),
        )
            .into_response()
    }
}

/// A JSON object (mind you, not a *value*, but an
/// [*object*](https://www.json.org/json-en.html)).
pub type JsonObject = serde_json::Map<String, serde_json::Value>;

/// Inspired from <https://jsonapi.org/format/#error-objects>.
#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct ErrorObject {
    /// The HTTP status that best describes the error.
    #[serde(with = "serde_status_code")]
    pub status: StatusCode,
    /// A short, human-readable description of the error.
    pub title: String,
    /// Structured details about the error, if available.
    pub details: JsonObject,
}

mod serde_status_code {
    use axum::http::StatusCode;

    pub fn serialize<S>(status_code: &StatusCode, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u16(status_code.as_u16())
    }
}

/// Exactly like [`serde_json::Value`], but returns a JSON object instead of a
/// JSON value.
#[macro_export]
macro_rules! json_obj {
    ($($json:tt)+) => {
        match ::serde_json::json!($($json)+) {
            ::serde_json::Value::Object(obj) => obj,
            _ => panic!("json_obj! macro returned non-object value"),
        }
    };
}

/// Customizes the given [`Router`] with a set of preconfigured "layers" that
/// are a good starting point for building production-ready JSON APIs.
pub fn preconfigured_router_layers<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // Tracing span with unique ID per request:
    // <https://github.com/imbolc/tower-request-id/blob/main/examples/logging.rs>
    let trace_layer = TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
        // We get the request id from the extensions
        let request_id = request
            .extensions()
            .get::<RequestId>()
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown".into());
        // And then we put it along with other information into the `request` span
        error_span!(
            "request",
            id = %request_id,
            method = %request.method(),
            uri = %request.uri(),
        )
    });
    router
        .layer(trace_layer)
        // This layer creates a new id for each request and puts it into the request extensions.
        // Note that it should be added after the Trace layer. (Filippo: why? I
        // don't know, I copy-pasted this.)
        .layer(RequestIdLayer)
        .layer(
            tower::ServiceBuilder::new()
                // Tracing.
                .layer(TraceLayer::new_for_http())
                // Compress responses with GZIP.
                .layer(CompressionLayer::new())
                // Propagate `X-Request-Id`s from requests to responses.
                .layer(PropagateHeaderLayer::new(HeaderName::from_static(
                    "x-request-id",
                ))),
        )
        .fallback(errors::global_404)
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use crate::test_utils::uri_with_query_params;

    proptest! {
        // Ideally we'd also test with types other than strings. E.g. integers?
        #[test]
        fn any_query_param_can_be_serialized(key: String, value: String) {
            // As long as it doesn't crash, we're good and the test succeeds.
            uri_with_query_params([(key, value)]);
        }
    }
}
