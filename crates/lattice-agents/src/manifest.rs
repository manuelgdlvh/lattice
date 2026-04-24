//! Agent manifest — the declarative description of how to detect and
//! invoke one class of external CLI agent.
//!
//! Manifests are plain TOML. A `cursor-agent` manifest ships bundled
//! with the binary (see [`BUNDLED_CURSOR_AGENT`]); users can add more
//! by dropping files into `$LATTICE_CONFIG_DIR/agents/<id>.toml`.
//!
//! Example:
//!
//! ```toml
//! id = "cursor-agent"
//! display_name = "Cursor Agent"
//! binary = "cursor-agent"
//!
//! [detect]
//! args = ["--version"]
//! version_regex = "^cursor-agent\\s+(\\S+)"
//!
//! [invocation]
//! mode = "stdin"
//! args = ["--print", "--trust"]
//!
//! [runtime]
//! working_dir = "project"
//! kill_grace_ms = 5000
//!
//! [env]
//! NO_COLOR = "1"
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use lattice_core::ids::AgentId;

use crate::error::{AgentError, AgentResult};

/// Full, validated manifest. TOML bytes are deserialized into this and
/// then passed around by value (cheap to clone — strings only).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentManifest {
    pub id: AgentId,
    pub display_name: String,

    /// Either the name of the binary to look up on `PATH` (most common)
    /// or an absolute path. Resolved lazily by the registry.
    pub binary: String,

    #[serde(default)]
    pub detect: DetectSpec,

    #[serde(default)]
    pub invocation: InvocationSpec,

    #[serde(default)]
    pub runtime: RuntimeSpec,

    /// Extra env vars passed to every spawn. Merged on top of the
    /// inherited process environment — per-run code may override.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl AgentManifest {
    pub fn from_toml(bytes: &[u8], source_path: &Path) -> AgentResult<Self> {
        let s = std::str::from_utf8(bytes).map_err(|e| AgentError::ManifestIo {
            path: source_path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;
        toml::from_str(s).map_err(|source| AgentError::ManifestDecode {
            path: source_path.to_path_buf(),
            source,
        })
    }

    /// Sanity checks we run after decoding. Keeps "legally parsed but
    /// obviously broken" manifests out of the registry.
    pub fn validate(&self) -> AgentResult<()> {
        if self.id.as_str().is_empty() {
            return Err(AgentError::Invocation(
                "manifest `id` must be non-empty".into(),
            ));
        }
        if self.binary.trim().is_empty() {
            return Err(AgentError::Invocation(format!(
                "agent `{}` has empty `binary`",
                self.id
            )));
        }
        if self.invocation.mode == InvocationMode::Arg
            && !self
                .invocation
                .args
                .iter()
                .any(|a| a.contains("{prompt}") || a.contains("{prompt_file}"))
        {
            return Err(AgentError::Invocation(format!(
                "agent `{}`: invocation.mode = \"arg\" requires a {{prompt}} \
                 or {{prompt_file}} placeholder in invocation.args",
                self.id
            )));
        }
        if self.runtime.kill_grace_ms > 60_000 {
            return Err(AgentError::Invocation(format!(
                "agent `{}`: kill_grace_ms must be <= 60000",
                self.id
            )));
        }
        Ok(())
    }
}

/// How to probe for installed-ness and (optionally) extract a version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectSpec {
    /// argv run to detect the agent. Defaults to `["--version"]` when
    /// the table is present but `args` is missing, and to an empty
    /// Vec (skip detection beyond "is the binary on PATH?") otherwise.
    pub args: Vec<String>,

    /// Regex applied to detect-command stdout to extract a version.
    /// Capture group 1 is used when present, otherwise the full match.
    pub version_regex: Option<String>,

    /// Timeout for the detect command. Keep tight — users shouldn't
    /// wait long just to see the agent list.
    #[serde(default = "default_detect_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_detect_timeout_ms() -> u64 {
    2_000
}

