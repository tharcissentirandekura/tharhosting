/*!
 * Request logging middleware.
 */

use std::time::Instant;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/**
 * A middleware for each request received to log the request message
 */
pub async fn log_requests(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let started = Instant::now();

    let response = next.run(req).await;

    log::info!(
        "{method} {path} -> {} ({} ms)",
        response.status().as_u16(),
        started.elapsed().as_millis()
    );
    response
}
