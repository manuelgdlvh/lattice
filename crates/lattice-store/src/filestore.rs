//! TOML-on-disk implementation of all `Store` traits.
//!
//! Every entity is serialized via `toml::to_string_pretty` and written
//! atomically through [`crate::fs::atomic_write_bytes`]. Blocking I/O
//! is wrapped in `tokio::task::spawn_blocking` so the async traits
//! don't stall the tokio executor.
//!
//! The concrete layout matches [`crate::paths::Paths`] docs exactly.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use tokio::task;

use lattice_core::entities::{Settings, Task, Template};
use lattice_core::ids::{TaskId, TemplateId};

use crate::error::{StoreError, StoreResult};
use crate::fs::{atomic_write_str, read_optional_bytes, remove_dir_if_exists};
use crate::paths::Paths;
use crate::store::{SettingsStore, Tasks, Templates};

/// Shared backend: holds the paths and provides sync helpers. The
/// `Store` traits wrap these helpers in `spawn_blocking`.
#[derive(Clone, Debug)]
pub struct FileStore {
    paths: Paths,
}

impl FileStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// Blocking helper: load an entity from a single TOML file, returning
    /// `Ok(None)` when the file is missing.
    fn load_toml_blocking<T: DeserializeOwned>(path: &Path) -> StoreResult<Option<T>> {
        let Some(bytes) = read_optional_bytes(path)? else {
            return Ok(None);
        };
        let s = std::str::from_utf8(&bytes).map_err(|e| {
            StoreError::io(
                path,
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })?;
        let value = toml::from_str(s).map_err(|e| StoreError::toml_decode(path, e))?;
        Ok(Some(value))
    }

    fn save_toml_blocking<T: Serialize>(path: &Path, value: &T) -> StoreResult<()> {
        let s = toml::to_string_pretty(value)?;
        atomic_write_str(path, &s)
    }

    /// List immediate child directory names under `root`.
    ///
    /// Returns an empty vec when `root` doesn't exist (no entities yet).
    fn list_child_dirs_blocking(root: &Path) -> StoreResult<Vec<String>> {
        let entries = match std::fs::read_dir(root) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::io(root, e)),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| StoreError::io(root, e))?;
            if entry
                .file_type()
                .map_err(|e| StoreError::io(entry.path(), e))?
                .is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                out.push(name.to_string());
            }
        }
        out.sort();
        Ok(out)
    }

}

/// Run a blocking closure on the tokio blocking pool. Flattens the join
/// error into an `io::Error` so the `StoreError::Io` path handles it.
async fn blocking<F, T>(path: PathBuf, f: F) -> StoreResult<T>
where
    F: FnOnce() -> StoreResult<T> + Send + 'static,
    T: Send + 'static,
{
    match task::spawn_blocking(f).await {
        Ok(res) => res,
        Err(join_err) => Err(StoreError::io(
            path,
            std::io::Error::other(join_err.to_string()),
        )),
    }
}

// -------- Templates --------------------------------------------------

#[async_trait]
impl Templates for FileStore {
    async fn list(&self) -> StoreResult<Vec<Template>> {
        let root = self.paths.templates_dir();
        let ids = blocking(root.clone(), move || {
            FileStore::list_child_dirs_blocking(&root)
        })
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let path = self.paths.template_file(&id);
            if let Some(t) = blocking(path.clone(), move || {
                FileStore::load_toml_blocking::<Template>(&path)
            })
            .await?
            {
                out.push(t);
            }
        }
        Ok(out)
    }

    async fn load(&self, id: TemplateId) -> StoreResult<Option<Template>> {
        let path = self.paths.template_file(&id.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Template>(&path)
        })
        .await
    }

    async fn save(&self, template: &Template) -> StoreResult<()> {
        let path = self.paths.template_file(&template.id.to_string());
        let value = template.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn delete(&self, id: TemplateId) -> StoreResult<()> {
        let dir = self.paths.template_dir(&id.to_string());
        blocking(dir.clone(), move || remove_dir_if_exists(&dir)).await
    }
}

// -------- Tasks ------------------------------------------------------

