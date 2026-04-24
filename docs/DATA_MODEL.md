# DATA MODEL

Describes every entity, its on-disk representation, the directory layout,
the page cache, and the invariants the persistence layer must uphold.

---

## 1. Disk layout

lattice uses **two roots**, both XDG-compliant:

- **Config root** (`$XDG_CONFIG_HOME/lattice` — default
  `~/.config/lattice`):
  User-authored and hand-editable. Safe to check into git / dotfiles.

- **State root** (`$XDG_DATA_HOME/lattice` — default
  `~/.local/share/lattice`):
  Runtime outputs. Large, log-heavy, not for git.

Both roots can be overridden with `--config-dir` and `--state-dir` CLI
flags or env vars (`LATTICE_CONFIG_DIR`, `LATTICE_STATE_DIR`).

```
$XDG_CONFIG_HOME/lattice/
├── lattice.version                 # single line, e.g. "1"
├── settings.toml                     # global settings
├── agents/                           # user-level agent manifest overrides
│   └── cursor-agent.toml
├── projects/
│   └── <project_id>.toml             # one file per project
├── templates/
│   ├── <template_id>/
│   │   ├── current.toml              # latest version
│   │   ├── v1.toml                   # frozen prior version (N-1)
│   │   ├── v2.toml
│   │   └── ...                       # kept forever until user purges
│   └── ...
└── tasks/
    └── <project_id>/
        └── <task_id>/
            ├── task.toml             # task metadata + field values
            ├── template_snapshot.toml# frozen copy of the template
            └── draft.flag            # present while task is a draft
```

```
$XDG_DATA_HOME/lattice/
├── queues/
│   └── <project_id>.toml             # persisted queue state
├── runs/
│   └── <project_id>/
│       └── <run_id>/
│           ├── run.toml              # run metadata
│           ├── prompt.md             # frozen rendered prompt (dispatched as-is)
│           ├── task_snapshot.toml    # frozen copy of the task
│           ├── template_snapshot.toml# frozen copy of the template
│           ├── stdout.log
│           ├── stderr.log
│           └── exit.toml             # exit status, timestamps, signals
├── cache/
│   └── index.toml                    # optional indices (e.g. history idx)
└── lattice.log                     # rotating log (10 MB × 5)
```

**Why files per entity, not a single JSON blob**:

- Git-friendly diffs.
- Failure isolation: one corrupt file does not take down the app.
- Atomic writes per entity are trivial.
- Easy partial loading (load project list without loading all templates).

---

## 2. Entity specs

Every entity uses TOML (author-facing) unless stated otherwise. IDs are
**UUID v7** (time-sortable). Timestamps are RFC-3339 UTC.

Examples below are illustrative; full JSON-schema-style references live in
`TEMPLATES.md` and `AGENTS.md` for their respective entities.

### 2.1 Settings — `settings.toml`

```toml
schema_version = 1

[runtime]
max_concurrent_agents = 0            # 0 == unlimited
fail_fast = true                     # per-project default; overridable per project
kill_grace_seconds = 5

[cache]
max_entries = 4096
max_bytes = 67_108_864               # 64 MiB

[logging]
level = "info"                       # error|warn|info|debug|trace
```

### 2.2 Project — `projects/<id>.toml`

```toml
schema_version = 1
id   = "019f2d5a-8b70-7a3a-b6c1-56fc42a0d3b1"
name = "acme-backend"
description = "Our payment gateway service."
path = "/home/manu/code/acme-backend"
created_at = "2026-04-24T10:12:00Z"
updated_at = "2026-04-24T10:12:00Z"

[queue]
fail_fast = true                     # overrides global default

[tags]
owner = "payments-team"
```

Invariants:
- `path` must exist and be a directory at load time (lazy check; warning
  if missing, not a hard error — user may have unplugged a drive).
- `id` must match filename.

### 2.3 Template — `templates/<id>/current.toml`

See `TEMPLATES.md` for the full schema. Header example:

```toml
schema_version = 1
id      = "019f2d5a-b1e2-7b1d-93c0-8c7d5a42a100"
name    = "refactor-module"
version = 7                          # monotonic
description = "Refactor a Rust module for readability + tests."
created_at = "2026-03-10T14:00:00Z"
updated_at = "2026-04-20T09:00:00Z"

[[fields]]
id       = "module_path"
kind     = "textarea"
label    = "Target module"
required = true
[fields.validation]
regex = "^src/.*\\.rs$"

# ... more fields ...

[prompt]
template = """
## Context
You are working on a Rust codebase. Follow the project's existing style.
Run `cargo fmt` and `cargo clippy` before reporting completion.

## Target
Project: `{{ project.name }}` at `{{ project.path }}`
Module:  `{{ task.fields.module_path }}`

## Constraints
{% for c in task.fields.constraints %}- {{ c }}
{% endfor %}

## Acceptance Criteria
{{ task.fields.acceptance | bullet }}

## Deliverables
A minimal diff, formatted, clippy-clean, with updated tests.
"""
```

Invariants:
- `version` bumps monotonically. Saving the same content is a no-op (no
  version bump).
- Prior versions saved as `v<N>.toml` for reference; current is
  `current.toml`.
- Field `id`s unique within a template; stable (renames require tooling,
  not allowed via UI).

### 2.4 Task — `tasks/<project_id>/<task_id>/task.toml`

```toml
schema_version = 1
id         = "019f2d5a-c103-7d14-8b2e-9a44c77aa210"
project_id = "019f2d5a-8b70-7a3a-b6c1-56fc42a0d3b1"
template_id      = "019f2d5a-b1e2-7b1d-93c0-8c7d5a42a100"
template_version = 7
name       = "refactor auth middleware"
created_at = "2026-04-24T10:30:00Z"
status     = "draft"                 # draft|queued|running|succeeded|failed|killed|interrupted

[fields]
module_path = "src/auth/middleware.rs"
constraints = ["no new deps", "preserve public API"]
acceptance  = "All existing tests pass; new tests for edge cases."
```

Companion file `template_snapshot.toml` is a byte copy of the template at
instantiation time. This decouples the task from later template edits.

A `draft.flag` (empty file) exists iff `status == "draft"`. This lets us
enumerate drafts without parsing every `task.toml`.

Invariants:
- Once `status` leaves `draft`, field values are frozen (edits require
  re-running with edits, which creates a new task).
- `template_version` must equal the version in `template_snapshot.toml`.

### 2.5 Queue — `queues/<project_id>.toml`

```toml
schema_version = 1
project_id = "019f2d5a-8b70-7a3a-b6c1-56fc42a0d3b1"
paused = false
paused_reason = ""

[[entries]]
task_id  = "019f2d5a-c103-7d14-8b2e-9a44c77aa210"
agent_id = "cursor-agent"
enqueued_at = "2026-04-24T10:40:00Z"
```

Invariants:
- Order is authoritative: head is index 0.
- A running task is **removed** from `entries` at dispatch; the
  corresponding `run.toml` is the source of truth until completion.
- When the app exits mid-run, its queue file is unchanged; on restart,
  the in-progress `run.toml` is marked `interrupted` and the queue is
  paused with `paused_reason = "previous run interrupted"`.

### 2.6 Run — `runs/<project_id>/<run_id>/...`

`run.toml`:

```toml
schema_version = 1
id         = "019f2d5a-d201-7e3f-a1bd-1c44a8f2b330"
project_id = "019f2d5a-8b70-7a3a-b6c1-56fc42a0d3b1"
task_id    = "019f2d5a-c103-7d14-8b2e-9a44c77aa210"
agent_id   = "cursor-agent"
agent_version = "2.3.1"
status     = "running"               # queued|running|succeeded|failed|killed|interrupted
queued_at  = "2026-04-24T10:40:00Z"
started_at = "2026-04-24T10:40:02Z"
finished_at = ""                     # empty until done
pid        = 482910

[log]
stdout_bytes = 0
stderr_bytes = 0
truncated    = false
```

`exit.toml` (written on completion):

```toml
exit_code = 0
signal    = ""                       # "SIGKILL" if killed
finished_at = "2026-04-24T10:45:31Z"
duration_ms = 329_000
```

`prompt.md` is the frozen rendered Markdown sent to the agent. Never
modified after dispatch.

`stdout.log` / `stderr.log` are raw bytes captured from the child.
Line-buffered append; `fsync` on exit.

