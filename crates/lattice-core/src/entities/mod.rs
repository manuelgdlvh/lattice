//! Domain entities: `Template`, `Task`, `Settings`.
//! All are plain data types; no I/O.

mod settings;
mod task;
mod template;

pub use settings::*;
pub use task::*;
pub use template::*;

/// Every persisted entity carries a `schema_version`. This is the
/// version we emit today; the store layer uses it to drive migrations.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;
