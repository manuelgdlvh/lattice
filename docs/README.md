# lattice — docs

The name evokes the thesis: *tasks sit in a structured lattice of schemas,
constraints, and relationships — not in a free-form chat*.

`lattice` is a **task-first, schema-driven prompt tool** delivered as a
terminal UI. It is *not* a chat client. Its thesis is simple:

> The quality of AI code output is a function of the quality of the brief.
> A rich, enforced schema produces a rich, reviewable output — and reduces the
> cognitive load of reviewing what the AI did.

Where "vibe coding" tools optimize for *speed of input*, lattice optimizes
for *quality of input* without making authoring painful, by pushing rich
structure (forms and validations)
through templates that then render into a well-structured Markdown prompt.

---

## Document map

User-facing docs:

- **[TEMPLATES.md](./TEMPLATES.md)** — how to author templates and fields.
- **[DATA_MODEL.md](./DATA_MODEL.md)** — where templates/tasks live on disk.

---

## Core principles (applied everywhere)

- **Structure over prose.** Every input is schema-validated before it becomes
  part of a prompt.
- **Local-first, file-first.** All user-authored artifacts are human-readable
  files on disk. SQLite is explicitly rejected. A memory page-cache accelerates
  reads but disk is always source of truth.
- **Tasks are immutable instances.** Editing a template never retroactively
  changes prior tasks or history.
- **Guardrails by default.** Derived values come from an allow-listed set of
  providers; no arbitrary shell interpolation; prompts are previewable before
  saving; nothing runs automatically.
- **Extensibility via Rust traits, not plugins.** v0.1 is trait-based in-tree;
  the trait surface is designed to be WASM-portable later without breaking
  templates.
- **Reduce review cognition.** Every task run produces a frozen prompt, a
  frozen template snapshot, and a structured history entry — so a reviewer
  knows exactly what was asked and what was produced.
