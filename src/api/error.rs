/*!
 * The error type shared by all API handlers, and how it becomes an HTTP response.
 */

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/**
 * Application error type for the HTTP API.
 */
#[derive(Debug)]
pub enum AppError {
    NotFound(&'static str),
    BadRequest(String),
    Conflict(String),
    Unauthorized,
    // internal server error: 500
    Internal(String),
}

/**
 * Turns an application error into a JSON HTTP response: `{ "error": "<message>" }`
 * with the matching status code.
 */
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::NotFound(error) => (StatusCode::NOT_FOUND, format!("{error} not found")),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            AppError::Internal(m) => {
                log::error!("internal error: {m}");
                (StatusCode::INTERNAL_SERVER_ERROR, m)
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

/**
 * Converts any I/O error into `AppError::Internal`, so handlers can apply `?`
 * directly to I/O calls without mapping the error manually.
 */
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

/**
 * Result type for API operations: `T` on success, `AppError` on failure.
 */
pub type ApiResult<T> = Result<T, AppError>;