/// How the task prompt is delivered to the agent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationMode {
    /// Pipe the prompt into the agent's stdin. Best default: keeps the
    /// prompt out of process listings and works for arbitrarily long
    /// inputs.
    #[default]
    Stdin,

    /// Substitute `{prompt}` into `invocation.args`. Short prompts
    /// only; the OS-specific argv length cap applies.
    Arg,

    /// Write the prompt to a temporary file and substitute
    /// `{prompt_file}` into `invocation.args`.
    File,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InvocationSpec {
    pub mode: InvocationMode,

    /// argv tail appended after the binary. `{prompt}` / `{prompt_file}`
    /// placeholders are substituted when `mode` is Arg / File.
    pub args: Vec<String>,
}

/// Runtime-only policy that has nothing to do with how the prompt is
/// delivered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeSpec {
    /// Where the child process runs. `project` means the target
    /// project's `path`; `custom` lets the manifest pin an explicit
    /// directory (useful for testing).
    pub working_dir: WorkingDir,

    /// Graceful shutdown window before SIGKILL. 0 means "kill
    /// immediately". Capped at 60s by `validate()`.
    pub kill_grace_ms: u64,
}

impl Default for RuntimeSpec {
    fn default() -> Self {
        Self {
            working_dir: WorkingDir::Project,
            kill_grace_ms: 5_000,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkingDir {
    /// Run inside the target project's directory (the default).
    #[default]
    Project,
    /// Run inside a manifest-provided custom path. Useful for agents
    /// that insist on being run from their own install directory.
    Custom { path: PathBuf },
}

/// Manifest text bundled with the binary so the bare-bones install has
/// a working cursor-agent entry without requiring the user to drop
/// files anywhere.
pub const BUNDLED_CURSOR_AGENT: &str = include_str!("../manifests/cursor-agent.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_cursor_agent_parses_and_validates() {
        let m = AgentManifest::from_toml(
            BUNDLED_CURSOR_AGENT.as_bytes(),
            Path::new("bundled/cursor-agent.toml"),
        )
        .unwrap();
        assert_eq!(m.id.as_str(), "cursor-agent");
        m.validate().unwrap();
    }

    #[test]
    fn missing_fields_default_reasonably() {
        let toml_text = r#"
id = "minimal"
display_name = "Minimal"
binary = "minimal-cli"
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("test.toml")).unwrap();
        assert_eq!(m.invocation.mode, InvocationMode::Stdin);
        assert_eq!(m.runtime.working_dir, WorkingDir::Project);
        assert_eq!(m.runtime.kill_grace_ms, 5_000);
        m.validate().unwrap();
    }

    #[test]
    fn empty_binary_rejected() {
        let toml_text = r#"
id = "bad"
display_name = "Bad"
binary = ""
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("test.toml")).unwrap();
        assert!(m.validate().is_err());
    }

    #[test]
    fn arg_mode_without_prompt_placeholder_rejected() {
        let toml_text = r#"
id = "no-placeholder"
display_name = "Bad"
binary = "x"

[invocation]
mode = "arg"
args = ["--run"]
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("test.toml")).unwrap();
        let err = m.validate().unwrap_err();
        assert!(
            matches!(err, AgentError::Invocation(ref msg) if msg.contains("{prompt}")),
            "got {err:?}"
        );
    }

    #[test]
    fn kill_grace_ms_capped() {
        let toml_text = r#"
id = "slow"
display_name = "Slow"
binary = "x"

[runtime]
kill_grace_ms = 999999
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("test.toml")).unwrap();
        assert!(m.validate().is_err());
    }

    #[test]
    fn custom_working_dir_roundtrips() {
        let toml_text = r#"
id = "pinned"
display_name = "Pinned"
binary = "x"

[runtime.working_dir]
kind = "custom"
path = "/opt/pinned"
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("test.toml")).unwrap();
        match m.runtime.working_dir {
            WorkingDir::Custom { path } => assert_eq!(path, PathBuf::from("/opt/pinned")),
            WorkingDir::Project => panic!("expected Custom"),
        }
    }

    #[test]
    fn env_vars_parse() {
        let toml_text = r#"
id = "envy"
display_name = "Envy"
binary = "x"

[env]
NO_COLOR = "1"
FOO = "bar"
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("test.toml")).unwrap();
        assert_eq!(m.env.get("NO_COLOR").map(String::as_str), Some("1"));
        assert_eq!(m.env.get("FOO").map(String::as_str), Some("bar"));
    }
}
