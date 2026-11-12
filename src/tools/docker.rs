/*!
 * Thin wrappers around the `docker` CLI: build images, run and remove the
 * per-app container, and query its state.
 */

use std::path::Path;
use tokio::process::Command;
use super::process::{run_capture, run_logged};
use crate::db::model::App;

/**
 * The `key=value` label attached to every image and container we create, so
 * that everything belonging to one app can be found again by `app.id`.
 */
fn app_label(app: &App) -> String {
    const LABEL_KEY: &str = "tharhosting.app";
    format!("{LABEL_KEY}={}", app.id)
}

/**
 * Builds `image` from the app's Dockerfile (`app.dockerfile`, relative to
 * `context`), labelled with the app id.
 * Build output is streamed to the `log` file.
 * 
 * We fork the docker process and run it with arguments `build -t <image>
 * -f <context>/<dockerfile> --label <label> <context>`. Without `-f`, docker
 * only looks for a file named exactly `Dockerfile`.
 * 
 * The build output is streamed to the `log` file.
 *
 * # Errors
 *
 * Returns the failure message if `docker build` exits unsuccessfully.
 */
pub async fn build(
    docker: &str,
    app: &App,
    image: &str,
    context: &Path,
    log: &Path,
) -> Result<(), String> {
    let mut cmd = Command::new(docker);
    cmd.args(["build", "-t", image, "-f"])
        .arg(context.join(&app.dockerfile))
        .arg("--label")
        .arg(app_label(app))
        .arg(context);
    let shown = format!("docker build -t {image} -f {} .", app.dockerfile);
    run_logged(cmd, &shown, log).await
}

/**
 * Removes the app's current container (if any) and starts `image` in its place.
 *
 * The container publishes `container_port` on `bind:host_port`, restarts
 * automatically unless stopped, and receives the app's env vars. Env values are
 * masked (`***`) in the command echoed to the log, so secrets are not leaked.
 *
 * # Errors
 *
 * Returns the failure message if `docker run` exits unsuccessfully.
 */
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
    let mut args: Vec<String> = [
        "run",
        "-d",
        "--name",
        &name,
        "--restart",
        "unless-stopped",
        "--label",
        &app_label(app),
        "-p",
        &publish,
    ]
    .map(String::from)
    .into();

    // Heroku-style: tell the app which port to listen on unless overridden.
    if !app.env.contains_key("PORT") {
        args.extend(["-e".to_string(), format!("PORT={}", app.container_port)]);
    }
    for (key, value) in &app.env {
        args.extend(["-e".to_string(), format!("{key}={value}")]);
    }
    args.push(image.to_string());

    let masked_env: String = app.env.keys().map(|key| format!(" -e {key}=***")).collect();
    let shown = format!("docker run -d --name {name} -p {publish}{masked_env} {image}");

    let mut cmd = Command::new(docker);
    cmd.args(&args);
    run_logged(cmd, &shown, log).await
}

/**
 * Force-removes the app's container, running or not.
 *
 * # Errors
 *
 * Returns the failure message if there is no such container or docker fails.
 */
pub async fn remove_container(docker: &str, app: &App) -> Result<String, String> {
    let mut cmd = Command::new(docker);
    cmd.args(["rm", "-f", &app.container_name()]);
    run_capture(cmd).await
}

/**
 * Stops the app's container without removing it.
 *
 * # Errors
 *
 * Returns the failure message if there is no such container or docker fails.
 */
pub async fn stop(docker: &str, app: &App) -> Result<String, String> {
    let mut cmd = Command::new(docker);
    cmd.args(["stop", &app.container_name()]);
    run_capture(cmd).await
}

/**
 * Starts the app's existing, stopped container.
 *
 * # Errors
 *
 * Returns the failure message if there is no such container or docker fails.
 */
pub async fn start(docker: &str, app: &App) -> Result<String, String> {
    let mut cmd = Command::new(docker);
    cmd.args(["start", &app.container_name()]);
    run_capture(cmd).await
}

/**
 * Returns the container state as docker reports it (`"running"`, `"exited"`,
 * ...), or `None` if there is no container.
 */
pub async fn container_state(docker: &str, app: &App) -> Option<String> {
    let mut cmd = Command::new(docker);
    cmd.args(["inspect", "-f", "{{.State.Status}}", &app.container_name()]);
    run_capture(cmd).await.ok()
}

/**
 * Best-effort removal of every image built for this app.
 *
 * Failures are ignored: images that are missing or still in use are left alone.
 */
pub async fn remove_images(docker: &str, app: &App) {
    let mut list = Command::new(docker);
    list.args(["images", "-q", "--filter"])
        .arg(format!("label={}", app_label(app)));
    let Ok(output) = run_capture(list).await else {
        return;
    };

    let mut ids: Vec<&str> = output.lines().filter(|line| !line.is_empty()).collect();
    ids.dedup();
    if ids.is_empty() {
        return;
    }

    let mut remove = Command::new(docker);
    remove.args(["rmi", "-f"]).args(&ids);
    let _ = run_capture(remove).await;
}
