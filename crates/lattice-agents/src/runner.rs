//! `AgentRunner` + `RunHandle` — spawn an agent for one task, tee its
//! stdout/stderr to disk and a broadcast channel, and supervise the
//! process until it exits or is killed.
//!
//! ## Supervision model
//!
//! ```text
//!   ┌────────────┐     spawn       ┌─────────────┐
//!   │ AgentRunner├─────────────────►  child proc │
//!   └─────┬──────┘                 └──────┬──────┘
//!         │ RunHandle (clone-able)        │ stdout / stderr pipes
//!         ▼                               ▼
//!   ┌────────────┐   broadcast     ┌─────────────┐
//!   │  UI / TUI  │ ◄───────────────┤  stream tee │
//!   │  tail logs │                 │  task       │
//!   └────────────┘                 └──────┬──────┘
//!                                         │ append bytes
//!                                         ▼
//!                                 ┌─────────────┐
//!                                 │ stdout.log  │
//!                                 │ stderr.log  │
//!                                 └─────────────┘
//! ```
//!
//! The runner does not *own* `Run`/`RunExit` entities — that's the
//! queue/store layer's job. Callers that want those written to disk
//! do so after observing the final exit via [`RunHandle::wait`].

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::fs::{self as tokio_fs, File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, broadcast, watch};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use lattice_core::entities::TaskStatus;
use lattice_core::ids::RunId;

use crate::error::{AgentError, AgentResult};
use crate::manifest::{AgentManifest, InvocationMode, WorkingDir};

const BROADCAST_CAPACITY: usize = 1024;

/// Everything the runner needs to spawn a single agent instance.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    pub run_id: RunId,
    /// Fully validated manifest — the registry hands this out.
    pub manifest: AgentManifest,
    /// The target project's root directory. Used when
    /// `manifest.runtime.working_dir == Project`.
    pub project_root: PathBuf,
    /// The task prompt (already rendered from the template).
    pub prompt: String,
    /// Where to append stdout. Parent must exist.
    pub stdout_path: PathBuf,
    /// Where to append stderr. Parent must exist.
    pub stderr_path: PathBuf,
    /// For `InvocationMode::File` — where to write the prompt so the
    /// agent can read it back. The file is NOT removed automatically;
    /// it lives inside the run dir for post-mortem inspection.
    pub prompt_file_path: PathBuf,
    /// Extra env overlaid on the manifest's `env` table.
    pub extra_env: Vec<(String, String)>,
}

/// Which stream a log line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// One tailed line from the agent. Lines are UTF-8 best-effort
/// (invalid bytes are replaced) — agents that emit binary-only streams
/// are out of scope for v0.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
}

/// Final disposition of a spawned agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitReport {
    pub status: TaskStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub duration_ms: u64,
}

/// Clone-able handle. Dropping every clone does not abort the child;
/// the supervisor task keeps running until the process exits or
/// [`Self::kill`] is called.
#[derive(Clone, Debug)]
pub struct RunHandle {
    inner: Arc<RunHandleInner>,
}

#[derive(Debug)]
struct RunHandleInner {
    run_id: RunId,
    pid: Option<u32>,
    log_tx: broadcast::Sender<LogLine>,
    status_rx: watch::Receiver<TaskStatus>,
    /// `None` once the supervisor task has finished; we `take()` it
    /// in `wait` and join.
    supervisor: AsyncMutex<Option<JoinHandle<ExitReport>>>,
    /// `None` after kill has been issued so we don't double-kill.
    kill_tx: AsyncMutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Final exit is cached so repeated `wait()` calls return the same
    /// report without requiring `Mutex<Option<JoinHandle>>` re-entry.
    exit_cache: AsyncMutex<Option<ExitReport>>,
}

impl RunHandle {
    pub fn run_id(&self) -> RunId {
        self.inner.run_id
    }

    pub fn pid(&self) -> Option<u32> {
        self.inner.pid
    }

