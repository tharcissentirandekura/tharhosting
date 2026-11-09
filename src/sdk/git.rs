use std::path::Path;

use tokio::process::Command;

use super::{run_capture, run_logged};

/// Shallow-clone a single branch into `dest` (which must not exist).
pub async fn clone(
    git: &str,
    url: &str,
    branch: &str,
    dest: &Path,
    log: &Path,
) -> Result<(), String> {
    let mut cmd = Command::new(git);
    cmd.args(["clone", "--depth", "1", "--single-branch", "--branch", branch, "--", url])
        .arg(dest)
        // Fail fast on private repos instead of hanging on a credential prompt.
        .env("GIT_TERMINAL_PROMPT", "0");
    let shown = format!("git clone --depth 1 --branch {branch} {url}");
    run_logged(cmd, &shown, log).await
}

pub async fn head_commit(git: &str, repo: &Path) -> Result<String, String> {
    let mut cmd = Command::new(git);
    cmd.arg("-C").arg(repo).args(["rev-parse", "HEAD"]);
    run_capture(cmd).await
}
