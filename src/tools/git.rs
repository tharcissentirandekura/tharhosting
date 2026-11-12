/*!
 * Wrappers around the `git` CLI: cloning a repository and reading its HEAD commit.
 */

use std::path::Path;

use tokio::process::Command;

use super::process::{run_capture, run_logged};

/**
 * Shallow-clones a single branch of `url` into `dest`, which must not exist yet.
 *
 * Only the latest commit is fetched (`--depth 1`), which keeps clones fast.
 * Git's output is streamed to the `log` file. The `--` before the URL stops git
 * from ever reading the URL as a command-line option.
 *
 * # Errors
 *
 * Returns the failure message if `git clone` fails, for example when the
 * branch doesn't exist or the repository is private.
 */
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

/**
 * Returns the full commit SHA that `HEAD` points to in the repository at `repo`.
 *
 * # Errors
 *
 * Returns git's error output if `repo` is not a git repository.
 */
pub async fn head_commit(git: &str, repo: &Path) -> Result<String, String> {
    let mut cmd = Command::new(git);
    cmd.arg("-C").arg(repo).args(["rev-parse", "HEAD"]);
    run_capture(cmd).await
}
