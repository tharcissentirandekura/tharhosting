use std::path::Path;

use tokio::process::Command;

use super::{run_capture, run_logged};
use crate::db::model::App;

const LABEL: &str = "tharhosting.app";

pub async fn build(
    docker: &str,
    app: &App,
    image: &str,
    context: &Path,
    log: &Path,
) -> Result<(), String> {
    let mut cmd = Command::new(docker);
    cmd.args(["build", "-t", image, "--label"])
        .arg(format!("{LABEL}={}", app.id))
        .arg(context);
    run_logged(cmd, &format!("docker build -t {image} ."), log).await
}

/// Remove the app's current container (if any) and start `image` in its place.
pub async fn replace_container(
    docker: &str,
    app: &App,
    image: &str,
    bind: &str,
    log: &Path,
) -> Result<(), String> {
    let name = app.container_name();
    // Ignore failure: there may be no previous container.
    let _ = remove_container(docker, app).await;

    let publish = format!("{bind}:{}:{}", app.host_port, app.container_port);
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        name.clone(),
        "--restart".into(),
        "unless-stopped".into(),
        "--label".into(),
        format!("{LABEL}={}", app.id),
        "-p".into(),
        publish.clone(),
    ];
    // Heroku-style: tell the app which port to listen on unless overridden.
    if !app.env.contains_key("PORT") {
        args.push("-e".into());
        args.push(format!("PORT={}", app.container_port));
    }
    let mut shown_env = String::new();
    for (k, v) in &app.env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
        shown_env.push_str(&format!(" -e {k}=***"));
    }
    args.push(image.into());

    let mut cmd = Command::new(docker);
    cmd.args(&args);
    let shown = format!("docker run -d --name {name} -p {publish}{shown_env} {image}");
    run_logged(cmd, &shown, log).await
}

pub async fn remove_container(docker: &str, app: &App) -> Result<String, String> {
    let mut cmd = Command::new(docker);
    cmd.args(["rm", "-f", &app.container_name()]);
    run_capture(cmd).await
}

pub async fn stop(docker: &str, app: &App) -> Result<String, String> {
    let mut cmd = Command::new(docker);
    cmd.args(["stop", &app.container_name()]);
    run_capture(cmd).await
}

pub async fn start(docker: &str, app: &App) -> Result<String, String> {
    let mut cmd = Command::new(docker);
    cmd.args(["start", &app.container_name()]);
    run_capture(cmd).await
}

/// Container state as docker reports it ("running", "exited", ...),
/// or None if there is no container.
pub async fn container_state(docker: &str, app: &App) -> Option<String> {
    let mut cmd = Command::new(docker);
    cmd.args(["inspect", "-f", "{{.State.Status}}", &app.container_name()]);
    run_capture(cmd).await.ok()
}

/// Best-effort removal of every image built for this app.
pub async fn remove_images(docker: &str, app: &App) {
    let mut cmd = Command::new(docker);
    cmd.args(["images", "-q", "--filter"])
        .arg(format!("label={LABEL}={}", app.id));
    let Ok(ids) = run_capture(cmd).await else { return };
    let mut ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();
    ids.dedup();
    if ids.is_empty() {
        return;
    }
    let mut rmi = Command::new(docker);
    rmi.args(["rmi", "-f"]).args(&ids);
    let _ = run_capture(rmi).await;
}
