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

use lattice_core::entities::{Project, Queue, Run, RunExit, Settings, Task, Template};
use lattice_core::ids::{ProjectId, RunId, TaskId, TemplateId};

use crate::error::{StoreError, StoreResult};
use crate::fs::{atomic_write_str, read_optional_bytes, remove_dir_if_exists, remove_if_exists};
use crate::paths::Paths;
use crate::store::{Projects, Queues, Runs, SettingsStore, Tasks, Templates};

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

    /// List `*.toml` files in a flat directory; returns stem names sorted.
    fn list_toml_stems_blocking(root: &Path) -> StoreResult<Vec<String>> {
        let entries = match std::fs::read_dir(root) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::io(root, e)),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| StoreError::io(root, e))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                out.push(stem.to_string());
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

// -------- Projects ---------------------------------------------------

#[async_trait]
impl Projects for FileStore {
    async fn list(&self) -> StoreResult<Vec<Project>> {
        let root = self.paths.projects_dir();
        let ids = blocking(root.clone(), move || {
            FileStore::list_child_dirs_blocking(&root)
        })
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let path = self.paths.project_file(&id);
            if let Some(p) = blocking(path.clone(), move || {
                FileStore::load_toml_blocking::<Project>(&path)
            })
            .await?
            {
                out.push(p);
            }
        }
        Ok(out)
    }

    async fn load(&self, id: ProjectId) -> StoreResult<Option<Project>> {
        let path = self.paths.project_file(&id.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Project>(&path)
        })
        .await
    }

    async fn save(&self, project: &Project) -> StoreResult<()> {
        let path = self.paths.project_file(&project.id.to_string());
        let value = project.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn delete(&self, id: ProjectId) -> StoreResult<()> {
        let dir = self.paths.project_dir(&id.to_string());
        blocking(dir.clone(), move || remove_dir_if_exists(&dir)).await
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
    async fn list_for_project(&self, project: ProjectId) -> StoreResult<Vec<Task>> {
        let pid = project.to_string();
        let dir = self.paths.tasks_dir(&pid);
        let ids = blocking(dir.clone(), move || {
            FileStore::list_child_dirs_blocking(&dir)
        })
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let path = self.paths.task_file(&pid, &id);
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

    async fn load(&self, project: ProjectId, id: TaskId) -> StoreResult<Option<Task>> {
        let path = self.paths.task_file(&project.to_string(), &id.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Task>(&path)
        })
        .await
    }

    async fn save(&self, task: &Task) -> StoreResult<()> {
        let path = self
            .paths
            .task_file(&task.project_id.to_string(), &task.id.to_string());
        let value = task.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn save_snapshot(&self, task: &Task, template: &Template) -> StoreResult<()> {
        let path = self
            .paths
            .task_template_snapshot(&task.project_id.to_string(), &task.id.to_string());
        let value = template.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn save_prompt(&self, task: &Task, prompt: &str) -> StoreResult<()> {
        let path = self
            .paths
            .task_prompt(&task.project_id.to_string(), &task.id.to_string());
        let body = prompt.to_string();
        blocking(path.clone(), move || atomic_write_str(&path, &body)).await
    }

    async fn delete(&self, project: ProjectId, id: TaskId) -> StoreResult<()> {
        let dir = self.paths.task_dir(&project.to_string(), &id.to_string());
        blocking(dir.clone(), move || remove_dir_if_exists(&dir)).await
    }
}

// -------- Runs -------------------------------------------------------

#[async_trait]
impl Runs for FileStore {
    async fn list_for_project(&self, project: ProjectId) -> StoreResult<Vec<Run>> {
        let pid = project.to_string();
        let dir = self.paths.runs_dir(&pid);
        let ids = blocking(dir.clone(), move || {
            FileStore::list_child_dirs_blocking(&dir)
        })
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let path = self.paths.run_file(&pid, &id);
            if let Some(r) = blocking(path.clone(), move || {
                FileStore::load_toml_blocking::<Run>(&path)
            })
            .await?
            {
                out.push(r);
            }
        }
        Ok(out)
    }

    async fn load(&self, project: ProjectId, id: RunId) -> StoreResult<Option<Run>> {
        let path = self.paths.run_file(&project.to_string(), &id.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Run>(&path)
        })
        .await
    }

    async fn save(&self, run: &Run) -> StoreResult<()> {
        let path = self
            .paths
            .run_file(&run.project_id.to_string(), &run.id.to_string());
        let value = run.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn save_exit(&self, project: ProjectId, id: RunId, exit: &RunExit) -> StoreResult<()> {
        let path = self
            .paths
            .run_exit_file(&project.to_string(), &id.to_string());
        let value = exit.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn load_exit(&self, project: ProjectId, id: RunId) -> StoreResult<Option<RunExit>> {
        let path = self
            .paths
            .run_exit_file(&project.to_string(), &id.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<RunExit>(&path)
        })
        .await
    }

    async fn delete(&self, project: ProjectId, id: RunId) -> StoreResult<()> {
        let dir = self.paths.run_dir(&project.to_string(), &id.to_string());
        blocking(dir.clone(), move || remove_dir_if_exists(&dir)).await
    }
}

// -------- Queues -----------------------------------------------------

#[async_trait]
impl Queues for FileStore {
    async fn list(&self) -> StoreResult<Vec<Queue>> {
        let dir = self.paths.queues_dir();
        let stems = blocking(dir.clone(), move || {
            FileStore::list_toml_stems_blocking(&dir)
        })
        .await?;
        let mut out = Vec::with_capacity(stems.len());
        for stem in stems {
            let path = self.paths.queue_file(&stem);
            if let Some(q) = blocking(path.clone(), move || {
                FileStore::load_toml_blocking::<Queue>(&path)
            })
            .await?
            {
                out.push(q);
            }
        }
        Ok(out)
    }

    async fn load(&self, project: ProjectId) -> StoreResult<Option<Queue>> {
        let path = self.paths.queue_file(&project.to_string());
        blocking(path.clone(), move || {
            FileStore::load_toml_blocking::<Queue>(&path)
        })
        .await
    }

    async fn save(&self, queue: &Queue) -> StoreResult<()> {
        let path = self.paths.queue_file(&queue.project_id.to_string());
        let value = queue.clone();
        blocking(path.clone(), move || {
            FileStore::save_toml_blocking(&path, &value)
        })
        .await
    }

    async fn delete(&self, project: ProjectId) -> StoreResult<()> {
        let path = self.paths.queue_file(&project.to_string());
        blocking(path.clone(), move || remove_if_exists(&path)).await
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
    use lattice_core::entities::{Project, QueueEntry, Task, TaskStatus, Template};
    use lattice_core::ids::{AgentId, ProjectId, TemplateId};
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
    async fn projects_roundtrip() {
        let (store, _c, _s) = mkstore();
        assert!(Projects::list(&store).await.unwrap().is_empty());
        let p = Project::new("acme", "/tmp/acme", now());
        Projects::save(&store, &p).await.unwrap();
        let loaded = Projects::load(&store, p.id).await.unwrap().unwrap();
        assert_eq!(loaded, p);
        let all = Projects::list(&store).await.unwrap();
        assert_eq!(all.len(), 1);
        Projects::delete(&store, p.id).await.unwrap();
        assert!(Projects::load(&store, p.id).await.unwrap().is_none());
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
        let project = Project::new("p", "/tmp/p", now());
        let template = Template::new("t", now());
        Projects::save(&store, &project).await.unwrap();
        Templates::save(&store, &template).await.unwrap();

        let mut task = Task::new(project.id, template.id, template.version, "name", now());
        task.status = TaskStatus::Draft;
        Tasks::save(&store, &task).await.unwrap();
        Tasks::save_snapshot(&store, &task, &template)
            .await
            .unwrap();
        Tasks::save_prompt(&store, &task, "## rendered prompt")
            .await
            .unwrap();

        let paths = store.paths().clone();
        assert!(
            paths
                .task_file(&project.id.to_string(), &task.id.to_string())
                .exists()
        );
        assert!(
            paths
                .task_template_snapshot(&project.id.to_string(), &task.id.to_string())
                .exists()
        );
        assert_eq!(
            std::fs::read_to_string(
                paths.task_prompt(&project.id.to_string(), &task.id.to_string())
            )
            .unwrap(),
            "## rendered prompt"
        );

        let listed = Tasks::list_for_project(&store, project.id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, task.id);
    }

    #[tokio::test]
    async fn queues_roundtrip() {
        let (store, _c, _s) = mkstore();
        let pid = ProjectId::new();
        let mut q = lattice_core::entities::Queue::empty(pid);
        q.push_back(QueueEntry {
            task_id: lattice_core::ids::TaskId::new(),
            agent_id: AgentId::new("cursor-agent"),
            enqueued_at: now(),
        });
        Queues::save(&store, &q).await.unwrap();
        let loaded = Queues::load(&store, pid).await.unwrap().unwrap();
        assert_eq!(loaded.entries.len(), 1);
        let all = Queues::list(&store).await.unwrap();
        assert_eq!(all.len(), 1);
        Queues::delete(&store, pid).await.unwrap();
        assert!(Queues::load(&store, pid).await.unwrap().is_none());
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
        s.runtime.max_concurrent_agents = 4;
        SettingsStore::save(&store, &s).await.unwrap();
        let loaded = SettingsStore::load(&store).await.unwrap();
        assert_eq!(loaded.runtime.max_concurrent_agents, 4);
    }

    #[tokio::test]
    async fn delete_nonexistent_is_noop() {
        let (store, _c, _s) = mkstore();
        // Deleting something that was never saved must succeed silently.
        Projects::delete(&store, ProjectId::new()).await.unwrap();
        Templates::delete(&store, TemplateId::new()).await.unwrap();
        Queues::delete(&store, ProjectId::new()).await.unwrap();
    }

    #[tokio::test]
    async fn list_empty_before_any_write() {
        let (store, _c, _s) = mkstore();
        // None of the root dirs exist yet; list must still succeed.
        assert!(Projects::list(&store).await.unwrap().is_empty());
        assert!(Templates::list(&store).await.unwrap().is_empty());
        assert!(Queues::list(&store).await.unwrap().is_empty());
    }
}
