//! Store-layer errors.
//!
//! Wraps `std::io::Error`, serde errors, and `CoreError`. The public
//! surface stays narrow — callers match on `StoreError` and, if they
//! care, downcast to the inner error via `.source()`.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("toml decode error at {path:?}: {source}")]
    TomlDecode {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("toml encode error: {0}")]
    TomlEncode(#[from] toml::ser::Error),

    #[error("entity not found: {kind} `{id}`")]
    NotFound { kind: &'static str, id: String },

    #[error("entity already exists: {kind} `{id}`")]
    AlreadyExists { kind: &'static str, id: String },

    #[error("path is not under the store root: {0:?}")]
    PathEscape(PathBuf),

    #[error("directories crate could not resolve a home directory")]
    NoHomeDir,

    #[error("store is read-only in this context")]
    ReadOnly,

    #[error(transparent)]
    Core(#[from] lattice_core::error::CoreError),
}

impl StoreError {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn toml_decode(path: impl Into<PathBuf>, source: toml::de::Error) -> Self {
        Self::TomlDecode {
            path: path.into(),
            source,
        }
    }

    pub fn not_found(kind: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            kind,
            id: id.into(),
        }
    }

    pub fn already_exists(kind: &'static str, id: impl Into<String>) -> Self {
        Self::AlreadyExists {
            kind,
            id: id.into(),
        }
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_is_displayable() {
        let e = StoreError::not_found("Project", "abc");
        assert_eq!(e.to_string(), "entity not found: Project `abc`");
    }
}
