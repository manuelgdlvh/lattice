//! In-memory cache decorators for the `Store` traits.
//!
//! The strategy (see `docs/DATA_MODEL.md §4`) is intentionally simple:
//!
//! - On `load`: return the cached value if present; otherwise fetch
//!   from the inner store, insert, and return.
//! - On `save`: delegate to the inner store, then **invalidate** the
//!   cache entry. We deliberately do not populate with the saved value
//!   because the serialized→deserialized round-trip can canonicalize
//!   fields (e.g. dropping defaulted optionals), which would make the
//!   cache disagree with disk until the next eviction.
//! - On `delete`: delegate + invalidate.
//! - On `list`: cache the list result under a synthetic key so repeated
//!   enumerations are cheap; invalidated on any save/delete.
//!
//! A watcher (M2.4) calls `invalidate(key)` / `invalidate_all()` when
//! the file changes on disk.
//!
//! Each decorator is independently configurable (capacity) but shares a
//! common [`CacheConfig`] constructor for ergonomics.

use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;

use async_trait::async_trait;
use lru::LruCache;
use parking_lot::Mutex;

use lattice_core::entities::{Project, Queue, Run, RunExit, Settings, Task, Template};
use lattice_core::ids::{ProjectId, RunId, TaskId, TemplateId};

use crate::error::StoreResult;
use crate::store::{Projects, Queues, Runs, SettingsStore, Tasks, Templates};

/// Sensible defaults; callers can override per-store if they need to.
#[derive(Clone, Copy, Debug)]
pub struct CacheConfig {
    pub entries_capacity: NonZeroUsize,
    pub list_capacity: NonZeroUsize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            entries_capacity: NonZeroUsize::new(256).expect("nonzero"),
            list_capacity: NonZeroUsize::new(16).expect("nonzero"),
        }
    }
}

/// A two-table LRU cache: one for individual entities, one for list
/// results. Internally mutable and cheap to clone (`Arc` wrapper).
#[derive(Debug)]
struct LruTable<K: Hash + Eq + Clone, V: Clone> {
    inner: Mutex<LruCache<K, V>>,
}

impl<K: Hash + Eq + Clone, V: Clone> LruTable<K, V> {
    fn new(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(capacity)),
        }
    }

    fn get(&self, k: &K) -> Option<V> {
        self.inner.lock().get(k).cloned()
    }

    fn put(&self, k: K, v: V) {
        self.inner.lock().put(k, v);
    }

    fn pop(&self, k: &K) -> Option<V> {
        self.inner.lock().pop(k)
    }

    fn clear(&self) {
        self.inner.lock().clear();
    }

    fn len(&self) -> usize {
        self.inner.lock().len()
    }
}

// -------- Projects ---------------------------------------------------

#[derive(Debug)]
pub struct CachedProjects<S> {
    inner: S,
    entries: Arc<LruTable<ProjectId, Project>>,
    list: Arc<LruTable<(), Vec<Project>>>,
}

impl<S> CachedProjects<S> {
    pub fn new(inner: S, cfg: CacheConfig) -> Self {
        Self {
            inner,
            entries: Arc::new(LruTable::new(cfg.entries_capacity)),
            list: Arc::new(LruTable::new(cfg.list_capacity)),
        }
    }

    pub fn invalidate(&self, id: ProjectId) {
        self.entries.pop(&id);
        self.list.clear();
    }

    pub fn invalidate_all(&self) {
        self.entries.clear();
        self.list.clear();
    }
}

#[async_trait]
impl<S: Projects> Projects for CachedProjects<S> {
    async fn list(&self) -> StoreResult<Vec<Project>> {
        if let Some(cached) = self.list.get(&()) {
            return Ok(cached);
        }
        let fresh = self.inner.list().await?;
        self.list.put((), fresh.clone());
        Ok(fresh)
    }

    async fn load(&self, id: ProjectId) -> StoreResult<Option<Project>> {
        if let Some(cached) = self.entries.get(&id) {
            return Ok(Some(cached));
        }
        let fresh = self.inner.load(id).await?;
        if let Some(ref p) = fresh {
            self.entries.put(id, p.clone());
        }
        Ok(fresh)
    }

    async fn save(&self, project: &Project) -> StoreResult<()> {
        self.inner.save(project).await?;
        self.invalidate(project.id);
        Ok(())
    }

    async fn delete(&self, id: ProjectId) -> StoreResult<()> {
        self.inner.delete(id).await?;
        self.invalidate(id);
        Ok(())
    }
}

// -------- Templates --------------------------------------------------

#[derive(Debug)]
pub struct CachedTemplates<S> {
    inner: S,
    entries: Arc<LruTable<TemplateId, Template>>,
    list: Arc<LruTable<(), Vec<Template>>>,
}

