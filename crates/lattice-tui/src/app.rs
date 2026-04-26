//! `App` — orchestrates the main loop: translates terminal events to
//! `Msg`s, folds them into the `Model`, fires the resulting `Cmd`s,
//! and redraws.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event as CtEvent, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::time::sleep;
use tracing::warn;

use lattice_core::entities::{Task, Template};
use lattice_core::time::Timestamp;

use lattice_store::fs::atomic_write_str;

use crate::context::AppContext;
use crate::event::{AppEvent, spawn_terminal_reader};
use crate::keybind::translate;
use crate::model::{Cmd, FormState, FormSubmit, Model, Msg, update};
use crate::view::render;

/// Public entry point. `run` takes ownership of the terminal and
/// restores it on drop.
pub struct App {
    ctx: AppContext,
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
        Self { ctx }
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
                () = sleep(timeout) => AppEvent::Tick,
            };

            last_tick = Instant::now();

            match ev {
                AppEvent::Terminal(CtEvent::Key(k)) => {
                    if let Some(msg) = translate(&model, k) {
                        self.run_update(&mut model, msg).await;
                    }
                }
                AppEvent::Terminal(CtEvent::Mouse(m)) => {
                    // Lightweight mouse support: wheel scroll drives the Tasks prompt preview.
                    // Only when no overlay is open so scrolling doesn't fight modals.
                    let overlays_open = model.palette_open
                        || model.confirm.is_some()
                        || model.form.is_some()
                        || model.picker.is_some()
                        || model.sequence_editor.is_some();
                    if !overlays_open
                        && matches!(model.screen, crate::model::Screen::Tasks)
                        && matches!(
                            m.kind,
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        )
                    {
                        let delta = match m.kind {
                            MouseEventKind::ScrollUp => -3,
                            MouseEventKind::ScrollDown => 3,
                            _ => 0,
                        };
                        if delta != 0 {
                            self.run_update(&mut model, crate::model::Msg::TaskPromptScroll(delta))
                                .await;
                        }
                    }
                }
                AppEvent::Terminal(_) => {}
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

    async fn dispatch(&self, cmd: Cmd, model: &mut Model) {
        self.run_cmd(cmd, model).await;
    }

    async fn run_update(&self, model: &mut Model, msg: Msg) {
        let should_clamp_task_scroll = matches!(msg, Msg::TaskPromptScroll(_));
        if let Some(cmd) = update(model, msg) {
            self.run_cmd(cmd, model).await;
        }
        if should_clamp_task_scroll {
            self.clamp_tasks_prompt_scroll(model);
        }
    }

    fn clamp_tasks_prompt_scroll(&self, model: &mut Model) {
        if !matches!(model.screen, crate::model::Screen::Tasks) {
            return;
        }
        let Some(t) = model.tasks.get(model.task_cursor) else {
            model.task_prompt_scroll = 0;
            return;
        };
        let prompt = model
            .templates
            .iter()
            .find(|tpl| tpl.id == t.template_id)
            .and_then(|tpl| lattice_core::prompt::render(tpl, t, Timestamp::now()).ok())
            .unwrap_or_else(|| {
                "Prompt preview unavailable.\n\n- Ensure the template exists.\n- Ensure the template prompt Jinja renders (no undefined vars)."
                    .to_string()
            });
        let line_count = prompt.lines().count();
        let max_scroll = line_count.saturating_sub(1);
        model.task_prompt_scroll = model.task_prompt_scroll.min(max_scroll);
    }

    async fn run_cmd(&self, cmd: Cmd, model: &mut Model) {
        match cmd {
            Cmd::Dispatch(msg) => Box::pin(self.run_update(model, msg)).await,
            Cmd::SubmitForm(form) => self.submit_form(model, form).await,
            Cmd::DeleteTemplate(id) => match self.ctx.templates.delete(id).await {
                Ok(()) => {
                    self.reload_templates(model).await;
                }
                Err(e) => model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Error,
                    format!("delete template: {e}"),
                )),
            },
            Cmd::DeleteTask(tid) => match self.ctx.tasks.delete(tid).await {
                Ok(()) => {
                    self.reload_tasks(model).await;
                }
                Err(e) => model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Error,
                    format!("delete task: {e}"),
                )),
            },
        }
    }

    async fn submit_form(&self, model: &mut Model, form: FormState) {
        // Keep a copy of the typed data so we can restore the form
        // (with the user's input intact) on any validation or save
        // failure. Without this, a missing field silently closes the
        // form and the user has to retype everything.
        let backup = form.clone();
        match form.submit {
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
            FormSubmit::CreateTask(tid) => {
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

                let mut task = Task::new(tid, 1, name, Timestamp::now());
                task.fields = values;
                let Some(tpl) = self.ctx.templates.load(tid).await.ok().flatten() else {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        "template not found",
                    ));
                    return;
                };
                task.template_version = tpl.version;

                let prompt = match lattice_core::prompt::render(&tpl, &task, Timestamp::now()) {
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
                self.reload_tasks(model).await;
            }
            FormSubmit::EditTask(task_id) => {
                let Some(mut task) = self.ctx.tasks.load(task_id).await.ok().flatten() else {
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

                let prompt = match lattice_core::prompt::render(&tpl, &task, Timestamp::now()) {
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
                self.reload_tasks(model).await;
            }
            FormSubmit::SaveTaskPromptToFile(task_id) => {
                let raw_name = form
                    .fields
                    .first()
                    .map(|f| f.value.clone())
                    .unwrap_or_default();
                let file_stem = sanitize_filename(&raw_name);
                if file_stem.is_empty() {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Warn,
                        "file name is required",
                    ));
                    return;
                }
                let file_name = if file_stem.ends_with(".md") {
                    file_stem
                } else {
                    format!("{file_stem}.md")
                };

                let cwd = match std::env::current_dir() {
                    Ok(d) => d,
                    Err(e) => {
                        model.form = Some(backup);
                        model.toasts.push(crate::toast::Toast::new(
                            crate::toast::ToastLevel::Error,
                            format!("current_dir: {e}"),
                        ));
                        return;
                    }
                };
                let dest = cwd.join(&file_name);

                // Prefer the stored rendered prompt (fast, exactly what the app rendered).
                // If missing, re-render from the latest template/task/project.
                let prompt_path = self.ctx.paths.task_prompt(&task_id.to_string());
                let prompt = match tokio::fs::read_to_string(&prompt_path).await {
                    Ok(s) if !s.trim().is_empty() => s,
                    _ => {
                        let Some(task) = self.ctx.tasks.load(task_id).await.ok().flatten() else {
                            model.form = Some(backup);
                            model.toasts.push(crate::toast::Toast::new(
                                crate::toast::ToastLevel::Error,
                                "task not found",
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
                        match lattice_core::prompt::render(&tpl, &task, Timestamp::now()) {
                            Ok(p) => p,
                            Err(e) => {
                                model.form = Some(backup);
                                model.toasts.push(crate::toast::Toast::new(
                                    crate::toast::ToastLevel::Error,
                                    format!("render prompt: {e}"),
                                ));
                                return;
                            }
                        }
                    }
                };

                // Wrap into a minimal markdown document.
                let md = format!("{prompt}\n");
                if let Err(e) = atomic_write_str(&dest, &md) {
                    model.form = Some(backup);
                    model.toasts.push(crate::toast::Toast::new(
                        crate::toast::ToastLevel::Error,
                        format!("write {}: {e}", dest.display()),
                    ));
                    return;
                }

                model.toasts.push(crate::toast::Toast::new(
                    crate::toast::ToastLevel::Info,
                    format!("wrote {}", dest.display()),
                ));
            }
        }
    }

    async fn reload_all(&self, model: &mut Model) {
        self.reload_tasks(model).await;
        self.reload_templates(model).await;
    }
    async fn reload_templates(&self, model: &mut Model) {
        match self.ctx.templates.list().await {
            Ok(list) => {
                let _ = update(model, Msg::SetTemplates(list));
            }
            Err(e) => warn!("templates.list: {e}"),
        }
    }
    async fn reload_tasks(&self, model: &mut Model) {
        match self.ctx.tasks.list().await {
            Ok(list) => {
                let _ = update(model, Msg::SetTasks(list));
            }
            Err(e) => warn!("tasks.list: {e}"),
        }
    }
}

fn sanitize_filename(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Keep it strictly within the current directory: strip path separators
    // and collapse unsafe characters.
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if matches!(ch, '/' | '\\') {
            continue;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
        // else: drop
    }
    let out = out.trim_matches('.').trim_matches('_').to_string();
    // Prevent empty or dotfiles from being generated accidentally.
    if out.is_empty() {
        return String::new();
    }
    out
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
