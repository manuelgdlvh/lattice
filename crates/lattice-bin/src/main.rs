//! `lattice` — task-first schema-driven AI dev orchestrator.
//!
//! This binary wires the store and TUI into a running app.

#![deny(unsafe_code)]

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use lattice_store::filestore::FileStore;
use lattice_store::paths::Paths;
use lattice_store::store::{SettingsStore, Templates};
use lattice_tui::{App, AppContext};

mod default_templates;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_tracing().context("init tracing")?;

    // Resolve where we keep state.
    let paths = Paths::from_env_or_xdg().context("resolve XDG paths")?;
    std::fs::create_dir_all(paths.config_root()).ok();
    std::fs::create_dir_all(paths.state_root()).ok();

    // Build the file-backed store (no SQLite — flat TOML + atomic writes).
    let store: Arc<FileStore> = Arc::new(FileStore::new(paths.clone()));

    // Seed built-in templates on first run (templates dir is empty).
    match <FileStore as Templates>::list(&store).await {
        Ok(existing) if existing.is_empty() => {
            let now = lattice_core::time::Timestamp::now();
            for t in default_templates::default_templates(now) {
                if let Err(e) = <FileStore as Templates>::save(&*store, &t).await {
                    tracing::warn!("seed default template failed: {e}");
                }
            }
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("templates.list on boot: {e}"),
    }

    // Load settings — the defaults kick in when `settings.toml` is
    // missing on first run.
    let _settings = SettingsStore::load(&*store).await.unwrap_or_default();

    let ctx = AppContext {
        templates: store.clone(),
        tasks: store.clone(),
        settings: store.clone(),
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
