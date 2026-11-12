/*!
 * The data model: apps, their deployments, and the `Data` container that is
 * saved to `db.json`.
 */

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/**
 * Where an app is in its lifecycle.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppStatus {
    /** Registered, never deployed. */
    Created,
    Deploying,
    Running,
    Stopped,
    Failed,
}

/**
 * Progress of a single deployment attempt.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Queued,
    Building,
    Succeeded,
    Failed,
}

/**
 * What started a deployment.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /** Requested through the API. */
    Manual,
    /** Started by a GitHub push event. */
    Webhook,
}

/**
 * The Dockerfile used when an app doesn't name one: `Dockerfile` at the
 * repository root.
 *
 * Also the `serde` default for [`App::dockerfile`], so apps saved before that
 * field existed load with this value.
 */
pub fn default_dockerfile() -> String {
    "Dockerfile".into()
}

/**
 * An application tharhosting builds from a git repository and runs in a container.
 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: Uuid,
    /** DNS-ish slug, unique; used for the container and image names. */
    pub name: String,
    pub repo_url: String,
    pub branch: String,
    /** Path of the Dockerfile inside the repo, relative to the repo root. */
    #[serde(default = "default_dockerfile")]
    pub dockerfile: String,
    /** Port the app listens on inside its container. */
    pub container_port: u16,
    /** Port on the host that forwards to `container_port`. */
    pub host_port: u16,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub status: AppStatus,
    pub current_deployment: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl App {
    /** Name of the app's docker container: `th-<name>`. */
    pub fn container_name(&self) -> String {
        format!("th-{}", self.name)
    }

    /** Docker image repository for the app: `tharhosting/<name>`. */
    pub fn image_repo(&self) -> String {
        format!("tharhosting/{}", self.name)
    }
}

/**
 * One attempt to build and run an app.
 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub id: Uuid,
    pub app_id: Uuid,
    pub status: DeploymentStatus,
    pub trigger: Trigger,
    /** Commit SHA that was built, once known. */
    pub commit: Option<String>,
    pub image: Option<String>,
    /** Why the deployment failed, if it did. */
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Deployment {
    /** Creates a queued deployment for `app_id` with a fresh id. */
    pub fn new(app_id: Uuid, trigger: Trigger) -> Self {
        Deployment {
            id: Uuid::new_v4(),
            app_id,
            status: DeploymentStatus::Queued,
            trigger,
            commit: None,
            image: None,
            error: None,
            created_at: Utc::now(),
            finished_at: None,
        }
    }
}

/**
 * Everything persisted in db.json.
 */
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub apps: Vec<App>,
    #[serde(default)]
    pub deployments: Vec<Deployment>,
}

impl Data {
    /** Looks an app up by UUID or, if `key` isn't a UUID, by name. */
    pub fn find_app(&self, key: &str) -> Option<&App> {
        match Uuid::parse_str(key) {
            Ok(id) => self.apps.iter().find(|a| a.id == id),
            Err(_) => self.apps.iter().find(|a| a.name == key),
        }
    }

    /** Looks an app up by id. */
    pub fn app(&self, id: Uuid) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    /** Looks an app up by id, for modification. */
    pub fn app_mut(&mut self, id: Uuid) -> Option<&mut App> {
        self.apps.iter_mut().find(|a| a.id == id)
    }

    /** Looks a deployment up by id. */
    pub fn deployment(&self, id: Uuid) -> Option<&Deployment> {
        self.deployments.iter().find(|d| d.id == id)
    }

    /** Looks a deployment up by id, for modification. */
    pub fn deployment_mut(&mut self, id: Uuid) -> Option<&mut Deployment> {
        self.deployments.iter_mut().find(|d| d.id == id)
    }

    /**
     * Returns the smallest port >= `start` that is not already assigned to an
     * app and can currently be bound on this machine, or `None` if there is none.
     */
    pub fn next_free_port(&self, start: u16) -> Option<u16> {
        (start..=u16::MAX).find(|p| {
            !self.apps.iter().any(|a| a.host_port == *p)
                && std::net::TcpListener::bind(("0.0.0.0", *p)).is_ok()
        })
    }

    /**
     * Marks anything left mid-flight by a previous process as failed, and
     * returns how many deployments were affected.
     *
     * Called at startup: a deployment that was `Queued` or `Building` when the
     * server stopped will never finish, so it is closed out as failed. Apps
     * stuck in `Deploying` are set to `Failed` too.
     */
    pub fn recover_interrupted(&mut self) -> usize {
        let now = Utc::now();
        let mut n = 0;
        for d in &mut self.deployments {
            if matches!(d.status, DeploymentStatus::Queued | DeploymentStatus::Building) {
                d.status = DeploymentStatus::Failed;
                d.error = Some("interrupted: tharhosting restarted mid-deploy".into());
                d.finished_at = Some(now);
                n += 1;
            }
        }
        for a in &mut self.apps {
            if a.status == AppStatus::Deploying {
                a.status = AppStatus::Failed;
                a.updated_at = now;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_saved_before_dockerfile_existed_default_to_dockerfile() {
        let old_record = r#"{
            "id": "6f1d1c3e-8f0a-4c52-9d0a-0e3b6f6f2a11",
            "name": "old", "repo_url": "https://github.com/x/y", "branch": "main",
            "container_port": 8080, "host_port": 10000, "status": "created",
            "current_deployment": null,
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let app: App = serde_json::from_str(old_record).unwrap();
        assert_eq!(app.dockerfile, "Dockerfile");
    }
}
