# TEMPLATES

Authoring guide for templates — the heart of lattice. Covers the full
schema, field types, validation, conditionals, derived values, and the
prompt rendering model, plus three worked examples.

A template is a **schema-driven form** that produces a **frozen Markdown
prompt** when instantiated as a task. The quality of the schema directly
determines the review ergonomics of the AI's output. Invest in schemas.

---

## 1. Template file structure

On disk: `templates/<id>/current.toml`, plus `v<N>.toml` for prior
versions. The schema below is authoritative.

```toml
schema_version = 1                       # required
id          = "<uuid v7>"                # required
name        = "refactor-module"          # required, unique within lattice
description = "Refactor a Rust module for readability + tests."
version     = 3                          # required, monotonic, bumped on save
tags        = ["rust", "refactor"]
created_at  = "2026-03-10T14:00:00Z"
updated_at  = "2026-04-20T09:00:00Z"

# ---- Derived values: resolved at task-render time from allow-listed providers.
[derived]
current_branch = { cmd = ["git", "rev-parse", "--abbrev-ref", "HEAD"] }
readme         = { file = "README.md", max_bytes = 4096 }
tree           = { tree = { depth = 2, exclude = ["target", "node_modules"] } }
proxy          = { env  = "HTTP_PROXY", optional = true }

# ---- Fields: ordered list of inputs the user fills to instantiate a task. ----
[[fields]]
id       = "module_path"
kind     = "textarea"
label    = "Target module"
help     = "Path to the Rust file to refactor."
required = true
[fields.validation]
regex = "^src/.*\\.rs$"

[[fields]]
id       = "goals"
kind     = "multiselect"
label    = "Refactor goals"
required = true
options  = ["readability", "performance", "testability", "error-handling"]

[[fields]]
id       = "constraints"
kind     = "textarea"
label    = "Additional constraints"
help     = "Free-form additional constraints, one per line."
required = false
placeholder = "- No new dependencies.\n- Preserve public API."

[[fields]]
id    = "perf_budget_ms"
kind  = "textarea"
label = "Performance budget (ms)"
show_if = "'performance' in task.fields.goals"
required = true

# ---- Groups: a purely presentational grouping of fields. ----
[[groups]]
title = "Target"
help  = "What to refactor."
fields = ["module_path"]

[[groups]]
title = "Intent"
fields = ["goals", "perf_budget_ms", "constraints"]

# ---- Prompt: the MiniJinja template rendered at task dispatch. ----
[prompt]
template = """
{% block context %}
## Context
You are working on a Rust codebase.
Follow existing conventions. Never break the public API.
Run `cargo fmt` and `cargo clippy` before declaring completion.

Project: `{{ project.name }}` at `{{ project.path }}`
Current branch: `{{ derived.current_branch }}`

Repository snapshot:

```
{{ derived.tree }}
```
{% endblock %}

{% block target %}
## Target
Module: `{{ task.fields.module_path }}`
Goals: {{ task.fields.goals | bullet }}
{% if 'performance' in task.fields.goals %}
Performance budget: **{{ task.fields.perf_budget_ms }} ms**.
{% endif %}
{% endblock %}

{% block constraints %}
## Constraints
{% if task.fields.constraints %}
{{ task.fields.constraints | indent(0) }}
{% else %}
No additional constraints.
{% endif %}
- Preserve the public API of the module.
- Run `cargo fmt` and `cargo clippy` with no warnings.
{% endblock %}

{% block acceptance %}
## Acceptance Criteria
- All existing tests pass.
- New tests cover at least the added branches.
- The refactor does not regress any benchmark in `benches/`.
{% endblock %}

{% block deliverables %}
## Deliverables
A minimal diff, formatted, clippy-clean, with an updated `CHANGELOG.md`
entry.
{% endblock %}
"""
```

---

## 2. Field kinds (v0.1)

| Kind | JSON value shape | Notes |
|---|---|---|
| `textarea` | `"multi\nline"` | Multi-line. `placeholder` supported. |
| `select` | `"option-id"` | `options: [..]` required. |
| `multiselect` | `["a", "b"]` | `options: [..]` required. |
| `sequence-gram` | `"string"` | Sequence diagram text that can be rendered as Mermaid `sequenceDiagram` via the `sequence_gram` prompt filter. |

### 2.1 Common field properties

```
id            string  required   # unique within template; stable (rename → new field)
kind          string  required   # see table above
label         string  required
help          string             # shown under the label
placeholder   string
required      bool    default=false
default       any                # default value (must satisfy validation if set)
show_if       string             # MiniJinja boolean expression over task.fields.*
validation    table              # kind-dependent; see §3
```

### 2.2 Kind-specific properties

- `select` / `multiselect`: `options = ["a", "b", "c"]` **or**
  `options = [{ id = "a", label = "Alpha" }]`.
- `sequence-gram`: author tuigram/Mermaid body text; render in prompts with `{{ task.fields.<id> | sequence_gram }}`.

---

## 3. Validation

All validations compose with `required`. Field kinds define which are
applicable:

```
required        bool
min_length      int   (textarea, sequence-gram)
max_length      int   (textarea, sequence-gram)
regex           str   (textarea, sequence-gram)  — Rust regex syntax
allowed_values  list  (any)                                — whitelist
```

Validation errors appear inline next to the field and in a summary panel
next to the "Dispatch" button. A task cannot be dispatched while any
required field is invalid.

---

## 4. Conditional fields (`show_if`)

`show_if` is a MiniJinja **boolean expression** evaluated against
`task.fields.*` plus the `derived.*` and `project.*` scopes. If the
expression is false, the field is:

- **Hidden** in the UI.
- **Skipped** during validation (even if `required = true`).
- **Excluded** from the prompt unless the template explicitly references
  its value (in which case the variable is `undefined` and MiniJinja's
  strict mode raises — handle with `{% if task.fields.x is defined %}`).

Examples:

```toml
show_if = "task.fields.type == 'bug'"
show_if = "'performance' in task.fields.goals"
show_if = "task.fields.perf_budget_ms > 100"
```

Expressions must be **pure** and must only reference the documented
scopes. Arbitrary function calls are rejected at template-parse time.

---

## 5. Derived values

`[derived]` declares computed inputs that are **resolved at task creation
time** (not dispatch time) and stored in the task's `derived` map.
Freezing happens at task creation so re-renders / previews are
deterministic.

Allowed providers (v0.1, allow-listed):

| Provider | Shape | Result |
|---|---|---|
| `file`  | `{ file = "README.md", max_bytes = 4096 }` | UTF-8 string, truncated. |
| `cmd`   | `{ cmd = ["git", "rev-parse", "HEAD"], timeout_ms = 5000 }` | Trimmed stdout; non-zero exit fails the task creation unless `optional = true`. |
| `env`   | `{ env = "HTTP_PROXY" }` | Env value or empty; `optional = true` suppresses errors. |
| `tree`  | `{ tree = { depth = 2, exclude = [..], max_entries = 500 } }` | Multiline listing of the project dir. |

Common properties:

- `optional = false` (default) — resolution failure blocks task creation.
- `optional = true` — resolution failure yields empty / none.
- `cache_ttl_seconds = 0` (default) — re-resolved on every task creation.

**Never supported:** raw shell strings, user-editable shell commands at
dispatch, interactive pagers. All enforced at manifest-parse time.

---

## 6. Prompt rendering

### 6.1 Rendering pipeline

```
derived values resolved at task-create time
              +
field values validated and frozen at task-queue time
              +
template frozen on task.template_snapshot.toml
                       │
                       ▼
          MiniJinja render → Markdown string
                       │
                       ▼
         prompt.md (immutable, sent verbatim to agent)
```

### 6.2 MiniJinja scope

| Name | Description |
|---|---|
| `project.{id,name,path,description}` | Target project. |
| `task.id`, `task.name`, `task.created_at` | Task metadata. |
| `task.fields.<id>` | User-provided field values. |
| `derived.<name>` | Resolved derived values. |
| `template.{id,name,version}` | Frozen template snapshot. |
| `now` | Render timestamp (RFC-3339 UTC). |

### 6.3 Custom filters

- `bullet` — list → Markdown bullets.
- `indent(n)` — prepend `n` spaces to each line.
- `code_block(lang="rust")` — wrap in a fenced code block.
- `quote` — prefix `> ` to each line.
- `truncate(n)` — cap to `n` bytes with a trailing `...`.

### 6.4 Strict mode

MiniJinja runs in strict undefined mode. Any reference to an undefined
variable is a render error, surfaced in the preview. This catches typos
and missing fields early.

---

## 7. Worked examples

Three complete, copy-pasteable templates. Each one is shipped as a
fixture in `crates/lattice-core/tests/fixtures/templates/` and its
rendered output is snapshot-tested.

### 7.1 Bug fix

```toml
schema_version = 1
id          = "019f2d5a-e401-7b23-a08f-1f5522ab0011"
name        = "bug-fix"
description = "Fix a reported bug with failing test and minimal diff."
version     = 1

[derived]
recent_log = { cmd = ["git", "log", "--oneline", "-n", "15"] }

[[fields]]
id = "ticket"
kind = "textarea"
label = "Ticket / issue reference"
required = true
[fields.validation]
regex = "^[A-Z]+-[0-9]+$"

[[fields]]
id = "symptom"
kind = "textarea"
label = "Observed symptom"
required = true

[[fields]]
id = "repro_steps"
kind = "textarea"
label = "Reproduction steps"
required = true

[[fields]]
id = "scope"
kind = "select"
label = "Scope"
required = true
options = ["surface-fix", "root-cause"]

[prompt]
template = """
## Context
You are fixing a specific, reproducible bug. Keep the diff minimal and
write a failing test first if one does not already exist.

Project: `{{ project.name }}` at `{{ project.path }}`
Recent log:
```
{{ derived.recent_log }}
```

## Bug
Ticket: `{{ task.fields.ticket }}`

Symptom:
{{ task.fields.symptom | quote }}

Reproduction:
{{ task.fields.repro_steps | quote }}

## Constraints
- Produce a failing test first, then make it pass.
- Scope: **{{ task.fields.scope }}** — {% if task.fields.scope == 'surface-fix' %}minimal patch only{% else %}eliminate the root cause, even if diff grows{% endif %}.
- No unrelated refactors.

## Acceptance Criteria
- `cargo test` green.
- A new regression test proves the bug cannot recur.
- `git log` shows a single commit with the ticket id.

## Deliverables
A PR-ready diff with a commit message referencing `{{ task.fields.ticket }}`.
"""
```

