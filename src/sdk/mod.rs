//! Thin async wrappers around the external tools tharhosting drives
//! (git and the docker CLI).

pub mod docker;
pub mod git;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

/// Append a line to a deploy log. Logging failures are never fatal.
pub fn log_line(log: &Path, msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(f, "{msg}");
    }
}

/// Run a command with stdout+stderr streamed into `log`.
/// `shown` is what gets echoed into the log (so secrets can be redacted).
pub async fn run_logged(
    mut cmd: Command,
    shown: &str,
    log: &Path,
) -> Result<(), String> {
    log_line(log, &format!("$ {shown}"));
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("cannot open log: {e}"))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let status = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .status()
        .await
        .map_err(|e| format!("failed to start `{shown}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{shown}` failed ({status})"))
    }
}

/// Run a command and return trimmed stdout, or stderr as the error.
pub async fn run_capture(mut cmd: Command) -> Result<String, String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
