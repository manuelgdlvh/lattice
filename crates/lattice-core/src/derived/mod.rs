//! Derived-value providers.
//!
//! A derived value is resolved at **task-creation time** and frozen in
//! the task record. The allow-list in v0.1 is:
//!
//! - `file` — read a project-relative file, UTF-8, optionally truncated
//! - `cmd` — spawn an argv command (no shell), capture trimmed stdout
//! - `env` — read an environment variable
//! - `tree` — list files under the project root, optionally filtered
//!
//! Providers depend on two small traits — `FsProvider` and
//! `CmdProvider` — so tests can inject deterministic fakes. The default
//! production pair is `RealFs` / `RealCmd` and is implemented in
//! `lattice-store` (core stays I/O-free).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entities::DerivedSpec;
use crate::error::DeriveError;

/// File-system access the derived layer needs. Production impl is in
/// `lattice-store`; tests use an in-memory stub (see this module's
/// `tests` section).
pub trait FsProvider: Send + Sync {
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, std::io::Error>;
    fn list_tree(
        &self,
        root: &Path,
        depth: u32,
        exclude: &[String],
    ) -> Result<Vec<PathBuf>, std::io::Error>;
}

/// Command execution the derived layer needs. Production impl is in
/// `lattice-store`; tests stub it.
pub trait CmdProvider: Send + Sync {
    /// Execute `argv` in `cwd` with an optional timeout. Return captured
    /// stdout bytes and the exit status.
    fn run(&self, cwd: &Path, argv: &[String], timeout_ms: u64) -> Result<CmdOutcome, DeriveError>;
}

#[derive(Debug)]
pub struct CmdOutcome {
    pub stdout: Vec<u8>,
    pub exit_code: Option<i32>,
}

/// Look up a (whitelisted) env var. Tiny trait so tests can avoid
/// touching real env.
pub trait EnvProvider: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
}

/// Resolver bundling the three traits + the project root.
pub struct DerivedResolver<'a> {
    pub project_root: PathBuf,
    pub fs: &'a dyn FsProvider,
    pub cmd: &'a dyn CmdProvider,
    pub env: &'a dyn EnvProvider,
}

impl std::fmt::Debug for DerivedResolver<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedResolver")
            .field("project_root", &self.project_root)
            .finish()
    }
}

