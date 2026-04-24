//! `Project` entity — a reference to a local directory used as the
//! execution cwd for tasks.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::ProjectId;
use crate::time::Timestamp;

use super::CURRENT_SCHEMA_VERSION;

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Queue-policy overrides that live with the project (can diverge from
/// the global default in `Settings`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectQueueConfig {
    /// If set, overrides `Settings.runtime.fail_fast`.
    pub fail_fast: Option<bool>,
}

/// A Project points at a local directory and owns a per-project queue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    pub id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub path: PathBuf,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "ProjectQueueConfig::is_empty")]
    pub queue: ProjectQueueConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tags: BTreeMap<String, String>,
}

impl ProjectQueueConfig {
    fn is_empty(&self) -> bool {
        self.fail_fast.is_none()
    }
}

impl Project {
    /// Convenience constructor — used heavily in tests. Production code
    /// typically deserializes from disk instead.
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>, now: Timestamp) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: ProjectId::new(),
            name: name.into(),
            description: String::new(),
            path: path.into(),
            created_at: now,
            updated_at: now,
            queue: ProjectQueueConfig::default(),
            tags: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_toml() {
        let now = Timestamp::parse("2026-04-24T10:12:00Z").unwrap();
        let p = Project::new("acme", "/tmp/acme", now);
        let s = toml::to_string(&p).unwrap();
        let back: Project = toml::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn empty_queue_block_is_omitted() {
        let now = Timestamp::parse("2026-04-24T10:12:00Z").unwrap();
        let p = Project::new("acme", "/tmp/acme", now);
        let s = toml::to_string(&p).unwrap();
        assert!(!s.contains("[queue]"));
    }
}
