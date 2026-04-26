# SPEC

High-level product spec. This document is intentionally user-oriented.

---

## 1. Problem statement

Developers increasingly delegate work to AI coding tools. The review burden
created by a weak prompt is large and invisible until the diff lands. Chat
interfaces incentivize short, low-structure prompts. The result: re-work,
shallow solutions, and "vibe code" that passes a surface read but fails at
design, non-functional, or integration concerns.

lattice inverts this. It forces (with consent) the author to fill a
schema before use. Rich schemas produce rich prompts, rich prompts
produce reviewable output.

The product is a local TUI, because:

- It lives where the code lives.
- It is scriptable, composable, fast, and keyboard-first.
- It keeps templates, tasks, and prompts close to the repository and easy to
  reuse.

---

## 2. Target user

A single developer on Linux or macOS, working on one or more local Git
repositories. They want repeatable, reviewable task briefs (prompts) with
explicit guardrails.

Not in scope: teams, shared libraries of templates, auth.

---

## 3. Glossary

- **Template** — a reusable schema: fields, validation rules, and prompt
  rendering logic.
- **Field** — a typed input within a template.
- **Task** — an instance of a template with user-provided field values.
  Carries a **frozen copy** of the template it was created from.
- **Task draft** — a task being authored that has not yet been finalized.
  Does not need to satisfy required fields.
- **Prompt** — the rendered Markdown document produced from a task.
  Derived from the task and **previewable** in the TUI.

---

## 4. User stories

### 4.1 Template authoring
- US-T1 Create a template via the TUI editor: name, description, a prompt
  body, a list of typed fields with validation.
- US-T2 Edit an existing template. Each save bumps a monotonic version
  counter; old versions are kept on disk.
- US-T3 Delete a template (existing tasks that reference it are unaffected
  because they hold a frozen copy).
- US-T4 Duplicate a template to start a new one.
- US-T5 Import/export a template as a `.toml` file.

### 4.2 Task creation
- US-TK1 Pick a template and see a form with the template's fields.
- US-TK2 Fill the form; required fields show errors until satisfied;
  conditional fields appear/disappear based on other field values.
- US-TK3 Preview the rendered Markdown prompt.
- US-TK4 Save as draft (persists to disk; resumable later).
### 4.3 Settings
- US-S1 View the live config and field type reference.

---

## 5. Functional requirements

### 5.1 Persistence
- All user-authored data lives on disk as TOML (entities) or Markdown
  (prompts, logs as raw text). See `DATA_MODEL.md`.
- Disk writes are atomic: write to `<path>.tmp` → `fsync` → `rename`.
- A memory cache (LRU, configurable) accelerates reads. Invalidation is
  driven by the writer (own-process) and by `notify` file watching
  (foreign writes).
- First launch creates the directory layout.

### 5.2 Rendering
- Prompts are rendered with **MiniJinja** using the task's field values and
  a fixed set of built-in globals (see `TEMPLATES.md`).
- Templates should produce a structured Markdown document.
- Preview uses the exact same renderer as saving to `prompt.md`; output is
  byte-identical given the same inputs.

### 5.3 Guardrails
- No user-authored code is executed. Derived values come from an allow-list
  of providers (`file`, `cmd`-argv, `env`, `tree`) — never raw shell
  strings.
- Prompts must pass validation before saving.

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

## 7. Acceptance criteria

1. A fresh install creates disk layout and starts with zero panics.
2. A user can create a template with ≥5 field types and a task, preview the
   rendered prompt, and save it to a markdown file.
3. Relaunching after an ungraceful exit shows no data loss.
6. Editing a template does not change any existing task's frozen copy.
7. All example templates in `TEMPLATES.md` render without error and
   produce valid Markdown against an included golden file (snapshot test).
8. `cargo test` green; `cargo clippy -- -D warnings` clean.

---

## 8. Assumptions

- The target project is usually a Git repo (recommended).
- The terminal supports Unicode and 256 colors.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|---|---|
| MiniJinja templates are too flexible and authors create unreadable prompts | Ship strong authoring guidance and examples; `TEMPLATES.md` emphasizes reviewable structure. |
| File watcher fires on our own writes and causes reload storms | Tag own-process writes with a monotonic counter; watcher ignores known counters. |
| Users lose work when app crashes during task authoring | Autosave drafts every N keystrokes or on any field blur. |
| C4/interactive components (v0.2) explode scope | v0.1 ships a stub + trait; real impl is isolated to v0.2. |
| Rendered prompts leak secrets (env, local paths) | Derived-value providers are opt-in per template; previews are always shown before saving. |
