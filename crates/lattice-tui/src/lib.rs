//! Terminal UI for lattice.
//!
//! # Architecture
//!
//! The UI follows an Elm-ish loop:
//!
//! ```text
//!      ┌───────── Msg ─────────┐
//!      │                       │
//!      ▼                       │
//!    update(Model, Msg) ──► Model
//!                             │
//!                             ▼
//!                           view(Model)
//!                             │
//!                             ▼
//!                          ratatui frame
//! ```
//!
//! `Msg` is produced from three sources merged by [`app::event_stream`]:
//! - Terminal key/resize events via `crossterm`.
//! - Queue events from the agents layer.
//! - Filesystem events from the store watcher.
//!
//! The [`model::Model`] is the single source of UI truth. Screens don't
//! own persistent state — they read snapshots off the model and push
//! `Msg`s when the user does something.

#![deny(unsafe_code)]

pub mod app;
pub mod context;
pub mod event;
pub mod keybind;
pub mod model;
pub mod palette;
pub mod screens;
pub mod toast;
pub mod view;

pub use app::App;
pub use context::AppContext;
pub use model::{Model, Msg, Screen};

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
