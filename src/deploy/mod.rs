//! The deploy pipeline: clone → docker build → swap container.

use std::path::Path;

use chrono::Utc;
use uuid::Uuid;

use crate::db::model::{AppStatus, Deployment, DeploymentStatus, Trigger};
use crate::http::AppError;
use crate::sdk::{docker, git, log_line};
use crate::types::AppState;

/// Record a new deployment and run it in the background.
/// Fails with 409 if the app already has a deploy in flight.
pub fn start(state: &AppState, app_id: Uuid, trigger: Trigger) -> Result<Deployment, AppError> {
    if !state.inflight.lock().unwrap().insert(app_id) {
        return Err(AppError::Conflict(
            "a deploy is already in progress for this app".into(),
        ));
    }

    let dep = Deployment::new(app_id, trigger);
    let recorded = state.store.write(|d| {
        let app = d.app_mut(app_id)?;
        let prev = app.status;
        app.status = AppStatus::Deploying;
        app.updated_at = Utc::now();
        d.deployments.push(dep.clone());
        Some(prev)
    });
    let prev = match recorded {
        Ok(Some(prev)) => prev,
        Ok(None) => {
            state.inflight.lock().unwrap().remove(&app_id);
            return Err(AppError::NotFound("app"));
        }
        Err(e) => {
            state.inflight.lock().unwrap().remove(&app_id);
            return Err(e.into());
        }
    };

    let st = state.clone();
    let dep_id = dep.id;
    tokio::spawn(async move {
        run(&st, app_id, dep_id, prev).await;
        st.inflight.lock().unwrap().remove(&app_id);
    });
    Ok(dep)
}

struct Failure {
    msg: String,
    /// Whether the old container had already been torn down.
    replaced: bool,
}

impl From<String> for Failure {
    fn from(msg: String) -> Self {
        Failure { msg, replaced: false }
    }
}

pub fn log_path(state: &AppState, dep_id: Uuid) -> std::path::PathBuf {
    state.config.logs_dir().join(format!("{dep_id}.log"))
}

async fn run(state: &AppState, app_id: Uuid, dep_id: Uuid, prev: AppStatus) {
    let log = log_path(state, dep_id);
    let _ = std::fs::create_dir_all(state.config.logs_dir());
    let result = pipeline(state, app_id, dep_id, &log).await;
    let now = Utc::now();

    let saved = match result {
        Ok((commit, image)) => {
            log_line(&log, "==> deploy succeeded");
            log::info!("deployment {dep_id} succeeded");
            state.store.write(|d| {
                if let Some(dep) = d.deployment_mut(dep_id) {
                    dep.status = DeploymentStatus::Succeeded;
                    dep.commit = commit;
                    dep.image = Some(image);
                    dep.finished_at = Some(now);
                }
                if let Some(app) = d.app_mut(app_id) {
                    app.status = AppStatus::Running;
                    app.current_deployment = Some(dep_id);
                    app.updated_at = now;
                }
            })
        }
        Err(f) => {
            log_line(&log, &format!("==> deploy failed: {}", f.msg));
            log::warn!("deployment {dep_id} failed: {}", f.msg);
            state.store.write(|d| {
                if let Some(dep) = d.deployment_mut(dep_id) {
                    dep.status = DeploymentStatus::Failed;
                    dep.error = Some(f.msg);
                    dep.finished_at = Some(now);
                }
                if let Some(app) = d.app_mut(app_id) {
                    // If the build failed before we touched the running
                    // container, the previous version is still serving.
                    app.status = match (f.replaced, prev) {
                        (false, AppStatus::Running | AppStatus::Stopped) => prev,
                        _ => AppStatus::Failed,
                    };
                    app.updated_at = now;
                }
            })
        }
    };
    if let Err(e) = saved {
        log::error!("could not persist result of deployment {dep_id}: {e}");
    }
}

async fn pipeline(
    state: &AppState,
    app_id: Uuid,
    dep_id: Uuid,
    log: &Path,
) -> Result<(Option<String>, String), Failure> {
    let cfg = &state.config;
    let app = state
        .store
        .write(|d| {
            if let Some(dep) = d.deployment_mut(dep_id) {
                dep.status = DeploymentStatus::Building;
            }
            d.app(app_id).cloned()
        })
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "app was deleted".to_string())?;

    log_line(
        log,
        &format!(
            "==> deploying {} from {} ({}) at {}",
            app.name,
            app.repo_url,
            app.branch,
            Utc::now().to_rfc3339()
        ),
    );

    let src = cfg.builds_dir().join(app.id.to_string());
    if src.exists() {
        tokio::fs::remove_dir_all(&src)
            .await
            .map_err(|e| format!("cannot clear build dir: {e}"))?;
    }
    tokio::fs::create_dir_all(cfg.builds_dir())
        .await
        .map_err(|e| format!("cannot create build dir: {e}"))?;

    git::clone(&cfg.git_bin, &app.repo_url, &app.branch, &src, log).await?;
    let commit = git::head_commit(&cfg.git_bin, &src).await.ok();
    if let Some(c) = &commit {
        log_line(log, &format!("==> commit {c}"));
    }

    if !src.join("Dockerfile").exists() {
        return Err("no Dockerfile at the repository root".to_string().into());
    }

    let tag = &dep_id.simple().to_string()[..12];
    let image = format!("{}:{tag}", app.image_repo());
    docker::build(&cfg.docker_bin, &app, &image, &src, log).await?;

    log_line(log, "==> starting container");
    docker::replace_container(&cfg.docker_bin, &app, &image, &cfg.app_bind, log)
        .await
        .map_err(|msg| Failure { msg, replaced: true })?;
    log_line(
        log,
        &format!(
            "==> live at http://{}:{}",
            cfg.public_host, app.host_port
        ),
    );
    Ok((commit, image))
}
