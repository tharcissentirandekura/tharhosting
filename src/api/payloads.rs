/*!
 * Request bodies and query parameters accepted by the API.
 *
 * These are plain data shapes. Checking their values is done in
 * [`validation`](super::validation).
 */

use std::collections::BTreeMap;

use serde::Deserialize;

/**
 * Request body for creating a new application.
 *
 * Only `name` and `repo_url` are required. `branch`, `dockerfile`,
 * `container_port`, `env` and `deploy` fall back to defaults when missing from
 * the JSON.
 */
#[derive(Debug, Deserialize)]
pub struct CreateAppRequest {
    pub name: String,
    pub repo_url: String,
    // `default_branch` supplies the value when the field is missing from the JSON.
    #[serde(default = "default_branch")]
    pub branch: String,
    /** Dockerfile path inside the repo, relative to its root. */
    #[serde(default = "crate::db::model::default_dockerfile")]
    pub dockerfile: String,
    #[serde(default = "default_container_port")]
    pub container_port: u16,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /** Kick off the first deploy right away. */
    #[serde(default)]
    pub deploy: bool,
}

/**
 * Returns the default branch name for a new application.
 */
fn default_branch() -> String {
    "main".into()
}

/**
 * Returns the default container port for a new application.
 */
fn default_container_port() -> u16 {
    8080
}

/**
 * Request body for updating an existing application.
 *
 * Every field is optional, so a client sends only what it wants to change.
 * A missing field means "leave unchanged".
 */
#[derive(Debug, Deserialize)]
pub struct UpdateAppRequest {
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub dockerfile: Option<String>,
    pub container_port: Option<u16>,
    /** Replaces the whole env map. */
    pub env: Option<BTreeMap<String, String>>,
}

/**
 * Query parameters for reading a deployment's logs.
 *
 * `offset` lets a client resume from the byte position where it stopped, which
 * is how log tailing works without re-sending the whole file.
 */
#[derive(Debug, Deserialize)]
pub struct LogQuery {
    /** Byte offset to resume from (for tailing). */
    #[serde(default)]
    pub offset: u64,
}
