# ARCHITECTURE

Technical architecture for lattice. Targets v0.1 but calls out seams for
v0.2–v1.0.

---

## 1. Tech stack

| Concern | Choice | Rationale |
|---|---|---|
| TUI | `ratatui` + `crossterm` | De-facto standard; custom-widget friendly for C4 later. |
| Async runtime | `tokio` (multi-thread) | Process supervision, file I/O, watchers. |
| Templating | `minijinja` | Jinja2-compatible, actively maintained, Rust-native. |
| Serialization | `serde` + `toml` (author-facing) + `serde_json` (machine-facing) | Human-readable for templates, JSON where we need round-tripping with agents. |
| Logging | `tracing` + `tracing-appender` | Structured logs to rotating file. |
| Errors | `thiserror` (library), `anyhow` (binary boundary) | Standard Rust split. |
| File watching | `notify` | Reliable cross-platform watchers. |
| Async channels | `tokio::sync::{mpsc, broadcast, watch}` | No lock contention in hot paths. |
| Keybindings | custom | Arrow-first; vim is out. |
| Testing | `insta` for snapshots, `rstest` for parametric, `tokio-test` | Snapshot test prompts & UI renders. |
| Process | `tokio::process::Command` | With `kill_on_drop(true)` in v0.1. |
| Config dirs | `directories` crate | XDG-compliant. |
| IDs | `uuid` v7 (time-ordered) | Sortable, no central registry needed. |

Crate graph stays **shallow** — no framework-on-framework stacks.

---

## 2. Workspace layout

lattice is a single binary plus library crates in a Cargo workspace. This
enforces the extensibility seams and keeps tests fast.

```
structui/                       # workspace root
├── Cargo.toml                  # [workspace] manifest
├── crates/
│   ├── lattice-core/         # domain model + pure logic (no I/O)
│   │   └── src/
│   │       ├── entities/       # Project, Template, Task, Run, Field, ...
│   │       ├── prompt/         # MiniJinja glue, skeleton, renderer
│   │       ├── validation/     # field validation rules
│   │       ├── derive/         # allow-listed derived-value providers
│   │       └── error.rs
│   ├── lattice-store/        # persistence (traits + file-backed impl)
│   │   └── src/
│   │       ├── store.rs        # Store<T> trait
│   │       ├── file_store.rs   # TOML/MD file-per-entity impl
│   │       ├── cache.rs        # LRU page cache
│   │       ├── watcher.rs      # notify bridge
│   │       └── atomic.rs       # tmp-fsync-rename helpers
│   ├── lattice-agents/       # agent manifests, detection, spawn, supervise
│   │   └── src/
│   │       ├── manifest.rs     # schema + loader
│   │       ├── detect.rs       # PATH probing
│   │       ├── runner.rs       # spawn + tee + lifecycle
│   │       └── builtin/        # shipped manifests (cursor-agent.toml)
│   ├── lattice-components/   # InteractiveComponent trait + builtins (v0.2+)
│   │   └── src/
│   │       ├── trait_.rs
│   │       ├── registry.rs
│   │       └── builtin/        # c4_container.rs, c4_component.rs, ...
│   ├── lattice-tui/          # ratatui views, widgets, keymap
│   │   └── src/
│   │       ├── app.rs          # Model/Msg/update
│   │       ├── screens/        # one module per screen
│   │       ├── widgets/        # reusable TUI widgets
│   │       ├── theme.rs
│   │       └── keymap.rs
│   └── lattice-bin/          # the `lattice` binary; wires everything
│       └── src/main.rs
└── docs/                       # these documents
```

`lattice-core` has **zero** I/O dependencies — it is pure Rust. Everything
else depends on it. This makes unit testing trivial and keeps the domain
portable (e.g., future WASM plugin host).

---

## 3. Runtime model (Elm-style)

