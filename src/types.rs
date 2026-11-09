use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::db::store::Store;

/// Runtime configuration, read from `THARHOSTING_*` environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the control-plane API binds to.
    pub host: String,
    pub port: u16,
    /// Where db.json, build checkouts and deploy logs live.
    pub data_dir: PathBuf,
    /// First host port handed out to apps.
    pub port_range_start: u16,
    /// Interface app containers publish their port on.
    pub app_bind: String,
    /// Hostname used when printing app URLs.
    pub public_host: String,
    pub docker_bin: String,
    pub git_bin: String,
    /// If set, every /api request needs `Authorization: Bearer <token>`.
    pub api_token: Option<String>,
    /// If set, GitHub webhooks must carry a valid X-Hub-Signature-256.
    pub webhook_secret: Option<String>,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let parse_port = |key: &str, default: &str| -> Result<u16, String> {
            env_or(key, default)
                .parse()
                .map_err(|_| format!("{key} must be a port number"))
        };
        Ok(Config {
            host: env_or("THARHOSTING_HOST", "127.0.0.1"),
            port: parse_port("THARHOSTING_PORT", "8900")?,
            data_dir: PathBuf::from(env_or(
                "THARHOSTING_DATA",
                &format!("{home}/.tharhosting"),
            )),
            port_range_start: parse_port("THARHOSTING_APP_PORT_START", "10000")?,
            app_bind: env_or("THARHOSTING_APP_BIND", "0.0.0.0"),
            public_host: env_or("THARHOSTING_PUBLIC_HOST", "localhost"),
            docker_bin: env_or("THARHOSTING_DOCKER", "docker"),
            git_bin: env_or("THARHOSTING_GIT", "git"),
            api_token: env_opt("THARHOSTING_TOKEN"),
            webhook_secret: env_opt("THARHOSTING_WEBHOOK_SECRET"),
        })
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("db.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn builds_dir(&self) -> PathBuf {
        self.data_dir.join("builds")
    }
}

/// Shared state handed to every request handler and deploy task.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub config: Arc<Config>,
    /// Apps that currently have a deploy running (one at a time per app).
    pub inflight: Arc<Mutex<HashSet<Uuid>>>,
}

impl AppState {
    pub fn new(config: Config, store: Store) -> Self {
        AppState {
            store: Arc::new(store),
            config: Arc::new(config),
            inflight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn is_deploying(&self, app_id: Uuid) -> bool {
        self.inflight.lock().unwrap().contains(&app_id)
    }
}
