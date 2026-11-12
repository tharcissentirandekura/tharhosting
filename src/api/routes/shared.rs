/*!
 * Helpers shared by the route handlers: looking up apps, guarding against
 * concurrent deploys, and the JSON shape returned for an app.
 */

use serde::Serialize;

use super::super::error::{ApiResult, AppError};
use crate::db::model::App;
use crate::state::AppState;

/**
 * Finds an app by id or name, or fails with 404.
 */
pub fn find_app(state: &AppState, key: &str) -> ApiResult<App> {
    state
        .store
        .read(|d| d.find_app(key).cloned())
        .ok_or(AppError::NotFound("app"))
}

/**
 * Fails with 409 if `app` has a deploy running, so that it can't be deleted,
 * started or stopped mid-deploy.
 */
pub fn ensure_idle(state: &AppState, app: &App) -> ApiResult<()> {
    if state.is_deploying(app.id) {
        Err(AppError::Conflict("a deploy is in progress for this app".into()))
    } else {
        Ok(())
    }
}

/**
 * What the API returns for an app: the stored fields plus its public URL and
 * live container state.
 */
#[derive(Serialize)]
pub struct AppView {
    #[serde(flatten)]
    app: App,
    /** The public URL of the app. */
    url: String,
    /** Live container state from docker, if a container exists. */
    container: Option<String>,
}

/**
 * Wraps `app` into an [`AppView`], adding its public URL.
 */
pub fn view(state: &AppState, app: App, container: Option<String>) -> AppView {
    AppView {
        url: format!("http://{}:{}", state.config.public_host, app.host_port),
        app,
        container,
    }
}