impl DerivedResolver<'_> {
    /// Resolve every spec in a template into a `(name -> value)` map.
    /// Optional providers that fail return `Value::Null`; non-optional
    /// failures abort with an error.
    pub fn resolve_all(
        &self,
        specs: &BTreeMap<String, DerivedSpec>,
    ) -> Result<BTreeMap<String, Value>, DeriveError> {
        let mut out = BTreeMap::new();
        for (name, spec) in specs {
            let parsed = ProviderConfig::from_value(name, &spec.0)?;
            let val = self.resolve_one(name, &parsed);
            match val {
                Ok(v) => {
                    out.insert(name.clone(), v);
                }
                Err(e) if parsed.optional => {
                    tracing_or_noop(&format!("derived `{name}` optional failed: {e}"));
                    out.insert(name.clone(), Value::Null);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    fn resolve_one(&self, name: &str, cfg: &ProviderConfig) -> Result<Value, DeriveError> {
        match &cfg.kind {
            ProviderKind::File { file, max_bytes } => {
                let abs = self.project_root.join(file);
                let mut bytes =
                    self.fs
                        .read_file(&abs)
                        .map_err(|e| DeriveError::ProviderFailed {
                            name: name.into(),
                            reason: e.to_string(),
                        })?;
                if let Some(cap) = max_bytes
                    && bytes.len() > *cap
                {
                    bytes.truncate(*cap);
                }
                let s = String::from_utf8(bytes).map_err(|e| DeriveError::ProviderFailed {
                    name: name.into(),
                    reason: e.to_string(),
                })?;
                Ok(Value::String(s))
            }
            ProviderKind::Cmd { argv, timeout_ms } => {
                if argv.is_empty() {
                    return Err(DeriveError::ProviderFailed {
                        name: name.into(),
                        reason: "cmd argv must be non-empty".into(),
                    });
                }
                let outcome = self.cmd.run(&self.project_root, argv, *timeout_ms)?;
                if let Some(code) = outcome.exit_code
                    && code != 0
                {
                    return Err(DeriveError::ProviderFailed {
                        name: name.into(),
                        reason: format!("cmd exited with status {code}"),
                    });
                }
                let s =
                    String::from_utf8(outcome.stdout).map_err(|e| DeriveError::ProviderFailed {
                        name: name.into(),
                        reason: e.to_string(),
                    })?;
                Ok(Value::String(s.trim_end().to_string()))
            }
            ProviderKind::Env { key } => match self.env.get(key) {
                Some(v) => Ok(Value::String(v)),
                None => Err(DeriveError::ProviderFailed {
                    name: name.into(),
                    reason: format!("env var `{key}` is not set"),
                }),
            },
            ProviderKind::Tree {
                depth,
                exclude,
                max_entries,
            } => {
                let entries = self
                    .fs
                    .list_tree(&self.project_root, *depth, exclude)
                    .map_err(|e| DeriveError::ProviderFailed {
                        name: name.into(),
                        reason: e.to_string(),
                    })?;
                let limited: Vec<_> = entries.into_iter().take(*max_entries).collect();
                let mut buf = String::new();
                for p in limited {
                    let rel = p.strip_prefix(&self.project_root).unwrap_or(&p);
                    buf.push_str(&rel.to_string_lossy());
                    buf.push('\n');
                }
                Ok(Value::String(buf))
            }
        }
    }
}

/// Internal parsed form of a provider declaration. Kept private so
/// callers always go through `resolve_all` which validates shape.
#[derive(Debug)]
struct ProviderConfig {
    optional: bool,
    kind: ProviderKind,
}

#[derive(Debug)]
enum ProviderKind {
    File {
        file: String,
        max_bytes: Option<usize>,
    },
    Cmd {
        argv: Vec<String>,
        timeout_ms: u64,
    },
    Env {
        key: String,
    },
    Tree {
        depth: u32,
        exclude: Vec<String>,
        max_entries: usize,
    },
}

impl ProviderConfig {
    fn from_value(name: &str, v: &Value) -> Result<Self, DeriveError> {
        let Some(map) = v.as_object() else {
            return Err(DeriveError::ProviderFailed {
                name: name.into(),
                reason: "expected a table".into(),
            });
        };

        let optional = map
            .get("optional")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if let Some(v) = map.get("file") {
            let file = v.as_str().ok_or_else(|| DeriveError::ProviderFailed {
                name: name.into(),
                reason: "`file` must be a string".into(),
            })?;
            let max_bytes = map
                .get("max_bytes")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok());
            return Ok(Self {
                optional,
                kind: ProviderKind::File {
                    file: file.into(),
                    max_bytes,
                },
            });
        }

        if let Some(v) = map.get("cmd") {
            let arr = v.as_array().ok_or_else(|| DeriveError::ProviderFailed {
                name: name.into(),
                reason: "`cmd` must be an array of strings".into(),
            })?;
            let mut argv = Vec::with_capacity(arr.len());
            for a in arr {
                let s = a.as_str().ok_or_else(|| DeriveError::ProviderFailed {
                    name: name.into(),
                    reason: "`cmd` entries must be strings".into(),
                })?;
                argv.push(s.to_string());
            }
            let timeout_ms = map
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(5_000);
            return Ok(Self {
                optional,
                kind: ProviderKind::Cmd { argv, timeout_ms },
            });
        }

        if let Some(v) = map.get("env") {
            let key = v.as_str().ok_or_else(|| DeriveError::ProviderFailed {
                name: name.into(),
                reason: "`env` must be a string".into(),
            })?;
            return Ok(Self {
                optional,
                kind: ProviderKind::Env { key: key.into() },
            });
        }

        if let Some(v) = map.get("tree") {
            let inner = v.as_object().ok_or_else(|| DeriveError::ProviderFailed {
                name: name.into(),
                reason: "`tree` must be a table".into(),
            })?;
            let depth = inner
                .get("depth")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(2);
            let exclude =
                inner
                    .get("exclude")
                    .and_then(Value::as_array)
                    .map_or_else(Vec::new, |a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(ToString::to_string))
                            .collect()
                    });
            let max_entries = inner
                .get("max_entries")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(500);
            return Ok(Self {
                optional,
                kind: ProviderKind::Tree {
                    depth,
                    exclude,
                    max_entries,
                },
            });
        }

        Err(DeriveError::UnknownKind(name.into()))
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
struct _Marker;

fn tracing_or_noop(_m: &str) {
    // No-op; lattice-core cannot depend on `tracing` (pure-core rule).
    // The store/bin layer emits a real log when it sees a swallowed
    // optional error.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct FakeFs {
        files: HashMap<PathBuf, Vec<u8>>,
        tree: Vec<PathBuf>,
    }
    impl FsProvider for FakeFs {
        fn read_file(&self, p: &Path) -> Result<Vec<u8>, std::io::Error> {
            self.files
                .get(p)
                .cloned()
                .ok_or_else(|| std::io::Error::other(format!("no such file: {p:?}")))
        }
        fn list_tree(
            &self,
            _root: &Path,
            _depth: u32,
            _exclude: &[String],
        ) -> Result<Vec<PathBuf>, std::io::Error> {
            Ok(self.tree.clone())
        }
    }

    #[derive(Default)]
    struct FakeCmd {
        outcomes: HashMap<String, CmdOutcome>,
    }
    impl CmdProvider for FakeCmd {
        fn run(
            &self,
            _cwd: &Path,
            argv: &[String],
            _timeout_ms: u64,
        ) -> Result<CmdOutcome, DeriveError> {
            let key = argv.join(" ");
            self.outcomes
                .get(&key)
                .map(|o| CmdOutcome {
                    stdout: o.stdout.clone(),
                    exit_code: o.exit_code,
                })
                .ok_or(DeriveError::ProviderFailed {
                    name: "cmd".into(),
                    reason: format!("unexpected argv in test: {key}"),
                })
        }
    }

    #[derive(Default)]
    struct FakeEnv {
        vars: HashMap<String, String>,
    }
    impl EnvProvider for FakeEnv {
        fn get(&self, k: &str) -> Option<String> {
            self.vars.get(k).cloned()
        }
    }

    fn resolver<'a>(fs: &'a FakeFs, cmd: &'a FakeCmd, env: &'a FakeEnv) -> DerivedResolver<'a> {
        DerivedResolver {
            project_root: PathBuf::from("/project"),
            fs,
            cmd,
            env,
        }
    }

    #[test]
    fn file_provider_truncates() {
        let mut fs = FakeFs::default();
        fs.files
            .insert(PathBuf::from("/project/README.md"), vec![b'a'; 10_000]);
        let cmd = FakeCmd::default();
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "readme".into(),
            DerivedSpec(serde_json::json!({ "file": "README.md", "max_bytes": 4096 })),
        );
        let out = resolver(&fs, &cmd, &env).resolve_all(&specs).unwrap();
        assert_eq!(out["readme"].as_str().unwrap().len(), 4096);
    }

    #[test]
    fn cmd_provider_trims_output() {
        let fs = FakeFs::default();
        let mut cmd = FakeCmd::default();
        cmd.outcomes.insert(
            "git rev-parse HEAD".into(),
            CmdOutcome {
                stdout: b"deadbeef\n".to_vec(),
                exit_code: Some(0),
            },
        );
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "head".into(),
            DerivedSpec(serde_json::json!({ "cmd": ["git", "rev-parse", "HEAD"] })),
        );
        let out = resolver(&fs, &cmd, &env).resolve_all(&specs).unwrap();
        assert_eq!(out["head"].as_str().unwrap(), "deadbeef");
    }

    #[test]
    fn cmd_nonzero_exit_fails_unless_optional() {
        let fs = FakeFs::default();
        let mut cmd = FakeCmd::default();
        cmd.outcomes.insert(
            "false".into(),
            CmdOutcome {
                stdout: vec![],
                exit_code: Some(1),
            },
        );
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "x".into(),
            DerivedSpec(serde_json::json!({ "cmd": ["false"] })),
        );
        let res = resolver(&fs, &cmd, &env).resolve_all(&specs);
        assert!(res.is_err());

        // Now with optional=true
        let mut specs = BTreeMap::new();
        specs.insert(
            "x".into(),
            DerivedSpec(serde_json::json!({ "cmd": ["false"], "optional": true })),
        );
        let out = resolver(&fs, &cmd, &env).resolve_all(&specs).unwrap();
        assert_eq!(out["x"], Value::Null);
    }

    #[test]
    fn env_provider_missing_required_fails() {
        let fs = FakeFs::default();
        let cmd = FakeCmd::default();
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "proxy".into(),
            DerivedSpec(serde_json::json!({ "env": "HTTP_PROXY" })),
        );
        assert!(resolver(&fs, &cmd, &env).resolve_all(&specs).is_err());
    }

    #[test]
    fn env_provider_optional_is_null_when_missing() {
        let fs = FakeFs::default();
        let cmd = FakeCmd::default();
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "proxy".into(),
            DerivedSpec(serde_json::json!({ "env": "HTTP_PROXY", "optional": true })),
        );
        let out = resolver(&fs, &cmd, &env).resolve_all(&specs).unwrap();
        assert_eq!(out["proxy"], Value::Null);
    }

    #[test]
    fn tree_provider_respects_max_entries() {
        let fs = FakeFs {
            tree: (0..1000)
                .map(|i| PathBuf::from(format!("/project/f{i}.rs")))
                .collect(),
            ..FakeFs::default()
        };
        let cmd = FakeCmd::default();
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "tree".into(),
            DerivedSpec(serde_json::json!({ "tree": { "depth": 1, "max_entries": 5 } })),
        );
        let out = resolver(&fs, &cmd, &env).resolve_all(&specs).unwrap();
        let rendered = out["tree"].as_str().unwrap();
        assert_eq!(rendered.lines().count(), 5);
    }

    #[test]
    fn unknown_provider_errors() {
        let fs = FakeFs::default();
        let cmd = FakeCmd::default();
        let env = FakeEnv::default();
        let mut specs = BTreeMap::new();
        specs.insert(
            "oops".into(),
            DerivedSpec(serde_json::json!({ "unknown": "x" })),
        );
        let err = resolver(&fs, &cmd, &env).resolve_all(&specs).unwrap_err();
        assert!(matches!(err, DeriveError::UnknownKind(_)));
    }
}
