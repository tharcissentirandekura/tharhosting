use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppStatus {
    /// Registered, never deployed.
    Created,
    Deploying,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Queued,
    Building,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Manual,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: Uuid,
    /// DNS-ish slug, unique; used for the container and image names.
    pub name: String,
    pub repo_url: String,
    pub branch: String,
    /// Port the app listens on inside its container.
    pub container_port: u16,
    /// Port on the host that forwards to `container_port`.
    pub host_port: u16,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub status: AppStatus,
    pub current_deployment: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl App {
    pub fn container_name(&self) -> String {
        format!("th-{}", self.name)
    }

    pub fn image_repo(&self) -> String {
        format!("tharhosting/{}", self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub id: Uuid,
    pub app_id: Uuid,
    pub status: DeploymentStatus,
    pub trigger: Trigger,
    /// Commit SHA that was built, once known.
    pub commit: Option<String>,
    pub image: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Deployment {
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

/// Everything persisted in db.json.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub apps: Vec<App>,
    #[serde(default)]
    pub deployments: Vec<Deployment>,
}

impl Data {
    /// Look an app up by UUID or by name.
    pub fn find_app(&self, key: &str) -> Option<&App> {
        match Uuid::parse_str(key) {
            Ok(id) => self.apps.iter().find(|a| a.id == id),
            Err(_) => self.apps.iter().find(|a| a.name == key),
        }
    }

    pub fn app(&self, id: Uuid) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    pub fn app_mut(&mut self, id: Uuid) -> Option<&mut App> {
        self.apps.iter_mut().find(|a| a.id == id)
    }

    pub fn deployment(&self, id: Uuid) -> Option<&Deployment> {
        self.deployments.iter().find(|d| d.id == id)
    }

    pub fn deployment_mut(&mut self, id: Uuid) -> Option<&mut Deployment> {
        self.deployments.iter_mut().find(|d| d.id == id)
    }

    /// Smallest port >= `start` not already assigned to an app and
    /// currently bindable on this machine.
    pub fn next_free_port(&self, start: u16) -> Option<u16> {
        (start..=u16::MAX).find(|p| {
            !self.apps.iter().any(|a| a.host_port == *p)
                && std::net::TcpListener::bind(("0.0.0.0", *p)).is_ok()
        })
    }

    /// Mark anything left mid-flight by a previous process as failed.
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