#[async_trait]
impl Tasks for FileStore {
    async fn list(&self) -> StoreResult<Vec<Task>> {
        let dir = self.paths.tasks_root();
        let ids = blocking(dir.clone(), move || {
            FileStore::list_child_dirs_blocking(&dir)
        })
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let path = self.paths.task_file(&id);
            if let Some(t) = blocking(path.clone(), move || {
                FileStore::load_toml_blocking::<Task>(&path)
            })
            .await?
            {
                out.push(t);
            }
        }
        Ok(out)
    }

    async fn load(&self, id: TaskId) -> StoreResult<Option<Task>> {
        let path = self.paths.task_file(&id.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Task>(&path)
        })
        .await
    }

    async fn save(&self, task: &Task) -> StoreResult<()> {
        let path = self.paths.task_file(&task.id.to_string());
        let value = task.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn save_snapshot(&self, task: &Task, template: &Template) -> StoreResult<()> {
        let path = self.paths.task_template_snapshot(&task.id.to_string());
        let value = template.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn save_prompt(&self, task: &Task, prompt: &str) -> StoreResult<()> {
        let path = self.paths.task_prompt(&task.id.to_string());
        let body = prompt.to_string();
        blocking(path.clone(), move || atomic_write_str(&path, &body)).await
    }

    async fn delete(&self, id: TaskId) -> StoreResult<()> {
        let dir = self.paths.task_dir(&id.to_string());
        blocking(dir.clone(), move || remove_dir_if_exists(&dir)).await
    }
}

// -------- Settings ---------------------------------------------------

#[async_trait]
impl SettingsStore for FileStore {
    async fn load(&self) -> StoreResult<Settings> {
        let path = self.paths.settings_file();
        let loaded: Option<Settings> = blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Settings>(&path)
        })
        .await?;
        Ok(loaded.unwrap_or_default())
    }

    async fn save(&self, settings: &Settings) -> StoreResult<()> {
        let path = self.paths.settings_file();
        let value = settings.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::entities::{Task, Template};
    use lattice_core::ids::TemplateId;
    use lattice_core::time::Timestamp;
    use tempfile::TempDir;

    fn now() -> Timestamp {
        Timestamp::parse("2026-04-24T10:00:00Z").unwrap()
    }

    fn mkstore() -> (FileStore, TempDir, TempDir) {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());
        (FileStore::new(paths), cfg, state)
    }

    #[tokio::test]
    async fn templates_roundtrip() {
        let (store, _c, _s) = mkstore();
        let t = Template::new("bug-fix", now());
        Templates::save(&store, &t).await.unwrap();
        assert_eq!(Templates::list(&store).await.unwrap().len(), 1);
        let loaded = Templates::load(&store, t.id).await.unwrap().unwrap();
        assert_eq!(loaded, t);
        Templates::delete(&store, t.id).await.unwrap();
        assert!(Templates::load(&store, t.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn tasks_with_snapshot_and_prompt() {
        let (store, _c, _s) = mkstore();
        let template = Template::new("t", now());
        Templates::save(&store, &template).await.unwrap();

        let task = Task::new(template.id, template.version, "name", now());
        Tasks::save(&store, &task).await.unwrap();
        Tasks::save_snapshot(&store, &task, &template)
            .await
            .unwrap();
        Tasks::save_prompt(&store, &task, "## rendered prompt")
            .await
            .unwrap();

        let paths = store.paths().clone();
        assert!(paths.task_file(&task.id.to_string()).exists());
        assert!(paths.task_template_snapshot(&task.id.to_string()).exists());
        assert_eq!(
            std::fs::read_to_string(paths.task_prompt(&task.id.to_string())).unwrap(),
            "## rendered prompt"
        );

        let listed = Tasks::list(&store).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, task.id);
    }

    #[tokio::test]
    async fn settings_defaults_when_missing() {
        let (store, _c, _s) = mkstore();
        let s = SettingsStore::load(&store).await.unwrap();
        assert_eq!(s, lattice_core::entities::Settings::default());
    }

    #[tokio::test]
    async fn settings_persist_across_loads() {
        let (store, _c, _s) = mkstore();
        let mut s = lattice_core::entities::Settings::default();
        s.cache.max_entries = 123;
        SettingsStore::save(&store, &s).await.unwrap();
        let loaded = SettingsStore::load(&store).await.unwrap();
        assert_eq!(loaded.cache.max_entries, 123);
    }

    #[tokio::test]
    async fn delete_nonexistent_is_noop() {
        let (store, _c, _s) = mkstore();
        // Deleting something that was never saved must succeed silently.
        Templates::delete(&store, TemplateId::new()).await.unwrap();
    }

    #[tokio::test]
    async fn list_empty_before_any_write() {
        let (store, _c, _s) = mkstore();
        // None of the root dirs exist yet; list must still succeed.
        assert!(Templates::list(&store).await.unwrap().is_empty());
    }
}
