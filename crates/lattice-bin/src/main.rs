//! `lattice` — task-first schema-driven AI dev orchestrator.
//!
//! This binary wires the store, agent registry, queue engine, and TUI
//! into a running app. The heavy lifting lives in the library crates.

#![deny(unsafe_code)]

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use lattice_agents::{AgentRegistry, QueueConfig, QueueEngine};
use lattice_store::filestore::FileStore;
use lattice_store::paths::Paths;
use lattice_store::store::{Projects, SettingsStore};
use lattice_tui::{App, AppContext};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_tracing().context("init tracing")?;

    // Resolve where we keep state.
    let paths = Paths::from_env_or_xdg().context("resolve XDG paths")?;
    std::fs::create_dir_all(paths.config_root()).ok();
    std::fs::create_dir_all(paths.state_root()).ok();

    // Build the file-backed store (no SQLite — flat TOML + atomic writes).
    let store: Arc<FileStore> = Arc::new(FileStore::new(paths.clone()));

    // Load settings — the defaults kick in when `settings.toml` is
    // missing on first run.
    let settings = SettingsStore::load(&*store).await.unwrap_or_default();

    // Discover agents. `from_config_dir` auto-detects on load.
    let agents_dir = paths.agents_dir();
    std::fs::create_dir_all(&agents_dir).ok();
    let registry = Arc::new(AgentRegistry::from_config_dir(&agents_dir)?);

    // Queue engine.
    let cfg = QueueConfig {
        max_concurrent: settings.runtime.max_concurrent_agents,
        fail_fast: settings.runtime.fail_fast,
    };
    let engine = QueueEngine::new(
        store.clone(),
        store.clone(),
        store.clone(),
        registry.clone(),
        paths.clone(),
        cfg,
    );

    // Rehydrate any queues left over from a previous session. Invoke
    // through the trait because `FileStore` also impls other `list()`
    // methods for queues/templates.
    match <FileStore as Projects>::list(&store).await {
        Ok(projects) => {
            let pairs: Vec<_> = projects.iter().map(|p| (p.id, p.path.clone())).collect();
            if let Err(e) = engine.resume_with_paths(&pairs).await {
                tracing::warn!("resume_with_paths: {e}");
            }
        }
        Err(e) => tracing::warn!("projects.list on boot: {e}"),
    }

    let ctx = AppContext {
        projects: store.clone(),
        templates: store.clone(),
        tasks: store.clone(),
        runs: store.clone(),
        queues: store.clone(),
        settings: store.clone(),
        registry,
        engine,
        paths,
    };

    App::new(ctx).run().await.map_err(anyhow::Error::from)?;
    Ok(())
}

fn init_tracing() -> Result<()> {
    // Route logs to stderr by default. Users can crank via
    // `RUST_LOG=lattice=debug`.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
