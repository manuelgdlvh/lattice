//! Agent-layer errors.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent manifest decode error at {path:?}: {source}")]
    ManifestDecode {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("agent manifest I/O error at {path:?}: {source}")]
    ManifestIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("agent `{0}` is not installed on this host")]
    NotInstalled(String),

    #[error("agent `{0}` binary not found on PATH")]
    BinaryNotFound(String),

    #[error("spawn failed for agent `{id}`: {source}")]
    Spawn {
        id: String,
        #[source]
        source: std::io::Error,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invocation error: {0}")]
    Invocation(String),

    #[error("agent `{0}` already registered")]
    Duplicate(String),
}

pub type AgentResult<T> = Result<T, AgentError>;
