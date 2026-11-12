/*!
 * A small JSON-file database: the whole dataset is kept in memory and saved to
 * disk on every change.
 */

use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use super::model::Data;

/**
 * A tiny JSON-file database. The whole dataset lives in memory behind a
 * mutex and is rewritten atomically (tmp file + rename) on every change.
 *
 * Writing to a temporary file and renaming it means a crash mid-write can never
 * leave a half-written `db.json`.
 */
pub struct Store {
    path: PathBuf,
    data: Mutex<Data>,
}

impl Store {
    /**
     * Opens the store at `path`, creating the parent directory if needed.
     * An existing file is loaded. Otherwise the store starts empty.
     *
     * # Errors
     *
     * Returns an error if the directory can't be created, the file can't be
     * read, or its contents are not valid JSON.
     */
    pub fn open(path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            serde_json::from_str(&raw)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        } else {
            Data::default()
        };
        Ok(Store {
            path,
            data: Mutex::new(data),
        })
    }

    /**
     * Runs `f` with read access to the data and returns its result.
     *
     * The lock is held while `f` runs, so keep it short and don't call back
     * into the store from inside it.
     */
    pub fn read<R>(&self, f: impl FnOnce(&Data) -> R) -> R {
        f(&self.data.lock().unwrap())
    }

    /**
     * Runs `f` with write access to the data, then saves the result to disk
     * before releasing the lock. Returns whatever `f` returned.
     *
     * # Errors
     *
     * Returns an error if saving to disk fails. The in-memory change has
     * already been applied by then.
     */
    pub fn write<R>(&self, f: impl FnOnce(&mut Data) -> R) -> io::Result<R> {
        let mut guard = self.data.lock().unwrap();
        let out = f(&mut guard);
        self.persist(&guard)?;
        Ok(out)
    }

    /**
     * Saves `data` to disk atomically: write a `.json.tmp` file, then rename it
     * over the real one.
     */
    fn persist(&self, data: &Data) -> io::Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        let raw = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::model::{App, AppStatus};
    use chrono::Utc;
    use uuid::Uuid;

    fn app(name: &str, port: u16) -> App {
        App {
            id: Uuid::new_v4(),
            name: name.into(),
            repo_url: "https://github.com/x/y".into(),
            branch: "main".into(),
            dockerfile: "Dockerfile".into(),
            container_port: 8080,
            host_port: port,
            env: Default::default(),
            status: AppStatus::Created,
            current_deployment: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.json");
        let store = Store::open(path.clone()).unwrap();
        let a = app("hello", 10000);
        store.write(|d| d.apps.push(a.clone())).unwrap();

        let reopened = Store::open(path).unwrap();
        let name = reopened.read(|d| d.find_app(&a.id.to_string()).map(|x| x.name.clone()));
        assert_eq!(name.as_deref(), Some("hello"));
        assert!(reopened.read(|d| d.find_app("hello").is_some()));
    }

    #[test]
    fn port_allocation_skips_taken_ports() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("db.json")).unwrap();
        let held = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let base = held.local_addr().unwrap().port();
        store.write(|d| d.apps.push(app("a", base + 1))).unwrap();
        // `base` is bound by the listener, `base + 1` is assigned to an app.
        let p = store.read(|d| d.next_free_port(base)).unwrap();
        assert!(p >= base + 2);
    }
}