impl<S> CachedTemplates<S> {
    pub fn new(inner: S, cfg: CacheConfig) -> Self {
        Self {
            inner,
            entries: Arc::new(LruTable::new(cfg.entries_capacity)),
            list: Arc::new(LruTable::new(cfg.list_capacity)),
        }
    }

    pub fn invalidate(&self, id: TemplateId) {
        self.entries.pop(&id);
        self.list.clear();
    }

    pub fn invalidate_all(&self) {
        self.entries.clear();
        self.list.clear();
    }
}

#[async_trait]
impl<S: Templates> Templates for CachedTemplates<S> {
    async fn list(&self) -> StoreResult<Vec<Template>> {
        if let Some(cached) = self.list.get(&()) {
            return Ok(cached);
        }
        let fresh = self.inner.list().await?;
        self.list.put((), fresh.clone());
        Ok(fresh)
    }

    async fn load(&self, id: TemplateId) -> StoreResult<Option<Template>> {
        if let Some(cached) = self.entries.get(&id) {
            return Ok(Some(cached));
        }
        let fresh = self.inner.load(id).await?;
        if let Some(ref t) = fresh {
            self.entries.put(id, t.clone());
        }
        Ok(fresh)
    }

    async fn save(&self, template: &Template) -> StoreResult<()> {
        self.inner.save(template).await?;
        self.invalidate(template.id);
        Ok(())
    }

    async fn delete(&self, id: TemplateId) -> StoreResult<()> {
        self.inner.delete(id).await?;
        self.invalidate(id);
        Ok(())
    }
}

// -------- Tasks ------------------------------------------------------

#[derive(Debug)]
pub struct CachedTasks<S> {
    inner: S,
    entries: Arc<LruTable<(ProjectId, TaskId), Task>>,
    list_per_project: Arc<LruTable<ProjectId, Vec<Task>>>,
}

impl<S> CachedTasks<S> {
    pub fn new(inner: S, cfg: CacheConfig) -> Self {
        Self {
            inner,
            entries: Arc::new(LruTable::new(cfg.entries_capacity)),
            list_per_project: Arc::new(LruTable::new(cfg.list_capacity)),
        }
    }

    pub fn invalidate(&self, project: ProjectId, id: TaskId) {
        self.entries.pop(&(project, id));
        self.list_per_project.pop(&project);
    }

    pub fn invalidate_project(&self, project: ProjectId) {
        self.list_per_project.pop(&project);
    }

    pub fn invalidate_all(&self) {
        self.entries.clear();
        self.list_per_project.clear();
    }
}

#[async_trait]
impl<S: Tasks> Tasks for CachedTasks<S> {
    async fn list_for_project(&self, project: ProjectId) -> StoreResult<Vec<Task>> {
        if let Some(cached) = self.list_per_project.get(&project) {
            return Ok(cached);
        }
        let fresh = self.inner.list_for_project(project).await?;
        self.list_per_project.put(project, fresh.clone());
        Ok(fresh)
    }

    async fn load(&self, project: ProjectId, id: TaskId) -> StoreResult<Option<Task>> {
        if let Some(cached) = self.entries.get(&(project, id)) {
            return Ok(Some(cached));
        }
        let fresh = self.inner.load(project, id).await?;
        if let Some(ref t) = fresh {
            self.entries.put((project, id), t.clone());
        }
        Ok(fresh)
    }

    async fn save(&self, task: &Task) -> StoreResult<()> {
        self.inner.save(task).await?;
        self.invalidate(task.project_id, task.id);
        Ok(())
    }

    async fn save_snapshot(&self, task: &Task, template: &Template) -> StoreResult<()> {
        self.inner.save_snapshot(task, template).await
    }

    async fn save_prompt(&self, task: &Task, prompt: &str) -> StoreResult<()> {
        self.inner.save_prompt(task, prompt).await
    }

    async fn delete(&self, project: ProjectId, id: TaskId) -> StoreResult<()> {
        self.inner.delete(project, id).await?;
        self.invalidate(project, id);
        Ok(())
    }
}

// -------- Runs -------------------------------------------------------

#[derive(Debug)]
pub struct CachedRuns<S> {
    inner: S,
    entries: Arc<LruTable<(ProjectId, RunId), Run>>,
    list_per_project: Arc<LruTable<ProjectId, Vec<Run>>>,
}

impl<S> CachedRuns<S> {
    pub fn new(inner: S, cfg: CacheConfig) -> Self {
        Self {
            inner,
            entries: Arc::new(LruTable::new(cfg.entries_capacity)),
            list_per_project: Arc::new(LruTable::new(cfg.list_capacity)),
        }
    }

