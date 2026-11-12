/*!
 * Handlers for deployment history: list an app's deployments, get one, and
 * read its log.
 */

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use uuid::Uuid;

use super::shared::find_app;
use super::super::error::{ApiResult, AppError};
use super::super::payloads::LogQuery;
use crate::db::model::{Deployment, DeploymentStatus};
use crate::deploy;
use crate::state::AppState;

/**
 * `GET /api/apps/{app}/deployments`: lists the app's deployments, newest first.
 */
pub async fn list_deployments(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<Json<Vec<Deployment>>> {
    let app = find_app(&state, &key)?;
    let mut deps: Vec<Deployment> = state.store.read(|d| {
        d.deployments
            .iter()
            .filter(|x| x.app_id == app.id)
            .cloned()
            .collect()
    });
    deps.sort_by_key(|d| std::cmp::Reverse(d.created_at));
    Ok(Json(deps))
}

/**
 * Parses a deployment id from the URL. A malformed id is reported as 404,
 * since no such deployment can exist.
 */
fn parse_id(id: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| AppError::NotFound("deployment"))
}

/**
 * `GET /api/deployments/{id}`: returns one deployment.
 */
pub async fn get_deployment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Deployment>> {
    let id = parse_id(&id)?;
    state
        .store
        .read(|d| d.deployment(id).cloned())
        .map(Json)
        .ok_or(AppError::NotFound("deployment"))
}

/**
 * `GET /api/deployments/{id}/logs`: returns the deploy log as plain text.
 *
 * Pass `?offset=N` (taken from the previous response's `x-log-offset` header)
 * to fetch only new output. The `x-deploy-done` header is `true` once the
 * deployment has finished, so a client knows when to stop polling.
 */
pub async fn deployment_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<LogQuery>,
) -> ApiResult<impl IntoResponse> {
    let id = parse_id(&id)?;
    let deployment = state
        .store
        .read(|d| d.deployment(id).cloned())
        .ok_or(AppError::NotFound("deployment"))?;
    let bytes = match tokio::fs::read(deploy::log_path(&state, id)).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let start = (q.offset as usize).min(bytes.len());
    let next = bytes.len().to_string();
    let done = matches!(
        deployment.status,
        DeploymentStatus::Succeeded | DeploymentStatus::Failed
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (header::HeaderName::from_static("x-log-offset"), next),
            (header::HeaderName::from_static("x-deploy-done"), done.to_string()),
        ],
        bytes[start..].to_vec(),
    ))
}
