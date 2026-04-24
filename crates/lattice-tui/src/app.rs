//! `App` — orchestrates the main loop: translates terminal events to
//! `Msg`s, folds them into the `Model`, fires the resulting `Cmd`s,
//! and redraws.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event as CtEvent};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tracing::warn;

use lattice_agents::{EnqueueRequest, LogStream, QueueEvent, RunHandle, RunningRun};
use lattice_core::entities::{Project, Task, Template};
use lattice_core::ids::ProjectId;
use lattice_core::time::Timestamp;

use crate::context::AppContext;
use crate::event::{AppEvent, spawn_terminal_reader};
use crate::keybind::translate;
use crate::model::{Cmd, FormState, FormSubmit, Model, Msg, update};
use crate::view::render;

/// How many final lines of `stderr.log` we surface in the error toast
/// when a run finishes with a non-`Succeeded` status. Big enough to
/// show a typical panic/traceback, small enough to not blow up the
/// status strip.
const STDERR_TAIL_LINES: usize = 12;
const HISTORY_TAIL_LINES: usize = 200;
const HISTORY_TAIL_MAX_BYTES: usize = 64 * 1024;

/// Public entry point. `run` takes ownership of the terminal and
/// restores it on drop.
pub struct App {
    ctx: AppContext,
    log_tx: mpsc::Sender<String>,
    log_rx: Option<mpsc::Receiver<String>>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("ctx", &self.ctx)
            .finish_non_exhaustive()
    }
}

impl App {
    pub fn new(ctx: AppContext) -> Self {
        let (log_tx, log_rx) = mpsc::channel::<String>(512);
        Self {
            ctx,
            log_tx,
            log_rx: Some(log_rx),
        }
    }

    /// Run the UI until the user quits.
    pub async fn run(mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = self.event_loop(&mut terminal).await;

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        res
    }

    async fn event_loop<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> io::Result<()> {
        let mut model = Model::new();
        let mut term_rx = spawn_terminal_reader();
        let mut queue_rx = self.ctx.engine.subscribe();
        let mut log_rx = self
            .log_rx
            .take()
            .expect("event_loop called twice on the same App");
        let mut last_tick = Instant::now();
        let tick = Duration::from_millis(250);

        // Initial data load.
        self.dispatch(
            Cmd::Dispatch(Msg::ToastInfo("lattice started".into())),
            &mut model,
        )
        .await;
        self.reload_all(&mut model).await;

        loop {
            terminal.draw(|f| render(f, &model))?;
            if model.quitting {
                break;
            }

            let timeout = tick
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_millis(0));

            let ev: AppEvent = tokio::select! {
                maybe_term = term_rx.recv() => match maybe_term {
                    Some(e) => AppEvent::Terminal(e),
                    None => AppEvent::Shutdown,
                },
                q = queue_rx.recv() => match q {
                    Ok(e) => AppEvent::Queue(e),
                    Err(broadcast::error::RecvError::Lagged(_)) => AppEvent::Tick,
                    Err(_) => AppEvent::Shutdown,
                },
                l = log_rx.recv() => match l {
                    Some(s) => AppEvent::LogLine(s),
                    None => AppEvent::Tick,
                },
                () = sleep(timeout) => AppEvent::Tick,
            };

            last_tick = Instant::now();

            match ev {
                AppEvent::Terminal(CtEvent::Key(k)) => {
                    if let Some(msg) = translate(&model, k) {
                        self.run_update(&mut model, msg).await;
                    }
                }
                AppEvent::Terminal(_) => {}
                AppEvent::Queue(qe) => {
                    self.handle_queue_event(&mut model, qe).await;
                    self.reload_tasks_runs(&mut model).await;
                    let running = self.ctx.engine.running().await;
                    self.run_update(&mut model, Msg::SetRunning(running)).await;
                }
                AppEvent::LogLine(s) => {
                    self.run_update(&mut model, Msg::AppendInspectLine(s)).await;
                }
                AppEvent::Tick => {
                    // Drop stale info/warn toasts after a few seconds.
                    // Error toasts (which may carry stderr tails) stay
                    // pinned until the user dismisses them so the
                    // failure details are never lost to a timeout.
                    if !model.toasts.is_empty() && last_tick.elapsed() > Duration::from_secs(4) {
                        model
                            .toasts
                            .retain(|t| t.level == crate::toast::ToastLevel::Error);
                    }
                }
                AppEvent::Shutdown => break,
            }
        }