    /// Subscribe to the live log stream. Late subscribers miss lines
    /// emitted before they called `subscribe`; the full log is always
    /// available on disk at `stdout_path` / `stderr_path`.
    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.inner.log_tx.subscribe()
    }

    /// Watch the run's status transitions: `Running → {Succeeded |
    /// Failed | Killed | Interrupted}`.
    pub fn status(&self) -> watch::Receiver<TaskStatus> {
        self.inner.status_rx.clone()
    }

    /// Block until the child exits and return the final [`ExitReport`].
    pub async fn wait(&self) -> ExitReport {
        if let Some(r) = self.inner.exit_cache.lock().await.clone() {
            return r;
        }
        let handle = {
            let mut guard = self.inner.supervisor.lock().await;
            guard.take()
        };
        let report = match handle {
            Some(h) => h.await.unwrap_or(ExitReport {
                status: TaskStatus::Failed,
                exit_code: None,
                signal: Some("supervisor_join_failed".into()),
                duration_ms: 0,
            }),
            None => {
                // Supervisor already joined by a previous call; cache
                // should be populated but might still be lagging on a
                // very narrow race. Default to Interrupted.
                self.inner
                    .exit_cache
                    .lock()
                    .await
                    .clone()
                    .unwrap_or(ExitReport {
                        status: TaskStatus::Interrupted,
                        exit_code: None,
                        signal: None,
                        duration_ms: 0,
                    })
            }
        };
        *self.inner.exit_cache.lock().await = Some(report.clone());
        report
    }

    /// Request graceful shutdown. SIGTERM → wait `kill_grace_ms` →
    /// SIGKILL. Idempotent — subsequent calls are ignored.
    pub async fn kill(&self) {
        let tx = self.inner.kill_tx.lock().await.take();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }
}

/// Runner is essentially stateless; its only job is to own the
/// `spawn()` method. Kept as a struct so it can grow config (global
/// env, hooks, etc.) without another API break.
#[derive(Debug, Default, Clone)]
pub struct AgentRunner;

impl AgentRunner {
    pub fn new() -> Self {
        Self
    }

