/*!
 * The deploy pipeline: clone → docker build → swap container.
 */

use std::path::Path;

use chrono::Utc;
use uuid::Uuid;

use crate::db::model::{AppStatus, Deployment, DeploymentStatus, Trigger};
use crate::api::error::AppError;
use crate::tools::process::log_line;
use crate::tools::{docker, git};
use crate::state::AppState;

/**
 * Records a new deployment for `app_id` and runs it in the background.
 *
 * The app is marked `Deploying` straight away and the deployment is returned
 * without waiting for the build. The `inflight` set guarantees that only one
 * deploy per app runs at a time.
 *
 * # Errors
 *
 * - [`AppError::Conflict`] if the app already has a deploy in flight.
 * - [`AppError::NotFound`] if the app doesn't exist.
 * - [`AppError::Internal`] if the new state can't be saved.
 */
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

/**
 * Why a deploy failed, and how far it got.
 *
 * `replaced` decides the app's status afterwards: if the old container was
 * never touched, the previous version is still serving.
 */
struct Failure {
    msg: String,
    /** Whether the old container had already been torn down. */
    replaced: bool,
}

/**
 * Lets pipeline steps use `?` on plain `String` errors. Such errors happen
 * before the old container is touched, so `replaced` is `false`.
 */
impl From<String> for Failure {
    fn from(msg: String) -> Self {
        Failure { msg, replaced: false }
    }
}

/**
 * Returns the path of the log file for deployment `dep_id`.
 */
pub fn log_path(state: &AppState, dep_id: Uuid) -> std::path::PathBuf {
    state.config.logs_dir().join(format!("{dep_id}.log"))
}

/**
 * Runs the pipeline for one deployment, then records the outcome.
 *
 * On success the deployment is `Succeeded` and the app `Running`. On failure
 * the deployment is `Failed`, and the app returns to its `prev` status if the
 * running container was never replaced.
 */
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

/**
 * The deploy steps: clone the repo, check for a Dockerfile, build the image,
 * and replace the running container. Progress goes to the `log` file.
 *
 * Returns the built commit SHA (if it could be read) and the image tag.
 *
 * # Errors
 *
 * Returns a [`Failure`] describing the first step that failed.
 */
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

    // The Dockerfile must be a real file that still resolves inside the checkout,
    // so a symlink in the repo can't point the build at files elsewhere.
    let dockerfile = src.join(&app.dockerfile);
    let inside_repo = match (dockerfile.canonicalize(), src.canonicalize()) {
        (Ok(file), Ok(root)) => file.is_file() && file.starts_with(root),
        _ => false,
    };
    if !inside_repo {
        return Err(format!("no {} in the repository", app.dockerfile).into());
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
