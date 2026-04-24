//! Filesystem watcher: translates raw `notify` events into
//! [`StoreEvent`]s and fan-outs via a `tokio::sync::broadcast`.
//!
//! We watch the entire state root recursively and classify each path
//! against the known layout (see [`crate::paths`]). Unknown paths are
//! dropped. Writes through [`crate::fs::atomic_write_bytes`] go
//! `tmp → rename → target`, so we only emit an event once the *final*
//! target path is observed (create/modify of the entity TOML).
//!
//! A tiny debouncer coalesces duplicate events for the same logical
//! entity within `DEBOUNCE_WINDOW`. This smooths over notify's habit
//! of emitting multiple events per modification (kernel + VFS layers).

use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::event::EventKind;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use lattice_core::ids::{ProjectId, RunId, TaskId, TemplateId};

use crate::error::{StoreError, StoreResult};
use crate::paths::Paths;
use crate::store::StoreEvent;

const BROADCAST_CAPACITY: usize = 256;
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(100);

/// Parses an id string into an entity id newtype.
fn parse_id<T: std::str::FromStr>(s: &std::ffi::OsStr) -> Option<T> {
    s.to_str().and_then(|s| s.parse().ok())
}

/// Classifies a raw path into a [`StoreEvent`] if it belongs to a known
/// entity. `removed` picks the `*Removed` variant when the path has
/// disappeared from disk.
fn classify(paths: &Paths, path: &Path, removed: bool) -> Option<StoreEvent> {
    // `notify` always gives us absolute, OS-canonical paths. Strip the
    // state root so we can match on the logical layout; fall back to
    // the raw path if stripping fails.
    let rel = path.strip_prefix(paths.state_root()).unwrap_or(path);
    let components: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| {
            if let Component::Normal(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    match components.as_slice() {
        // projects/<id>/project.toml
        [top, id, leaf] if *top == "projects" && *leaf == "project.toml" => {
            let id: ProjectId = parse_id(id)?;
            Some(if removed {
                StoreEvent::ProjectRemoved(id)
            } else {
                StoreEvent::ProjectChanged(id)
            })
        }
        // Project dir deleted wholesale (no leaf).
        [top, id] if *top == "projects" && removed => {
            let id: ProjectId = parse_id(id)?;
            Some(StoreEvent::ProjectRemoved(id))
        }

        // templates/<id>/template.toml
        [top, id, leaf] if *top == "templates" && *leaf == "template.toml" => {
            let id: TemplateId = parse_id(id)?;
            Some(if removed {
                StoreEvent::TemplateRemoved(id)
            } else {
                StoreEvent::TemplateChanged(id)
            })
        }
        [top, id] if *top == "templates" && removed => {
            let id: TemplateId = parse_id(id)?;
            Some(StoreEvent::TemplateRemoved(id))
        }

        // tasks/<project>/<task>/task.toml
        [top, project, task, leaf] if *top == "tasks" && *leaf == "task.toml" => {
            let project: ProjectId = parse_id(project)?;
            let task: TaskId = parse_id(task)?;
            Some(if removed {
                StoreEvent::TaskRemoved { project, task }
            } else {
                StoreEvent::TaskChanged { project, task }
            })
        }
        [top, project, task] if *top == "tasks" && removed => {
            let project: ProjectId = parse_id(project)?;
            let task: TaskId = parse_id(task)?;
            Some(StoreEvent::TaskRemoved { project, task })
        }

        // runs/<project>/<run>/run.toml
        [top, project, run, leaf] if *top == "runs" && *leaf == "run.toml" => {
            let project: ProjectId = parse_id(project)?;
            let run: RunId = parse_id(run)?;
            Some(if removed {
                StoreEvent::RunRemoved { project, run }
            } else {
                StoreEvent::RunChanged { project, run }
            })
        }
        [top, project, run] if *top == "runs" && removed => {
            let project: ProjectId = parse_id(project)?;
            let run: RunId = parse_id(run)?;
            Some(StoreEvent::RunRemoved { project, run })
        }

        // queues/<project>.toml
        [top, leaf] if *top == "queues" => {
            let stem = Path::new(leaf).file_stem().and_then(|s| s.to_str())?;
            let project: ProjectId = stem.parse().ok()?;
            Some(StoreEvent::QueueChanged(project))
        }

        _ => None,
    }
}

/// Classify a path from the config root (settings + agent manifests).
fn classify_config(paths: &Paths, path: &Path) -> Option<StoreEvent> {
    let rel = path.strip_prefix(paths.config_root()).unwrap_or(path);
    if rel == Path::new("settings.toml") {
        return Some(StoreEvent::SettingsChanged);
    }
    None
}

/// Debouncer: tracks the last time we emitted each key and suppresses
/// duplicates inside the configured window.
#[derive(Debug, Default)]
struct Debouncer {
    last: Mutex<HashMap<StoreEvent, Instant>>,
}

impl Debouncer {
    fn allow(&self, evt: &StoreEvent) -> bool {
        let mut map = self.last.lock();
        let now = Instant::now();
        let fresh = match map.get(evt) {
            Some(prev) => now.duration_since(*prev) >= DEBOUNCE_WINDOW,
            None => true,
        };
        if fresh {
            map.insert(evt.clone(), now);
        }
        fresh
    }
}

/// The public handle. Cheap to clone — all subscribers share the same
/// broadcast channel. Dropping every handle does **not** stop the
/// watcher; call [`Self::shutdown`] explicitly.
#[derive(Clone, Debug)]
pub struct FsWatcher {
    sender: broadcast::Sender<StoreEvent>,
    shutdown: Arc<Mutex<Option<ShutdownHandle>>>,
}

#[derive(Debug)]
struct ShutdownHandle {
    _watcher: RecommendedWatcher,
    stop_tx: mpsc::Sender<()>,
}

impl FsWatcher {
    /// Start watching `paths.state_root()` and `paths.config_root()`.
    pub fn start(paths: &Paths) -> StoreResult<Self> {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        let debouncer = Arc::new(Debouncer::default());

        // Bridge notify's sync callback → a std mpsc channel, then a
        // dedicated thread forwards into the tokio broadcast.
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res| {
            // This runs on notify's thread; a send failure means the
            // forwarding thread has already shut down.
            let _ = raw_tx.send(res);
        })
        .map_err(|e| StoreError::io(paths.state_root(), std::io::Error::other(e.to_string())))?;

        // Pre-create every known entity directory so notify's
        // recursive inotify watch arms them before the first write.
        // Otherwise, on Linux, a fast `mkdir X && write X/f` sequence
        // can fire `Create(X)` without a subsequent event for `X/f`:
        // the dir is created after the watch is attached, but the file
        // write inside lands before notify re-arms on the new subdir.
        for dir in [
            paths.state_root().to_path_buf(),
            paths.config_root().to_path_buf(),
            paths.projects_dir(),
            paths.templates_dir(),
            paths.tasks_root(),
            paths.runs_root(),
            paths.queues_dir(),
            paths.agents_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|e| StoreError::io(&dir, e))?;
        }

        watcher
            .watch(paths.state_root(), RecursiveMode::Recursive)
            .map_err(|e| {
                StoreError::io(paths.state_root(), std::io::Error::other(e.to_string()))
            })?;
        watcher
            .watch(paths.config_root(), RecursiveMode::Recursive)
            .map_err(|e| {
                StoreError::io(paths.config_root(), std::io::Error::other(e.to_string()))
            })?;

        let tx = sender.clone();
        let bg_paths = paths.clone();
        std::thread::spawn(move || {
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                match raw_rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(Ok(event)) => handle_event(&bg_paths, &debouncer, &tx, &event),
                    Ok(Err(e)) => warn!(error = %e, "notify error"),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            debug!("fs watcher loop exited");
        });

        Ok(Self {
            sender,
            shutdown: Arc::new(Mutex::new(Some(ShutdownHandle {
                _watcher: watcher,
                stop_tx,
            }))),
        })
    }

    /// Subscribe to the event stream. Late subscribers miss events
    /// emitted before their `subscribe()` call.
    pub fn subscribe(&self) -> broadcast::Receiver<StoreEvent> {
        self.sender.subscribe()
    }

    /// Stop the watcher. Subsequent events are dropped. Idempotent.
    pub fn shutdown(&self) {
        if let Some(handle) = self.shutdown.lock().take() {
            let _ = handle.stop_tx.send(());
            // RecommendedWatcher drops here, which also unregisters.
        }
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        // Only the last handle drop actually shuts down the thread,
        // because the shutdown handle is behind an `Arc<Mutex<Option<_>>>`.
        if Arc::strong_count(&self.shutdown) == 1 {
            self.shutdown();
        }
    }
}

