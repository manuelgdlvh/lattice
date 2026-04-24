//! Domain entities: `Project`, `Template`, `Task`, `Run`, `Queue`,
//! `Settings`. All are plain data types; no I/O.

mod project;
mod queue;
mod run;
mod settings;
mod task;
mod template;

pub use project::*;
pub use queue::*;
pub use run::*;
pub use settings::*;
pub use task::*;
pub use template::*;

/// Every persisted entity carries a `schema_version`. This is the
/// version we emit today; the store layer uses it to drive migrations.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;
