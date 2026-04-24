//! Agent manifests, detection, and process supervision (M3).
//!
//! ## Modules
//! - [`manifest`] — declarative TOML schema for each agent.
//! - `registry` (M3.2) — loads manifests + runs detection.
//! - `runner` (M3.4) — spawns agents and supervises their processes.

#![deny(unsafe_code)]

pub mod error;
pub mod manifest;
pub mod queue;
pub mod registry;
pub mod runner;

pub use error::{AgentError, AgentResult};
pub use manifest::{
    AgentManifest, BUNDLED_CURSOR_AGENT, DetectSpec, InvocationMode, InvocationSpec, RuntimeSpec,
    WorkingDir,
};
pub use queue::{EnqueueRequest, QueueConfig, QueueEngine, QueueEvent, RunningRun};
pub use registry::{AgentRegistry, AvailableAgent};
pub use runner::{AgentRunner, ExitReport, LogLine, LogStream, RunHandle, SpawnSpec};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