fn handle_event(
    paths: &Paths,
    debouncer: &Debouncer,
    tx: &broadcast::Sender<StoreEvent>,
    event: &notify::Event,
) {
    let removed = matches!(event.kind, EventKind::Remove(_));
    // Accept any Create or any Modify. inotify emits subtly different
    // kinds for our atomic-write flow (tmp create, rename → Modify(Name)
    // on the target, data flush → Modify(Data)); broadening to "any
    // Modify" is simpler and safer than enumerating the matrix.
    let created_or_modified = matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_));
    if !removed && !created_or_modified {
        return;
    }

    // When a new directory is created, inotify's per-dir watch is armed
    // *after* the Create event reaches us, meaning any immediate writes
    // inside (the common case for our `create_dir_all + atomic_write`
    // sequence) are missed. Counter this by scanning the new directory
    // ourselves and synthesizing events for anything already present.
    if matches!(
        event.kind,
        EventKind::Create(notify::event::CreateKind::Folder)
    ) {
        for dir in &event.paths {
            synthesize_from_scan(paths, debouncer, tx, dir);
        }
    }
    for raw_path in &event.paths {
        // Ignore our own tmp-write sibling files.
        if raw_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".tmp."))
        {
            continue;
        }

        let classified = classify(paths, raw_path, removed).or_else(|| {
            if removed {
                None
            } else {
                classify_config(paths, raw_path)
            }
        });

        let Some(evt) = classified else {
            continue;
        };

        if !debouncer.allow(&evt) {
            continue;
        }
        // `send` returns Err only if there are no active receivers; that
        // is not an error from the watcher's perspective.
        let _ = tx.send(evt);
    }
}

