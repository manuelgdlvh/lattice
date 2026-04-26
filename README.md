# lattice

**Task-first, schema-driven AI dev orchestrator — in your terminal.**

`lattice` is a TUI for creating **structured tasks** from **templates**.
A task is an instance of a template; a template defines the context, the
schema-enforced fields the user must fill, and the Markdown prompt that
gets rendered and previewed. The idea is simple: **force structure in, get
reviewable detail out.**

---

<img width="2081" height="634" alt="image" src="https://github.com/user-attachments/assets/7fab2773-3a8e-4505-8908-bcbb8e44fe7f" />
</br>

<img width="2081" height="640" alt="image" src="https://github.com/user-attachments/assets/702e4402-74dd-43ca-9b64-a6e90f6ffcfd" />
</br>

<img width="2079" height="650" alt="image" src="https://github.com/user-attachments/assets/6b4bc8ee-10f7-4001-a6e6-f395c95a2871" />
</br>

## Features

- **Templates** — author schema-driven prompt templates with required
  and optional fields (textarea, select, multiselect, sequence-gram, code-blocks),
  plus a Jinja-rendered prompt body.
- **Tasks** — instantiate a template, fill the fields, preview the rendered
  prompt, and save the prompt to a markdown file.
- **Settings** — read the live config and field type reference.

All state lives on disk as flat TOML/Markdown files (no SQLite), with
atomic writes and an LRU memory cache. State is recoverable between
runs.

## Workspace layout

```
crates/
  lattice-core/        entities, validation, prompt rendering
  lattice-store/       file-backed persistence, LRU cache, fs watcher
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
| `RUST_LOG` | e.g. `lattice=debug` |

## Keybindings

| key | action |
|---|---|
| `1`–`3` | jump to a tab (Templates, Tasks, Settings) |
| `Tab` / `Shift+Tab` | cycle tabs |
| `?` / `F1` | help overlay |
| `Ctrl+K` or `/` | command palette |
| `q` / `Ctrl+C` | quit |

Inside screens: arrow keys to navigate, `Enter` to open, `n` to add,
`e` to edit, `d` to delete.

## Architecture

- **Elm-ish TUI**: a single `Model` holds all UI state; `update(Msg)`
  is pure; the shell runs side effects (`Cmd`s) and dispatches the
  follow-up `Msg`s. This keeps the core UI headlessly testable.
- **Async runtime**: `tokio` multi-thread. One unified `AppEvent`
  stream combines terminal events and a heartbeat tick.
- **Storage**: `Paths` resolves XDG dirs; `FileStore` implements the
  `Templates` / `Tasks` / `SettingsStore` traits over atomic file writes. `CachedX`
  decorators add LRU caching. A `notify`-based watcher lets the UI
  react to on-disk edits.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Strict clippy and `unsafe_code = "forbid"` are enforced workspace-wide.

## License

See `Cargo.toml` for licensing metadata.
