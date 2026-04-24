//! Integration tests for `QueueEngine`.
//!
//! Wiring pattern (repeated across tests):
//!
//! 1. Build a `FileStore` on a temp `Paths`.
//! 2. Build an `AgentRegistry` with a bundled-like manifest that
//!    points at the `lattice-fake-agent` binary so every test runs
//!    real subprocesses but stays hermetic.
//! 3. Build a `QueueEngine`, enqueue, and assert via persisted state +
//!    the event channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::time::timeout;

use lattice_agents::manifest::{InvocationMode, InvocationSpec, RuntimeSpec, WorkingDir};
use lattice_agents::{
    AgentManifest, AgentRegistry, EnqueueRequest, QueueConfig, QueueEngine, QueueEvent,
};
use lattice_core::entities::{Project, Task, TaskStatus};
use lattice_core::ids::{AgentId, TaskId, TemplateId};
use lattice_core::time::Timestamp;
use lattice_store::filestore::FileStore;
use lattice_store::paths::Paths;
use lattice_store::store::{Projects, Queues, Runs, Tasks};

fn fake_agent_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lattice-fake-agent"))
}

/// Registry containing a single "fake" manifest that points at the
/// pre-built fake-agent binary. `env` drives behavior per task via
/// `extra_env` at enqueue time... but the queue engine doesn't accept
/// per-enqueue env overrides (by design: the agent/manifest is the
/// contract). So for the handful of tests that need to steer the
/// fake agent, we tune the manifest's own `env` table instead.
fn build_registry(env: Vec<(&str, &str)>) -> Arc<AgentRegistry> {
    let mut env_map = std::collections::BTreeMap::new();
    for (k, v) in env {
        env_map.insert(k.to_string(), v.to_string());
    }
    let manifest = AgentManifest {
        id: AgentId::new("fake"),
        display_name: "Fake".into(),
        binary: fake_agent_path().to_string_lossy().into_owned(),
        detect: lattice_agents::manifest::DetectSpec::default(),
        invocation: InvocationSpec {
            mode: InvocationMode::Stdin,
            args: vec![],
        },
        runtime: RuntimeSpec {
            working_dir: WorkingDir::Project,
            kill_grace_ms: 1_000,
        },
        env: env_map,
    };
    // `AgentRegistry` has no public "install raw manifest" method for
    // tests. But we can load from a temp config dir.
    let dir = TempDir::new().unwrap();
    let manifest_path = dir.path().join("fake.toml");
    std::fs::write(&manifest_path, toml::to_string(&manifest).unwrap()).unwrap();
    // The absolute `binary` path makes `which::which` a no-op, so the
    // registry will mark the agent `installed = true` as long as the
    // binary file actually exists (which it does — cargo built it).
    let reg = AgentRegistry::from_config_dir(dir.path()).unwrap();
    Arc::new(reg)
}

struct Harness {
    // Kept alive for the duration of the test; also exposes `path()`
    // so tests can create sibling files (e.g., a second project).
    tmp: TempDir,
    store: Arc<FileStore>,
    engine: QueueEngine,
    project: Project,
    project_dir: PathBuf,
}

async fn new_harness(cfg: QueueConfig, env: Vec<(&str, &str)>) -> Harness {
    let tmp = TempDir::new().unwrap();
    let config_root = tmp.path().join("config");
    let state_root = tmp.path().join("state");
    std::fs::create_dir_all(&config_root).unwrap();
    std::fs::create_dir_all(&state_root).unwrap();
    let paths = Paths::with_roots(&config_root, &state_root);
    let store = Arc::new(FileStore::new(paths.clone()));

    // Persist a project whose `path` is a real directory.
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project = Project::new("test-proj", &project_dir, Timestamp::now());
    Projects::save(&*store, &project).await.unwrap();

    let registry = build_registry(env);
    let engine = QueueEngine::new(
        store.clone(),
        store.clone(),
        store.clone(),
        registry,
        paths,
        cfg,
    );
    Harness {
        tmp,
        store,
        engine,
        project,
        project_dir,
    }
}

/// Persist a task with a pre-rendered prompt file.
async fn add_task(h: &Harness, name: &str, prompt_body: &str) -> TaskId {
    let now = Timestamp::now();
    let task = Task::new(h.project.id, TemplateId::new(), 1, name, now);
    Tasks::save(&*h.store, &task).await.unwrap();

    // Task prompt must exist on disk for the queue to pick it up.
    let prompt_path = h
        .store
        .paths()
        .task_prompt(&h.project.id.to_string(), &task.id.to_string());
    if let Some(parent) = prompt_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&prompt_path, prompt_body).unwrap();
    task.id
}

