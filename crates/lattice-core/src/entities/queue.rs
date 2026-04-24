//! `Queue` entity — the per-project ordered list of dispatched tasks.

use serde::{Deserialize, Serialize};

use crate::ids::{AgentId, ProjectId, TaskId};
use crate::time::Timestamp;

use super::CURRENT_SCHEMA_VERSION;

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// One queued task waiting to run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub enqueued_at: Timestamp,
}

/// The persisted per-project queue. Order is authoritative: head is index 0.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    pub project_id: ProjectId,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub paused_reason: String,
    #[serde(default, rename = "entries")]
    pub entries: Vec<QueueEntry>,
}

impl Queue {
    pub fn empty(project_id: ProjectId) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            project_id,
            paused: false,
            paused_reason: String::new(),
            entries: Vec::new(),
        }
    }

    pub fn push_back(&mut self, entry: QueueEntry) {
        self.entries.push(entry);
    }

    pub fn pop_front(&mut self) -> Option<QueueEntry> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_roundtrips_and_fifo() {
        let now = Timestamp::parse("2026-04-24T10:40:00Z").unwrap();
        let mut q = Queue::empty(ProjectId::new());
        let e1 = QueueEntry {
            task_id: TaskId::new(),
            agent_id: AgentId::new("a"),
            enqueued_at: now,
        };
        let e2 = QueueEntry {
            task_id: TaskId::new(),
            agent_id: AgentId::new("a"),
            enqueued_at: now,
        };
        q.push_back(e1.clone());
        q.push_back(e2.clone());
        let s = toml::to_string(&q).unwrap();
        let back: Queue = toml::from_str(&s).unwrap();
        assert_eq!(q, back);
        assert_eq!(q.pop_front().unwrap().task_id, e1.task_id);
        assert_eq!(q.pop_front().unwrap().task_id, e2.task_id);
        assert!(q.pop_front().is_none());
    }
}
