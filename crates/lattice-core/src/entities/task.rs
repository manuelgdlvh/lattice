//! `Task` entity — an instance of a template bound to a project with
//! filled-in field values. Tasks freeze a full copy of the template
//! they were created from on disk (as a sibling file); this struct only
//! holds the metadata + values.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{ProjectId, TaskId, TemplateId};
use crate::time::Timestamp;

use super::CURRENT_SCHEMA_VERSION;

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Lifecycle state of a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Authoring in progress. Not valid for dispatch yet.
    Draft,
    /// Validated and enqueued for its project.
    Queued,
    /// Currently executing.
    Running,
    /// Terminal: the agent exited with success.
    Succeeded,
    /// Terminal: the agent exited with non-zero / error.
    Failed,
    /// Terminal: user-initiated kill.
    Killed,
    /// Terminal: app restarted while the task was running and we could
    /// not safely re-attach.
    Interrupted,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Killed | Self::Interrupted
        )
    }

    pub fn is_pending(self) -> bool {
        matches!(self, Self::Draft | Self::Queued)
    }
}

/// Resolved derived-value snapshot captured at task-creation time.
pub type DerivedSnapshot = BTreeMap<String, Value>;

/// Map from field id to its filled-in JSON value.
pub type FieldValues = BTreeMap<String, Value>;

/// An authored task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    pub id: TaskId,
    pub project_id: ProjectId,
    pub template_id: TemplateId,
    pub template_version: u32,
    pub name: String,
    pub created_at: Timestamp,
    pub status: TaskStatus,
    #[serde(default)]
    pub fields: FieldValues,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub derived: DerivedSnapshot,
}

impl Task {
    pub fn new(
        project_id: ProjectId,
        template_id: TemplateId,
        template_version: u32,
        name: impl Into<String>,
        now: Timestamp,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: TaskId::new(),
            project_id,
            template_id,
            template_version,
            name: name.into(),
            created_at: now,
            status: TaskStatus::Draft,
            fields: FieldValues::new(),
            derived: DerivedSnapshot::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_classifiers() {
        assert!(TaskStatus::Draft.is_pending());
        assert!(TaskStatus::Queued.is_pending());
        assert!(!TaskStatus::Running.is_pending());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(TaskStatus::Succeeded.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Killed.is_terminal());
        assert!(TaskStatus::Interrupted.is_terminal());
    }

    #[test]
    fn task_roundtrips_through_toml() {
        let now = Timestamp::parse("2026-04-24T10:30:00Z").unwrap();
        let mut t = Task::new(ProjectId::new(), TemplateId::new(), 7, "refactor auth", now);
        t.fields
            .insert("module_path".into(), json!("src/auth/middleware.rs"));
        t.fields
            .insert("constraints".into(), json!(["no new deps"]));
        let s = toml::to_string(&t).unwrap();
        let back: Task = toml::from_str(&s).unwrap();
        assert_eq!(t, back);
    }
}