        Ok(())
    }

    async fn handle_queue_event(&self, model: &mut Model, ev: QueueEvent) {
        match ev {
            QueueEvent::Enqueued { .. } | QueueEvent::Drained { .. } => {}
            QueueEvent::Started { run, task, .. } => {
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    format!(
                        "run {} started (task {})",
                        &run.to_string()[..8],
                        &task.to_string()[..8]
                    ),
                ));
            }
            QueueEvent::Finished {
                project,
                run,
                status,
                ..
            } => {
                let level = match status {
                    lattice_core::entities::TaskStatus::Succeeded => crate::toast::ToastLevel::Info,
                    _ => crate::toast::ToastLevel::Error,
                };
                let base = format!("run {} → {:?}", &run.to_string()[..8], status);
                let text = if matches!(status, lattice_core::entities::TaskStatus::Succeeded) {
                    base
                } else {
                    let tail = self.read_stderr_tail(project, run).await;
                    if tail.is_empty() {
                        format!("{base} (stderr empty)")
                    } else {
                        format!("{base}\nstderr:\n{tail}")
                    }
                };
                model.toasts.push(crate::toast::Toast::new(level, text));
            }
            QueueEvent::Interrupted { task, reason, .. } => {
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Warn,
                    format!("interrupted {}: {}", &task.to_string()[..8], reason),
                ));
            }
            QueueEvent::Paused { project, .. } | QueueEvent::Resumed { project } => {
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    format!("queue {} state changed", &project.to_string()[..8]),
                ));
            }
        }
    }

    /// Best-effort tail of the run's stderr.log. Returns up to the last
    /// 12 lines or an empty string if the file is missing.
    async fn read_stderr_tail(
        &self,
        project: lattice_core::ids::ProjectId,
        run: lattice_core::ids::RunId,
    ) -> String {
        let path = self
            .ctx
            .paths
            .run_stderr(&project.to_string(), &run.to_string());
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            return String::new();
        };
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(STDERR_TAIL_LINES);
        lines[start..].join("\n")
    }

    async fn read_run_log_tails(
        &self,
        project: lattice_core::ids::ProjectId,
        run: lattice_core::ids::RunId,
        max_lines: usize,
        max_bytes: usize,
    ) -> (Vec<String>, Vec<String>) {
        let stdout_path = self
            .ctx
            .paths
            .run_stdout(&project.to_string(), &run.to_string());
        let stderr_path = self
            .ctx
            .paths
            .run_stderr(&project.to_string(), &run.to_string());
        let stdout_tail = read_tail_lines(&stdout_path, max_lines, max_bytes).await;
        let stderr_tail = read_tail_lines(&stderr_path, max_lines, max_bytes).await;
        (stdout_tail, stderr_tail)
    }

    async fn dispatch(&self, cmd: Cmd, model: &mut Model) {
        self.run_cmd(cmd, model).await;
    }

    async fn run_update(&self, model: &mut Model, msg: Msg) {
        if let Some(cmd) = update(model, msg) {
            self.run_cmd(cmd, model).await;
        }
    }

    async fn run_cmd(&self, cmd: Cmd, model: &mut Model) {
        match cmd {
            Cmd::Dispatch(msg) => Box::pin(self.run_update(model, msg)).await,
            Cmd::SubmitForm(form) => self.submit_form(model, form).await,
            Cmd::DispatchSelected(agent_id) => {
                let Some(pid) = model.selected_project else {
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "no project selected",
                    ));
                    return;
                };
                let project_path = match model.projects.iter().find(|p| p.id == pid) {
                    Some(p) => p.path.clone(),
                    None => return,
                };
                let ids = model.task_multi_select.clone();
                let mut any_ok = false;
                if ids.is_empty() {
                    let task_id = model
                        .tasks_for_selected_project()
                        .get(model.task_cursor)
                        .map(|t| t.id);
                    let Some(t) = task_id else {
                        return;
                    };
                    any_ok |= self
                        .enqueue_task(model, pid, project_path.clone(), t, agent_id.clone())
                        .await;
                } else {
                    for id in ids {
                        any_ok |= self
                            .enqueue_task(model, pid, project_path.clone(), id, agent_id.clone())
                            .await;
                    }
                    model.task_multi_select.clear();
                }
                // Jump to the Runtime tab on success so the user sees
                // the agent boot + stream logs in real time, instead of
                // staring at "Queued" on the Tasks list with no further
                // feedback. If every enqueue failed, stay put so the
                // error toast is read where it was raised.
                if any_ok {
                    model.prev_screen = model.screen;
                    model.screen = crate::model::Screen::Runtime;
                }
            }
            Cmd::DeleteProject(id) => match self.ctx.projects.delete(id).await {
                Ok(()) => {
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Info,
                        "project deleted",
                    ));
                    self.reload_projects(model).await;
                }
                Err(e) => model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Error,
                    format!("delete project: {e}"),
                )),
            },
            Cmd::DeleteTemplate(id) => match self.ctx.templates.delete(id).await {
                Ok(()) => {
                    self.reload_templates(model).await;
                }
                Err(e) => model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Error,
                    format!("delete template: {e}"),
                )),
            },
            Cmd::DeleteTask(pid, tid) => match self.ctx.tasks.delete(pid, tid).await {
                Ok(()) => {
                    self.reload_tasks_runs(model).await;
                }
                Err(e) => model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Error,
                    format!("delete task: {e}"),
                )),
            },
            Cmd::KillRun(id) => {
                let ok = self.ctx.engine.kill_run(id).await;
                let lvl = if ok {
                    crate::toast::ToastLevel::Info
                } else {
                    crate::toast::ToastLevel::Warn
                };
                model.toasts.push(crate::toast::Toast::new(
                    lvl,
                    if ok {
                        "kill sent".to_string()
                    } else {
                        "no such run".to_string()
                    },
                ));
            }
            Cmd::SubscribeRunLogs(run_id) => {
                // Best-effort tail: look up the handle in running and
                // subscribe to its broadcast.
                if let Some(r) = self
                    .ctx
                    .engine
                    .running()
                    .await
                    .into_iter()
                    .find(|r| r.run_id == run_id)
                {
                    self.spawn_log_tail(&r);
                } else {
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "run is not currently live",
                    ));
                }
            }
            Cmd::LoadHistoryLogs(run_id) => {
                let Some(pid) = model.selected_project else {
                    return;
                };
                let (stdout_tail, stderr_tail) = self
                    .read_run_log_tails(pid, run_id, HISTORY_TAIL_LINES, HISTORY_TAIL_MAX_BYTES)
                    .await;
                let _ = update(
                    model,
                    Msg::SetHistoryLogs {
                        run_id,
                        stdout_tail,
                        stderr_tail,
                    },
                );
            }
        }
    }

    /// Returns `true` when the task actually entered the queue. The
    /// caller uses that flag to decide whether to surface additional
    /// UI (e.g. jumping to the Runtime screen). Toasts are pushed here
    /// for both outcomes so success/failure is always visible.
    async fn enqueue_task(
        &self,
        model: &mut Model,
        project_id: ProjectId,
        project_path: std::path::PathBuf,
        task_id: lattice_core::ids::TaskId,
        agent_id: lattice_core::ids::AgentId,
    ) -> bool {
        match self
            .ctx
            .engine
            .enqueue(EnqueueRequest {
                project_id,
                project_path,
                task_id,
                agent_id: agent_id.clone(),
            })
            .await
        {
            Ok(()) => {
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    format!(
                        "enqueued {} → {} (watch Runtime tab)",
                        &task_id.to_string()[..8],
                        agent_id
                    ),
                ));
                true
            }
            Err(e) => {
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Error,
                    format!("enqueue: {e}"),
                ));
                false
            }
        }
    }

    async fn submit_form(&self, model: &mut Model, form: FormState) {
        // Keep a copy of the typed data so we can restore the form
        // (with the user's input intact) on any validation or save
        // failure. Without this, a missing field silently closes the
        // form and the user has to retype everything.
        let backup = form.clone();
        match form.submit {
            FormSubmit::CreateProject => {
                let name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                let path = form
                    .fields
                    .get(1)
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                let desc = form
                    .fields
                    .get(2)
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                if name.trim().is_empty() || path.trim().is_empty() {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "name and path are required",
                    ));
                    return;
                }
                let mut p = Project::new(name, std::path::PathBuf::from(path), Timestamp::now());
                p.description = desc;
                tracing::debug!(name = %p.name, path = %p.path.display(), "submit_form: CreateProject");
                match self.ctx.projects.save(&p).await {
                    Ok(()) => {
                        model.toasts.push(crate::toast::Toast::new(
                            crate::toast::ToastLevel::Info,
                            format!("project \"{}\" created", p.name),
                        ));
                        self.reload_projects(model).await;
                    }
                    Err(e) => {
                        model.form = Some(backup);
                        model.toasts.push(crate::toast::Toast::new(
                            crate::toast::ToastLevel::Error,
                            format!("save project: {e}"),
                        ));
                    }
                }
            }
            FormSubmit::EditProject(id) => {
                let Some(mut p) = self.ctx.projects.load(id).await.ok().flatten() else {
                    return;
                };
                p.name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or(p.name);
                p.path = form
                    .fields
                    .get(1)
                    .map(|f| std::path::PathBuf::from(&f.value))
                    .unwrap_or(p.path);
                p.description = form
                    .fields
                    .get(2)
                    .map(|f| f.value.clone())
                    .unwrap_or(p.description);
                p.updated_at = Timestamp::now();
                if let Err(e) = self.ctx.projects.save(&p).await {
                    warn!("save project: {e}");
                }
                self.reload_projects(model).await;
            }
            FormSubmit::CreateTemplate => {
                let name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                let fields_src = form
                    .fields
                    .get(1)
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                let jinja = form
                    .fields
                    .get(2)
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                if name.trim().is_empty() {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "name is required",
                    ));
                    return;
                }
                if jinja.trim().is_empty() {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "prompt jinja is required",
                    ));
                    return;
                }
                let parsed_fields = match crate::model::parse_fields_toml(&fields_src) {
                    Ok(v) => v,
                    Err(e) => {
                        model.form = Some(backup);
                        model.toasts.push(crate::toast::Toast::new(
                            crate::toast::ToastLevel::Error,
                            format!("fields TOML: {e}"),
                        ));
                        return;
                    }
                };
                let mut t = Template::new(name, Timestamp::now());
                t.fields = parsed_fields;
                t.prompt.template = jinja;
                if let Err(e) = self.ctx.templates.save(&t).await {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        format!("save template: {e}"),
                    ));
                    return;
                }
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    format!(
                        "template \"{}\" created ({} fields)",
                        t.name,
                        t.fields.len()
                    ),
                ));
                self.reload_templates(model).await;
            }
            FormSubmit::EditTemplate(id) => {
                let Some(mut t) = self.ctx.templates.load(id).await.ok().flatten() else {
                    return;
                };
                t.name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or(t.name);
                let fields_src = form
                    .fields
                    .get(1)
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                match crate::model::parse_fields_toml(&fields_src) {
                    Ok(v) => t.fields = v,
                    Err(e) => {
                        model.form = Some(backup);
                        model.toasts.push(crate::toast::Toast::new(
                            crate::toast::ToastLevel::Error,
                            format!("fields TOML: {e}"),
                        ));
                        return;
                    }
                }
                let jinja = form
                    .fields
                    .get(2)
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                if jinja.trim().is_empty() {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "prompt jinja is required",
                    ));
                    return;
                }
                t.prompt.template = jinja;
                t.version += 1;
                t.updated_at = Timestamp::now();
                if let Err(e) = self.ctx.templates.save(&t).await {
                    warn!("save template: {e}");
                }
                self.reload_templates(model).await;
            }
            FormSubmit::CreateTask(pid, tid) => {
                let name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                if name.trim().is_empty() {
                    // Park the cursor on the name row so the user
                    // immediately sees where to type. Without this the
                    // validation toast points at a field they may not
                    // even be focused on.
                    let mut restored = backup;
                    restored.cursor = 0;
                    model.form = Some(restored);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "name is required",
                    ));
                    return;
                }

                // Collect typed values from all form rows that were
                // generated from the template's fields. Rows without
                // `field_id` (like `Name`) are skipped.
                let mut values = std::collections::BTreeMap::new();
                for (idx, f) in form.fields.iter().enumerate().skip(1) {
                    let Some(id) = f.field_id.clone() else {
                        continue;
                    };
                    let kind = f.kind.unwrap_or(lattice_core::fields::FieldKind::Textarea);
                    match parse_field_value(kind, &f.value, f.required) {
                        Ok(Some(v)) => {
                            values.insert(id, v);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let mut restored = backup;
                            restored.cursor = idx;
                            model.form = Some(restored);
                            model.toasts.push(crate::toast::Toast::new(
                                crate::toast::ToastLevel::Warn,
                                format!("field `{}`: {e}", f.label),
                            ));
                            return;
                        }
                    }
                }

                let mut task = Task::new(pid, tid, 1, name, Timestamp::now());
                task.fields = values;

                let Some(project) = self.ctx.projects.load(pid).await.ok().flatten() else {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        "project not found",
                    ));
                    return;
                };
                let Some(tpl) = self.ctx.templates.load(tid).await.ok().flatten() else {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        "template not found",
                    ));
                    return;
                };
                task.template_version = tpl.version;

                let prompt =
                    match lattice_core::prompt::render(&tpl, &task, &project, Timestamp::now()) {
                        Ok(p) => p,
                        Err(e) => {
                            model.form = Some(backup);
                            // Strict undefined mode in MiniJinja (see
                            // `lattice_core::prompt::render`) means typos
                            // like `{{ foo }}` instead of
                            // `{{ task.fields.foo }}` surface here. Point
                            // the user back at the template they're using
                            // and remind them how to bail out.
                            model.toasts.push(crate::toast::Toast::new(
                                crate::toast::ToastLevel::Error,
                                format!(
                                    "render prompt (template \"{}\"): {e}\n\
                                     Fix the Prompt Jinja in the template.\n\
                                     Press Esc to dismiss.",
                                    tpl.name,
                                ),
                            ));
                            return;
                        }
                    };

                // These two sub-steps used to be silent `warn!`s; a TUI
                // user would have no way to see that a save partially
                // failed. Surface them as warn toasts so the failure is
                // actually visible, but don't abort the whole save.
                if let Err(e) = self.ctx.tasks.save_snapshot(&task, &tpl).await {
                    warn!("save_snapshot: {e}");
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        format!("snapshot save failed: {e}"),
                    ));
                }
                if let Err(e) = self.ctx.tasks.save(&task).await {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        format!("save task: {e}"),
                    ));
                    return;
                }
                if let Err(e) = self.ctx.tasks.save_prompt(&task, &prompt).await {
                    warn!("save_prompt: {e}");
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        format!("prompt save failed: {e}"),
                    ));
                }
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    format!("task \"{}\" created", task.name),
                ));
                self.reload_tasks_runs(model).await;
            }
            FormSubmit::EditTask(pid, task_id) => {
                let Some(mut task) = self.ctx.tasks.load(pid, task_id).await.ok().flatten() else {
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        "task not found",
                    ));
                    return;
                };

                let name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                if name.trim().is_empty() {
                    let mut restored = backup;
                    restored.cursor = 0;
                    model.form = Some(restored);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "name is required",
                    ));
                    return;
                }

                // Re-collect typed values (same rules as create).
                let mut values = std::collections::BTreeMap::new();
                for (idx, f) in form.fields.iter().enumerate().skip(1) {
                    let Some(id) = f.field_id.clone() else {
                        continue;
                    };
                    let kind = f.kind.unwrap_or(lattice_core::fields::FieldKind::Textarea);
                    match parse_field_value(kind, &f.value, f.required) {
                        Ok(Some(v)) => {
                            values.insert(id, v);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let mut restored = backup;
                            restored.cursor = idx;
                            model.form = Some(restored);
                            model.toasts.push(crate::toast::Toast::new(
                                crate::toast::ToastLevel::Warn,
                                format!("field `{}`: {e}", f.label),
                            ));
                            return;
                        }
                    }
                }

                task.name = name;
                task.fields = values;

                let Some(project) = self.ctx.projects.load(pid).await.ok().flatten() else {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        "project not found",
                    ));
                    return;
                };
                let Some(tpl) = self
                    .ctx
                    .templates
                    .load(task.template_id)
                    .await
                    .ok()
                    .flatten()
                else {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        "template not found",
                    ));
                    return;
                };

                // Align the task with the current template version; we only
                // have the latest template available through the store today.
                task.template_version = tpl.version;

                let prompt =
                    match lattice_core::prompt::render(&tpl, &task, &project, Timestamp::now()) {
                        Ok(p) => p,
                        Err(e) => {
                            model.form = Some(backup);
                            model.toasts.push(crate::toast::Toast::new(
                                crate::toast::ToastLevel::Error,
                                format!(
                                    "render prompt (template \"{}\"): {e}\n\
                                     Fix the Prompt Jinja in the template.\n\
                                     Press Esc to dismiss.",
                                    tpl.name,
                                ),
                            ));
                            return;
                        }
                    };

                if let Err(e) = self.ctx.tasks.save_snapshot(&task, &tpl).await {
                    warn!("save_snapshot: {e}");
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        format!("snapshot save failed: {e}"),
                    ));
                }
                if let Err(e) = self.ctx.tasks.save(&task).await {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        format!("save task: {e}"),
                    ));
                    return;
                }
                if let Err(e) = self.ctx.tasks.save_prompt(&task, &prompt).await {
                    warn!("save_prompt: {e}");
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        format!("prompt save failed: {e}"),
                    ));
                }
                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    "task updated".to_string(),
                ));
                self.reload_tasks_runs(model).await;
            }
        }
    }

    fn spawn_log_tail(&self, running: &RunningRun) {
        // The runtime screen inspects one run at a time. The caller
        // also owns a `log_tx` mpsc; we push formatted lines through
        // that so the main loop appends them via `AppEvent::LogLine`.
        let handle: RunHandle = running.handle.clone();
        let log_tx = self.log_tx.clone();
        tokio::spawn(async move {
            let mut rx = handle.subscribe();
            while let Ok(line) = rx.recv().await {
                let prefix = match line.stream {
                    LogStream::Stderr => "! ",
                    LogStream::Stdout => "  ",
                };
                if log_tx.send(format!("{prefix}{}", line.text)).await.is_err() {
                    break;
                }
            }
        });
    }

    async fn reload_all(&self, model: &mut Model) {
        self.reload_projects(model).await;
        // Once projects are loaded, `Model::ensure_selection_consistency`
        // will have picked a `selected_project` (when any exist). Load
        // that project's tasks/runs immediately so Tasks/History aren't
        // empty until the user creates something new.
        self.reload_tasks_runs(model).await;
        self.reload_templates(model).await;
        self.reload_agents(model);
        let running = self.ctx.engine.running().await;
        let _ = update(model, Msg::SetRunning(running));
    }

    async fn reload_projects(&self, model: &mut Model) {
        match self.ctx.projects.list().await {
            Ok(list) => {
                let _ = update(model, Msg::SetProjects(list));
            }
            Err(e) => warn!("projects.list: {e}"),
        }
    }
    async fn reload_templates(&self, model: &mut Model) {
        match self.ctx.templates.list().await {
            Ok(list) => {
                let _ = update(model, Msg::SetTemplates(list));
            }
            Err(e) => warn!("templates.list: {e}"),
        }
    }
    async fn reload_tasks_runs(&self, model: &mut Model) {
        let Some(pid) = model.selected_project else {
            return;
        };
        match self.ctx.tasks.list_for_project(pid).await {
            Ok(list) => {
                let _ = update(model, Msg::SetTasks(pid, list));
            }
            Err(e) => warn!("tasks.list: {e}"),
        }
        match self.ctx.runs.list_for_project(pid).await {
            Ok(list) => {
                let _ = update(model, Msg::SetRuns(pid, list));
            }
            Err(e) => warn!("runs.list: {e}"),
        }
    }
    fn reload_agents(&self, model: &mut Model) {
        let list: Vec<_> = self.ctx.registry.list().into_iter().cloned().collect();
        let _ = update(model, Msg::SetAgents(list));
    }
}

