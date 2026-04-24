# SPEC — lattice v0.1 (MVP)

The authoritative product spec for the MVP. Anything not listed here is out
of scope for v0.1.

---

## 1. Problem statement

Developers increasingly delegate work to AI coding agents. The review burden
created by a weak prompt is large and invisible until the diff lands. Chat
interfaces incentivize short, low-structure prompts. The result: re-work,
shallow solutions, and "vibe code" that passes a surface read but fails at
design, non-functional, or integration concerns.

lattice inverts this. It forces (with consent) the author to fill a
schema before dispatch. Rich schemas produce rich prompts, rich prompts
produce reviewable output.

The product is a local TUI, because:

- It lives where the code lives.
- It is scriptable, composable, fast, and keyboard-first.
- It can spawn and supervise agent CLIs in the project's actual working
  directory.

---

## 2. Target user (v0.1)

A single developer on Linux or macOS, working on one or more local Git
repositories, with one or more AI coding agent CLIs installed (`cursor-agent`
at minimum). They want repeatable, reviewable AI-driven tasks with explicit
guardrails.

Not in scope: teams, shared libraries of templates, auth.

---

## 3. Glossary

- **Project** — a reference to a local directory (usually a Git repo) where
  agents will be executed. The directory is the execution cwd.
- **Template** — a reusable schema: preamble, fields, validation rules,
  prompt rendering logic, a canonical skeleton.
- **Field** — a typed input within a template. Built-in primitive types in
  v0.1; interactive components arrive in v0.2.
- **Task** — an instance of a template, bound to a project, with all
  required fields filled. Carries a **frozen copy** of the template it was
  created from.
- **Task draft** — a task being authored that has not yet been dispatched.
  Does not need to satisfy required fields.
- **Prompt** — the rendered Markdown document that will be sent to an agent.
  Derived from the task at dispatch time and **frozen** at dispatch.
- **Run** — a single execution of one task by one agent. Has a lifecycle,
  logs, exit code, and timestamps.
- **Agent** — an external CLI that can accept a prompt and produce work in
  the project directory. Described by a **manifest**.
- **Queue** — per-project FIFO list of tasks awaiting execution.
- **Runtime** — the screen/state area showing currently running agents.
- **History** — the append-only record of past runs.

---

## 4. User stories (v0.1)

All must be satisfied for v0.1 exit.

### 4.1 Project management
- US-P1 Add a project by picking a directory from a file-picker.
- US-P2 Remove a project (does not delete files on disk; only the lattice
  reference).
- US-P3 Rename and re-describe a project.

### 4.2 Template authoring
- US-T1 Create a template via the TUI editor: name, description, preamble,
  prompt skeleton, a list of typed fields with validation.
- US-T2 Edit an existing template. Each save bumps a monotonic version
  counter; old versions are kept on disk.
- US-T3 Delete a template (existing tasks that reference it are unaffected
  because they hold a frozen copy).
- US-T4 Duplicate a template to start a new one.
- US-T5 Import/export a template as a `.toml` file.

### 4.3 Task creation
- US-TK1 Pick a template, pick a target project, see a form with the
  template's fields.
- US-TK2 Fill the form; required fields show errors until satisfied;
  conditional fields appear/disappear based on other field values.
- US-TK3 Preview the rendered Markdown prompt before dispatch.
- US-TK4 Save as draft (persists to disk; resumable later).
- US-TK5 Dispatch the task: pick an installed agent, the task is enqueued
  for the project.

### 4.4 Batch dispatch
- US-B1 Select multiple task drafts, dispatch them as a batch with the same
  agent; each is appended to its project's queue in the order selected.

### 4.5 Execution
- US-E1 At most N agents run concurrently across all projects
  (N configurable, default unlimited). At most one agent runs per project.
- US-E2 While running, stdout and stderr stream to disk and to a tail view.
- US-E3 Kill a running agent (SIGTERM → 5s grace → SIGKILL); queue proceeds
  to the next task unless the user has chosen fail-fast (default).
- US-E4 On queue failure with fail-fast = true, queue is paused; user is
  notified and must resume it manually.

### 4.6 Runtime & history
- US-R1 See all currently running agents, their project, task, duration,
  last stdout line.
- US-R2 Expand a running agent to see its full stdout/stderr tail.
- US-R3 See past runs filterable by project, template, agent, status.
- US-R4 Open a past run to see the frozen prompt, frozen task, exit code,
  full logs.
