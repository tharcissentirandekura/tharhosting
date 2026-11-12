/*!
 * The HTTP router: every endpoint in one place. The handlers themselves live
 * in the submodules:
 *
 * - `index`: the health check and the list of routes
 * - `apps`: app records (list, get, create, update, delete)
 * - `lifecycle`: deploy, start, stop
 * - `deployments`: deployment history and logs
 * - `shared`: helpers used by more than one handler file
 */

mod apps;
mod deployments;
mod index;
mod lifecycle;
mod shared;

use axum::routing::{get, post};
use axum::{Router, middleware};

use self::apps::{create_app, delete_app, get_app, list_apps, update_app};
use self::deployments::{deployment_logs, get_deployment, list_deployments};
use self::index::{health, index};
use self::lifecycle::{deploy_app, start_app, stop_app};
use super::auth::require_token;
use super::logging::log_requests;
use super::webhook::github_webhook;
use crate::state::AppState;

/**
 * Builds the HTTP router.
 *
 * - `/`, `/health` and `/api/health` return the health check, with no token.
 * - `/webhooks/github` is open too. It is protected by its own signature check.
 * - Everything else under `/api`, including the route list at `/api`, requires
 *   the bearer token when one is configured.
 */
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/", get(index))
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

    // `/api/health` is added after the token layer above, so it is not covered
    // by it and monitors can reach it without credentials.
    let api = Router::new().route("/health", get(health)).merge(protected);

    Router::new()
        .route("/", get(health))
        .route("/health", get(health))
        .route("/webhooks/github", post(github_webhook))
        .nest("/api", api)
        // Added last so it wraps every route above, including failed requests.
        .layer(middleware::from_fn(log_requests))
        .with_state(state)
}

#[cfg(test)]
mod tests;