### 8.2 Feature (schema-heavy)

```toml
schema_version = 1
id          = "019f2d5a-e511-7c34-b0ef-2e66a12f0012"
name        = "feature"
description = "Add a new feature with explicit non-functional constraints."
version     = 1

[[fields]]
id = "title"
kind = "textarea"
label = "Feature title"
required = true

[[fields]]
id = "user_story"
kind = "textarea"
label = "User story"
help = "As a ... I want ... so that ..."
required = true

[[fields]]
id = "entry_point"
kind = "textarea"
label = "Entry-point module"
required = true

[[fields]]
id = "nfrs"
kind = "multiselect"
label = "Non-functional requirements to address"
required = true
options = ["observability", "backwards-compatibility", "rate-limits",
           "error-handling", "security", "cost"]

[[fields]]
id = "sla_p95_ms"
kind = "textarea"
label = "Required p95 latency (ms)"
show_if = "'observability' in task.fields.nfrs"
required = true

[[fields]]
id = "flag_name"
kind = "textarea"
label = "Feature flag name (if any)"
show_if = "'backwards-compatibility' in task.fields.nfrs"

[[fields]]
id = "acceptance"
kind = "textarea"
label = "Acceptance criteria (bullet list)"
required = true

[prompt]
template = """
## Context
Ship a feature end-to-end. You are expected to reason about
observability, failure modes, and backward compatibility.
Project: `{{ project.name }}` at `{{ project.path }}`.

## Feature
Title: **{{ task.fields.title }}**

User story:
{{ task.fields.user_story | quote }}

Entry point: `{{ task.fields.entry_point }}`

## Non-functional constraints
{% for nfr in task.fields.nfrs %}
- **{{ nfr }}**{% if nfr == 'observability' %}: SLA p95 ≤ {{ task.fields.sla_p95_ms }} ms; emit metrics and traces.{% endif %}{% if nfr == 'backwards-compatibility' and task.fields.flag_name %}: guard behind feature flag `{{ task.fields.flag_name }}`.{% endif %}
{% endfor %}

## Acceptance Criteria
{{ task.fields.acceptance }}

## Deliverables
- Implementation diff.
- Tests (unit + integration).
- Metrics/trace spans if observability was selected.
- Changelog entry.
"""
```

### 8.3 Refactor (uses sequence-gram)

```toml
schema_version = 1
id          = "019f2d5a-e622-7d45-c11f-3f77b23f0013"
name        = "refactor-with-c4"
description = "Refactor a module guided by an explicit sequence diagram."
version     = 1

[[fields]]
id    = "module_path"
kind  = "textarea"
label = "Target module"
required = true

[[fields]]
id     = "target_diagram"
kind   = "sequence-gram"
label  = "Target sequence diagram"
required = true

[prompt]
template = """
## Context
Refactor guided by a container diagram describing the intended shape
after the change. Preserve public APIs unless the diagram explicitly
removes a container.

Project: `{{ project.name }}` at `{{ project.path }}`
Target module: `{{ task.fields.module_path }}`

## Target shape
{{ task.fields.target_diagram | sequence_gram }}

## Constraints
- Keep the diff coherent with the diagram.
- No new modules unless the diagram implies them.
- Tests must continue to pass.

## Acceptance Criteria
- `cargo test` green.
- Module boundaries match the diagram.

## Deliverables
- Reviewable diff.
- A short note (in the PR body) on any divergence from the diagram and
  why.
"""
```

> Note: `sequence-gram` fields can be edited with the built-in diagram editor.

---

## 9. Authoring guidance (opinionated)

These are lattice's opinions about *how* to author templates. They are
not enforced; they are strong defaults.

1. **Name the intent, not the activity.** `bug-fix` is better than
   `run-cargo-test`.
2. **Prefer enumerations over free text.** A `select` field beats a
   `text` field 9 times out of 10.
3. **Make non-functional concerns first-class.** If a template doesn't
   prompt for observability / error-handling / security, the AI won't
   volunteer them.
4. **Use `show_if` to keep forms short.** A 30-field form with 20 hidden
   behind conditionals is far better than a 10-field form that always
   asks the wrong 10 questions.
5. **Keep preambles short.** 3–5 lines. Long preambles dilute the
   per-task specifics.
6. **Freeze everything.** Use `derived` for context that must be
   captured at task-creation time (branch name, changelog, tree).
7. **Write the Acceptance Criteria as if the AI cannot ask.** It can't.
8. **Version intentionally.** If a template improves, bump the version;
   existing tasks will keep their frozen snapshot.