/// Drain the subscriber until we see a terminal event for `task_id`.
/// Using a caller-owned `rx` avoids the "late subscribe misses the
/// event" trap.
async fn wait_for_finished(
    rx: &mut tokio::sync::broadcast::Receiver<QueueEvent>,
    task_id: TaskId,
    budget: Duration,
) -> Option<TaskStatus> {
    timeout(budget, async move {
        loop {
            match rx.recv().await {
                Ok(QueueEvent::Finished { task, status, .. }) if task == task_id => {
                    return Some(status);
                }
                Ok(QueueEvent::Interrupted { task, .. }) if task == task_id => {
                    return Some(TaskStatus::Interrupted);
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return None,
            }
        }
    })
    .await
    .ok()
    .flatten()
}

#[tokio::test(flavor = "multi_thread")]
async fn fifo_order_preserved_within_project() {
    // Fake agent reads stdin and emits a sentinel line. By inspecting
    // stdout logs in timestamp order we can prove order was preserved.
    let h = new_harness(
        QueueConfig::default(),
        vec![("LATTICE_FAKE_READ_STDIN", "1")],
    )
    .await;
    let t1 = add_task(&h, "first", "PROMPT-A").await;
    let t2 = add_task(&h, "second", "PROMPT-B").await;
    let t3 = add_task(&h, "third", "PROMPT-C").await;

    // Subscribe BEFORE enqueuing so we don't miss the early events.
    let mut rx = h.engine.subscribe();

    for t in [t1, t2, t3] {
        h.engine
            .enqueue(EnqueueRequest {
                project_id: h.project.id,
                project_path: h.project_dir.clone(),
                task_id: t,
                agent_id: AgentId::new("fake"),
            })
            .await
            .unwrap();
    }

    // Observe Started events in the order they fire.
    let mut started_order = Vec::new();
    let mut finished_by_task = std::collections::HashMap::new();
    let _ = timeout(Duration::from_secs(15), async {
        loop {
            match rx.recv().await {
                Ok(QueueEvent::Started { task, .. }) => started_order.push(task),
                Ok(QueueEvent::Finished { task, status, .. }) => {
                    finished_by_task.insert(task, status);
                }
                Ok(_) => {}
                Err(_) => break,
            }
            if finished_by_task.len() == 3 {
                break;
            }
        }
    })
    .await;
    assert_eq!(
        started_order,
        vec![t1, t2, t3],
        "tasks must start in enqueue order"
    );
    for t in [t1, t2, t3] {
        assert_eq!(
            finished_by_task.get(&t).copied(),
            Some(TaskStatus::Succeeded)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fail_fast_marks_rest_interrupted() {
    // Every task exits 1 (manifest-wide EXIT_CODE=1). The first run
    // fails; fail-fast must interrupt the other two without running them.
    let h = new_harness(
        QueueConfig {
            max_concurrent: 4,
            fail_fast: true,
        },
        vec![
            ("LATTICE_FAKE_READ_STDIN", "1"),
            ("LATTICE_FAKE_EXIT_CODE", "1"),
        ],
    )
    .await;
    let t_first = add_task(&h, "first", "a").await;
    let t_second = add_task(&h, "second", "b").await;
    let t_third = add_task(&h, "third", "c").await;

    let mut rx = h.engine.subscribe();

    for t in [t_first, t_second, t_third] {
        h.engine
            .enqueue(EnqueueRequest {
                project_id: h.project.id,
                project_path: h.project_dir.clone(),
                task_id: t,
                agent_id: AgentId::new("fake"),
            })
            .await
            .unwrap();
    }

    let s_first = wait_for_finished(&mut rx, t_first, Duration::from_secs(10)).await;
    assert_eq!(s_first, Some(TaskStatus::Failed));
    let s_second = wait_for_finished(&mut rx, t_second, Duration::from_secs(5)).await;
    assert_eq!(s_second, Some(TaskStatus::Interrupted));
    let s_third = wait_for_finished(&mut rx, t_third, Duration::from_secs(5)).await;
    assert_eq!(s_third, Some(TaskStatus::Interrupted));
}

#[tokio::test(flavor = "multi_thread")]
async fn global_concurrency_cap_respected() {
    // max_concurrent = 1. Queue two tasks across two projects; only
    // one should be running at a time. We observe via `engine.running()`.
    let h1 = new_harness(
        QueueConfig {
            max_concurrent: 1,
            fail_fast: false,
        },
        vec![
            ("LATTICE_FAKE_READ_STDIN", "1"),
            ("LATTICE_FAKE_SLEEP_MS", "500"),
        ],
    )
    .await;

    // Share the same engine for both projects by creating a second
    // project inside the same store.
    let project_dir_2 = h1.tmp.path().join("project2");
    std::fs::create_dir_all(&project_dir_2).unwrap();
    let project2 = Project::new("p2", &project_dir_2, Timestamp::now());
    Projects::save(&*h1.store, &project2).await.unwrap();

    // Create one task per project.
    let t1 = add_task(&h1, "p1-task", "hi").await;
    let t2 = {
        let now = Timestamp::now();
        let task = Task::new(project2.id, TemplateId::new(), 1, "p2-task", now);
        Tasks::save(&*h1.store, &task).await.unwrap();
        let prompt_path = h1
            .store
            .paths()
            .task_prompt(&project2.id.to_string(), &task.id.to_string());
        std::fs::create_dir_all(prompt_path.parent().unwrap()).unwrap();
        std::fs::write(&prompt_path, "hi2").unwrap();
        task.id
    };

    // Enqueue both as fast as we can.
    h1.engine
        .enqueue(EnqueueRequest {
            project_id: h1.project.id,
            project_path: h1.project_dir.clone(),
            task_id: t1,
            agent_id: AgentId::new("fake"),
        })
        .await
        .unwrap();
    h1.engine
        .enqueue(EnqueueRequest {
            project_id: project2.id,
            project_path: project_dir_2.clone(),
            task_id: t2,
            agent_id: AgentId::new("fake"),
        })
        .await
        .unwrap();

    // Sample running count while tasks progress. With cap=1, we should
    // never observe >1 concurrently.
    let mut rx = h1.engine.subscribe();
    let max_observed = std::sync::Arc::new(tokio::sync::Mutex::new(0usize));
    let engine_clone = h1.engine.clone();
    let max_clone = max_observed.clone();
    let sampler = tokio::spawn(async move {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let n = engine_clone.running().await.len();
            let mut g = max_clone.lock().await;
            if n > *g {
                *g = n;
            }
            drop(g);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let mut finished = 0usize;
    let _ = timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(QueueEvent::Finished { .. }) => {
                    finished += 1;
                    if finished >= 2 {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;

    sampler.abort();
    let peak = *max_observed.lock().await;
    assert!(peak <= 1, "peak concurrent was {peak}");
    assert!(peak >= 1, "we never observed any run starting");

    // Make sure both tasks actually succeeded on disk.
    let r1 = Runs::list_for_project(&*h1.store, h1.project.id)
        .await
        .unwrap();
    let r2 = Runs::list_for_project(&*h1.store, project2.id)
        .await
        .unwrap();
    assert_eq!(r1.len(), 1);
    assert_eq!(r1[0].task_id, t1);
    assert_eq!(r2.len(), 1);
    assert_eq!(r2[0].task_id, t2);
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_run_terminates_and_reports_killed() {
    let h = new_harness(
        QueueConfig {
            max_concurrent: 4,
            fail_fast: false,
        },
        vec![
            ("LATTICE_FAKE_READ_STDIN", "1"),
            ("LATTICE_FAKE_SLEEP_MS", "30000"),
        ],
    )
    .await;
    let t = add_task(&h, "long", "stay alive").await;
    let mut rx = h.engine.subscribe();
    h.engine
        .enqueue(EnqueueRequest {
            project_id: h.project.id,
            project_path: h.project_dir.clone(),
            task_id: t,
            agent_id: AgentId::new("fake"),
        })
        .await
        .unwrap();

    // Wait for Started.
    let run_id = timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(QueueEvent::Started { run, task, .. }) = rx.recv().await
                && task == t
            {
                return run;
            }
        }
    })
    .await
    .expect("task should start");

    // Kill it.
    assert!(h.engine.kill_run(run_id).await);

    let status = wait_for_finished(&mut rx, t, Duration::from_secs(5)).await;
    assert_eq!(status, Some(TaskStatus::Killed));
}

#[tokio::test(flavor = "multi_thread")]
async fn persisted_queue_shrinks_as_tasks_complete() {
    let h = new_harness(
        QueueConfig::default(),
        vec![("LATTICE_FAKE_READ_STDIN", "1")],
    )
    .await;
    let t = add_task(&h, "only", "hi").await;
    let mut rx = h.engine.subscribe();
    h.engine
        .enqueue(EnqueueRequest {
            project_id: h.project.id,
            project_path: h.project_dir.clone(),
            task_id: t,
            agent_id: AgentId::new("fake"),
        })
        .await
        .unwrap();

    // queue has exactly 1 entry immediately after enqueue.
    let q0 = Queues::load(&*h.store, h.project.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q0.entries.len(), 1);

    assert_eq!(
        wait_for_finished(&mut rx, t, Duration::from_secs(10)).await,
        Some(TaskStatus::Succeeded)
    );

    // After completion, queue persisted state is empty.
    let q1 = Queues::load(&*h.store, h.project.id)
        .await
        .unwrap()
        .unwrap();
    assert!(q1.entries.is_empty(), "queue should be drained: {q1:?}");

    // And there's a Run on disk.
    let runs = Runs::list_for_project(&*h.store, h.project.id)
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, TaskStatus::Succeeded);
    // RunExit sibling should be written.
    let exit = Runs::load_exit(&*h.store, h.project.id, runs[0].id)
        .await
        .unwrap();
    assert!(exit.is_some(), "exit.toml should be persisted");
}
