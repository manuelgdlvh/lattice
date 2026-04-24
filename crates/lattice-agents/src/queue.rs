//! `QueueEngine` — per-project FIFO dispatcher with a global concurrency
//! cap.
//!
//! ## Guarantees
//!
//! - **Per-project ordering**: tasks enqueued for the same project run
//!   strictly in the order they were enqueued.
//! - **No intra-project concurrency**: at most one agent per project is
//!   running at any moment. This protects the project's working tree
//!   from simultaneous writes.
//! - **Global cap**: a single semaphore caps the total number of
//!   concurrent agents across all projects.
//! - **Fail-fast (configurable per project)**: if a run ends in
//!   `Failed` / `Killed` / `Interrupted`, the remaining queued entries
//!   for that project are drained without running and each one is
//!   recorded as `Interrupted`.
//! - **Persistence**: every enqueue and every state transition hits
//!   the `Queues`/`Runs`/`Tasks` stores atomically (via the store's
//!   own atomic-write machinery).
//! - **Recovery**: persisted queues survive restarts; calling
//!   [`QueueEngine::resume_from_disk`] rehydrates workers for every
//!   project that still has entries.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore, broadcast, mpsc};
use tracing::{debug, info, warn};

use lattice_core::entities::{Queue, QueueEntry, Run, RunExit, RunLogInfo, TaskStatus};
use lattice_core::ids::{AgentId, ProjectId, RunId, TaskId};
use lattice_core::time::Timestamp;
use lattice_store::paths::Paths;
use lattice_store::store::{Queues, Runs, Tasks};

use crate::error::{AgentError, AgentResult};
use crate::manifest::AgentManifest;
use crate::registry::AgentRegistry;
use crate::runner::{AgentRunner, ExitReport, RunHandle, SpawnSpec};

const WORKER_CHANNEL_SIZE: usize = 32;
const EVENT_CHANNEL_SIZE: usize = 256;

/// Request to enqueue a single task-for-agent pair.
#[derive(Clone, Debug)]
pub struct EnqueueRequest {
    pub project_id: ProjectId,
    pub project_path: PathBuf,
    pub task_id: TaskId,
    pub agent_id: AgentId,
}

/// Event emitted by the queue as runs progress. Subscribers (the TUI,
/// mostly) receive these via a broadcast channel.
#[derive(Clone, Debug)]
pub enum QueueEvent {
    Enqueued {
        project: ProjectId,
        task: TaskId,
    },
    Started {
        project: ProjectId,
        task: TaskId,
        run: RunId,
    },
    Finished {
        project: ProjectId,
        task: TaskId,
        run: RunId,
        status: TaskStatus,
    },
    Interrupted {
        project: ProjectId,
        task: TaskId,
        reason: String,
    },
    Drained {
        project: ProjectId,
    },
    Paused {
        project: ProjectId,
        reason: String,
    },
    Resumed {
        project: ProjectId,
    },
}

/// Snapshot of a currently-executing run. The runtime screen shows
/// a list of these and lets the user kill individual entries.
#[derive(Clone, Debug)]
pub struct RunningRun {
    pub run_id: RunId,
    pub project_id: ProjectId,
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub pid: Option<u32>,
    pub handle: RunHandle,
}

/// Opaque configuration handed to [`QueueEngine::new`].
#[derive(Clone, Debug)]
pub struct QueueConfig {
    /// Global concurrency cap across all projects. `0` means "unlimited".
    pub max_concurrent: usize,
    /// Default per-project fail-fast policy.
    pub fail_fast: bool,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 0,
            // A single failed run should not "wedge" the project queue by default.
            // Users who want strict pipelines can enable fail-fast explicitly.
            fail_fast: false,
        }
    }
}

/// The central dispatcher. `Clone` is cheap — it shares all state via
/// `Arc` — so the TUI can hand clones to multiple screens.
#[derive(Clone, Debug)]
pub struct QueueEngine {
    inner: Arc<Inner>,
}

struct Inner {
    tasks_store: Arc<dyn Tasks>,
    runs_store: Arc<dyn Runs>,
    queues_store: Arc<dyn Queues>,
    registry: Arc<AgentRegistry>,
    runner: AgentRunner,
    paths: Paths,
    cfg: QueueConfig,
    global_sem: Arc<Semaphore>,
    events: broadcast::Sender<QueueEvent>,
    workers: Mutex<HashMap<ProjectId, WorkerSlot>>,
    running: Mutex<HashMap<RunId, RunningRun>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("cfg", &self.cfg)
            .field("worker_count", &"<mutex>")
            .finish_non_exhaustive()
    }
}

