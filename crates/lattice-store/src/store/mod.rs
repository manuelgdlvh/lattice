//! Public `Store` traits.
//!
//! One trait per entity. This is intentional: it gives each entity a
//! precisely typed API (`fn save(&self, p: &Project)`), documents the
//! allowed ops, and lets the TUI layer depend on the minimal surface it
//! needs (e.g., the templates screen only takes `&dyn Templates`).
//!
//! All trait methods are **async** because the concrete backend may
//! defer I/O to a blocking thread pool.

use async_trait::async_trait;

use lattice_core::entities::{Settings, Task, Template};
use lattice_core::ids::{TaskId, TemplateId};

use crate::error::StoreResult;

/// Events emitted by the watcher when a backing file changes, regardless
/// of whether the change came from us or an external editor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StoreEvent {
    TemplateChanged(TemplateId),
    TemplateRemoved(TemplateId),
    TaskChanged(TaskId),
    TaskRemoved(TaskId),
    SettingsChanged,
}

#[async_trait]
pub trait Templates: Send + Sync {
    async fn list(&self) -> StoreResult<Vec<Template>>;
    async fn load(&self, id: TemplateId) -> StoreResult<Option<Template>>;
    async fn save(&self, template: &Template) -> StoreResult<()>;
    async fn delete(&self, id: TemplateId) -> StoreResult<()>;
}

#[async_trait]
pub trait Tasks: Send + Sync {
    async fn list(&self) -> StoreResult<Vec<Task>>;
    async fn load(&self, id: TaskId) -> StoreResult<Option<Task>>;
    async fn save(&self, task: &Task) -> StoreResult<()>;
    async fn save_snapshot(&self, task: &Task, template: &Template) -> StoreResult<()>;
    async fn save_prompt(&self, task: &Task, prompt: &str) -> StoreResult<()>;
    async fn delete(&self, id: TaskId) -> StoreResult<()>;
}

#[async_trait]
pub trait SettingsStore: Send + Sync {
    async fn load(&self) -> StoreResult<Settings>;
    async fn save(&self, settings: &Settings) -> StoreResult<()>;
}