    /// Spawn an agent asynchronously. Returns once the child has been
    /// launched (or failed to launch); the supervisor task then runs
    /// in the background.
    pub async fn spawn(&self, spec: SpawnSpec) -> AgentResult<RunHandle> {
        ensure_parent_dirs(&spec).await?;

        // Materialize the prompt file if needed before we build argv,
        // because the path goes into argv.
        if spec.manifest.invocation.mode == InvocationMode::File {
            write_prompt_file(&spec.prompt_file_path, &spec.prompt).await?;
        }

        let argv = build_argv(&spec);
        let working_dir = resolve_working_dir(&spec.manifest, &spec.project_root);

        debug!(
            run_id = %spec.run_id,
            agent = %spec.manifest.id,
            cwd = %working_dir.display(),
            "spawning agent"
        );

        let stdin_mode = spec.manifest.invocation.mode == InvocationMode::Stdin;

        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .current_dir(&working_dir)
            .stdin(if stdin_mode {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Manifest env, then overrides.
        for (k, v) in &spec.manifest.env {
            cmd.env(k, v);
        }
        for (k, v) in &spec.extra_env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(|e| AgentError::Spawn {
            id: spec.manifest.id.to_string(),
            source: e,
        })?;
        let pid = child.id();

        // Feed the prompt on stdin if that's the mode.
        if stdin_mode && let Some(mut stdin) = child.stdin.take() {
            let prompt = spec.prompt.clone();
            tokio::spawn(async move {
                if let Err(e) = stdin.write_all(prompt.as_bytes()).await {
                    warn!("failed to write prompt to agent stdin: {e}");
                    return;
                }
                // Close stdin so the agent sees EOF.
                drop(stdin);
            });
        }

        let (log_tx, _) = broadcast::channel::<LogLine>(BROADCAST_CAPACITY);
        let (status_tx, status_rx) = watch::channel(TaskStatus::Running);
        let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();

        let tee_stdout = spawn_tee(
            child.stdout.take(),
            LogStream::Stdout,
            spec.stdout_path.clone(),
            log_tx.clone(),
        );
        let tee_stderr = spawn_tee(
            child.stderr.take(),
            LogStream::Stderr,
            spec.stderr_path.clone(),
            log_tx.clone(),
        );

        let supervisor = tokio::spawn(supervise(
            spec.manifest.runtime.kill_grace_ms,
            child,
            tee_stdout,
            tee_stderr,
            kill_rx,
            status_tx,
            Instant::now(),
        ));

        Ok(RunHandle {
            inner: Arc::new(RunHandleInner {
                run_id: spec.run_id,
                pid,
                log_tx,
                status_rx,
                supervisor: AsyncMutex::new(Some(supervisor)),
                kill_tx: AsyncMutex::new(Some(kill_tx)),
                exit_cache: AsyncMutex::new(None),
            }),
        })
    }
}

async fn ensure_parent_dirs(spec: &SpawnSpec) -> AgentResult<()> {
    for p in [&spec.stdout_path, &spec.stderr_path, &spec.prompt_file_path] {
        if let Some(parent) = p.parent() {
            tokio_fs::create_dir_all(parent).await?;
        }
    }
    Ok(())
}

async fn write_prompt_file(path: &Path, prompt: &str) -> AgentResult<()> {
    let mut f = File::create(path).await?;
    f.write_all(prompt.as_bytes()).await?;
    f.sync_all().await?;
    Ok(())
}

fn build_argv(spec: &SpawnSpec) -> Vec<String> {
    let mut argv = Vec::with_capacity(1 + spec.manifest.invocation.args.len());
    argv.push(spec.manifest.binary.clone());
    for arg in &spec.manifest.invocation.args {
        argv.push(substitute_placeholders(arg, spec));
    }
    // `Arg` mode: the manifest's `args` MUST have contained a placeholder
    // (`validate()` enforces it) — so the substitution above already put
    // the prompt in argv. `File` mode: same story with `{prompt_file}`.
    // `Stdin` mode: prompt goes over the pipe, not argv.
    argv
}

fn substitute_placeholders(raw: &str, spec: &SpawnSpec) -> String {
    raw.replace("{prompt}", &spec.prompt)
        .replace("{prompt_file}", &spec.prompt_file_path.to_string_lossy())
        .replace("{project}", &spec.project_root.to_string_lossy())
        .replace("{run_id}", &spec.run_id.to_string())
}

fn resolve_working_dir(manifest: &AgentManifest, project_root: &Path) -> PathBuf {
    match &manifest.runtime.working_dir {
        WorkingDir::Project => project_root.to_path_buf(),
        WorkingDir::Custom { path } => path.clone(),
    }
}

fn spawn_tee<R>(
    pipe: Option<R>,
    stream: LogStream,
    dest_path: PathBuf,
    broadcaster: broadcast::Sender<LogLine>,
) -> Option<JoinHandle<()>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let pipe = pipe?;
    Some(tokio::spawn(async move {
        let mut reader = BufReader::new(pipe).lines();
        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&dest_path)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                warn!(path = %dest_path.display(), "failed to open log file: {e}");
                return;
            }
        };
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    let mut bytes = line.clone();
                    bytes.push('\n');
                    if let Err(e) = file.write_all(bytes.as_bytes()).await {
                        warn!(path = %dest_path.display(), "log write failed: {e}");
                        break;
                    }
                    // `send` errors only when there are no receivers,
                    // which is fine for a fire-and-forget tail.
                    let _ = broadcaster.send(LogLine { stream, text: line });
                }
                Ok(None) => break,
                Err(e) => {
                    warn!(path = %dest_path.display(), "log read failed: {e}");
                    break;
                }
            }
        }
        let _ = file.sync_data().await;
    }))
}

