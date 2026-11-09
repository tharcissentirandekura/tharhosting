//! REST API routes.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::db::model::{App, AppStatus, Deployment, Trigger};
use crate::deploy;
use crate::http::{ApiResult, AppError, require_token};
use crate::requests::*;
use crate::sdk::docker;
use crate::types::AppState;

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
        .with_state(state)
}

fn find_app(state: &AppState, key: &str) -> ApiResult<App> {
    state
        .store
        .read(|d| d.find_app(key).cloned())
        .ok_or(AppError::NotFound("app"))
}

fn ensure_idle(state: &AppState, app: &App) -> ApiResult<()> {
    if state.is_deploying(app.id) {
        Err(AppError::Conflict("a deploy is in progress for this app".into()))
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
struct AppView {
    #[serde(flatten)]
    app: App,
    url: String,
    /// Live container state from docker, if a container exists.
    container: Option<String>,
}

fn view(state: &AppState, app: App, container: Option<String>) -> AppView {
    AppView {
        url: format!("http://{}:{}", state.config.public_host, app.host_port),
        app,
        container,
    }
}

async fn list_apps(State(state): State<AppState>) -> Json<Vec<AppView>> {
    let apps = state.store.read(|d| d.apps.clone());
    Json(apps.into_iter().map(|a| view(&state, a, None)).collect())
}

async fn get_app(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<AppView>> {
    let app = find_app(&state, &key)?;
    let container = docker::container_state(&state.config.docker_bin, &app).await;
    Ok(Json(view(&state, app, container)))
}

async fn create_app(
    State(state): State<AppState>,
    Json(req): Json<CreateAppRequest>,
) -> ApiResult<impl IntoResponse> {
    validate_name(&req.name)?;
    validate_repo_url(&req.repo_url)?;
    validate_branch(&req.branch)?;
    validate_port(req.container_port)?;
    validate_env(&req.env)?;

    let start = state.config.port_range_start;
    let app = state.store.write(|d| {
        if d.apps.iter().any(|a| a.name == req.name) {
            return Err(AppError::Conflict(format!("app {:?} already exists", req.name)));
        }
        let host_port = d
            .next_free_port(start)
            .ok_or_else(|| AppError::Internal("no free host ports left".into()))?;
        let now = Utc::now();
        let app = App {
            id: Uuid::new_v4(),
            name: req.name,
            repo_url: req.repo_url,
            branch: req.branch,
            container_port: req.container_port,
            host_port,
            env: req.env,
            status: AppStatus::Created,
            current_deployment: None,
            created_at: now,
            updated_at: now,
        };
        d.apps.push(app.clone());
        Ok(app)
    })??;
    log::info!("created app {} on host port {}", app.name, app.host_port);

    let deployment = if req.deploy {
        Some(deploy::start(&state, app.id, Trigger::Manual)?)
    } else {
        None
    };
    let app = find_app(&state, &app.id.to_string())?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "app": view(&state, app, None), "deployment": deployment })),
    ))
}

async fn update_app(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<UpdateAppRequest>,
) -> ApiResult<Json<AppView>> {
    if let Some(u) = &req.repo_url {
        validate_repo_url(u)?;
    }
    if let Some(b) = &req.branch {
        validate_branch(b)?;
    }
    if let Some(p) = req.container_port {
        validate_port(p)?;
    }
    if let Some(e) = &req.env {
        validate_env(e)?;
    }
    let id = find_app(&state, &key)?.id;
    let app = state
        .store
        .write(|d| {
            let app = d.app_mut(id)?;
            if let Some(u) = req.repo_url {
                app.repo_url = u;
            }
            if let Some(b) = req.branch {
                app.branch = b;
            }
            if let Some(p) = req.container_port {
                app.container_port = p;
            }
            if let Some(e) = req.env {
                app.env = e;
            }
            app.updated_at = Utc::now();
            Some(app.clone())
        })?
        .ok_or(AppError::NotFound("app"))?;
    // Changes take effect on the next deploy.
    Ok(Json(view(&state, app, None)))
}

