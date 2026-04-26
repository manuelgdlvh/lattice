//! Persistence layer for lattice.
//!
//! This crate provides:
//!
//! - XDG-aware path resolution ([`paths::Paths`])
//! - Atomic file writes ([`fs::atomic_write_bytes`])
//! - Per-entity `Store` traits ([`store`])
//! - A TOML-backed `FileStore` (M2.2)
//! - An LRU cache wrapper (M2.3)
//! - A filesystem watcher emitting [`store::StoreEvent`]s (M2.4)
//! - Production impls of the `lattice-core` derived provider traits (M2.5)

#![deny(unsafe_code)]

pub mod cache;
pub mod error;
pub mod filestore;
pub mod fs;
pub mod paths;
pub mod providers;
pub mod store;
pub mod watcher;

pub use cache::{CacheConfig, CachedSettings, CachedTasks, CachedTemplates};
pub use error::{StoreError, StoreResult};
pub use filestore::FileStore;
pub use paths::Paths;
pub use providers::{RealCmd, RealEnv, RealFs};
pub use store::{SettingsStore, StoreEvent, Tasks, Templates};
pub use watcher::FsWatcher;

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
