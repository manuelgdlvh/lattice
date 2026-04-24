//! Production implementations of the `lattice-core` derived-value
//! provider traits. These are deliberately kept in `lattice-store` so
//! that `lattice-core` remains I/O-free and deterministic by default.
//!
//! - [`RealFs`] — reads files and walks project trees.
//! - [`RealCmd`] — spawns commands with a bounded timeout and captures
//!   stdout. No shell is ever involved; `argv` goes straight to
//!   `std::process::Command`.
//! - [`RealEnv`] — reads process environment variables.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use lattice_core::derived::{CmdOutcome, CmdProvider, EnvProvider, FsProvider};
use lattice_core::error::DeriveError;

// -------- Filesystem -------------------------------------------------

#[derive(Debug, Default)]
pub struct RealFs;

impl RealFs {
    pub fn new() -> Self {
        Self
    }
}

impl FsProvider for RealFs {
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read(path)
    }

    fn list_tree(
        &self,
        root: &Path,
        depth: u32,
        exclude: &[String],
    ) -> Result<Vec<PathBuf>, std::io::Error> {
        let mut out = Vec::new();
        walk(root, depth, exclude, &mut out)?;
        out.sort();
        Ok(out)
    }
}

fn is_excluded(name: &str, exclude: &[String]) -> bool {
    // Dumb substring match for v0.1. "node_modules", "target", ".git"
    // are the motivating cases, all unambiguous. A glob matcher can
    // come later without breaking the trait.
    exclude.iter().any(|e| name == e || name.contains(e))
}

