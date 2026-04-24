# AGENTS

Specification of the agent manifest system, the reference `cursor-agent`
manifest, the spawn / lifecycle contract, and the extension guide.

> **Note:** the file `AGENTS.md` at the repo root is a well-known
> convention for AI coding agents to read on startup. This document
> (`docs/AGENTS.md`) is lattice's *internal* agents spec. If a root-
> level `AGENTS.md` is ever added for conventions, it will reference this
> file, not replace it.

---

## 1. What is an agent?

An **agent** is an external CLI that lattice can invoke to perform work
in a project's directory given a Markdown prompt. Examples:
`cursor-agent`, `claude` (Claude Code CLI), `codex`, `aider`, `gemini`.

Each agent has different:

- Binary name and installation path.
- Prompt-delivery convention (argv, stdin, file).
- Working-directory convention (we standardize on `project.path`).
- Authentication surface (usually env vars / login-on-first-use — not
  managed by lattice).
- Exit semantics (exit code = done vs idle-wait).
- Progress/output format (plain text vs JSON lines).

The **manifest** abstracts all of the above so the rest of lattice
talks to agents uniformly.

---

## 2. Manifest schema

Manifests are TOML files. One agent per file. File name convention:
`<id>.toml`. Fields marked `required` must be present; others have
defaults.

```toml
manifest_version = 1                     # required, currently 1

id           = "cursor-agent"            # required, stable identifier
display_name = "Cursor Agent"            # required
description  = "Cursor's headless coding CLI."  # optional

# ---------- detection ----------
[detect]
# How to prove the binary is installed and usable.
# All three probes are run; agent is "available" only if all succeed.
binary  = "cursor-agent"                 # required, looked up in $PATH
version_cmd = ["cursor-agent", "--version"]  # optional
version_regex = "^cursor-agent ([0-9]+\\.[0-9]+\\.[0-9]+)"  # optional capture

# ---------- invocation ----------
[invoke]
# How to spawn the agent for a single run.
# Choose exactly ONE prompt_delivery mode.
prompt_delivery = "arg"                  # required: "arg" | "stdin" | "file"

# For prompt_delivery = "arg":
argv_template = [
  "cursor-agent",
  "-p", "{{ prompt }}",
  "--cwd", "{{ project.path }}",
]

# For prompt_delivery = "stdin":
# argv_template = ["cursor-agent"]
# stdin_template = "{{ prompt }}"

# For prompt_delivery = "file":
# argv_template = ["cursor-agent", "-f", "{{ prompt_file }}"]
# prompt_file_suffix = ".md"

cwd_strategy = "project_root"            # required, only value in v0.1
env_pass_through = true                  # default true
extra_env = { CI = "1" }                 # optional, merged after env

# ---------- lifecycle ----------
[lifecycle]
# How to tell the agent is done.
done_on = "exit"                         # required, only value in v0.1
timeout_seconds = 1800                   # optional, 0 == no timeout (per-run override available)

# v0.3 adds these:
# supports_stdin_interactive = false
# heartbeat_regex = ""
# completion_regex = ""

# ---------- output parsing (optional) ----------
[output]
format = "text"                          # "text" | "jsonl" (v0.3+)
# progress_regex helps the UI highlight progress lines in the tail view.
progress_regex = "^\\[.*\\]"

# ---------- capabilities (advertisement, for future features) ----------
[capabilities]
non_interactive = true
interactive_stdin = false                # v0.3
structured_output = false                # v0.3
```

### 2.1 Template variables available

Rendered with MiniJinja, same as prompts. Scope:

- `prompt` — the frozen Markdown string.
- `prompt_file` — the path to a file written by lattice immediately
  before spawn (only if `prompt_delivery = "file"`).
- `project.path`, `project.name`, `project.id`.
- `task.id`, `task.name`, `template.name`, `template.version`.
- `run.id`, `run.started_at`.
- `env.*` — values from the OS environment explicitly whitelisted by
  `invoke.env_whitelist` (not shown in the schema above; v0.3 adds it).

### 2.2 Validation

At load time, the manifest loader enforces:

- `manifest_version == 1` (bump = migrator).
- `id` matches `^[a-z0-9][a-z0-9-]{1,39}$`.
- Exactly one of `prompt_delivery` mode's required fields is present.
- `argv_template` non-empty; first element is the binary to spawn.
- No shell metacharacters interpreted; we execve via `tokio::process::Command`
  with the templated argv list. There is no shell.

---

## 3. Reference manifest — `cursor-agent`

Shipped embedded in the binary. User can override by placing a file with
the same `id` at `$XDG_CONFIG_HOME/lattice/agents/cursor-agent.toml`.

```toml
manifest_version = 1

id           = "cursor-agent"
display_name = "Cursor Agent"
description  = "Cursor's headless coding CLI (non-interactive mode)."

[detect]
binary  = "cursor-agent"
version_cmd   = ["cursor-agent", "--version"]
version_regex = "([0-9]+\\.[0-9]+\\.[0-9]+)"

[invoke]
prompt_delivery = "stdin"
argv_template   = ["cursor-agent", "-p", "--cwd", "{{ project.path }}"]
stdin_template  = "{{ prompt }}"
cwd_strategy    = "project_root"
env_pass_through = true

[lifecycle]
done_on = "exit"
timeout_seconds = 1800

[output]
format = "text"
progress_regex = "^\\["
```

> The exact `cursor-agent` invocation flags (`-p`, `--cwd`, stdin-prompt
> convention) are **subject to verification** against the current
> `cursor-agent --help` output at implementation time. The manifest
> above reflects the most commonly documented shape as of the time of
> writing. If the real flags differ, only this file needs to change —
> no code changes required.