#[allow(clippy::too_many_arguments)]
async fn supervise(
    kill_grace_ms: u64,
    mut child: Child,
    tee_stdout: Option<JoinHandle<()>>,
    tee_stderr: Option<JoinHandle<()>>,
    mut kill_rx: tokio::sync::oneshot::Receiver<()>,
    status_tx: watch::Sender<TaskStatus>,
    started_at: Instant,
) -> ExitReport {
    // Two concurrent futures: the child exiting naturally, or a kill
    // request arriving.
    let report = tokio::select! {
        res = child.wait() => {
            finalize_from_wait(&res, started_at, TaskStatus::Succeeded, TaskStatus::Failed)
        }
        _ = &mut kill_rx => {
            graceful_then_forceful_kill(&mut child, kill_grace_ms, started_at).await
        }
    };

    // Drain any remaining tee output so the log files are complete
    // before we consider the run finished.
    if let Some(h) = tee_stdout {
        let _ = h.await;
    }
    if let Some(h) = tee_stderr {
        let _ = h.await;
    }

    let _ = status_tx.send(report.status);
    report
}

async fn graceful_then_forceful_kill(
    child: &mut Child,
    kill_grace_ms: u64,
    started_at: Instant,
) -> ExitReport {
    let pid = child.id();

    // Step 1: SIGTERM (via `kill -15`) if we have a PID.
    if let Some(pid) = pid {
        send_signal(pid, false);
    }

    // Step 2: wait up to `kill_grace_ms` for a clean exit.
    let graceful = tokio::time::timeout(Duration::from_millis(kill_grace_ms), child.wait()).await;
    if let Ok(Ok(status)) = graceful {
        return ExitReport {
            status: TaskStatus::Killed,
            exit_code: status.code(),
            signal: Some("term".into()),
            duration_ms: elapsed_ms(started_at),
        };
    }

    // Step 3: SIGKILL.
    if let Err(e) = child.start_kill() {
        warn!("child.start_kill failed: {e}");
    }
    let _ = child.wait().await;
    ExitReport {
        status: TaskStatus::Killed,
        exit_code: None,
        signal: Some("kill".into()),
        duration_ms: elapsed_ms(started_at),
    }
}

fn finalize_from_wait(
    res: &std::io::Result<std::process::ExitStatus>,
    started_at: Instant,
    on_success: TaskStatus,
    on_failure: TaskStatus,
) -> ExitReport {
    match res {
        Ok(status) => {
            let code = status.code();
            let signal = signal_name(*status);
            let outcome = if status.success() {
                on_success
            } else {
                on_failure
            };
            ExitReport {
                status: outcome,
                exit_code: code,
                signal,
                duration_ms: elapsed_ms(started_at),
            }
        }
        Err(_) => ExitReport {
            status: TaskStatus::Failed,
            exit_code: None,
            signal: None,
            duration_ms: elapsed_ms(started_at),
        },
    }
}

fn elapsed_ms(start: Instant) -> u64 {
    // `u128` → `u64` saturating conversion. A run that takes >584
    // million years has bigger problems than a truncated duration.
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(unix)]
fn signal_name(status: std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|s| format!("signal:{s}"))
}

#[cfg(not(unix))]
fn signal_name(_: std::process::ExitStatus) -> Option<String> {
    None
}

/// Send SIGTERM (default) or SIGKILL (when `force=true`) to `pid`.
/// We avoid the `libc` crate entirely — the workspace forbids unsafe
/// code — and shell out to `/bin/kill`.
#[cfg(unix)]
fn send_signal(pid: u32, force: bool) {
    use std::process::Command as StdCommand;
    let sig = if force { "-9" } else { "-15" };
    // Don't poll the status — this is best-effort; the supervisor's
    // `wait()` will observe the result either way.
    std::thread::spawn(move || {
        let _ = StdCommand::new("kill")
            .arg(sig)
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
    // Give the signal a moment to land before we race onward.
    std::thread::sleep(Duration::from_millis(10));
}

#[cfg(not(unix))]
fn send_signal(_pid: u32, _force: bool) {
    // On Windows we rely on `Child::start_kill` for the forceful path.
    // Graceful termination on Windows is a can of worms we defer.
}