async fn delete_app(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<StatusCode> {
    let app = find_app(&state, &key)?;
    ensure_idle(&state, &app)?;
    let docker_bin = &state.config.docker_bin;
    let _ = docker::remove_container(docker_bin, &app).await;
    docker::remove_images(docker_bin, &app).await;
    let _ = tokio::fs::remove_dir_all(state.config.builds_dir().join(app.id.to_string())).await;

    let removed: Vec<Uuid> = state.store.write(|d| {
        d.apps.retain(|a| a.id != app.id);
        let ids = d
            .deployments
            .iter()
            .filter(|x| x.app_id == app.id)
            .map(|x| x.id)
            .collect();
        d.deployments.retain(|x| x.app_id != app.id);
        ids
    })?;
    for id in removed {
        let _ = tokio::fs::remove_file(deploy::log_path(&state, id)).await;
    }
    log::info!("deleted app {}", app.name);
    Ok(StatusCode::NO_CONTENT)
}

async fn deploy_app(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<(StatusCode, Json<Deployment>)> {
    let app = find_app(&state, &key)?;
    let dep = deploy::start(&state, app.id, Trigger::Manual)?;
    Ok((StatusCode::ACCEPTED, Json(dep)))
}

async fn set_running(state: &AppState, key: &str, run: bool) -> ApiResult<Json<AppView>> {
    let app = find_app(state, key)?;
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
    res.map_err(|e| AppError::Internal(format!("docker: {e}")))?;
    let status = if run { AppStatus::Running } else { AppStatus::Stopped };
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

async fn stop_app(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<AppView>> {
    set_running(&state, &key, false).await
}

async fn start_app(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<AppView>> {
    set_running(&state, &key, true).await
}

async fn list_deployments(
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

fn parse_id(id: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| AppError::NotFound("deployment"))
}

async fn get_deployment(
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

/// Plain-text build log. Pass `?offset=N` (from the previous response's
/// `x-log-offset` header) to fetch only new output.
async fn deployment_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<LogQuery>,
) -> ApiResult<impl IntoResponse> {
    let id = parse_id(&id)?;
    let dep = state
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
        dep.status,
        crate::db::model::DeploymentStatus::Succeeded | crate::db::model::DeploymentStatus::Failed
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

fn verify_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let Some(sig) = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("sha256="))
        .and_then(|v| hex::decode(v).ok())
    else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

/// GitHub push webhook: redeploys every app tracking the pushed repo+branch.
async fn github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(secret) = &state.config.webhook_secret
        && !verify_signature(secret, &headers, &body)
    {
        return Err(AppError::Unauthorized);
    }
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match event {
        "ping" => return Ok(Json(json!({ "ok": true, "pong": true }))),
        "push" => {}
        other => return Ok(Json(json!({ "ok": true, "ignored": other }))),
    }

    let push: GithubPush = serde_json::from_slice(&body)
        .map_err(|e| AppError::BadRequest(format!("bad push payload: {e}")))?;
    let Some(branch) = push.git_ref.strip_prefix("refs/heads/") else {
        return Ok(Json(json!({ "ok": true, "ignored": "not a branch push" })));
    };
    if push.deleted {
        return Ok(Json(json!({ "ok": true, "ignored": "branch deleted" })));
    }
    let keys: Vec<String> = push.repository.urls().map(normalize_repo_url).collect();

    let targets: Vec<App> = state.store.read(|d| {
        d.apps
            .iter()
            .filter(|a| a.branch == branch && keys.contains(&normalize_repo_url(&a.repo_url)))
            .cloned()
            .collect()
    });

    let mut deployed = Vec::new();
    let mut skipped = Vec::new();
    for app in targets {
        match deploy::start(&state, app.id, Trigger::Webhook) {
            Ok(dep) => deployed.push(json!({ "app": app.name, "deployment": dep.id })),
            Err(AppError::Conflict(m)) => skipped.push(json!({ "app": app.name, "reason": m })),
            Err(e) => return Err(e),
        }
    }
    log::info!("webhook push to {branch}: {} deploy(s) started", deployed.len());
    Ok(Json(json!({ "ok": true, "deployed": deployed, "skipped": skipped })))
}

#[cfg(test)]
mod tests;
