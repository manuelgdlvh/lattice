//! Headless smoke test for dispatching via the bundled `cursor-agent`.
//!
//! This is intended for developers to validate that:
//! - the bundled manifest is registered and detected as installed
//! - queue workers start and transition a task out of Queued
//! - a run reaches Started (and can be killed)
//!
//! It uses temp XDG roots so it never touches the user's real lattice state.

#![deny(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use lattice_agents::{EnqueueRequest, QueueConfig, QueueEngine, QueueEvent};
use lattice_core::entities::{Project, Task, Template};
use lattice_core::ids::AgentId;
use lattice_core::time::Timestamp;
use lattice_store::filestore::FileStore;
use lattice_store::paths::Paths;
use lattice_store::store::{Projects, Tasks, Templates};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // Keep it simple and human readable.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let tmp = tempfile::tempdir().context("tempdir")?;
    let config_root = tmp.path().join("config");
    let state_root = tmp.path().join("state");
    std::fs::create_dir_all(&config_root).ok();
    std::fs::create_dir_all(&state_root).ok();

    let paths = Paths::with_roots(config_root, state_root);
    let store: Arc<FileStore> = Arc::new(FileStore::new(paths.clone()));

    // Real project root dir for the agent to run in.
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).ok();

    let now = Timestamp::now();
    let proj = Project::new("smoke", project_root.clone(), now);
    <FileStore as Projects>::save(&*store, &proj)
        .await
        .context("save project")?;

    let tpl = Template::new("smoke-template", now);
    <FileStore as Templates>::save(&*store, &tpl)
        .await
        .context("save template")?;

    let mut task = Task::new(proj.id, tpl.id, tpl.version, "smoke-task", now);
    task.status = lattice_core::entities::TaskStatus::Draft;
    <FileStore as Tasks>::save(&*store, &task)
        .await
        .context("save task")?;

    // Write a prompt file exactly where the queue engine expects it.
    let prompt_path = paths.task_prompt(&proj.id.to_string(), &task.id.to_string());
    tokio::fs::create_dir_all(prompt_path.parent().unwrap())
        .await
        .ok();
    tokio::fs::write(
        &prompt_path,
        "Say 'started' and then wait. This is a smoke test.\n",
    )
    .await
    .context("write task prompt")?;

    // Registry: loads bundled cursor-agent + optional user overrides (none here).
    let agents_dir = paths.agents_dir();
    std::fs::create_dir_all(&agents_dir).ok();
    let registry = Arc::new(lattice_agents::AgentRegistry::from_config_dir(&agents_dir)?);

    let agent_id = AgentId::new("cursor-agent");
    let Some(a) = registry.get(&agent_id) else {
        return Err(anyhow!("cursor-agent not registered"));
    };
    if !a.installed {
        return Err(anyhow!(
            "cursor-agent is registered but not detected as installed (is it on PATH?)"
        ));
    }

    let engine = QueueEngine::new(
        store.clone(),
        store.clone(),
        store.clone(),
        registry.clone(),
        paths.clone(),
        QueueConfig {
            max_concurrent: 1,
            fail_fast: false,
        },
    );

    let mut rx = engine.subscribe();
    engine
        .enqueue(EnqueueRequest {
            project_id: proj.id,
            project_path: project_root,
            task_id: task.id,
            agent_id: agent_id.clone(),
        })
        .await
        .context("enqueue")?;

    // Wait for Started; then kill and wait for Finished/Interrupted.
    let started = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(QueueEvent::Started { run, task: tid, .. }) if tid == task.id => break run,
                Ok(_) => {}
                Err(_) => {}
            }
        }
    })
    .await
    .context("wait for Started")?;

    let ok = engine.kill_run(started).await;
    if !ok {
        return Err(anyhow!("kill_run returned false for run {started}"));
    }

    let _terminal = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(QueueEvent::Finished { run, .. }) if run == started => break (),
                Ok(QueueEvent::Interrupted { .. }) => break (),
                Ok(_) => {}
                Err(_) => {}
            }
        }
    })
    .await
    .context("wait for terminal event")?;

    eprintln!(
        "OK: cursor-agent started and was killable (run={})",
        started
    );
    Ok(())
}
