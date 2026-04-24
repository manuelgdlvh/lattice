//! Path resolution for lattice's on-disk layout.
//!
//! - **config root** — user-editable TOML (settings, agent manifests)
//!   - Linux:   `$XDG_CONFIG_HOME/lattice` (defaults to `~/.config/lattice`)
//! - **state root** — app-owned state (projects, templates, tasks, runs, queues)
//!   - Linux:   `$XDG_STATE_HOME/lattice` (defaults to `~/.local/state/lattice`)
//!   - *Note*: `directories` crate uses `data_dir()` for state. We use its value as
//!     our `state_root`. That lands at `~/.local/share/lattice` on Linux, which
//!     `docs/DATA_MODEL.md` documents as an acceptable default.
//!
//! Tests pass explicit roots via [`Paths::with_roots`] so nothing touches
//! the real XDG dirs.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{StoreError, StoreResult};

/// All the derived filesystem locations the store + sibling crates use.
///
/// Layouts (see `docs/DATA_MODEL.md §2`):
///
/// ```text
/// config_root/
///   settings.toml
///   agents/<agent_id>.toml
/// state_root/
///   projects/<project_id>/project.toml
///   templates/<template_id>/template.toml
///   tasks/<project_id>/<task_id>/{task.toml, template.snapshot.toml, prompt.md}
///   runs/<project_id>/<run_id>/{run.toml, exit.toml, stdout.log, stderr.log}
///   queues/<project_id>.toml
///   cache/           (opaque: LRU spill, if ever needed)
///   logs/lattice.log
///   tmp/             (atomic-write staging)
/// ```
#[derive(Clone, Debug)]
pub struct Paths {
    config_root: PathBuf,
    state_root: PathBuf,
}

impl Paths {
    /// Resolve via `directories::ProjectDirs` (production).
    pub fn xdg() -> StoreResult<Self> {
        let dirs = ProjectDirs::from("dev", "lattice", "lattice").ok_or(StoreError::NoHomeDir)?;
        Ok(Self {
            config_root: dirs.config_dir().to_path_buf(),
            state_root: dirs.data_dir().to_path_buf(),
        })
    }

    /// Explicit roots — used by tests and by users who set
    /// `LATTICE_CONFIG_DIR` / `LATTICE_STATE_DIR`.
    pub fn with_roots(config_root: impl Into<PathBuf>, state_root: impl Into<PathBuf>) -> Self {
        Self {
            config_root: config_root.into(),
            state_root: state_root.into(),
        }
    }

    /// Env-var-aware resolver. Falls back to XDG if the envs are unset.
    /// Keeps env parsing centralized so we can mock it in tests later.
    pub fn from_env_or_xdg() -> StoreResult<Self> {
        let cfg = std::env::var_os("LATTICE_CONFIG_DIR").map(PathBuf::from);
        let state = std::env::var_os("LATTICE_STATE_DIR").map(PathBuf::from);
        if let (Some(c), Some(s)) = (cfg.clone(), state.clone()) {
            return Ok(Self::with_roots(c, s));
        }
        let xdg = Self::xdg()?;
        let cfg = cfg.unwrap_or(xdg.config_root);
        let state = state.unwrap_or(xdg.state_root);
        Ok(Self::with_roots(cfg, state))
    }

    pub fn config_root(&self) -> &Path {
        &self.config_root
    }
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_root.join("settings.toml")
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.config_root.join("agents")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.state_root.join("projects")
    }

    pub fn project_dir(&self, project_id: &str) -> PathBuf {
        self.projects_dir().join(project_id)
    }

    pub fn project_file(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("project.toml")
    }

    pub fn templates_dir(&self) -> PathBuf {
        self.state_root.join("templates")
    }

    pub fn template_dir(&self, template_id: &str) -> PathBuf {
        self.templates_dir().join(template_id)
    }

    pub fn template_file(&self, template_id: &str) -> PathBuf {
        self.template_dir(template_id).join("template.toml")
    }

    pub fn tasks_root(&self) -> PathBuf {
        self.state_root.join("tasks")
    }

    pub fn tasks_dir(&self, project_id: &str) -> PathBuf {
        self.tasks_root().join(project_id)
    }

    pub fn task_dir(&self, project_id: &str, task_id: &str) -> PathBuf {
        self.tasks_dir(project_id).join(task_id)
    }

    pub fn task_file(&self, project_id: &str, task_id: &str) -> PathBuf {
        self.task_dir(project_id, task_id).join("task.toml")
    }

    pub fn task_template_snapshot(&self, project_id: &str, task_id: &str) -> PathBuf {
        self.task_dir(project_id, task_id)
            .join("template.snapshot.toml")
    }

    pub fn task_prompt(&self, project_id: &str, task_id: &str) -> PathBuf {
        self.task_dir(project_id, task_id).join("prompt.md")
    }

    pub fn runs_root(&self) -> PathBuf {
        self.state_root.join("runs")
    }

    pub fn runs_dir(&self, project_id: &str) -> PathBuf {
        self.runs_root().join(project_id)
    }

    pub fn run_dir(&self, project_id: &str, run_id: &str) -> PathBuf {
        self.runs_dir(project_id).join(run_id)
    }

    pub fn run_file(&self, project_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(project_id, run_id).join("run.toml")
    }

    pub fn run_exit_file(&self, project_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(project_id, run_id).join("exit.toml")
    }

    pub fn run_stdout(&self, project_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(project_id, run_id).join("stdout.log")
    }

    pub fn run_stderr(&self, project_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(project_id, run_id).join("stderr.log")
    }

    pub fn queues_dir(&self) -> PathBuf {
        self.state_root.join("queues")
    }

    pub fn queue_file(&self, project_id: &str) -> PathBuf {
        self.queues_dir().join(format!("{project_id}.toml"))
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.state_root.join("tmp")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.state_root.join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn with_roots_derives_all_locations() {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());

        assert_eq!(paths.settings_file(), cfg.path().join("settings.toml"));
        assert_eq!(paths.agents_dir(), cfg.path().join("agents"));
        assert_eq!(paths.projects_dir(), state.path().join("projects"));
        assert_eq!(
            paths.project_file("P1"),
            state.path().join("projects/P1/project.toml")
        );
        assert_eq!(
            paths.template_file("T1"),
            state.path().join("templates/T1/template.toml")
        );
        assert_eq!(
            paths.task_file("P1", "K1"),
            state.path().join("tasks/P1/K1/task.toml")
        );
        assert_eq!(
            paths.run_file("P1", "R1"),
            state.path().join("runs/P1/R1/run.toml")
        );
        assert_eq!(paths.queue_file("P1"), state.path().join("queues/P1.toml"));
        assert_eq!(paths.tmp_dir(), state.path().join("tmp"));
        assert_eq!(paths.logs_dir(), state.path().join("logs"));
    }
}