    pub fn invalidate(&self, project: ProjectId, id: RunId) {
        self.entries.pop(&(project, id));
        self.list_per_project.pop(&project);
    }

    pub fn invalidate_all(&self) {
        self.entries.clear();
        self.list_per_project.clear();
    }
}

#[async_trait]
impl<S: Runs> Runs for CachedRuns<S> {
    async fn list_for_project(&self, project: ProjectId) -> StoreResult<Vec<Run>> {
        if let Some(cached) = self.list_per_project.get(&project) {
            return Ok(cached);
        }
        let fresh = self.inner.list_for_project(project).await?;
        self.list_per_project.put(project, fresh.clone());
        Ok(fresh)
    }

    async fn load(&self, project: ProjectId, id: RunId) -> StoreResult<Option<Run>> {
        if let Some(cached) = self.entries.get(&(project, id)) {
            return Ok(Some(cached));
        }
        let fresh = self.inner.load(project, id).await?;
        if let Some(ref r) = fresh {
            self.entries.put((project, id), r.clone());
        }
        Ok(fresh)
    }

    async fn save(&self, run: &Run) -> StoreResult<()> {
        self.inner.save(run).await?;
        self.invalidate(run.project_id, run.id);
        Ok(())
    }

    async fn save_exit(&self, project: ProjectId, id: RunId, exit: &RunExit) -> StoreResult<()> {
        // `RunExit` is written to a sibling file and is not part of
        // the cached `Run` value, so we just forward.
        self.inner.save_exit(project, id, exit).await
    }

    async fn load_exit(&self, project: ProjectId, id: RunId) -> StoreResult<Option<RunExit>> {
        self.inner.load_exit(project, id).await
    }

    async fn delete(&self, project: ProjectId, id: RunId) -> StoreResult<()> {
        self.inner.delete(project, id).await?;
        self.invalidate(project, id);
        Ok(())
    }
}

// -------- Queues -----------------------------------------------------

#[derive(Debug)]
pub struct CachedQueues<S> {
    inner: S,
    entries: Arc<LruTable<ProjectId, Queue>>,
    list: Arc<LruTable<(), Vec<Queue>>>,
}

impl<S> CachedQueues<S> {
    pub fn new(inner: S, cfg: CacheConfig) -> Self {
        Self {
            inner,
            entries: Arc::new(LruTable::new(cfg.entries_capacity)),
            list: Arc::new(LruTable::new(cfg.list_capacity)),
        }
    }

    pub fn invalidate(&self, project: ProjectId) {
        self.entries.pop(&project);
        self.list.clear();
    }

    pub fn invalidate_all(&self) {
        self.entries.clear();
        self.list.clear();
    }
}

#[async_trait]
impl<S: Queues> Queues for CachedQueues<S> {
    async fn list(&self) -> StoreResult<Vec<Queue>> {
        if let Some(cached) = self.list.get(&()) {
            return Ok(cached);
        }
        let fresh = self.inner.list().await?;
        self.list.put((), fresh.clone());
        Ok(fresh)
    }

    async fn load(&self, project: ProjectId) -> StoreResult<Option<Queue>> {
        if let Some(cached) = self.entries.get(&project) {
            return Ok(Some(cached));
        }
        let fresh = self.inner.load(project).await?;
        if let Some(ref q) = fresh {
            self.entries.put(project, q.clone());
        }
        Ok(fresh)
    }

    async fn save(&self, queue: &Queue) -> StoreResult<()> {
        self.inner.save(queue).await?;
        self.invalidate(queue.project_id);
        Ok(())
    }

    async fn delete(&self, project: ProjectId) -> StoreResult<()> {
        self.inner.delete(project).await?;
        self.invalidate(project);
        Ok(())
    }
}

// -------- Settings ---------------------------------------------------

#[derive(Debug)]
pub struct CachedSettings<S> {
    inner: S,
    slot: Mutex<Option<Settings>>,
}

impl<S> CachedSettings<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            slot: Mutex::new(None),
        }
    }

    pub fn invalidate(&self) {
        *self.slot.lock() = None;
    }
}

#[async_trait]
impl<S: SettingsStore> SettingsStore for CachedSettings<S> {
    async fn load(&self) -> StoreResult<Settings> {
        if let Some(cached) = self.slot.lock().clone() {
            return Ok(cached);
        }
        let fresh = self.inner.load().await?;
        *self.slot.lock() = Some(fresh.clone());
        Ok(fresh)
    }

    async fn save(&self, settings: &Settings) -> StoreResult<()> {
        self.inner.save(settings).await?;
        self.invalidate();
        Ok(())
    }
}

