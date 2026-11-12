/*!
 * The GitHub push webhook: verifies GitHub's signature and redeploys the apps
 * that track the pushed repository and branch.
 */

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

use super::error::{ApiResult, AppError};
use super::github::GithubPush;
use super::validation::normalize_repo_url;
use crate::db::model::{App, Trigger};
use crate::deploy;
use crate::state::AppState;

/**
 * Checks GitHub's `X-Hub-Signature-256` header against an HMAC-SHA256 of
 * `body` keyed with `secret`. The comparison is constant-time.
 *
 * Returns `false` if the header is missing or malformed.
 */
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

/**
 * `POST /webhooks/github`: redeploys every app tracking the pushed repo and branch.
 *
 * Repositories are matched by [`normalize_repo_url`], so https, ssh and `.git`
 * forms all match. Apps that already have a deploy running are skipped and
 * listed in the response, not treated as errors.
 *
 * # Errors
 *
 * 401 if a webhook secret is configured and the signature is wrong, 400 if the
 * push payload is malformed.
 */
pub async fn github_webhook(
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
