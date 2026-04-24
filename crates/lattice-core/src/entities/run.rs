//! `Run` entity — a single execution of a task by an agent.

use serde::{Deserialize, Serialize};

use crate::ids::{AgentId, ProjectId, RunId, TaskId};
use crate::time::Timestamp;

use super::CURRENT_SCHEMA_VERSION;
use super::task::TaskStatus;

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Outcome classification. The underlying type is `TaskStatus` so runs
/// and tasks share vocabulary.
pub type RunStatus = TaskStatus;

/// Log-size accounting. Kept separate from the hot run record because
/// it changes at a different cadence.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunLogInfo {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub truncated: bool,
}

/// A single run of a task by an agent. Stored on disk as
/// `runs/<project>/<run>/run.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    pub id: RunId,
    pub project_id: ProjectId,
    pub task_id: TaskId,
    pub agent_id: AgentId,
    #[serde(default)]
    pub agent_version: String,
    pub status: RunStatus,
    pub queued_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub log: RunLogInfo,
}

impl Run {
    pub fn new(
        project_id: ProjectId,
        task_id: TaskId,
        agent_id: AgentId,
        queued_at: Timestamp,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: RunId::new(),
            project_id,
            task_id,
            agent_id,
            agent_version: String::new(),
            status: RunStatus::Queued,
            queued_at,
            started_at: None,
            finished_at: None,
            pid: None,
            log: RunLogInfo::default(),
        }
    }
}

/// The `exit.toml` sibling file written when a run completes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunExit {
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub finished_at: Option<Timestamp>,
    pub duration_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_roundtrips() {
        let now = Timestamp::parse("2026-04-24T10:40:00Z").unwrap();
        let r = Run::new(
            ProjectId::new(),
            TaskId::new(),
            AgentId::new("cursor-agent"),
            now,
        );
        let s = toml::to_string(&r).unwrap();
        let back: Run = toml::from_str(&s).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn exit_is_optional() {
        let e = RunExit::default();
        let s = toml::to_string(&e).unwrap();
        let back: RunExit = toml::from_str(&s).unwrap();
        assert_eq!(e, back);
    }
}