A single-threaded event loop owns the `Model`. All state mutation goes
through `update(Model, Msg) -> (Model, Vec<Cmd>)`. Side effects are
expressed as `Cmd`s executed on a tokio pool; completed side effects emit
new `Msg`s back into the loop via an `mpsc` channel.

```
┌───────────────┐   Msg    ┌──────────┐    Model'    ┌──────────┐
│  input / bg   ├─────────►│  update  ├─────────────►│   view   │
│  task / tick  │          │          │              │          │
└───────────────┘          └─────┬────┘              └────┬─────┘
                                 │ Cmd                     │ frame
                                 ▼                         ▼
                           ┌──────────┐              ┌──────────┐
                           │ tokio    │              │ terminal │
                           │ effects  │              │ (ratatui)│
                           └────┬─────┘              └──────────┘
                                │ Msg (async result)
                                └───────► back to input queue
```

**Advantages:**
- Serializable snapshots of `Model` for debug / test (reducing flakiness).
- Deterministic tests: feed a `Vec<Msg>` into `update`, assert on `Model`.
- No UI thread / logic thread race conditions.

**`Model` sketch:**

```rust
pub struct Model {
    pub screen: Screen,
    pub nav_focus: NavFocus,
    pub projects: EntityList<Project>,
    pub templates: EntityList<Template>,
    pub tasks: EntityList<Task>,
    pub runs: EntityList<Run>,
    pub queues: HashMap<ProjectId, Queue>,
    pub runtime: RuntimeView,
    pub palette: Option<CommandPaletteState>,
    pub toasts: VecDeque<Toast>,
    pub settings: Settings,
    pub agents: AgentCatalog,      // detected
    pub draft_task: Option<TaskDraft>,
    pub editor: Option<TemplateEditorState>,
}

pub enum Msg {
    // input
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,

    // async results
    StoreLoaded(Result<StoreSnapshot, StoreError>),
    AgentsDetected(Vec<DetectedAgent>),
    RunStdout { run_id: RunId, line: String },
    RunStderr { run_id: RunId, line: String },
    RunExited { run_id: RunId, status: RunStatus },

    // user intents (produced by screens)
    Navigate(Screen),
    CreateProject(ProjectDraft),
    DeleteProject(ProjectId),
    EditTemplate(TemplateId),
    SaveTemplate(TemplateBody),
    DispatchTask { task_id: TaskId, agent_id: AgentId },
    KillRun(RunId),
    ResumeQueue(ProjectId),
    // ...
}
```

---

## 4. Side-effect commands (`Cmd`)

`Cmd` is an enum describing an intent; an executor translates each variant
into an async task.

```rust
pub enum Cmd {
    LoadStore,
    SaveProject(Project),
    DeleteProject(ProjectId),
    SaveTemplate(Template),
    // ...
    SpawnRun(SpawnRequest),
    KillRun(RunId),
    WatchDataDir,
    Detect Agents,
}
```

The executor:

1. Owns the `Store` handle, agent runner, file watcher.
2. Receives `Cmd` over an `mpsc::UnboundedSender<Cmd>`.
3. For each `Cmd`, spawns a `tokio::task` that performs the work and sends
   one or more `Msg`s back through the UI-bound channel.

This is the single concurrency boundary in the app — everything else is
immutable data flowing between the loop and effects.

---

## 5. Persistence layer

See `DATA_MODEL.md` for on-disk layout. Here we focus on the API.

```rust
#[async_trait]
pub trait Store<T: Entity>: Send + Sync {
    async fn get(&self, id: &T::Id) -> Result<Option<T>>;
    async fn list(&self) -> Result<Vec<T>>;
    async fn put(&self, entity: &T) -> Result<()>;
    async fn delete(&self, id: &T::Id) -> Result<()>;

    /// Subscribe to mutations (from self or file-watcher).
    fn subscribe(&self) -> broadcast::Receiver<Mutation<T::Id>>;
}
```

- `FileStore<T>` implements this on top of TOML/MD files.
- `CachedStore<T>` wraps any `Store<T>` with an LRU (`lru` crate).
- `WatchedStore<T>` wraps any `Store<T>` and re-emits external changes.

