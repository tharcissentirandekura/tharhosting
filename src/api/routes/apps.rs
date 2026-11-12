/*!
 * Handlers for app records: list, get, create, update and delete.
 */

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::shared::{AppView, ensure_idle, find_app, view};
use super::super::error::{ApiResult, AppError};
use super::super::payloads::*;
use super::super::validation::*;
use crate::db::model::{App, AppStatus, Trigger};
use crate::deploy;
use crate::state::AppState;
use crate::tools::docker;

/**
 * `GET /api/apps`: lists all apps. Container state is left out (`None`) to
 * avoid running a docker command per app.
 */
pub async fn list_apps(State(state): State<AppState>) -> Json<Vec<AppView>> {
    let apps = state.store.read(|d| d.apps.clone());
    Json(apps.into_iter().map(|a| view(&state, a, None)).collect())
}

/**
 * `GET /api/apps/{app}`: returns one app, including its live container state.
 */
pub async fn get_app(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<AppView>> {
    let app = find_app(&state, &key)?;
    let container = docker::container_state(&state.config.docker_bin, &app).await;
    Ok(Json(view(&state, app, container)))
}

/**
 * `POST /api/apps`: validates the request, registers the app on the next free
 * host port, and optionally starts its first deploy. Responds `201 Created`.
 *
 * # Errors
 *
 * 400 for invalid input, 409 if the name is already taken.
 */
pub async fn create_app(
    State(state): State<AppState>,
    Json(req): Json<CreateAppRequest>,
) -> ApiResult<impl IntoResponse> {
    validate_name(&req.name)?;
    validate_repo_url(&req.repo_url)?;
    validate_branch(&req.branch)?;
    validate_dockerfile(&req.dockerfile)?;
    validate_port(req.container_port)?;
    validate_env(&req.env)?;

    let start_range = state.config.port_range_start;
    let app = state.store.write(|data| {
        if data.apps.iter().any(|a| a.name == req.name) {
            return Err(AppError::Conflict(format!("app {:?} already exists", req.name)));
        }
        let host_port = data
            .next_free_port(start_range)
            .ok_or_else(|| AppError::Internal("no free host ports left".into()))?;
        
        let now = Utc::now();

        let app = App {
            id: Uuid::new_v4(),
            name: req.name,
            repo_url: req.repo_url,
            branch: req.branch,
            dockerfile: req.dockerfile,
            container_port: req.container_port,
            host_port,
            env: req.env,
            status: AppStatus::Created,
            current_deployment: None,
            created_at: now,
            updated_at: now,
        };
        data.apps.push(app.clone());
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

/**
 * `PATCH /api/apps/{app}`: changes only the fields present in the request.
 * Changes take effect on the next deploy, not on the running container.
 */
pub async fn update_app(
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
    if let Some(f) = &req.dockerfile {
        validate_dockerfile(f)?;
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
            if let Some(f) = req.dockerfile {
                app.dockerfile = f;
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

/**
 * `DELETE /api/apps/{app}`: removes the app's container, images, checkout,
 * records and deploy logs. Responds `204 No Content`.
 *
 * Cleanup of docker and files is best-effort. Only the database update must
 * succeed.
 *
 * # Errors
 *
 * 409 if a deploy is running.
 */
pub async fn delete_app(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<StatusCode> {
    let app = find_app(&state, &key)?;
    // Ensure no deploy is running.
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
    // Remove the deployment logs.
    for id in removed {
        let _ = tokio::fs::remove_file(deploy::log_path(&state, id)).await;
    }

    log::info!("deleted app {}", app.name);
    
    Ok(StatusCode::NO_CONTENT)
}
