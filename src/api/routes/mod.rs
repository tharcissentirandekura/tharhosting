/*!
 * The HTTP router: every endpoint in one place. The handlers themselves live
 * in the submodules:
 *
 * - `apps`: app records (list, get, create, update, delete)
 * - `lifecycle`: deploy, start, stop
 * - `deployments`: deployment history and logs
 * - `shared`: helpers used by more than one handler file
 */

mod apps;
mod deployments;
mod lifecycle;
mod shared;

use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use serde_json::json;

use self::apps::{create_app, delete_app, get_app, list_apps, update_app};
use self::deployments::{deployment_logs, get_deployment, list_deployments};
use self::lifecycle::{deploy_app, start_app, stop_app};
use super::auth::require_token;
use super::logging::log_requests;
use super::webhook::github_webhook;
use crate::state::AppState;

/**
 * Builds the HTTP router.
 *
 * - `/health` and `/webhooks/github` are open. The webhook is protected by its
 *   own signature check instead.
 * - Everything under `/api` requires the bearer token when one is configured.
 */
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/apps", get(list_apps).post(create_app))
        .route(
            "/apps/{app}",
            get(get_app).patch(update_app).delete(delete_app),
        )
        .route("/apps/{app}/deploy", post(deploy_app))
        .route("/apps/{app}/stop", post(stop_app))
        .route("/apps/{app}/start", post(start_app))
        .route("/apps/{app}/deployments", get(list_deployments))
        .route("/deployments/{id}", get(get_deployment))
        .route("/deployments/{id}/logs", get(deployment_logs))
        .layer(middleware::from_fn_with_state(state.clone(), require_token));

Router::new()
    .route("/health", get(|| async { Json(json!({ "ok": true })) }))
    .route("/webhooks/github", post(github_webhook))
    .nest("/api", api)
    .layer(middleware::from_fn(log_requests))   // new: wraps every route above
    .with_state(state)

}

#[cfg(test)]
mod tests;
