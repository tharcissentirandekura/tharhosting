/*!
 * Helpers for running external commands and writing deploy logs, shared by
 * the `git` and `docker` wrappers.
 */

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

/**
 * Opens the deploy log for appending, creating it if it doesn't exist.
 */
fn open_log(log: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(log)
}

/**
 * Appends one line to the deploy log.
 *
 * Logging is best-effort: if the log can't be opened or written, the error is
 * ignored so that a logging problem never fails a deploy.
 */
pub fn log_line(log: &Path, msg: &str) {
    if let Ok(mut file) = open_log(log) {
        let _ = writeln!(file, "{msg}");
    }
}

/**
 * Runs `cmd` with its stdout and stderr streamed into the `log` file.
 *
 * `shown` is the command line echoed into the log as `$ <shown>`. It is passed
 * separately from `cmd` so that secrets (such as env values) can be redacted.
 * The command's stdin is closed, so it can never block waiting for input.
 *
 * # Errors
 *
 * Returns a message if the log can't be opened, the command fails to start,
 * or it exits with a non-zero status.
 */
pub async fn run_logged(mut cmd: Command, shown: &str, log: &Path) -> Result<(), String> {
    log_line(log, &format!("$ {shown}"));

    let stdout = open_log(log).map_err(|e| format!("cannot open log: {e}"))?;
    let stderr = stdout.try_clone().map_err(|e| e.to_string())?;

    let status = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .await
        .map_err(|e| format!("failed to start `{shown}`: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("`{shown}` failed ({status})"))
    }
}

/**
 * Runs `cmd` and returns its trimmed stdout. The command's stdin is closed.
 *
 * # Errors
 *
 * Returns the command's trimmed stderr if it exits with a non-zero status, or
 * the OS error message if it can't be started.
 */
pub async fn run_capture(mut cmd: Command) -> Result<String, String> {
    let output = cmd
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
