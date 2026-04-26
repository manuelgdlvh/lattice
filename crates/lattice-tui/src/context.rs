//! `AppContext` — the bundle of services the TUI talks to.
//!
//! Building one is the responsibility of `lattice-bin`; the TUI just
//! consumes it. Having a single struct lets tests build a lean
//! in-memory harness and the real app wire the file-backed variant.

use std::sync::Arc;

use lattice_store::paths::Paths;
use lattice_store::store::{SettingsStore, Tasks, Templates};

/// All the services a screen may need. Everything is behind `Arc` so
/// the context is cheap to clone and move across tasks.
#[derive(Clone)]
pub struct AppContext {
    pub templates: Arc<dyn Templates>,
    pub tasks: Arc<dyn Tasks>,
    pub settings: Arc<dyn SettingsStore>,
    pub paths: Paths,
}

impl std::fmt::Debug for AppContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppContext")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}
