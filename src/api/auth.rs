/*!
 * Bearer-token authentication for the `/api` routes.
 */

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use super::error::AppError;
use crate::state::AppState;

/**
 * Compares two byte slices in constant time, so that checking a token doesn't
 * leak, through timing, how many leading bytes were correct.
 *
 * The lengths are compared first and are not treated as secret.
 */
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.ct_eq(b).into()
}

/**
 * Middleware that requires `Authorization: Bearer <THARHOSTING_TOKEN>`.
 *
 * When no token is configured, every request is let through.
 *
 * # Errors
 *
 * Returns [`AppError::Unauthorized`] if a token is configured and the request's
 * bearer token is missing or wrong.
 */
pub async fn require_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    if let Some(expected) = &state.config.api_token {
        let given = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        let same_tokens = ct_eq(given.as_bytes(), expected.as_bytes());
        if !same_tokens {
            return Err(AppError::Unauthorized);
        }
    }
    let remaining = next.run(req).await;
    Ok(remaining)
}
