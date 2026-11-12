/*!
 * The shared application state passed to request handlers and deploy tasks.
 */

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::config::Config;
use crate::db::store::Store;

/**
 * Shared state handed to every request handler and deploy task.
 *
 * Cloning is cheap: every field is behind an `Arc`.
 */
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub config: Arc<Config>,
    /** Apps that currently have a deploy running (one at a time per app). */
    pub inflight: Arc<Mutex<HashSet<Uuid>>>,
}

impl AppState {
    /** Creates the state with no deploys in flight. */
    pub fn new(config: Config, store: Store) -> Self {
        AppState {
            store: Arc::new(store),
            config: Arc::new(config),
            inflight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /** Returns whether `app_id` currently has a deploy running. */
    pub fn is_deploying(&self, app_id: Uuid) -> bool {
        self.inflight.lock().unwrap().contains(&app_id)
    }
}