### 2.7 Agent manifest — see `AGENTS.md`

Schema and full example live in `AGENTS.md`. Built-in manifests are
embedded via `include_str!`; user manifests live in
`$XDG_CONFIG_HOME/lattice/agents/<id>.toml`.

---

## 3. IDs, naming, and URLs

- UUID v7 everywhere. Encoded as lowercase hyphenated `019f…`.
- Filenames contain only the UUID.
- A `slug` derived from `name` is displayed in the UI but never used to
  address files (renames must not break history).

---

## 4. Write contract (atomicity & durability)

Every single entity write follows this routine (`atomic::write_toml`):

1. Serialize the entity to bytes with a terminating newline.
2. Open `<path>.tmp.<random>` with `O_WRONLY | O_CREAT | O_EXCL`.
3. Write; call `sync_all` on the file handle.
4. `rename` to the final path.
5. Open the parent directory and call `sync_all`.

Guarantees:

- After `rename` returns, a crash leaves either the old file or the new
  file — never a partial file.
- `fsync` on the parent makes the rename durable across power loss on
  ext4/xfs/apfs.

Directory mutations (create project dir, run dir) use `mkdir -p`
semantics, are idempotent, and are also parent-fsynced on critical
moments.

---

## 5. Page cache

Motivation: reading hundreds of task/run files per screen render is too
slow; the LRU keeps hot reads in memory without becoming authoritative.

Design:

- `lru::LruCache<CacheKey, Arc<CacheEntry>>` inside `CachedStore<T>`.
- Keys: `(EntityKind, EntityId)`.
- Values: serialized bytes + deserialized typed entity (double-cached to
  avoid re-parse).
- Size accounting: per-entry heap size (`mem::size_of_val` for primitives,
  explicit byte lengths for `String`/`Vec<u8>`).
- Eviction bounds: `max_entries` and `max_bytes`; whichever fires first.

Consistency with disk:

- Writes go through the cache **first** (so next read is fast) — but only
  the disk bytes are authoritative.
- Every write bumps a `WriteEpoch(u64)` atomic counter and registers
  `(path, epoch)` in a short-lived `OwnWriteLedger` consulted by the
  watcher.
- Foreign writes (detected by the watcher; `epoch` not in the ledger)
  invalidate the cache entry and emit a `Mutation::External(id)` event.

Concurrency:

- Cache uses a `parking_lot::Mutex` (short critical sections, no async
  suspension). Per-key fine-grained locks are not worth the complexity at
  v0.1.

---

## 6. File watcher

- `notify::recommended_watcher` on both config root and state root
  (state root mostly for runs; usually the writer is us, but we still
  want to detect external tamper).
- Events coalesced with a 100 ms debounce.
- Foreign-write events → cache invalidation → `Mutation::External`
  broadcast → UI lists re-load affected entities.

Guardrails:

- Watcher ignores anything under `cache/` and `lattice.log`.
- Watcher ignores `*.tmp.*` (staging files).
- Watcher ignores own writes within 200 ms of the recorded epoch.

---

## 7. Migrations

`lattice.version` at the config root tracks the schema version of the
layout. On startup:

1. Read version.
2. If < current, apply migrations in order (pure functions over files).
3. Write the new version **after** all migrations succeed (atomic).
4. If a migration fails, abort with a clear message; leave version file
   untouched.

v0.1 ships with `schema_version = 1` only. Migrations will matter starting
v0.4 when the template schema evolves.

---

## 8. Integrity checks

`lattice doctor` (planned v1.0, but the primitives exist in v0.1):

- Validate every TOML file parses.
- Ensure every task references an existing project and has a
  `template_snapshot.toml`.
- Ensure every queue entry references an existing non-draft task.
- Ensure every run directory has `run.toml`; if `exit.toml` is present,
  `status` must be a terminal status.

---

## 9. Size budgets

- A typical project file: < 2 KiB.
- A typical template: < 20 KiB.
- A typical task: < 10 KiB.
- A typical run: tens of KB metadata + up to MBs of logs.

Targets:

- 10,000 history runs on a single disk layout should still feel fast.
- History screen uses an on-disk index (`cache/index.toml`) built lazily
  from `run.toml` files to avoid scanning all directories per render.
