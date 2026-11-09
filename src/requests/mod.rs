//! Request bodies accepted by the API, plus their validation.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::http::AppError;

#[derive(Debug, Deserialize)]
pub struct CreateAppRequest {
    pub name: String,
    pub repo_url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default = "default_container_port")]
    pub container_port: u16,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Kick off the first deploy right away.
    #[serde(default)]
    pub deploy: bool,
}

fn default_branch() -> String {
    "main".into()
}

fn default_container_port() -> u16 {
    8080
}

#[derive(Debug, Deserialize)]
pub struct UpdateAppRequest {
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub container_port: Option<u16>,
    /// Replaces the whole env map.
    pub env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    /// Byte offset to resume from (for tailing).
    #[serde(default)]
    pub offset: u64,
}

/// The parts of a GitHub `push` event we care about.
#[derive(Debug, Deserialize)]
pub struct GithubPush {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub repository: GithubRepo,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct GithubRepo {
    pub clone_url: Option<String>,
    pub html_url: Option<String>,
    pub ssh_url: Option<String>,
}

impl GithubRepo {
    pub fn urls(&self) -> impl Iterator<Item = &str> {
        [&self.clone_url, &self.html_url, &self.ssh_url]
            .into_iter()
            .flatten()
            .map(String::as_str)
    }
}

pub fn validate_name(name: &str) -> Result<(), AppError> {
    let ok = (1..=40).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "name must be 1-40 chars of a-z, 0-9 and '-', not starting or ending with '-'".into(),
        ))
    }
}

pub fn validate_repo_url(url: &str) -> Result<(), AppError> {
    let ok = ["https://", "http://", "ssh://", "git@", "file://"]
        .iter()
        .any(|p| url.starts_with(p))
        && !url.chars().any(char::is_whitespace);
    if ok {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "repo_url must be an https://, ssh://, git@ or file:// git URL".into(),
        ))
    }
}

pub fn validate_branch(branch: &str) -> Result<(), AppError> {
    let ok = !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains("..")
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(AppError::BadRequest("invalid branch name".into()))
    }
}

pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), AppError> {
    for k in env.keys() {
        let ok = !k.is_empty()
            && !k.starts_with(|c: char| c.is_ascii_digit())
            && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !ok {
            return Err(AppError::BadRequest(format!("invalid env var name: {k:?}")));
        }
    }
    Ok(())
}

pub fn validate_port(port: u16) -> Result<(), AppError> {
    if port == 0 {
        Err(AppError::BadRequest("container_port must be 1-65535".into()))
    } else {
        Ok(())
    }
}

/// Reduce a git URL to `host/owner/repo` so https, ssh and `.git`
/// variants of the same repo compare equal.
pub fn normalize_repo_url(url: &str) -> String {
    let mut s = url.trim().to_ascii_lowercase();
    for p in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
        }
    }
    if let Some(at) = s.find('@') {
        // git@github.com:owner/repo or user@host/...
        if !s[..at].contains('/') {
            s = s[at + 1..].to_string();
        }
    }
    s = s.replacen(':', "/", 1);
    let s = s.trim_end_matches('/');
    s.strip_suffix(".git").unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_urls_normalize_to_the_same_key() {
        let want = "github.com/thar/site";
        for u in [
            "https://github.com/Thar/site",
            "https://github.com/thar/site.git",
            "https://github.com/thar/site/",
            "git@github.com:thar/site.git",
            "ssh://git@github.com/thar/site.git",
        ] {
            assert_eq!(normalize_repo_url(u), want, "{u}");
        }
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("my-app2").is_ok());
        for bad in ["", "-a", "a-", "My", "a_b", &"x".repeat(41)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn branches_and_urls_are_validated() {
        assert!(validate_branch("feature/x-1.2").is_ok());
        assert!(validate_branch("--upload-pack=x").is_err());
        assert!(validate_repo_url("https://github.com/a/b").is_ok());
        assert!(validate_repo_url("--config=x").is_err());
        assert!(validate_repo_url("/etc").is_err());
    }
}