/// Per-project worker handle: a mailbox we can wake.
struct WorkerSlot {
    mailbox: mpsc::Sender<WorkerMsg>,
}

enum WorkerMsg {
    /// A new entry was persisted; drain what you can.
    Wake,
    /// Operator pause. `reason` is currently only surfaced as an event;
    /// the worker just needs to know that it should stop running new
    /// entries until it sees a `Resume`.
    Pause,
    /// Operator resume.
    Resume,
}

impl QueueEngine {
    pub fn new(
        tasks_store: Arc<dyn Tasks>,
        runs_store: Arc<dyn Runs>,
        queues_store: Arc<dyn Queues>,
        registry: Arc<AgentRegistry>,
        paths: Paths,
        cfg: QueueConfig,
    ) -> Self {
        // Semaphore permits: `0` is shorthand for "unlimited" but the
        // semaphore type needs a real number, so pick something large.
        let permits = if cfg.max_concurrent == 0 {
            // 1024 is way more than any sane deployment will ever hit
            // and still small enough to avoid allocator stress.
            1024
        } else {
            cfg.max_concurrent
        };
        let (events, _) = broadcast::channel(EVENT_CHANNEL_SIZE);
        Self {
            inner: Arc::new(Inner {
                tasks_store,
                runs_store,
                queues_store,
                registry,
                runner: AgentRunner::new(),
                paths,
                cfg,
                global_sem: Arc::new(Semaphore::new(permits)),
                events,
                workers: Mutex::new(HashMap::new()),
                running: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<QueueEvent> {
        self.inner.events.subscribe()
    }

    pub async fn running(&self) -> Vec<RunningRun> {
        self.inner.running.lock().await.values().cloned().collect()
    }

    /// Enqueue `(project, task, agent)`. Appends to the persisted queue
    /// and wakes the project's worker (creating one if needed).
    pub async fn enqueue(&self, req: EnqueueRequest) -> AgentResult<()> {
        // Validate task + agent exist up front so the UI gets an
        // immediate error instead of finding out at dispatch time.
        let task = self
            .inner
            .tasks_store
            .load(req.project_id, req.task_id)
            .await
            .map_err(AgentError::from)?
            .ok_or_else(|| AgentError::Invocation(format!("task {} not found", req.task_id)))?;
        if task.project_id != req.project_id {
            return Err(AgentError::Invocation(
                "task.project_id does not match request.project_id".into(),
            ));
        }

        let available =
            self.inner.registry.get(&req.agent_id).ok_or_else(|| {
                AgentError::Invocation(format!("unknown agent `{}`", req.agent_id))
            })?;
        if !available.installed {
            return Err(AgentError::NotInstalled(req.agent_id.to_string()));
        }

        // Persist queue append.
        let now = Timestamp::now();
        let entry = QueueEntry {
            task_id: req.task_id,
            agent_id: req.agent_id.clone(),
            enqueued_at: now,
        };
        let mut queue = self
            .inner
            .queues_store
            .load(req.project_id)
            .await
            .map_err(AgentError::from)?
            .unwrap_or_else(|| Queue::empty(req.project_id));
        // If the queue was auto-paused by a previous fail-fast run,
        // treat a new enqueue as an operator intent to continue.
        if queue.paused && queue.paused_reason.starts_with("fail-fast:") {
            queue.paused = false;
            queue.paused_reason.clear();
        }
        queue.push_back(entry);
        self.inner
            .queues_store
            .save(&queue)
            .await
            .map_err(AgentError::from)?;

        // Mark the task as queued (best-effort — a task already running
        // should stay running; we only re-mark Drafts/Queued).
        if task.status == TaskStatus::Draft {
            let mut updated = task.clone();
            updated.status = TaskStatus::Queued;
            self.inner
                .tasks_store
                .save(&updated)
                .await
                .map_err(AgentError::from)?;
        }

        // Ensure a worker exists.
        self.ensure_worker(req.project_id, req.project_path).await;

        let _ = self.inner.events.send(QueueEvent::Enqueued {
            project: req.project_id,
            task: req.task_id,
        });
        Ok(())
    }

    /// Start workers for every persisted non-empty queue. The caller
    /// supplies a project→path map so the worker can resolve working
    /// directories without another store roundtrip per entry.
    pub async fn resume_with_paths(&self, paths: &[(ProjectId, PathBuf)]) -> AgentResult<()> {
        let queues = self
            .inner
            .queues_store
            .list()
            .await
            .map_err(AgentError::from)?;
        let path_map: HashMap<ProjectId, PathBuf> = paths.iter().cloned().collect();
        for q in queues {
            if q.entries.is_empty() {
                continue;
            }
            if let Some(pp) = path_map.get(&q.project_id) {
                self.ensure_worker(q.project_id, pp.clone()).await;
            } else {
                warn!(
                    project = %q.project_id,
                    "queued project has no path mapping; worker not resumed"
                );
            }
        }
        Ok(())
    }

    /// Pause a project's worker. In-flight runs complete normally;
    /// no new runs start until [`Self::resume`] is called.
    pub async fn pause(&self, project: ProjectId, reason: impl Into<String>) {
        let reason = reason.into();
        let workers = self.inner.workers.lock().await;
        if let Some(slot) = workers.get(&project) {
            let _ = slot.mailbox.send(WorkerMsg::Pause).await;
        }
        let _ = self
            .inner
            .events
            .send(QueueEvent::Paused { project, reason });
    }

    pub async fn resume(&self, project: ProjectId) {
        let workers = self.inner.workers.lock().await;
        if let Some(slot) = workers.get(&project) {
            let _ = slot.mailbox.send(WorkerMsg::Resume).await;
        }
        let _ = self.inner.events.send(QueueEvent::Resumed { project });
    }

    /// Kill a running agent instance. Idempotent.
    pub async fn kill_run(&self, run_id: RunId) -> bool {
        let handle_opt = self.inner.running.lock().await.get(&run_id).cloned();
        if let Some(rr) = handle_opt {
            rr.handle.kill().await;
            true
        } else {
            false
        }
    }

    async fn ensure_worker(&self, project: ProjectId, project_path: PathBuf) {
        let mut workers = self.inner.workers.lock().await;
        if let Some(slot) = workers.get(&project) {
            let _ = slot.mailbox.send(WorkerMsg::Wake).await;
            return;
        }
        let (tx, rx) = mpsc::channel::<WorkerMsg>(WORKER_CHANNEL_SIZE);
        workers.insert(
            project,
            WorkerSlot {
                mailbox: tx.clone(),
            },
        );
        drop(workers);

        let engine = self.clone();
        tokio::spawn(async move {
            engine.worker_loop(project, project_path, rx).await;
        });
        // First wake so the worker picks up anything persisted before
        // it started.
        let workers = self.inner.workers.lock().await;
        if let Some(slot) = workers.get(&project) {
            let _ = slot.mailbox.send(WorkerMsg::Wake).await;
        }
    }

    async fn worker_loop(
        &self,
        project: ProjectId,
        project_path: PathBuf,
        mut rx: mpsc::Receiver<WorkerMsg>,
    ) {
        info!(project = %project, "queue worker started");
        let mut paused = false;
        loop {
            // Drain the mailbox; Wake is the common case.
            let Some(msg) = rx.recv().await else {
                debug!(project = %project, "mailbox closed; worker exiting");
                break;
            };
            match msg {
                WorkerMsg::Pause => {
                    paused = true;
                    continue;
                }
                WorkerMsg::Resume => {
                    paused = false;
                }
                WorkerMsg::Wake => {}
            }
            if paused {
                continue;
            }

            // Drain as many entries as we can before going back to sleep.
            loop {
                // Load current queue snapshot.
                let queue = match self.inner.queues_store.load(project).await {
                    Ok(Some(q)) => q,
                    Ok(None) => break,
                    Err(e) => {
                        warn!(project = %project, "queue load failed: {e}");
                        break;
                    }
                };
                if queue.paused || queue.entries.is_empty() {
                    let _ = self.inner.events.send(QueueEvent::Drained { project });
                    break;
                }
                let entry = queue.entries[0].clone();

                // Step: run the head entry. This may take a long time.
                let outcome = self.run_entry(project, &project_path, &entry).await;

                // If we aborted before the task ever transitioned to
                // Running/Finished (e.g. prompt file missing), make
                // sure the user doesn't get stuck with a forever-Queued
                // task after we pop the queue head.
                if matches!(outcome, RunOutcome::Aborted { .. })
                    && let Ok(Some(mut t)) =
                        self.inner.tasks_store.load(project, entry.task_id).await
                    && t.status == TaskStatus::Queued
                {
                    t.status = TaskStatus::Interrupted;
                    let _ = self.inner.tasks_store.save(&t).await;
                }

                // Pop head BEFORE broadcasting the terminal event so
                // that subscribers observing `Finished` can immediately
                // rely on the persisted queue being accurate.
                if let Err(e) = self.pop_head_and_save(project).await {
                    warn!(project = %project, "pop-head save failed: {e}");
                }

                // Broadcast terminal event.
                match &outcome {
                    RunOutcome::Completed { run, status } => {
                        let _ = self.inner.events.send(QueueEvent::Finished {
                            project,
                            task: entry.task_id,
                            run: *run,
                            status: *status,
                        });
                    }
                    RunOutcome::Aborted { reason } => {
                        let _ = self.inner.events.send(QueueEvent::Interrupted {
                            project,
                            task: entry.task_id,
                            reason: reason.clone(),
                        });
                    }
                }

                // Fail-fast: any non-success triggers the rest-interrupt.
                let should_fail_fast = match &outcome {
                    RunOutcome::Completed { status, .. } => *status != TaskStatus::Succeeded,
                    RunOutcome::Aborted { .. } => true,
                };
                if should_fail_fast && self.inner.cfg.fail_fast {
                    let reason = match outcome {
                        RunOutcome::Completed { status, .. } => format!("task ended in {status:?}"),
                        RunOutcome::Aborted { reason } => reason,
                    };
                    self.interrupt_rest(project, &reason).await;
                    break;
                }
            }
        }
        // Remove self from workers map so a future enqueue respawns.
        self.inner.workers.lock().await.remove(&project);
    }

    async fn pop_head_and_save(&self, project: ProjectId) -> AgentResult<()> {
        let mut queue = self
            .inner
            .queues_store
            .load(project)
            .await
            .map_err(AgentError::from)?
            .unwrap_or_else(|| Queue::empty(project));
        let _ = queue.pop_front();
        self.inner
            .queues_store
            .save(&queue)
            .await
            .map_err(AgentError::from)
    }

    async fn interrupt_rest(&self, project: ProjectId, reason: &str) {
        let Ok(Some(queue)) = self.inner.queues_store.load(project).await else {
            return;
        };
        for entry in &queue.entries {
            // Mark each remaining task as interrupted on disk.
            if let Ok(Some(mut t)) = self.inner.tasks_store.load(project, entry.task_id).await {
                t.status = TaskStatus::Interrupted;
                let _ = self.inner.tasks_store.save(&t).await;
            }
            let _ = self.inner.events.send(QueueEvent::Interrupted {
                project,
                task: entry.task_id,
                reason: reason.to_string(),
            });
        }
        let mut q = queue;
        q.entries.clear();
        q.paused = true;
        q.paused_reason = format!("fail-fast: {reason}");
        let _ = self.inner.queues_store.save(&q).await;
    }

    /// Run a single queue entry end-to-end.
    async fn run_entry(
        &self,
        project: ProjectId,
        project_path: &std::path::Path,
        entry: &QueueEntry,
    ) -> RunOutcome {
        // Hydrate task + agent.
        let task = match self.inner.tasks_store.load(project, entry.task_id).await {
            Ok(Some(t)) => t,
            Ok(None) => return RunOutcome::aborted(format!("task {} missing", entry.task_id)),
            Err(e) => return RunOutcome::aborted(format!("task load failed: {e}")),
        };
        let available = match self.inner.registry.get(&entry.agent_id) {
            Some(a) if a.installed => a,
            Some(_) => {
                return RunOutcome::aborted(format!("agent {} not installed", entry.agent_id));
            }
            None => return RunOutcome::aborted(format!("unknown agent {}", entry.agent_id)),
        };

        // Load the prompt the UI rendered when the task was created.
        let prompt_path = self
            .inner
            .paths
            .task_prompt(&project.to_string(), &task.id.to_string());
        let prompt = match tokio::fs::read_to_string(&prompt_path).await {
            Ok(s) => s,
            Err(e) => {
                return RunOutcome::aborted(format!("read prompt {}: {e}", prompt_path.display()));
            }
        };

        // Mark task Running + create Run.
        let mut running_task = task.clone();
        running_task.status = TaskStatus::Running;
        if let Err(e) = self.inner.tasks_store.save(&running_task).await {
            return RunOutcome::aborted(format!("task save failed: {e}"));
        }
        let now = Timestamp::now();
        let mut run = Run::new(project, task.id, entry.agent_id.clone(), now);
        run.started_at = Some(now);
        run.status = TaskStatus::Running;
        run.agent_version = available.version.clone().unwrap_or_default();
        if let Err(e) = self.inner.runs_store.save(&run).await {
            return RunOutcome::aborted(format!("run save failed: {e}"));
        }

        // Acquire global permit (this is where max_concurrent bites).
        let permit = match self.inner.global_sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(e) => return RunOutcome::aborted(format!("semaphore closed: {e}")),
        };

        // Spawn.
        let manifest: AgentManifest = available.manifest.clone();
        let spawn_spec = SpawnSpec {
            run_id: run.id,
            manifest,
            project_root: project_path.to_path_buf(),
            prompt,
            stdout_path: self
                .inner
                .paths
                .run_stdout(&project.to_string(), &run.id.to_string()),
            stderr_path: self
                .inner
                .paths
                .run_stderr(&project.to_string(), &run.id.to_string()),
            prompt_file_path: self
                .inner
                .paths
                .run_dir(&project.to_string(), &run.id.to_string())
                .join("prompt.txt"),
            extra_env: Vec::new(),
        };
        let handle = match self.inner.runner.spawn(spawn_spec).await {
            Ok(h) => h,
            Err(e) => {
                drop(permit);
                self.finalize_run(
                    &mut run,
                    TaskStatus::Failed,
                    None,
                    Some(format!("spawn: {e}")),
                )
                .await;
                // Mark task failed too.
                let mut final_task = task;
                final_task.status = TaskStatus::Failed;
                let _ = self.inner.tasks_store.save(&final_task).await;
                return RunOutcome::Completed {
                    run: run.id,
                    status: TaskStatus::Failed,
                };
            }
        };

        // Record run.pid for the store.
        run.pid = handle.pid();
        let _ = self.inner.runs_store.save(&run).await;

        // Track in runtime table.
        let running_entry = RunningRun {
            run_id: run.id,
            project_id: project,
            task_id: task.id,
            agent_id: entry.agent_id.clone(),
            pid: handle.pid(),
            handle: handle.clone(),
        };
        self.inner
            .running
            .lock()
            .await
            .insert(run.id, running_entry);

        let _ = self.inner.events.send(QueueEvent::Started {
            project,
            task: task.id,
            run: run.id,
        });

        // Wait for exit.
        let report: ExitReport = handle.wait().await;

        // Release permit before disk work — this is the moment another
        // project's worker can proceed.
        drop(permit);

        // Drop running-entry.
        self.inner.running.lock().await.remove(&run.id);

        // Persist Run + Task final state + RunExit.
        let final_status = report.status;
        self.finalize_run(
            &mut run,
            final_status,
            report.exit_code,
            report.signal.clone(),
        )
        .await;

        // Task final status mirrors the run.
        let mut final_task = task;
        final_task.status = final_status;
        let _ = self.inner.tasks_store.save(&final_task).await;

        RunOutcome::Completed {
            run: run.id,
            status: final_status,
        }
    }

    async fn finalize_run(
        &self,
        run: &mut Run,
        status: TaskStatus,
        exit_code: Option<i32>,
        signal: Option<String>,
    ) {
        let now = Timestamp::now();
        run.status = status;
        run.finished_at = Some(now);
        run.log = RunLogInfo {
            stdout_bytes: file_size(
                &self
                    .inner
                    .paths
                    .run_stdout(&run.project_id.to_string(), &run.id.to_string()),
            ),
            stderr_bytes: file_size(
                &self
                    .inner
                    .paths
                    .run_stderr(&run.project_id.to_string(), &run.id.to_string()),
            ),
            truncated: false,
        };
        let _ = self.inner.runs_store.save(run).await;
        let exit = RunExit {
            exit_code,
            signal,
            finished_at: Some(now),
            duration_ms: None,
        };
        let _ = self
            .inner
            .runs_store
            .save_exit(run.project_id, run.id, &exit)
            .await;
    }
}

/// Outcome of `run_entry`. Distinguishes "the agent actually ran and
/// we have a terminal status for a real `RunId`" from "we aborted
/// before the agent could even produce an exit" (e.g., missing prompt
/// file). The former produces a `Finished` event; the latter an
/// `Interrupted` one.
#[derive(Debug)]
enum RunOutcome {
    Completed { run: RunId, status: TaskStatus },
    Aborted { reason: String },
}

impl RunOutcome {
    fn aborted(reason: String) -> Self {
        Self::Aborted { reason }
    }
}

fn file_size(p: &std::path::Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

// ---- Re-exports to spare downstream crates the ceremony -------------

// Convert store errors into agent errors transparently so `?` works.
impl From<lattice_store::error::StoreError> for AgentError {
    fn from(e: lattice_store::error::StoreError) -> Self {
        AgentError::Invocation(format!("store: {e}"))
    }
}
