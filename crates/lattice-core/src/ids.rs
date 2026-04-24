//! Strongly-typed entity identifiers.
//!
//! All ids are UUID v7 under the hood (time-sortable, no central
//! registry), but each entity gets its own newtype so the compiler
//! rejects cross-wiring (e.g., passing a `ProjectId` where a `TaskId`
//! is expected).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generate a new time-ordered (UUID v7) id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Nil (all-zeroes) id. Useful for tests and sentinel values.
            pub const fn nil() -> Self {
                Self(Uuid::nil())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a [`Project`](crate::entities::Project).
    ProjectId
);
define_id!(
    /// Identifies a [`Template`](crate::entities::Template).
    TemplateId
);
define_id!(
    /// Identifies a [`Task`](crate::entities::Task).
    TaskId
);
define_id!(
    /// Identifies a [`Run`](crate::entities::Run).
    RunId
);

/// Stable identifier for an agent manifest (`cursor-agent`, ...).
///
/// Agents are keyed by a short slug rather than a UUID because users type
/// them, reference them in manifests, and they must be portable across
/// installs.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_by_type() {
        let p = ProjectId::new();
        let t = TemplateId::new();
        // Two independent generations must not be equal.
        assert_ne!(p.0, t.0);
    }

    #[test]
    fn ids_roundtrip_via_string() {
        let p = ProjectId::new();
        let parsed: ProjectId = p.to_string().parse().unwrap();
        assert_eq!(p, parsed);
    }

    #[test]
    fn uuid_v7_is_time_ordered() {
        let a = TaskId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = TaskId::new();
        assert!(a < b, "v7 ids must be monotonically increasing over time");
    }

    #[test]
    fn agent_id_string_form() {
        let a = AgentId::new("cursor-agent");
        assert_eq!(a.as_str(), "cursor-agent");
        assert_eq!(a.to_string(), "cursor-agent");
    }
}
