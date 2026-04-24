//! `AgentRegistry` — loads manifests and detects which agents are
//! actually installed on the host.
//!
//! Lookup order (later entries override earlier ones if they share an `id`):
//!
//! 1. Bundled manifests compiled into the binary (currently: `cursor-agent`).
//! 2. Every `*.toml` under `$LATTICE_CONFIG_DIR/agents/`.
//!
//! Detection runs the manifest's `detect.args` with a short timeout.
//! A missing binary is not an error — the agent is simply reported as
//! not installed. Detection runs once at registry construction; callers
//! can `reload()` to pick up env / filesystem changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use regex::Regex;
use tracing::{debug, warn};

use lattice_core::ids::AgentId;

use crate::error::{AgentError, AgentResult};
use crate::manifest::{AgentManifest, BUNDLED_CURSOR_AGENT};

/// Result of detection: the manifest paired with whether we could find
/// and probe the binary, and if so, an optional parsed version string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableAgent {
    pub manifest: AgentManifest,
    pub installed: bool,
    pub resolved_path: Option<PathBuf>,
    pub version: Option<String>,
    /// Populated when detection returned an unexpected error (not
    /// "binary missing" — that is reported via `installed = false`).
    pub detection_warning: Option<String>,
}

/// Registry holds every known manifest keyed by `AgentId`.
#[derive(Debug, Default)]
pub struct AgentRegistry {
    agents: BTreeMap<AgentId, AvailableAgent>,
}

impl AgentRegistry {
    /// Build a registry from bundled manifests alone (ignoring the
    /// user's config dir). Useful for tests.
    pub fn bundled_only() -> AgentResult<Self> {
        let mut reg = Self::default();
        reg.install(bundled_manifests())?;
        reg.detect_all();
        Ok(reg)
    }

    /// Build a registry using bundled manifests plus everything under
    /// `config_dir/agents/*.toml`. A missing or empty config dir is
    /// fine; the registry is only populated with the bundled entries.
    pub fn from_config_dir(config_agents_dir: &Path) -> AgentResult<Self> {
        let mut reg = Self::default();
        reg.install(bundled_manifests())?;
        if config_agents_dir.exists() {
            reg.install(load_manifests_from_dir(config_agents_dir)?)?;
        }
        reg.detect_all();
        Ok(reg)
    }

    /// Re-detect every manifest. Call this after PATH changes or when
    /// the user installs/removes an agent without restarting lattice.
    pub fn reload_detection(&mut self) {
        self.detect_all();
    }

    pub fn list(&self) -> Vec<&AvailableAgent> {
        self.agents.values().collect()
    }

    pub fn installed(&self) -> Vec<&AvailableAgent> {
        self.agents.values().filter(|a| a.installed).collect()
    }

    pub fn get(&self, id: &AgentId) -> Option<&AvailableAgent> {
        self.agents.get(id)
    }

    /// Install or replace entries. Bundled manifests come first; user
    /// manifests with the same `id` overwrite bundled ones by design.
    fn install(&mut self, manifests: Vec<AgentManifest>) -> AgentResult<()> {
        for m in manifests {
            m.validate()?;
            let id = m.id.clone();
            self.agents.insert(
                id,
                AvailableAgent {
                    manifest: m,
                    installed: false,
                    resolved_path: None,
                    version: None,
                    detection_warning: None,
                },
            );
        }
        Ok(())
    }

    fn detect_all(&mut self) {
        for entry in self.agents.values_mut() {
            let (installed, path, version, warning) = detect_one(&entry.manifest);
            entry.installed = installed;
            entry.resolved_path = path;
            entry.version = version;
            entry.detection_warning = warning;
        }
    }
}

fn bundled_manifests() -> Vec<AgentManifest> {
    let mut out = Vec::new();
    match AgentManifest::from_toml(
        BUNDLED_CURSOR_AGENT.as_bytes(),
        Path::new("<bundled>/cursor-agent.toml"),
    ) {
        Ok(m) => out.push(m),
        Err(e) => {
            // A broken bundled manifest is a build-time bug — but we
            // prefer to log and skip rather than panic in production.
            warn!("bundled cursor-agent manifest failed to parse: {e}");
        }
    }
    out
}

fn load_manifests_from_dir(dir: &Path) -> AgentResult<Vec<AgentManifest>> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|source| AgentError::ManifestIo {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| AgentError::ManifestIo {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|source| AgentError::ManifestIo {
            path: path.clone(),
            source,
        })?;
        let m = AgentManifest::from_toml(&bytes, &path)?;
        out.push(m);
    }
    Ok(out)
}

/// Run detection for a single manifest. Never panics; returns
/// `(installed, resolved_path, version, warning)`.
fn detect_one(manifest: &AgentManifest) -> (bool, Option<PathBuf>, Option<String>, Option<String>) {
    // Step 1: resolve the binary on PATH (or accept a literal absolute path).
    let Ok(resolved) = resolve_binary(&manifest.binary) else {
        debug!(agent = %manifest.id, "binary not found on PATH");
        return (false, None, None, None);
    };

    // Step 2: optional detect probe.
    if manifest.detect.args.is_empty() {
        return (true, Some(resolved), None, None);
    }

    match run_probe(&resolved, &manifest.detect.args, manifest.detect.timeout_ms) {
        Ok(stdout) => {
            let version = manifest
                .detect
                .version_regex
                .as_deref()
                .and_then(|pat| extract_version(pat, &stdout));
            (true, Some(resolved), version, None)
        }
        Err(e) => {
            // A spawn-level failure means the binary exists but is
            // broken or the regex compile failed; keep the agent marked
            // installed but surface the warning in the UI.
            (true, Some(resolved), None, Some(e))
        }
    }
}

