/*!
 * Root-level endpoints: the health check and the list of available routes.
 */

use axum::Json;
use serde_json::{Value, json};

/**
 * Health check: always `{ "ok": true }` while the server is running.
 *
 * Served at `/`, `/health` and `/api/health`, all without authentication, so
 * uptime monitors and load balancers don't need the token.
 */
pub async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

/**
 * `GET /api`: lists every route, so a client can discover the API.
 *
 * Needs the bearer token when one is configured, like the rest of `/api`.
 *
 * The list is written by hand, so it must be updated when a route is added.
 * The `every_listed_route_exists` test catches entries that don't match a real
 * route, but not a real route that is missing from the list.
 */
pub async fn index() -> Json<Value> {
    const ROUTES: &[(&str, &str, &str, &str)] = &[
        ("GET", "/", "none", "Health check"),
        ("GET", "/health", "none", "Health check"),
        ("GET", "/api/health", "none", "Health check"),
        ("GET", "/api", "token", "This list of routes"),
        ("GET", "/api/apps", "token", "List apps"),
        ("POST", "/api/apps", "token", "Create an app, optionally deploying it"),
        ("GET", "/api/apps/{app}", "token", "Get one app by name or id, with live container state"),
        ("PATCH", "/api/apps/{app}", "token", "Change an app's settings (applies on next deploy)"),
        ("DELETE", "/api/apps/{app}", "token", "Delete an app, its container, images and logs"),
        ("POST", "/api/apps/{app}/deploy", "token", "Start a deploy"),
        ("POST", "/api/apps/{app}/stop", "token", "Stop the app's container"),
        ("POST", "/api/apps/{app}/start", "token", "Start the app's container"),
        ("GET", "/api/apps/{app}/deployments", "token", "List the app's deployments, newest first"),
        ("GET", "/api/deployments/{id}", "token", "Get one deployment"),
        ("GET", "/api/deployments/{id}/logs", "token", "Read a deployment's log (?offset=N to tail)"),
        ("POST", "/webhooks/github", "signature", "GitHub push webhook"),
    ];

    let routes: Vec<Value> = ROUTES
        .iter()
        .map(|(method, path, auth, about)| {
            json!({ "method": method, "path": path, "auth": auth, "about": about })
        })
        .collect();

    Json(json!({
        "name": "tharhosting",
        "version": env!("CARGO_PKG_VERSION"),
        "routes": routes,
    }))
}
