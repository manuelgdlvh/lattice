# DATA MODEL

Describes every entity, its on-disk representation, the directory layout,
the page cache, and the invariants the persistence layer must uphold.

---

## 1. Disk layout

lattice uses **two roots**, both XDG-compliant:

- **Config root** (`$XDG_CONFIG_HOME/lattice` — default `~/.config/lattice`):
  User-authored and hand-editable. Safe to check into git / dotfiles.

- **State root** (`$XDG_DATA_HOME/lattice` — default `~/.local/share/lattice`):
  App-owned state (templates, tasks, cache, logs).

Both roots can be overridden with env vars (`LATTICE_CONFIG_DIR`, `LATTICE_STATE_DIR`).

```
$XDG_CONFIG_HOME/lattice/
└── settings.toml                     # global settings
```

```
$XDG_DATA_HOME/lattice/
├── templates/
│   └── <template_id>/
│       └── template.toml             # current template definition
├── tasks/
│   └── <task_id>/
│       ├── task.toml                 # task metadata + field values
│       ├── template.snapshot.toml    # frozen template copy
│       └── prompt.md                 # rendered prompt (preview artifact)
├── cache/
└── logs/
    └── lattice.log                  # rotating log
```

**Why files per entity, not a single JSON blob**:

- Git-friendly diffs.
- Failure isolation: one corrupt file does not take down the app.
- Atomic writes per entity are trivial.
- Easy partial loading (e.g. list templates without loading every task).

---

## 2. Entity specs

Every entity uses TOML (author-facing) unless stated otherwise. IDs are
**UUID v7** (time-sortable). Timestamps are RFC-3339 UTC.

Examples below are illustrative; the full template schema reference lives in
`TEMPLATES.md`.

### 2.1 Settings — `settings.toml`

Settings includes cache/logging settings.

### 2.2 Template — `templates/<id>/template.toml`

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
- Field `id`s unique within a template; stable (renames require tooling).

### 2.3 Task — `tasks/<task_id>/task.toml`

```toml
schema_version = 1
id         = "019f2d5a-c103-7d14-8b2e-9a44c77aa210"
template_id      = "019f2d5a-b1e2-7b1d-93c0-8c7d5a42a100"
template_version = 7
name       = "refactor auth middleware"
created_at = "2026-04-24T10:30:00Z"

[fields]
module_path = "src/auth/middleware.rs"
constraints = ["no new deps", "preserve public API"]
acceptance  = "All existing tests pass; new tests for edge cases."
```

Companion file `template.snapshot.toml` is a byte copy of the template at
instantiation time. This decouples the task from later template edits.

Invariants:
- `template_version` must equal the version in `template_snapshot.toml`.

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

Directory mutations (create template/task dir) use `mkdir -p`
semantics, are idempotent, and are also parent-fsynced on critical
moments.

---

## 5. Page cache

Motivation: reading many files per render is too slow; the LRU keeps hot reads
in memory without becoming authoritative.

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

- `notify::recommended_watcher` on the config root and state root.
- Events coalesced with a 100 ms debounce.
- Foreign-write events → cache invalidation → `Mutation::External`
  broadcast → UI lists re-load affected entities.

Guardrails:

- Watcher ignores anything under `cache/` and `lattice.log`.
- Watcher ignores `*.tmp.*` (staging files).
- Watcher ignores own writes within 200 ms of the recorded epoch.

---

## 7. Integrity checks

- Every TOML file must parse.
- Every task must have a `template.snapshot.toml`.
- Every task's `template_id` must reference an existing template.

---

## 8. Size budgets

- A typical template: < 20 KiB.
- A typical task: < 10 KiB.

Targets:

- Thousands of tasks and templates on disk should still feel fast.