fn resolve_binary(binary: &str) -> Result<PathBuf, which::Error> {
    // Absolute paths bypass the which crate.
    let p = PathBuf::from(binary);
    if p.is_absolute() {
        if p.is_file() {
            return Ok(p);
        }
        return Err(which::Error::CannotFindBinaryPath);
    }
    which::which(binary)
}

fn run_probe(binary: &Path, args: &[String], timeout_ms: u64) -> Result<String, String> {
    let child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Err(format!("spawn probe: {e}")),
    };

    let (tx, rx) = mpsc::channel();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let pid = child.id();
    thread::spawn(move || {
        let status = child.wait();
        let mut out = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = std::io::Read::read_to_end(s, &mut out);
        }
        let mut err = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = std::io::Read::read_to_end(s, &mut err);
        }
        let _ = tx.send((status, out, err));
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok((Ok(_), stdout_bytes, stderr_bytes)) => {
            // Some agents print their version to stderr (`java -version`
            // being the canonical example). Concatenate so the regex
            // matcher sees both.
            let mut s = String::from_utf8_lossy(&stdout_bytes).into_owned();
            if s.trim().is_empty() {
                s = String::from_utf8_lossy(&stderr_bytes).into_owned();
            }
            Ok(s)
        }
        Ok((Err(e), _, _)) => Err(format!("wait failed: {e}")),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill_pid(pid);
            Err(format!("detection timed out after {timeout_ms}ms"))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("probe thread disconnected".into()),
    }
}

fn extract_version(pattern: &str, text: &str) -> Option<String> {
    let Ok(re) = Regex::new(pattern) else {
        return None;
    };
    let captures = re.captures(text)?;
    if let Some(group) = captures.get(1) {
        return Some(group.as_str().to_string());
    }
    captures.get(0).map(|m| m.as_str().to_string())
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_pid(pid: u32) {
    let _ = Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/F")
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn bundled_registry_contains_cursor_agent_entry() {
        let reg = AgentRegistry::bundled_only().unwrap();
        let id = AgentId::new("cursor-agent");
        let entry = reg.get(&id).expect("cursor-agent should be registered");
        assert_eq!(entry.manifest.id, id);
        // We don't require the binary to be present on the test host;
        // `installed` is whatever the host happens to report.
    }

    #[test]
    fn user_manifest_overrides_bundled() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cursor-agent.toml");
        std::fs::write(
            &path,
            r#"
id = "cursor-agent"
display_name = "User Cursor"
binary = "nonexistent-cursor-agent-xyz"
"#,
        )
        .unwrap();

        let reg = AgentRegistry::from_config_dir(dir.path()).unwrap();
        let entry = reg.get(&AgentId::new("cursor-agent")).unwrap();
        assert_eq!(entry.manifest.display_name, "User Cursor");
        assert!(!entry.installed, "override points at a missing binary");
    }

    #[test]
    fn missing_config_dir_is_not_an_error() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        let reg = AgentRegistry::from_config_dir(&missing).unwrap();
        assert!(reg.get(&AgentId::new("cursor-agent")).is_some());
    }

    #[test]
    fn broken_user_manifest_surfaces_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("broken.toml"), b"this is not toml: :: :").unwrap();
        let err = AgentRegistry::from_config_dir(dir.path()).unwrap_err();
        assert!(matches!(err, AgentError::ManifestDecode { .. }));
    }

    #[test]
    fn extract_version_with_capture_group() {
        let v = extract_version(r"^cursor-agent\s+(\S+)", "cursor-agent 0.42.1\n");
        assert_eq!(v.as_deref(), Some("0.42.1"));
    }

    #[test]
    fn extract_version_fallback_to_full_match() {
        let v = extract_version(r"\d+\.\d+\.\d+", "vX 1.2.3 beta");
        assert_eq!(v.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn extract_version_invalid_regex_returns_none() {
        let v = extract_version("(", "whatever");
        assert_eq!(v, None);
    }

    // Detection against a real binary: use `echo` on Unix as a
    // universally-available stand-in.
    #[cfg(unix)]
    #[test]
    fn detect_one_with_stub_binary_reports_installed() {
        let toml_text = r#"
id = "stub"
display_name = "Stub"
binary = "echo"

[detect]
args = ["stub-version", "9.9.9"]
version_regex = '(\d+\.\d+\.\d+)'
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("t.toml")).unwrap();
        let (installed, path, version, warning) = detect_one(&m);
        assert!(installed);
        assert!(path.is_some());
        assert_eq!(version.as_deref(), Some("9.9.9"));
        assert!(warning.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn detect_one_with_missing_binary_reports_not_installed() {
        let toml_text = r#"
id = "ghost"
display_name = "Ghost"
binary = "this-binary-does-not-exist-lattice-test-xyz"
"#;
        let m = AgentManifest::from_toml(toml_text.as_bytes(), Path::new("t.toml")).unwrap();
        let (installed, path, _, _) = detect_one(&m);
        assert!(!installed);
        assert!(path.is_none());
    }
}