---

## 4. Detection lifecycle

On app start (and on demand via `Settings > Detect agents`):

1. Collect all manifests: embedded built-ins + user-level overrides.
   User overrides replace built-ins by `id`.
2. For each manifest, asynchronously:
   - `which(<binary>)` via `which` crate.
   - If found, run `version_cmd` with a 2s timeout.
   - If `version_regex` is set, capture group 1 is the version string.
3. Emit `DetectedAgent { id, path, version, available: bool, error? }`.
4. Save the catalog to the in-memory `AgentCatalog`. Not persisted on
   disk (always re-detected).

Detection errors are non-fatal; they surface in the Settings screen and
in the Dispatch modal (the agent appears greyed out with a reason).

---

## 5. Spawn & supervision

For each dispatched run, the `Runner`:

1. Creates `runs/<project_id>/<run_id>/` if not exists.
2. Writes `prompt.md` (frozen Markdown; always, regardless of delivery
   mode — this is the audit artifact).
3. If `prompt_delivery = "file"`, writes a separate
   `prompt_in.<suffix>` for the agent (usually same bytes).
4. Renders `argv_template` and `stdin_template` with MiniJinja +
   manifest scope.
5. Spawns with `tokio::process::Command`:
   - `.current_dir(project.path)`.
   - `.kill_on_drop(true)` (v0.1).
   - `.env_clear()` **not** applied by default (`env_pass_through =
     true`); manifest `extra_env` merges after.
   - stdout/stderr piped.
6. Records `pid`, `started_at` into `run.toml`.
7. Tees:
   - Each line into `stdout.log` / `stderr.log` (append + line buffered).
   - Each line into a broadcast channel consumed by the Runtime view.
8. Applies `timeout_seconds`: if exceeded, kills like a user kill but
   records `status = "killed"` with `reason = "timeout"`.

On child exit:

- Serialize `exit.toml` with exit code, signal (if any), duration.
- Update `run.toml` status.
- Emit `RunExited` to the queue engine.

**Kill:**
- User triggers kill → `SIGTERM` → wait up to `kill_grace_seconds` (5s
  default) → `SIGKILL`.
- Status = `killed`; `exit.toml.signal` reflects the terminating signal.

---

## 6. Reattach (v0.3 preview)

v0.1 does not reattach, but we reserve the field layout now:

- `run.toml.pid` is already present.
- On startup, if a `run.toml` exists without `exit.toml`, its state is
  flipped to `interrupted`. The pid is *not* trusted for reattach in
  v0.1 (race with PID reuse).
- v0.3 adds:
  - A `pidfile` approach (`runs/.../run.pid`) with process-start-time
    verification (`/proc/<pid>/stat` on Linux; `proc_pidinfo` on macOS).
  - stdout/stderr written via a **pre-opened file descriptor** that is
    kept open across app restarts through a detached helper process
    (to be designed; tentative: a tiny per-run supervisor binary).

---

## 7. Extension guide (adding a new agent)

Until the user-facing `lattice add-agent` command exists (v0.3), the
path is:

1. Create
   `$XDG_CONFIG_HOME/lattice/agents/<id>.toml` with the schema in §2.
2. Restart lattice (or use `Settings > Detect agents`).
3. Agent appears in the Dispatch modal if detection passes.

### 7.1 Recipes

**Agent takes prompt via `-p`:**
```toml
[invoke]
prompt_delivery = "arg"
argv_template = ["agent", "-p", "{{ prompt }}"]
```

**Agent reads prompt from a file:**
```toml
[invoke]
prompt_delivery   = "file"
argv_template     = ["agent", "--prompt-file", "{{ prompt_file }}"]
prompt_file_suffix = ".md"
```

**Agent reads prompt from stdin:**
```toml
[invoke]
prompt_delivery = "stdin"
argv_template   = ["agent"]
stdin_template  = "{{ prompt }}"
```

**Agent needs a specific model flag:**
```toml
[invoke]
argv_template = ["claude", "code", "--model", "claude-sonnet-4", "-p", "{{ prompt }}"]
```

---

## 8. Security considerations

- No shell invocation anywhere. We go through `execve` via Tokio's
  `Command::new(argv[0]).args(&argv[1..])`.
- No user-authored strings are interpolated into a shell. All
  interpolation lands in individual argv elements.
- Env pass-through is controllable per manifest (default: pass-through;
  whitelists arrive in v0.3).
- `extra_env` from manifests is the **only** way to inject additional
  env vars — users may use it deliberately (e.g., `HTTP_PROXY`, model
  overrides).
- lattice never reads or forwards the user's shell init files.
- Agents run with the same privileges as lattice; lattice never
  escalates.

---

## 9. Logging & observability per agent run

- Every run has a `tracing` span `run_id=<uuid>`.
- Log lines from the runner include:
  - `agent_spawn { agent_id, pid, argv_redacted }` at info.
  - `agent_exit  { agent_id, exit_code, duration_ms, signal? }` at info.
  - `agent_stdout { bytes }` / `agent_stderr { bytes }` at trace (off by
    default; users can flip level temporarily from Settings).
- stdout/stderr never go to the rotating app log — only to the per-run
  log files.

---

## 10. Testing agents offline

For development and CI, ship a scripted fake agent at
`crates/lattice-agents/tests/fake_agent.rs` compiled as a bin. Its
manifest lives in `crates/lattice-agents/tests/fixtures/fake.toml` and
is only loaded by the test harness. The fake agent:

- Reads prompt per the delivery mode under test.
- Emits deterministic stdout with timestamps stripped.
- Supports `--fail`, `--sleep N`, `--partial` flags to exercise error
  paths, timeouts, partial log capture.