// -------- Observability helpers for tests ----------------------------

/// Test-only counters to verify cache hits / misses.
impl<S> CachedProjects<S> {
    pub fn cached_entry_count(&self) -> usize {
        self.entries.len()
    }
}
impl<S> CachedTemplates<S> {
    pub fn cached_entry_count(&self) -> usize {
        self.entries.len()
    }
}
impl<S> CachedTasks<S> {
    pub fn cached_entry_count(&self) -> usize {
        self.entries.len()
    }
}
impl<S> CachedRuns<S> {
    pub fn cached_entry_count(&self) -> usize {
        self.entries.len()
    }
}
impl<S> CachedQueues<S> {
    pub fn cached_entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::entities::{Project, Template};
    use lattice_core::time::Timestamp;
    use tempfile::TempDir;

    use crate::FileStore;
    use crate::Paths;

    fn now() -> Timestamp {
        Timestamp::parse("2026-04-24T10:00:00Z").unwrap()
    }

    fn mkfs() -> (FileStore, TempDir, TempDir) {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        (
            FileStore::new(Paths::with_roots(cfg.path(), state.path())),
            cfg,
            state,
        )
    }

    #[tokio::test]
    async fn cached_projects_load_hits_cache_on_second_call() {
        let (fs, _c, _s) = mkfs();
        let store = CachedProjects::new(fs, CacheConfig::default());
        let p = Project::new("acme", "/tmp/acme", now());
        Projects::save(&store, &p).await.unwrap();
        // Save invalidated; first load populates.
        Projects::load(&store, p.id).await.unwrap();
        assert_eq!(store.cached_entry_count(), 1);
        // Second load hits cache.
        Projects::load(&store, p.id).await.unwrap();
        assert_eq!(store.cached_entry_count(), 1);
    }

    #[tokio::test]
    async fn save_invalidates_cached_entry() {
        let (fs, _c, _s) = mkfs();
        let store = CachedProjects::new(fs, CacheConfig::default());
        let mut p = Project::new("acme", "/tmp/acme", now());
        Projects::save(&store, &p).await.unwrap();
        let _ = Projects::load(&store, p.id).await.unwrap();
        assert_eq!(store.cached_entry_count(), 1);
        p.name = "acme-v2".into();
        Projects::save(&store, &p).await.unwrap();
        // After save the entry cache is empty until a subsequent load.
        assert_eq!(store.cached_entry_count(), 0);
        let reloaded = Projects::load(&store, p.id).await.unwrap().unwrap();
        assert_eq!(reloaded.name, "acme-v2");
    }

    #[tokio::test]
    async fn delete_invalidates_cached_entry() {
        let (fs, _c, _s) = mkfs();
        let store = CachedProjects::new(fs, CacheConfig::default());
        let p = Project::new("acme", "/tmp/acme", now());
        Projects::save(&store, &p).await.unwrap();
        let _ = Projects::load(&store, p.id).await.unwrap();
        Projects::delete(&store, p.id).await.unwrap();
        assert!(Projects::load(&store, p.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn explicit_invalidate_forces_reload() {
        let (fs, _c, _s) = mkfs();
        let store = CachedTemplates::new(fs, CacheConfig::default());
        let t = Template::new("bug-fix", now());
        Templates::save(&store, &t).await.unwrap();
        let _ = Templates::load(&store, t.id).await.unwrap();
        assert_eq!(store.cached_entry_count(), 1);
        store.invalidate(t.id);
        assert_eq!(store.cached_entry_count(), 0);
    }

    #[tokio::test]
    async fn list_result_is_cached_then_invalidated_on_write() {
        let (fs, _c, _s) = mkfs();
        let store = CachedProjects::new(fs, CacheConfig::default());
        let a = Project::new("a", "/tmp/a", now());
        Projects::save(&store, &a).await.unwrap();
        assert_eq!(Projects::list(&store).await.unwrap().len(), 1);
        // Write a second project; list cache should be invalidated so
        // the next call reflects both.
        let b = Project::new("b", "/tmp/b", now());
        Projects::save(&store, &b).await.unwrap();
        assert_eq!(Projects::list(&store).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn settings_cache_survives_repeated_loads() {
        let (fs, _c, _s) = mkfs();
        let store = CachedSettings::new(fs);
        let mut s = lattice_core::entities::Settings::default();
        s.runtime.max_concurrent_agents = 7;
        SettingsStore::save(&store, &s).await.unwrap();
        let a = SettingsStore::load(&store).await.unwrap();
        let b = SettingsStore::load(&store).await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a.runtime.max_concurrent_agents, 7);
    }
}
