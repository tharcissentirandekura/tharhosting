/*!
 * Runtime configuration, read once at startup from `THARHOSTING_*`
 * environment variables.
 */

use std::path::PathBuf;

/**
 * Runtime configuration, read from `THARHOSTING_*` environment variables.
 */
#[derive(Debug, Clone)]
pub struct Config {
    /** Address the control-plane API binds to. */
    pub host: String,
    pub port: u16,
    /**
     * Where db.json, build checkouts and deploy logs live. Defaults to `data/`
     * in the directory the server is started from.
     */
    pub data_dir: PathBuf,
    /** First host port handed out to apps. */
    pub port_range_start: u16,
    /** Interface app containers publish their port on. */
    pub app_bind: String,
    /** Hostname used when printing app URLs. */
    pub public_host: String,
    pub docker_bin: String,
    pub git_bin: String,
    /** If set, every /api request needs `Authorization: Bearer <token>`. */
    pub api_token: Option<String>,
    /** If set, GitHub webhooks must carry a valid X-Hub-Signature-256. */
    pub webhook_secret: Option<String>,
}

/**
 * Reads env var `key`, falling back to `default` when it is unset or empty.
 */
fn env_or(key: &str, default: &str) -> String {
    env_opt(key).unwrap_or_else(|| default.to_string())
}

/**
 * Reads env var `key`, treating an unset or empty value as `None`.
 */
fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

impl Config {
    /**
     * Builds the configuration from the environment, applying defaults.
     *
     * # Errors
     *
     * Returns a message if a port variable is not a valid port number.
     */
    pub fn from_env() -> Result<Self, String> {
        let parse_port = |key: &str, default: &str| -> Result<u16, String> {
            env_or(key, default)
                .parse()
                .map_err(|_| format!("{key} must be a port number"))
        };
        // A relative path resolves against the directory the server is started
        // from, so the default `data/` lives inside the project. It is made
        // absolute once here so it stays the same for the whole run.
        let data_dir = PathBuf::from(env_or("THARHOSTING_DATA", "data"));
        let data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir);

        Ok(Config {
            host: env_or("THARHOSTING_HOST", "127.0.0.1"),
            port: parse_port("THARHOSTING_PORT", "8900")?,
            data_dir,
            port_range_start: parse_port("THARHOSTING_APP_PORT_START", "10000")?,
            app_bind: env_or("THARHOSTING_APP_BIND", "0.0.0.0"),
            public_host: env_or("THARHOSTING_PUBLIC_HOST", "localhost"),
            docker_bin: env_or("THARHOSTING_DOCKER", "docker"),
            git_bin: env_or("THARHOSTING_GIT", "git"),
            api_token: env_opt("THARHOSTING_TOKEN"),
            webhook_secret: env_opt("THARHOSTING_WEBHOOK_SECRET"),
        })
    }

    /** Path of the JSON database file. */
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("db.json")
    }

    /** Directory holding one log file per deployment. */
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /** Directory holding the git checkouts that get built. */
    pub fn builds_dir(&self) -> PathBuf {
        self.data_dir.join("builds")
    }
}
