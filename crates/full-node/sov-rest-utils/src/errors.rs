//! Common error types.

use axum::extract::OriginalUri;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::error;

use crate::{json_obj, ErrorObject};

/// A 404 response useful as a [`axum::Router::fallback`].
pub async fn global_404(OriginalUri(uri): OriginalUri) -> Response {
    ErrorObject {
        status: StatusCode::NOT_FOUND,
        title: "Not Found".to_string(),
        details: json_obj!({
            "url": uri.to_string(),
        }),
    }
    .into_response()
}

/// Returns a 501 error.
pub fn not_implemented_501() -> Response {
    ErrorObject {
        status: StatusCode::NOT_IMPLEMENTED,
        title: "Not implemented yet".to_string(),
        details: Default::default(),
    }
    .into_response()
}

/// Returns a 404 error when the given resource was not found.
pub fn not_found_404(resource_name_capitalized: &str, resource_id: impl ToString) -> Response {
    ErrorObject {
        status: StatusCode::NOT_FOUND,
        title: format!(
            "{} '{}' not found",
            resource_name_capitalized,
            resource_id.to_string()
        ),
        details: json_obj!({
            "id": resource_id.to_string(),
        }),
    }
    .into_response()
}

/// Returns a custom 400 error.
pub fn bad_request_400(message: &str, err: impl ToString) -> Response {
    ErrorObject {
        status: StatusCode::BAD_REQUEST,
        title: message.to_string(),
        details: json_obj!({
            "message": err.to_string(),
        }),
    }
    .into_response()
}

/// Returns a 500 error to be used when a database error occurred.
pub fn database_error_response_500(err: impl ToString) -> Response {
    // We don't include the database error in the response, because it may
    // contain sensitive information.
    //
    // FIXME(security): logging it is useful, but we should not even be doing that.
    error!(
        error = err.to_string(),
        "Database error while serving request."
    );

    ErrorObject {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        title: "Database error".to_string(),
        details: json_obj!({}),
    }
    .into_response()
}

/// Returns a 500 internal server error.
pub fn internal_server_error_response_500(err: impl ToString) -> Response {
    tracing::error!(error = err.to_string(), "500 error while serving request");

    ErrorObject {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        title: "Internal server error".to_string(),
        details: json_obj!({
            "message": err.to_string(),
        }),
    }
    .into_response()
}
