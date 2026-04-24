# lattice

**Task-first, schema-driven AI dev orchestrator — in your terminal.**

`lattice` is a TUI for orchestrating CLI coding agents (Cursor, Claude
Code, …) around *tasks*, not chats. A task is an instance of a
template; a template defines the context, the schema-enforced fields
the user must fill, and the markdown prompt that gets rendered for
the agent. The idea is simple: **force structure in, get reviewable
detail out.**

---

## Features (v0.1)

- **Projects** — register local directories as execution targets.
- **Templates** — author schema-driven prompt templates with required
  and optional fields (text, choice, number, date, path, `json`…),
  plus a Jinja-rendered prompt body.
- **Tasks** — instantiate a template against a project, fill the
  fields, preview the rendered prompt, then queue it up.
- **Dispatch** — auto-detects installed CLI agents from bundled +
  user TOML manifests under `$XDG_CONFIG_HOME/lattice/agents/`.
- **Queue engine** — per-project FIFO, global concurrency cap,
  fail-fast policy, crash-safe persistent queue.
- **Runtime view** — live list of running agents, stdout tailing,
  kill button.
- **History** — every completed run with exit status and logs.
- **Settings** — read the live config, see which agents are detected.

All state lives on disk as flat TOML/Markdown files (no SQLite), with
atomic writes and an LRU memory cache. State is recoverable between
runs.

## Workspace layout

```
crates/
  lattice-core/        entities, validation, prompt rendering
  lattice-store/       file-backed persistence, LRU cache, fs watcher
  lattice-agents/      manifest registry, runner, queue engine
  lattice-components/  extensible interactive field components (C4 in v0.2)
  lattice-tui/         ratatui UI (Elm-ish Model/Msg/update/view)
  lattice-bin/         `lattice` binary — wires everything together
```

## Install & run

Requires a stable Rust toolchain.

```bash
cargo install --path crates/lattice-bin
lattice
```

Or during development:

```bash
cargo run -p lattice-bin
```

### Environment overrides

| var | effect |
|---|---|
| `LATTICE_CONFIG_DIR` | override `$XDG_CONFIG_HOME/lattice` |
| `LATTICE_STATE_DIR` | override `$XDG_DATA_HOME/lattice` |
| `RUST_LOG` | e.g. `lattice=debug,lattice_agents=trace` |

## Keybindings

| key | action |
|---|---|
| `1`–`6` | jump to a tab (Projects, Templates, Tasks, Runtime, History, Settings) |
| `Tab` / `Shift+Tab` | cycle tabs |
| `?` / `F1` | help overlay |
| `Ctrl+K` or `/` | command palette |
| `q` / `Ctrl+C` | quit |

Inside screens: arrow keys to navigate, `Enter` to open, `a` to add,
`e` to edit, `d` to delete, `x` to dispatch tasks, `k` to kill a
running agent.

## Architecture

- **Elm-ish TUI**: a single `Model` holds all UI state; `update(Msg)`
  is pure; the shell runs side effects (`Cmd`s) and dispatches the
  follow-up `Msg`s. This keeps the core UI headlessly testable.
- **Async runtime**: `tokio` multi-thread. One unified `AppEvent`
  stream combines terminal events, queue events, live log lines, and
  a heartbeat tick.
- **Storage**: `Paths` resolves XDG dirs; `FileStore` implements the
  `Projects` / `Templates` / `Tasks` / `Runs` / `Queues` /
  `SettingsStore` traits over atomic file writes. `CachedX`
  decorators add LRU caching. A `notify`-based watcher lets the UI
  react to on-disk edits.
- **Agents**: `AgentRegistry` loads bundled + user manifests, detects
  installed binaries, and exposes `AvailableAgent`s. `AgentRunner`
  spawns processes, tees stdout/stderr to the run directory, and
  exposes a `RunHandle` for subscribing to log lines and killing the
  process (SIGTERM → SIGKILL grace).
- **Queue engine**: per-project FIFO workers gated by a global
  `tokio::sync::Semaphore`. Tasks transition `draft → queued →
  running → {succeeded, failed, killed, interrupted}`. Fail-fast
  marks remaining queued tasks `interrupted`.

## Extending

- **Adding an agent** — drop a TOML manifest under
  `$XDG_CONFIG_HOME/lattice/agents/<id>.toml` (see
  `crates/lattice-agents/src/registry/bundled/` for examples).
- **Adding a field component** — implement the `Component` trait in
  `lattice-components` and register it; templates pick it by `kind`.
- **Custom derived providers** — the `lattice-core` prompt
  templating can pull structured values from `RealFs`, `RealCmd`, or
  `RealEnv`; swap in fakes for offline tests.

## Development

```bash
cargo test --workspace        # all tests (core + store + agents + tui)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Strict clippy and `unsafe_code = "forbid"` are enforced workspace-wide.

## Roadmap

- **v0.1** — this release.
- **v0.2** — embeddable C4 component modeler; richer diff preview;
  pause / resume queues across restarts; interactive agent REPL.
- **v0.3+** — shared task library; team settings sync; agent output
  post-processors.

## License

See `Cargo.toml` for licensing metadata.