fn walk(
    current: &Path,
    remaining_depth: u32,
    exclude: &[String],
    out: &mut Vec<PathBuf>,
) -> Result<(), std::io::Error> {
    if remaining_depth == 0 {
        return Ok(());
    }
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if is_excluded(name_str, exclude) {
            continue;
        }
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(&path, remaining_depth - 1, exclude, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

// -------- Command ----------------------------------------------------

#[derive(Debug, Default)]
pub struct RealCmd;

impl RealCmd {
    pub fn new() -> Self {
        Self
    }
}

impl CmdProvider for RealCmd {
    fn run(&self, cwd: &Path, argv: &[String], timeout_ms: u64) -> Result<CmdOutcome, DeriveError> {
        let Some((exe, rest)) = argv.split_first() else {
            return Err(DeriveError::ProviderFailed {
                name: "cmd".into(),
                reason: "argv must be non-empty".into(),
            });
        };

        let mut cmd = Command::new(exe);
        cmd.args(rest)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| DeriveError::ProviderFailed {
            name: "cmd".into(),
            reason: format!("spawn `{exe}`: {e}"),
        })?;

        // We need wait-with-timeout. `std::process::Child` doesn't
        // offer one, so we bounce the wait through a thread and race
        // against a timeout.
        let (tx, rx) = mpsc::channel();
        let pid = child.id();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        thread::spawn(move || {
            let status = child.wait();
            // Capture streams after the process has exited; reads on a
            // closed pipe complete immediately.
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
            Ok((Ok(status), stdout, _stderr)) => Ok(CmdOutcome {
                stdout,
                exit_code: status.code(),
            }),
            Ok((Err(e), _, _)) => Err(DeriveError::ProviderFailed {
                name: "cmd".into(),
                reason: format!("wait failed for pid {pid}: {e}"),
            }),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Best-effort kill. We can't join the thread cleanly
                // here, but since the child is killed, its wait will
                // return and the thread terminates.
                kill_pid(pid);
                Err(DeriveError::ProviderFailed {
                    name: "cmd".into(),
                    reason: format!("timed out after {timeout_ms}ms"),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DeriveError::ProviderFailed {
                name: "cmd".into(),
                reason: "wait thread disconnected".into(),
            }),
        }
    }
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    // SAFETY-conscious path: `pid` is a `u32`; `kill(2)` accepts
    // `pid_t`. We avoid the raw `libc` crate by shelling out to /bin/kill.
    let _ = std::process::Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/F")
        .status();
}

// -------- Environment ------------------------------------------------

#[derive(Debug, Default)]
pub struct RealEnv;

impl RealEnv {
    pub fn new() -> Self {
        Self
    }
}

impl EnvProvider for RealEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::derived::DerivedResolver;
    use lattice_core::entities::DerivedSpec;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn real_fs_reads_and_walks() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), b"hello").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), b"fn main() {}").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/junk"), b"").unwrap();

        let fs = RealFs;
        let bytes = fs.read_file(&root.join("README.md")).unwrap();
        assert_eq!(bytes, b"hello");

        let files = fs.list_tree(root, 5, &["target".into()]).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("README.md")));
        assert!(
            names
                .iter()
                .any(|n| n.ends_with("src/lib.rs") || n.ends_with("src\\lib.rs"))
        );
        assert!(!names.iter().any(|n| n.contains("target")));
    }

    #[test]
    fn real_fs_respects_depth() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("top.txt"), b"x").unwrap();
        std::fs::write(root.join("a/mid.txt"), b"x").unwrap();
        std::fs::write(root.join("a/b/c/deep.txt"), b"x").unwrap();

        let fs = RealFs;
        // depth=1 sees only top-level files.
        let shallow = fs.list_tree(root, 1, &[]).unwrap();
        assert_eq!(shallow.len(), 1);
        assert!(shallow[0].ends_with("top.txt"));

        // depth=10 sees everything.
        let deep = fs.list_tree(root, 10, &[]).unwrap();
        assert_eq!(deep.len(), 3);
    }

    #[test]
    fn real_env_missing_var_returns_none() {
        // Using a vanishingly unlikely key avoids clashing with the
        // real environment; directly testing the `Some` path would
        // require calling `set_var`, which is `unsafe` in Rust 2024
        // and disallowed by the workspace lint.
        let key = "LATTICE_TEST_NEVER_SET_PLEASE_12345";
        assert_eq!(RealEnv.get(key), None);
    }

    #[test]
    fn real_env_reads_path_from_process() {
        // PATH is essentially always set on Unix test hosts. We only
        // assert it's non-empty, not its value.
        if let Some(v) = RealEnv.get("PATH") {
            assert!(!v.is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn real_cmd_captures_stdout() {
        let dir = TempDir::new().unwrap();
        let out = RealCmd
            .run(dir.path(), &["echo".into(), "hello".into()], 3_000)
            .unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn real_cmd_reports_nonzero_exit() {
        let dir = TempDir::new().unwrap();
        let out = RealCmd.run(dir.path(), &["false".into()], 3_000).unwrap();
        assert_ne!(out.exit_code, Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn real_cmd_times_out() {
        let dir = TempDir::new().unwrap();
        let res = RealCmd.run(dir.path(), &["sleep".into(), "10".into()], 200);
        assert!(matches!(res, Err(DeriveError::ProviderFailed { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn derived_resolver_end_to_end_on_disk() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), b"hi").unwrap();

        let fs = RealFs;
        let cmd = RealCmd;
        let env = RealEnv;
        let resolver = DerivedResolver {
            project_root: root.to_path_buf(),
            fs: &fs,
            cmd: &cmd,
            env: &env,
        };

        let mut specs = BTreeMap::new();
        specs.insert(
            "readme".into(),
            DerivedSpec(serde_json::json!({ "file": "README.md" })),
        );
        specs.insert(
            "echo".into(),
            DerivedSpec(serde_json::json!({ "cmd": ["echo", "from-cmd"], "timeout_ms": 3000 })),
        );
        let out = resolver.resolve_all(&specs).unwrap();
        assert_eq!(out["readme"].as_str().unwrap(), "hi");
        assert_eq!(out["echo"].as_str().unwrap(), "from-cmd");
    }
}
