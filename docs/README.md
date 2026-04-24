# lattice — design documents

> **Product name:** `lattice`
> **Repo / Cargo package:** `structui` (will be renamed to `lattice` before v0.1.0).

The name evokes the thesis: *tasks sit in a structured lattice of schemas,
constraints, and relationships — not in a free-form chat*.

`lattice` is a **task-first, schema-driven AI dev orchestrator** delivered as a
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

Read in this order for a first pass:

1. **[ROADMAP.md](./ROADMAP.md)** — phased plan from v0.1 MVP to v1.0, with exit
   criteria per phase and explicit non-goals.
2. **[SPEC.md](./SPEC.md)** — authoritative product + feature spec for v0.1.
   What ships, what doesn't, user stories, acceptance criteria.
3. **[ARCHITECTURE.md](./ARCHITECTURE.md)** — module layout, crate choices,
   runtime model, threading, extensibility seams.
4. **[DATA_MODEL.md](./DATA_MODEL.md)** — entities, on-disk layout, file
   formats, page-cache design, concurrency/atomicity contracts.
5. **[UX.md](./UX.md)** — screen inventory, navigation, keybindings, widget
   catalog.
6. **[AGENTS.md](./AGENTS.md)** — agent manifest spec, cursor-agent reference
   manifest, extension guide, process lifecycle, reattach plan.
7. **[TEMPLATES.md](./TEMPLATES.md)** — template authoring guide, field type
   reference, prompt rendering rules, worked examples.
8. **[BUILD_PLAN.md](./BUILD_PLAN.md)** — milestone-by-milestone build plan
   for v0.1, each milestone scoped to a reviewable PR.

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
  execution; nothing runs without explicit user dispatch.
- **Extensibility via Rust traits, not plugins.** v0.1 is trait-based in-tree;
  the trait surface is designed to be WASM-portable later without breaking
  templates.
- **Reduce review cognition.** Every task run produces a frozen prompt, a
  frozen template snapshot, and a structured history entry — so a reviewer
  knows exactly what was asked and what was produced.
