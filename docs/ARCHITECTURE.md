# ARCHITECTURE

Technical architecture for lattice.

---

## 1. Tech stack

| Concern | Choice | Rationale |
|---|---|---|
| TUI | `ratatui` + `crossterm` | De-facto standard; custom-widget friendly. |
| Async runtime | `tokio` (multi-thread) | Process supervision, file I/O, watchers. |
| Templating | `minijinja` | Jinja2-compatible, actively maintained, Rust-native. |
| Serialization | `serde` + `toml` (author-facing) + `serde_json` (machine-facing) | TOML for templates/tasks; JSON for field values and derived values. |
| Logging | `tracing` + `tracing-appender` | Structured logs to rotating file. |
| Errors | `thiserror` (library), `anyhow` (binary boundary) | Standard Rust split. |
| File watching | `notify` | Reliable cross-platform watchers. |
| Async channels | `tokio::sync::{mpsc, broadcast, watch}` | No lock contention in hot paths. |
| Keybindings | custom | Arrow-first; vim is out. |
| Testing | `insta` for snapshots, `rstest` for parametric, `tokio-test` | Snapshot test prompts & UI renders. |
| Process | `tokio::process::Command` | Process APIs if needed. |
| Config dirs | `directories` crate | XDG-compliant. |
| IDs | `uuid` v7 (time-ordered) | Sortable, no central registry needed. |

Crate graph stays **shallow** — no framework-on-framework stacks.

---

## 2. Workspace layout

lattice is a single binary plus library crates in a Cargo workspace. This
enforces the extensibility seams and keeps tests fast.

```
lattice/                        # workspace root
├── Cargo.toml                  # [workspace] manifest
├── crates/
│   ├── lattice-core/         # domain model + pure logic (no I/O)
│   │   └── src/
│   │       ├── entities/       # Template, Task, Settings, Field, ...
│   │       ├── prompt/         # MiniJinja glue + renderer
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
│   # lattice-components/     # out of scope
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
portable (e.g., future alternate frontends).

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

The current build keeps the same Elm-style structure but only includes
Templates/Tasks/Settings UI plus overlays (palette, forms, picker, toasts).

---

## 4. Side-effect commands (`Cmd`)

`Cmd` is an enum describing an intent; an executor translates each variant
into an async task.

The current build's side effects are store operations (list/load/save/delete)
and filesystem watcher events.

The executor:

1. Owns the `Store` handle and file watcher.
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

## 6. Template rendering pipeline

```
Template.body (string with MiniJinja tags)
            + Task.field_values (JSON object)
            + built-in globals (task, template, derived.*, now)
                              │
                              ▼
                 MiniJinja render → Markdown string
                              │
                              ▼
                 Prompt preview shown in the TUI and optionally saved to `prompt.md`
```

**Built-in globals available in templates:**

| Global | Description |
|---|---|
| `task.id`, `task.name`, `task.created_at` | Task metadata. |
| `task.fields.*` | User-provided field values. |
| `template.version`, `template.name` | Frozen template snapshot. |
| `now` | UTC RFC-3339 timestamp (frozen at render). |
| `derived.<key>` | Resolved derived-value providers. |

**Custom filters**:
- `indent(n)`, `bullet`, `code_block(lang="rust")`, `quote`, `truncate(n)`.

---

## 7. Extensibility seams

1. **Field type registry** — `FieldType` is an open enum modeled via a
   `FieldKind` string + `serde_json::Value` params. The registry resolves a
   `FieldKind` to a `FieldRenderer + FieldValidator` pair. New primitives
   register into the registry in `main.rs`.

2. **`InteractiveComponent` trait** (reserved for future interactive fields):
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

3. **Derived-value provider registry** — `DerivedProvider` trait; new
   providers register in core.

4. **Storage layout** — layout changes are handled by code updates; disk is
   always the source of truth.

---

## 8. Threading & concurrency contract

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

---

## 9. Error model

- `LatticeError` in `lattice-core` is a `thiserror` enum per subsystem.
- Binary-level errors are `anyhow::Error` with context.
- UI surfaces errors as **toasts** (non-fatal) or **modal banners**
  (requires acknowledgement). All errors are also written to the log file.
- Errors during startup (missing dirs, permissions) abort with a clear
  message to stderr; the TUI does not enter alt-screen until startup is
  clean.

---

## 10. Observability

- `tracing` spans per `Msg` and per `Cmd`.
- Log levels: `error | warn | info | debug | trace`, default `info`.
- Log destination: `$XDG_STATE_HOME/lattice/lattice.log` with
  rotation (10 MB × 5 files).
- Every run carries a `run_id` span, so grepping the log for a run is
  trivial.

---

## 11. Testing strategy

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
6. **End-to-end smoke** — basic TUI render and store roundtrip tests.

---

## 12. Packaging

- `cargo install --path crates/lattice-bin`.
- Single binary `lattice`.
- No bundled assets.
- macOS + Linux; Windows unsupported officially.