/// Walk a newly-created directory and emit classified events for any
/// files or subdirs already inside. Bounded to two levels of recursion
/// so a race where the whole `tasks/<proj>/<task>/` tree lands at once
/// still fires the right events.
fn synthesize_from_scan(
    paths: &Paths,
    debouncer: &Debouncer,
    tx: &broadcast::Sender<StoreEvent>,
    dir: &Path,
) {
    fn walk(
        paths: &Paths,
        debouncer: &Debouncer,
        tx: &broadcast::Sender<StoreEvent>,
        dir: &Path,
        depth: u32,
    ) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                walk(paths, debouncer, tx, &path, depth - 1);
                continue;
            }
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains(".tmp."))
            {
                continue;
            }
            let Some(evt) = classify(paths, &path, false).or_else(|| classify_config(paths, &path))
            else {
                continue;
            };
            if !debouncer.allow(&evt) {
                continue;
            }
            let _ = tx.send(evt);
        }
    }
    walk(paths, debouncer, tx, dir, 3);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::entities::{Project, Template};
    use lattice_core::time::Timestamp;
    use tempfile::TempDir;
    use tokio::time::{Duration as TokioDuration, timeout};

    use crate::{FileStore, Projects, SettingsStore, Templates};

    fn now() -> Timestamp {
        Timestamp::parse("2026-04-24T10:00:00Z").unwrap()
    }

    async fn recv_until<F>(
        rx: &mut broadcast::Receiver<StoreEvent>,
        mut predicate: F,
    ) -> Option<StoreEvent>
    where
        F: FnMut(&StoreEvent) -> bool,
    {
        let fut = async {
            loop {
                match rx.recv().await {
                    Ok(evt) if predicate(&evt) => return Some(evt),
                    Ok(_) => {}
                    Err(_) => return None,
                }
            }
        };
        timeout(TokioDuration::from_secs(5), fut)
            .await
            .ok()
            .flatten()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn project_save_emits_changed_event() {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());
        let watcher = FsWatcher::start(&paths).unwrap();
        let mut rx = watcher.subscribe();

        let store = FileStore::new(paths);
        let p = Project::new("acme", "/tmp/acme", now());
        Projects::save(&store, &p).await.unwrap();

        let expected = StoreEvent::ProjectChanged(p.id);
        let got = recv_until(&mut rx, |e| *e == expected).await;
        assert_eq!(got, Some(expected));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn template_save_emits_changed_event() {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());
        let watcher = FsWatcher::start(&paths).unwrap();
        let mut rx = watcher.subscribe();

        let store = FileStore::new(paths);
        let t = Template::new("bug", now());
        Templates::save(&store, &t).await.unwrap();

        let expected = StoreEvent::TemplateChanged(t.id);
        let got = recv_until(&mut rx, |e| *e == expected).await;
        assert_eq!(got, Some(expected));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn project_delete_emits_removed_event() {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());
        let watcher = FsWatcher::start(&paths).unwrap();
        let mut rx = watcher.subscribe();

        let store = FileStore::new(paths);
        let p = Project::new("acme", "/tmp/acme", now());
        Projects::save(&store, &p).await.unwrap();
        // Drain the `Changed` event first.
        let _ = recv_until(&mut rx, |e| matches!(e, StoreEvent::ProjectChanged(_))).await;

        Projects::delete(&store, p.id).await.unwrap();
        let got = recv_until(
            &mut rx,
            |e| matches!(e, StoreEvent::ProjectRemoved(id) if *id == p.id),
        )
        .await;
        assert!(
            got.is_some(),
            "expected a ProjectRemoved({}), got none in time",
            p.id
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn settings_save_emits_settings_changed() {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());
        let watcher = FsWatcher::start(&paths).unwrap();
        let mut rx = watcher.subscribe();

        let store = FileStore::new(paths);
        let s = lattice_core::entities::Settings::default();
        SettingsStore::save(&store, &s).await.unwrap();

        let got = recv_until(&mut rx, |e| *e == StoreEvent::SettingsChanged).await;
        assert_eq!(got, Some(StoreEvent::SettingsChanged));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_paths_are_ignored() {
        let cfg = TempDir::new().unwrap();
        let state = TempDir::new().unwrap();
        let paths = Paths::with_roots(cfg.path(), state.path());
        let watcher = FsWatcher::start(&paths).unwrap();
        let mut rx = watcher.subscribe();

        // Write an unrelated file under state root.
        std::fs::create_dir_all(state.path().join("custom")).unwrap();
        std::fs::write(state.path().join("custom/random.txt"), b"hi").unwrap();

        let got = timeout(TokioDuration::from_millis(500), rx.recv())
            .await
            .ok()
            .and_then(Result::ok);
        assert!(
            got.is_none(),
            "expected no event for unknown path, got {got:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn debouncer_suppresses_duplicates() {
        let d = Debouncer::default();
        let evt = StoreEvent::SettingsChanged;
        assert!(d.allow(&evt));
        assert!(!d.allow(&evt));
        std::thread::sleep(DEBOUNCE_WINDOW + Duration::from_millis(10));
        assert!(d.allow(&evt));
    }
}