async fn read_tail_lines(
    path: &std::path::Path,
    max_lines: usize,
    max_bytes: usize,
) -> Vec<String> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };
    // Keep memory bounded for very large logs.
    let slice = if content.len() > max_bytes {
        &content[content.len() - max_bytes..]
    } else {
        &content
    };
    let mut lines: Vec<&str> = slice.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines.drain(0..start);
    lines.into_iter().map(|s| s.to_string()).collect()
}

/// Convert the string typed into a form row into the appropriate
/// `serde_json::Value` for the declared `FieldKind`. Returns
/// `Ok(None)` when the input is empty and the field is optional so
/// the task `fields` map stays minimal.
fn parse_field_value(
    kind: lattice_core::fields::FieldKind,
    raw: &str,
    required: bool,
) -> Result<Option<serde_json::Value>, String> {
    use lattice_core::fields::FieldKind;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        if required {
            return Err("required".into());
        }
        return Ok(None);
    }
    let v = match kind {
        FieldKind::Textarea | FieldKind::SequenceGram | FieldKind::Select => {
            serde_json::Value::String(raw.to_string())
        }
        FieldKind::Multiselect => serde_json::Value::Array(
            trimmed
                .split(',')
                .map(|s| serde_json::Value::String(s.trim().to_string()))
                .filter(|v| !v.as_str().unwrap_or("").is_empty())
                .collect(),
        ),
    };
    Ok(Some(v))
}