- US-R5 Re-run a past run identically (reuses the frozen prompt exactly).
- US-R6 Re-run with edits (creates a new task draft pre-filled from the old
  task's field values).

### 4.7 Settings
- US-S1 Change global agent-concurrency cap.
- US-S2 View detected agents and their manifest paths.
- US-S3 Override a built-in manifest with a user-level manifest file.

---

## 5. Functional requirements

### 5.1 Persistence
- All user-authored data lives on disk as TOML (entities) or Markdown
  (prompts, logs as raw text). See `DATA_MODEL.md`.
- Disk writes are atomic: write to `<path>.tmp` → `fsync` → `rename`.
- A memory cache (LRU, configurable) accelerates reads. Invalidation is
  driven by the writer (own-process) and by `notify` file watching
  (foreign writes).
- First launch creates the directory layout and writes a
  `lattice.version` file.

### 5.2 Rendering
- Prompts are rendered with **MiniJinja** using the task's field values and
  a fixed set of built-in globals (see `TEMPLATES.md`).
- The default skeleton produces a structured Markdown document.
- Preview uses the exact same renderer; preview and dispatch are
  byte-identical given the same inputs.

### 5.3 Agent lifecycle
- Child processes are spawned with `tokio::process::Command` and
  `kill_on_drop(true)` in v0.1 (so they die with the app — reattach is
  v0.3).
- stdout/stderr are captured line-by-line, tee'd to an on-disk log file and
  a bounded in-memory ring buffer for the tail view.
- Exit code, end timestamp, truncation flags persist atomically.

### 5.4 Queueing
- Per project: one FIFO.
- Global: a semaphore of size `max_concurrent_agents`.
- A task transitions: `draft` → `queued` → `running` → `succeeded` |
  `failed` | `killed` | `interrupted`.
- On app start, any `running` tasks are reclassified as `interrupted` with
  a note in history. Any `queued` tasks remain queued but the queue starts
  paused; user resumes with a key.

### 5.5 Guardrails
- No user-authored code is executed. Derived values come from an allow-list
  of providers (`file`, `cmd`-argv, `env`, `tree`) — never raw shell
  strings.
- Prompts must pass validation before dispatch.
- Dispatch always requires explicit user confirmation (Enter on the preview
  screen).

---

## 6. Non-functional requirements

- **Startup:** < 250 ms to first frame on a warm cache, < 1 s cold, for a
  disk layout with up to 1,000 tasks.
- **Input latency:** < 16 ms per keystroke in the editor; arrow navigation
  feels instantaneous.
- **Memory:** < 150 MB RSS steady-state with 10,000 history entries.
- **Crash safety:** a `kill -9` of lattice must not corrupt on-disk
  files (guaranteed by atomic writes).
- **Portability:** Linux and macOS. A 64-bit terminal emulator that
  supports 256-color + Unicode box-drawing.

---

## 7. Acceptance criteria (v0.1 exit)

1. A fresh install creates disk layout and starts with zero panics.
2. A user can create a project, a template with ≥5 field types, and a
   task, then dispatch it to `cursor-agent` and see stdout stream live.
3. Killing the agent mid-run cleanly terminates the process, records the
   kill in history, and pauses the queue.
4. Relaunching after an ungraceful exit shows no data loss.
5. Re-running a history entry reproduces the same frozen prompt byte-for-
   byte.
6. Editing a template does not change any existing task's frozen copy.
7. All example templates in `TEMPLATES.md` render without error and
   produce valid Markdown against an included golden file (snapshot test).
8. `cargo test` green; `cargo clippy -- -D warnings` clean.

---

## 8. Assumptions

- The user has `cursor-agent` in `$PATH` and an authenticated session.
- The target project is a Git repo (not required by v0.1 but strongly
  recommended — future versions will use Git for diffs).
- The terminal supports Unicode and 256 colors.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|---|---|
| MiniJinja templates are too flexible and authors create unreadable prompts | Ship a default skeleton; encourage overrides only when necessary; `TEMPLATES.md` emphasizes canonical structure. |
| Agents vary widely in invocation conventions | Manifest abstraction from day one, with `cursor-agent` proving the contract. |
| File watcher fires on our own writes and causes reload storms | Tag own-process writes with a monotonic counter; watcher ignores known counters. |
| Users lose work when app crashes during task authoring | Autosave drafts every N keystrokes or on any field blur. |
| C4/interactive components (v0.2) explode scope | v0.1 ships a stub + trait; real impl is isolated to v0.2. |
| Rendered prompts leak secrets (env, local paths) | Derived-value providers are opt-in per template; previews are always shown before dispatch. |
