/*!
 * Handlers that change what an app is doing: deploy, start and stop.
 */

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;

use super::super::error::{ApiResult, AppError};
use super::shared::{AppView, ensure_idle, find_app, view};
use crate::db::model::{AppStatus, Deployment, Trigger};
use crate::deploy;
use crate::state::AppState;
use crate::tools::docker;

/**
 * `POST /api/apps/{app}/deploy`: starts a deploy in the background and
 * responds `202 Accepted` with the new deployment.
 */
pub async fn deploy_app(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<(StatusCode, Json<Deployment>)> {
    let app = find_app(&state, &key)?;
    let dep = deploy::start(&state, app.id, Trigger::Manual)?;
    Ok((StatusCode::ACCEPTED, Json(dep)))
}

/**
 * Shared by the start and stop endpoints: starts the app's container if `run`
 * is true, stops it otherwise, and records the new status.
 *
 * # Errors
 *
 * 409 if a deploy is running or the app has never been deployed.
 */
async fn set_running(state: &AppState, key: &str, run: bool) -> ApiResult<Json<AppView>> {
    let app = find_app(state, key)?;
    // Ensure the app is idle before attempting to start or stop it.
    ensure_idle(state, &app)?;
    if app.current_deployment.is_none() {
        return Err(AppError::Conflict("app has never been deployed".into()));
    }
    let docker_bin = &state.config.docker_bin;
    let res = if run {
        docker::start(docker_bin, &app).await
    } else {
        docker::stop(docker_bin, &app).await
    };
    // Return an error if Docker fails to start or stop the container.
    res.map_err(|e| AppError::Internal(format!("docker: {e}")))?;

    let status = if run {
        AppStatus::Running
    } else {
        AppStatus::Stopped
    };
    // Update the app's status in the database.
    let app = state
        .store
        .write(|d| {
            let a = d.app_mut(app.id)?;
            a.status = status;
            a.updated_at = Utc::now();
            Some(a.clone())
        })?
        .ok_or(AppError::NotFound("app"))?;

    let container = docker::container_state(docker_bin, &app).await;

    Ok(Json(view(state, app, container)))
}

/** `POST /api/apps/{app}/stop`: stops the app's container. */
pub async fn stop_app(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<Json<AppView>> {
    set_running(&state, &key, false).await
}

/** `POST /api/apps/{app}/start`: starts the app's stopped container. */
pub async fn start_app(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<Json<AppView>> {
    set_running(&state, &key, true).await
}