**Atomic write procedure** (in `atomic.rs`):
1. Write bytes to `<path>.tmp.<uuid>`.
2. `File::sync_all()`.
3. `fs::rename` (`<path>.tmp` → `<path>`) — atomic on POSIX.
4. `sync_all` on the parent directory.

**Own-write suppression for the watcher:**
- Every write bumps an atomic `WriteEpoch(u64)` and registers the written
  path in a short-lived `HashSet` keyed by `(path, epoch)`.
- The watcher checks the set on every event; hits within a grace window
  (e.g., 200 ms) are suppressed.

---

## 6. Agents subsystem

See `AGENTS.md` for the manifest spec. The subsystem:

- On startup, loads built-in manifests (embedded via `include_str!`) plus
  any user-level manifests under `$XDG_CONFIG_HOME/lattice/agents/*.toml`
  (user overrides built-in by `id`).
- Runs `detect` commands in parallel. Produces an `AgentCatalog`.
- `Runner` accepts a `SpawnRequest`:

```rust
pub struct SpawnRequest {
    pub run_id: RunId,
    pub agent: AgentManifest,
    pub project_dir: PathBuf,
    pub prompt: String,          // frozen markdown
    pub log_dir: PathBuf,        // where stdout.log / stderr.log go
}
```

- Spawns the child per the manifest's prompt-delivery strategy.
- Tees each line into:
  - `stdout.log` / `stderr.log` files (append, line-buffered, fsync on exit).
  - An `mpsc::Sender<RunEvent>` consumed by the Elm loop for tailing UI.
- On exit, writes `exit.toml` with status + timestamps.

v0.3 will swap `kill_on_drop(true)` for detached processes and PID files to
enable reattach.

---

## 7. Queue engine

One actor (tokio task) per process, owning:

- A `HashMap<ProjectId, ProjectQueue>`.
- A global semaphore of size `settings.max_concurrent_agents`
  (`usize::MAX` if unlimited).
- A `mpsc::Receiver<QueueCommand>` from the UI.

Algorithm:

1. On `Enqueue { project, task, agent }`, append to the project's deque.
2. On `Tick` or state change, for each project with no running task:
   - Try `semaphore.try_acquire_owned()`.
   - If acquired, pop head → spawn run → stash permit in the run record.
3. On `RunExited`:
   - Release permit.
   - If status != success and `fail_fast` is set, mark the queue paused.
   - Otherwise continue.
4. On `Pause(project)` / `Resume(project)`: flip a flag; scheduler checks it
   before popping.

The engine is fully driven by messages; no timers are required. A 250 ms
tick keeps the UI fresh.

---

## 8. Template rendering pipeline

```
Template.body (string with MiniJinja tags)
            + Task.field_values (JSON object)
            + built-in globals (project, now, derived.*, env-whitelist)
                              │
                              ▼
                 MiniJinja render → Markdown string
                              │
                              ▼
                 Skeleton post-process (section ordering)
                              │
                              ▼
                 Frozen prompt written to run.prompt.md
```

**Built-in globals available in templates:**

| Global | Description |
|---|---|
| `task.id`, `task.name`, `task.created_at` | Task metadata. |
| `task.fields.*` | User-provided field values. |
| `project.name`, `project.path`, `project.description` | Target project. |
| `template.version`, `template.name` | Frozen template snapshot. |
| `now` | UTC RFC-3339 timestamp (frozen at render). |
| `derived.<key>` | Resolved derived-value providers. |
| `component.<field>.markdown` | Rendered markdown block of an interactive component (v0.2+). |
| `component.<field>.json` | Raw JSON of an interactive component (v0.2+). |

**Custom filters**:
- `indent(n)`, `bullet`, `code_block(lang="rust")`, `quote`, `truncate(n)`.

---

## 9. Extensibility seams

Locked contracts in v0.1 so later phases don't break existing templates:

1. **Field type registry** — `FieldType` is an open enum modeled via a
   `FieldKind` string + `serde_json::Value` params. The registry resolves a
   `FieldKind` to a `FieldRenderer + FieldValidator` pair. New primitives
   register into the registry in `main.rs`.

2. **`InteractiveComponent` trait** (v0.2, but signature reserved now):
   ```rust
   pub trait InteractiveComponent: Send + 'static {
       fn kind(&self) -> &'static str;
       fn init(&mut self, state: Option<Value>) -> Result<()>;
       fn handle_event(&mut self, ev: ComponentEvent) -> ComponentAction;
       fn render(&self, frame: &mut Frame, area: Rect);
       fn value(&self) -> Value;              // canonical JSON
       fn to_markdown(&self, v: &Value) -> String;
       fn validate(&self, v: &Value) -> Result<(), Vec<FieldError>>;
   }
   ```
   Registered via a `ComponentRegistry` keyed by `kind`.

3. **Agent manifest schema** — frozen in `AGENTS.md`. Breaking changes go
   through a `manifest_version` bump and a migrator.

4. **Derived-value provider registry** — `DerivedProvider` trait; new
   providers register in core. Only `file | cmd | env | tree` in v0.1.

5. **Storage layout** — `lattice.version` at the data root enables
   migrations on start.

---

## 10. Threading & concurrency contract

- **UI thread:** runs the event loop, owns `Model`. Never blocks on I/O.
- **Executor pool:** tokio multi-thread. Handles all I/O, watchers,
  process supervision.
- **Shared state:** none. Everything flows through channels.
- **Interior mutability:** allowed only inside executor-owned components
  (e.g., `Store`), never inside `Model`.

**Channel inventory:**
- `ui_tx: mpsc::UnboundedSender<Msg>` — executor → UI.
- `cmd_tx: mpsc::UnboundedSender<Cmd>` — UI → executor.
- `watcher_tx: broadcast::Sender<FsEvent>` — watcher → store layers.
- `run_events: broadcast::Sender<RunEvent>` — runner → many consumers
  (runtime view, history writer).

---

## 11. Error model

- `LatticeError` in `lattice-core` is a `thiserror` enum per subsystem.
- Binary-level errors are `anyhow::Error` with context.
- UI surfaces errors as **toasts** (non-fatal) or **modal banners**
  (requires acknowledgement). All errors are also written to the log file.
- Errors during startup (missing dirs, permissions) abort with a clear
  message to stderr; the TUI does not enter alt-screen until startup is
  clean.

---

## 12. Observability

- `tracing` spans per `Msg` and per `Cmd`.
- Log levels: `error | warn | info | debug | trace`, default `info`.
- Log destination: `$XDG_STATE_HOME/lattice/lattice.log` with
  rotation (10 MB × 5 files).
- Every run carries a `run_id` span, so grepping the log for a run is
  trivial.

---

## 13. Testing strategy

1. **Unit tests** in `lattice-core`: validation, rendering, derived-value
   resolution. 90%+ coverage target here.
2. **Store tests** against a temp dir; assert atomic-write invariants
   (chaos test that kills between `sync_all` and `rename` and proves the
   file is either old or new, never half-written).
3. **Update tests** — pure: feed `(Model, Msg)` → assert next `Model`.
4. **View snapshot tests** — use `insta` on ratatui's `Buffer::dump()` for
   representative screens. Locks down UX regressions.
5. **Prompt-render golden tests** — for every example template, compare
   rendered output against a `.md` fixture.
6. **End-to-end smoke** — a `tests/e2e_cursor_stub.rs` that replaces
   `cursor-agent` with a scripted fake binary (compiled in the test) and
   runs a full dispatch lifecycle.

---

## 14. Packaging (v0.1)

- `cargo install --path crates/lattice-bin`.
- Single binary `lattice`.
- No bundled assets (manifests embedded via `include_str!`).
- macOS + Linux; Windows unsupported officially.
