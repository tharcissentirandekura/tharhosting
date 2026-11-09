use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::db::store::Store;
use crate::types::Config;

struct Harness {
    state: AppState,
    _dir: tempfile::TempDir,
}

fn harness(token: Option<&str>, secret: Option<&str>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        host: "127.0.0.1".into(),
        port: 0,
        data_dir: dir.path().to_path_buf(),
        port_range_start: 20000,
        app_bind: "127.0.0.1".into(),
        public_host: "localhost".into(),
        // `false` makes every git/docker call fail fast, so tests never
        // touch the real docker daemon.
        docker_bin: "false".into(),
        git_bin: "false".into(),
        api_token: token.map(Into::into),
        webhook_secret: secret.map(Into::into),
    };
    let store = Store::open(config.db_path()).unwrap();
    Harness {
        state: AppState::new(config, store),
        _dir: dir,
    }
}

async fn call(
    h: &Harness,
    method: &str,
    uri: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = router(h.state.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, v)
}

fn new_app(name: &str) -> Value {
    json!({ "name": name, "repo_url": "https://github.com/thar/site.git" })
}

#[tokio::test]
async fn app_crud() {
    let h = harness(None, None);
    let (s, v) = call(&h, "POST", "/api/apps", Some(new_app("site")), &[]).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["app"]["status"], "created");
    assert_eq!(v["app"]["branch"], "main");
    assert!(v["app"]["host_port"].as_u64().unwrap() >= 20000);

    let (s, _) = call(&h, "POST", "/api/apps", Some(new_app("site")), &[]).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(&h, "POST", "/api/apps", Some(new_app("Bad Name")), &[]).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, v) = call(&h, "GET", "/api/apps", None, &[]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v.as_array().unwrap().len(), 1);

    let (s, v) = call(
        &h,
        "PATCH",
        "/api/apps/site",
        Some(json!({ "branch": "prod", "env": { "API_KEY": "x" } })),
        &[],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["branch"], "prod");
    assert_eq!(v["env"]["API_KEY"], "x");

    let (s, _) = call(&h, "POST", "/api/apps/site/stop", None, &[]).await;
    assert_eq!(s, StatusCode::CONFLICT, "never deployed");

    let (s, _) = call(&h, "DELETE", "/api/apps/site", None, &[]).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(&h, "GET", "/api/apps/site", None, &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

async fn wait_for_deploy(h: &Harness, dep_id: &str) -> Value {
    for _ in 0..100 {
        let (_, v) = call(h, "GET", &format!("/api/deployments/{dep_id}"), None, &[]).await;
        if v["status"] == "failed" || v["status"] == "succeeded" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("deploy never finished");
}

#[tokio::test]
async fn failed_clone_marks_deploy_and_app_failed() {
    let h = harness(None, None);
    call(&h, "POST", "/api/apps", Some(new_app("site")), &[]).await;
    let (s, dep) = call(&h, "POST", "/api/apps/site/deploy", None, &[]).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let id = dep["id"].as_str().unwrap().to_string();

    let done = wait_for_deploy(&h, &id).await;
    assert_eq!(done["status"], "failed");
    assert!(done["error"].as_str().unwrap().contains("git clone"));

    let (_, app) = call(&h, "GET", "/api/apps/site", None, &[]).await;
    assert_eq!(app["status"], "failed");

    let (_, logs) = call(&h, "GET", &format!("/api/deployments/{id}/logs"), None, &[]).await;
    assert!(logs.as_str().unwrap().contains("deploy failed"), "{logs}");

    let (_, list) = call(&h, "GET", "/api/apps/site/deployments", None, &[]).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn token_is_required_when_configured() {
    let h = harness(Some("s3cret"), None);
    let (s, _) = call(&h, "GET", "/api/apps", None, &[]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&h, "GET", "/api/apps", None, &[("authorization", "Bearer nope")]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&h, "GET", "/api/apps", None, &[("authorization", "Bearer s3cret")]).await;
    assert_eq!(s, StatusCode::OK);
    // Health stays open.
    let (s, _) = call(&h, "GET", "/health", None, &[]).await;
    assert_eq!(s, StatusCode::OK);
}

fn sign(secret: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn github_push_triggers_matching_apps_only() {
    let h = harness(None, Some("hook"));
    call(&h, "POST", "/api/apps", Some(new_app("site")), &[]).await;
    call(
        &h,
        "POST",
        "/api/apps",
        Some(json!({ "name": "other", "repo_url": "https://github.com/thar/other" })),
        &[],
    )
    .await;

    let payload = json!({
        "ref": "refs/heads/main",
        "repository": {
            "clone_url": "https://github.com/thar/site.git",
            "ssh_url": "git@github.com:thar/site.git"
        }
    })
    .to_string();

    // Unsigned -> rejected.
    let (s, _) = call(
        &h,
        "POST",
        "/webhooks/github",
        Some(serde_json::from_str(&payload).unwrap()),
        &[("x-github-event", "push")],
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let sig = sign("hook", &payload);
    let req = Request::builder()
        .method("POST")
        .uri("/webhooks/github")
        .header("x-github-event", "push")
        .header("x-hub-signature-256", &sig)
        .body(Body::from(payload.clone()))
        .unwrap();
    let res = router(h.state.clone()).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let deployed = v["deployed"].as_array().unwrap();
    assert_eq!(deployed.len(), 1, "{v}");
    assert_eq!(deployed[0]["app"], "site");
}
