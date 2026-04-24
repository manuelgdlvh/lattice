//! UI model, `Msg`, and the pure `update` function.
//!
//! This module deliberately depends only on the domain types — it
//! never touches crossterm or ratatui. That lets us test state
//! transitions without a terminal.

use std::collections::BTreeMap;

use lattice_agents::{AvailableAgent, RunningRun};
use lattice_core::entities::{Project, Run, Task, TaskStatus, Template};
use lattice_core::fields::FieldKind;
use lattice_core::ids::{AgentId, ProjectId, RunId, TaskId, TemplateId};

use crate::toast::{Toast, ToastLevel};

/// Top-level screens, in tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Projects,
    Templates,
    Tasks,
    Runtime,
    History,
    Info,
    Help,
}

impl Screen {
    pub const TABS: &'static [Screen] = &[
        Screen::Projects,
        Screen::Templates,
        Screen::Tasks,
        Screen::Runtime,
        Screen::History,
        Screen::Info,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Templates => "Templates",
            Self::Tasks => "Tasks",
            Self::Runtime => "Runtime",
            Self::History => "History",
            Self::Info => "Info",
            Self::Help => "Help",
        }
    }
}

/// All UI state, rendered on every frame.
#[derive(Clone, Debug)]
pub struct Model {
    pub screen: Screen,
    pub prev_screen: Screen,
    pub quitting: bool,

    /// Currently-selected project, the "focus" for Tasks/Runtime/History.
    pub selected_project: Option<ProjectId>,
    pub selected_template: Option<TemplateId>,

    pub projects: Vec<Project>,
    pub project_cursor: usize,

    pub templates: Vec<Template>,
    pub template_cursor: usize,

    /// All loaded tasks indexed by project, flattened for the Tasks
    /// screen's filtered view.
    pub tasks_by_project: BTreeMap<ProjectId, Vec<Task>>,
    pub task_cursor: usize,
    pub task_multi_select: Vec<TaskId>,

    pub runs_by_project: BTreeMap<ProjectId, Vec<Run>>,
    pub run_cursor: usize,
    /// Cached tails for the currently-highlighted run on the History screen.
    /// Populated by `Cmd::LoadHistoryLogs`.
    pub history_run: Option<RunId>,
    pub history_stdout_tail: Vec<String>,
    pub history_stderr_tail: Vec<String>,

    pub available_agents: Vec<AvailableAgent>,

    /// Currently-running runs keyed by `RunId`.
    pub running: BTreeMap<RunId, RunningRun>,
    pub runtime_cursor: usize,

    /// Live tail buffer for the inspected run on the Runtime screen.
    pub inspect_run: Option<RunId>,
    pub inspect_tail: Vec<String>,

    pub toasts: Vec<Toast>,
    pub status_message: Option<String>,

    pub palette_open: bool,
    pub palette_input: String,
    pub palette_cursor: usize,

    pub confirm: Option<ConfirmPrompt>,
    pub form: Option<FormState>,
    /// Generic modal list overlay. Used for the template picker during
    /// task creation, the project picker on the Tasks screen, and the
    /// agent picker during dispatch. Each item carries its own accept
    /// message so the picker itself is message-agnostic.
    pub picker: Option<Picker>,
    /// Modal sequence diagram editor for `sequence-gram` fields.
    pub sequence_editor: Option<SequenceEditorState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceEditorState {
    /// Index into `form.fields` we are editing.
    pub form_field_index: usize,
    /// Multiple diagrams inside one field.
    pub diagrams: Vec<SequenceDiagram>,
    /// Selected diagram.
    pub diagram_cursor: usize,
    /// Selected event within the current diagram.
    pub event_cursor: usize,
    /// Selected participant within the current diagram.
    pub participant_cursor: usize,
    pub mode: SequenceEditorMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceDiagram {
    pub name: String,
    pub participants: Vec<String>,
    pub events: Vec<SequenceEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceEditorMode {
    Browse,
    AddParticipant {
        input: String,
    },
    RenameDiagram {
        input: String,
    },
    AddDiagram {
        input: String,
    },
    AddMessage {
        from: usize,
        to: usize,
        dashed: bool,
        input: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceEvent {
    Message {
        from: String,
        to: String,
        dashed: bool,
        text: String,
    },
}

/// Modal list overlay: labels + per-item accept `Msg`. Accepting runs
/// `items[cursor].accept`; cancelling just drops the picker.
#[derive(Clone, Debug)]
pub struct Picker {
    pub title: String,
    pub items: Vec<PickerItem>,
    pub cursor: usize,
}

#[derive(Clone, Debug)]
pub struct PickerItem {
    pub label: String,
    pub accept: Msg,
}

/// Generic yes/no confirmation prompt.
#[derive(Clone, Debug)]
pub struct ConfirmPrompt {
    pub title: String,
    pub message: String,
    /// The `Msg` dispatched if the user accepts.
    pub accept: Box<Msg>,
}

/// Simple form state used across screens for add/edit flows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormState {
    pub title: String,
    pub fields: Vec<FormField>,
    pub cursor: usize,
    pub submit: FormSubmit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormField {
    pub label: String,
    pub value: String,
    pub required: bool,
    pub multiline: bool,
    /// Byte offset of the insertion caret inside `value`. Always on a
    /// UTF-8 character boundary and always `<= value.len()`. Mutations
    /// go through [`Self::set_caret`] so we can't wedge this invariant.
    pub caret: usize,
    /// When set, this form row represents a template-declared field
    /// whose typed value should be serialized into `task.fields` on
    /// submit. `field_id` is the key used in that map.
    pub field_id: Option<String>,
    /// Kind of the template-declared field, used both to render a
    /// type hint next to the label and to parse `value` into the
    /// correct `serde_json::Value` shape at submit time.
    pub kind: Option<FieldKind>,
    /// Optional list of allowed option ids for `select` / `multiselect`
    /// — shown as a hint under the field label.
    pub options: Vec<String>,
}

impl FormField {
    pub fn plain(
        label: impl Into<String>,
        value: impl Into<String>,
        required: bool,
        multiline: bool,
    ) -> Self {
        let value = value.into();
        let caret = value.len();
        Self {
            label: label.into(),
            value,
            required,
            multiline,
            caret,
            field_id: None,
            kind: None,
            options: Vec::new(),
        }
    }

    /// Clamp `pos` to the nearest valid UTF-8 boundary in `value`.
    pub fn set_caret(&mut self, pos: usize) {
        let pos = pos.min(self.value.len());
        let mut snapped = pos;
        while snapped > 0 && !self.value.is_char_boundary(snapped) {
            snapped -= 1;
        }
        self.caret = snapped;
    }

    /// Insert `c` at the current caret and advance past it.
    pub fn insert_char(&mut self, c: char) {
        let pos = self.caret.min(self.value.len());
        self.value.insert(pos, c);
        self.caret = pos + c.len_utf8();
    }

    /// Delete the character before the caret (backspace). No-op at
    /// position 0.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let mut start = self.caret - 1;
        while start > 0 && !self.value.is_char_boundary(start) {
            start -= 1;
        }
        self.value.drain(start..self.caret);
        self.caret = start;
    }

    /// Move caret one grapheme (really: one scalar) left.
    pub fn caret_left(&mut self) {
        if self.caret == 0 {
            return;
        }
        let mut pos = self.caret - 1;
        while pos > 0 && !self.value.is_char_boundary(pos) {
            pos -= 1;
        }
        self.caret = pos;
    }

    /// Move caret one grapheme (really: one scalar) right.
    pub fn caret_right(&mut self) {
        if self.caret >= self.value.len() {
            return;
        }
        let mut pos = self.caret + 1;
        while pos < self.value.len() && !self.value.is_char_boundary(pos) {
            pos += 1;
        }
        self.caret = pos;
    }

    /// Move caret to the start of the current logical line.
    pub fn caret_home(&mut self) {
        self.caret = self.value[..self.caret].rfind('\n').map_or(0, |p| p + 1);
    }

    /// Move caret to the end of the current logical line.
    pub fn caret_end(&mut self) {
        let rest = &self.value[self.caret..];
        self.caret += rest.find('\n').unwrap_or(rest.len());
    }

    /// Move caret to the previous line at (roughly) the same column.
    pub fn caret_up(&mut self) {
        let (col, line_start) = self.current_col_and_line_start();
        if line_start == 0 {
            self.caret = 0;
            return;
        }
        let prev_line_end = line_start - 1; // the `\n` before line_start
        let prev_line_start = self.value[..prev_line_end].rfind('\n').map_or(0, |p| p + 1);
        self.caret = column_to_offset(&self.value, prev_line_start, prev_line_end, col);
    }

    /// Move caret to the next line at (roughly) the same column.
    pub fn caret_down(&mut self) {
        let (col, line_start) = self.current_col_and_line_start();
        let line_end = self.value[line_start..]
            .find('\n')
            .map_or(self.value.len(), |p| line_start + p);
        if line_end == self.value.len() {
            self.caret = self.value.len();
            return;
        }
        let next_line_start = line_end + 1;
        let next_line_end = self.value[next_line_start..]
            .find('\n')
            .map_or(self.value.len(), |p| next_line_start + p);
        self.caret = column_to_offset(&self.value, next_line_start, next_line_end, col);
    }

    /// `(visual_column_in_chars, byte_offset_of_current_line_start)`.
    fn current_col_and_line_start(&self) -> (usize, usize) {
        let line_start = self.value[..self.caret].rfind('\n').map_or(0, |p| p + 1);
        let col = self.value[line_start..self.caret].chars().count();
        (col, line_start)
    }
}

/// Return the byte offset inside `s` that lands at `target_col`
/// characters into the slice `s[line_start..line_end]`, clamped to the
/// line's actual length (so cursoring "up" into a shorter line lands
/// at end-of-line instead of past it).
fn column_to_offset(s: &str, line_start: usize, line_end: usize, target_col: usize) -> usize {
    let line = &s[line_start..line_end];
    let mut off = line_start;
    for (i, (byte_idx, _)) in line.char_indices().enumerate() {
        if i == target_col {
            return line_start + byte_idx;
        }
        off = line_start + byte_idx + line[byte_idx..].chars().next().map_or(0, char::len_utf8);
    }
    off
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormSubmit {
    CreateProject,
    EditProject(ProjectId),
    CreateTemplate,
    EditTemplate(TemplateId),
    CreateTask(ProjectId, TemplateId),
    EditTask(ProjectId, TaskId),
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            screen: Screen::Projects,
            prev_screen: Screen::Projects,
            quitting: false,
            selected_project: None,
            selected_template: None,
            projects: Vec::new(),
            project_cursor: 0,
            templates: Vec::new(),
            template_cursor: 0,
            tasks_by_project: BTreeMap::new(),
            task_cursor: 0,
            task_multi_select: Vec::new(),
            runs_by_project: BTreeMap::new(),
            run_cursor: 0,
            history_run: None,
            history_stdout_tail: Vec::new(),
            history_stderr_tail: Vec::new(),
            available_agents: Vec::new(),
            running: BTreeMap::new(),
            runtime_cursor: 0,
            inspect_run: None,
            inspect_tail: Vec::new(),
            toasts: Vec::new(),
            status_message: None,
            palette_open: false,
            palette_input: String::new(),
            palette_cursor: 0,
            confirm: None,
            form: None,
            picker: None,
            sequence_editor: None,
        }
    }

    pub fn tasks_for_selected_project(&self) -> &[Task] {
        self.selected_project
            .and_then(|p| self.tasks_by_project.get(&p).map(Vec::as_slice))
            .unwrap_or(&[])
    }

    pub fn runs_for_selected_project(&self) -> &[Run] {
        self.selected_project
            .and_then(|p| self.runs_by_project.get(&p).map(Vec::as_slice))
            .unwrap_or(&[])
    }

    /// If a project is selected on the Projects screen, pick it up.
    pub fn ensure_selection_consistency(&mut self) {
        if self.selected_project.is_none()
            && !self.projects.is_empty()
            && self.project_cursor < self.projects.len()
        {
            self.selected_project = Some(self.projects[self.project_cursor].id);
        }
        if let Some(pid) = self.selected_project
            && !self.projects.iter().any(|p| p.id == pid)
        {
            self.selected_project = self.projects.first().map(|p| p.id);
        }
        self.project_cursor = self
            .project_cursor
            .min(self.projects.len().saturating_sub(1));
        self.template_cursor = self
            .template_cursor
            .min(self.templates.len().saturating_sub(1));
    }
}

/// UI messages. Anything that mutates `Model` goes through this type.
/// The naming is `VerbNoun` to keep `match` arms scannable.
///
/// Note: we don't derive `PartialEq` because some payloads (tasks,
/// runs, templates, running runs) carry floats, JSON values, and
/// process handles. Tests that need value comparison use `matches!`
/// on the variant shape instead.
#[derive(Clone, Debug)]
pub enum Msg {
    // global -----------------------------------------------------------
    Quit,
    GoScreen(Screen),
    NextTab,
    PrevTab,
    ShowHelp,
    PalToggle,
    PalInput(char),
    PalBackspace,
    PalAccept,
    PalMove(isize),
    DismissToast,
    AckConfirm,
    CancelConfirm,

    // data snapshot updates -------------------------------------------
    SetProjects(Vec<Project>),
    SetTemplates(Vec<Template>),
    SetTasks(ProjectId, Vec<Task>),
    SetRuns(ProjectId, Vec<Run>),
    SetAgents(Vec<AvailableAgent>),
    SetRunning(Vec<RunningRun>),
    AppendInspectLine(String),
    /// Replace cached History tails for `run_id`.
    SetHistoryLogs {
        run_id: RunId,
        stdout_tail: Vec<String>,
        stderr_tail: Vec<String>,
    },
    ToastInfo(String),
    ToastWarn(String),
    ToastError(String),

    // projects --------------------------------------------------------
    ProjectCursor(isize),
    SelectProject(ProjectId),
    /// Convenience: set `selected_project` and jump to the Tasks tab,
    /// with a confirmation toast. Emitted from the Projects screen's
    /// `Enter` key so users have a one-step way to pick the target
    /// project their subsequent actions apply to.
    SelectAndGoToTasks(ProjectId),
    /// Open a picker to change the currently-selected project from
    /// anywhere (today: the Tasks screen's `p` key).
    OpenProjectPicker,
    OpenCreateProject,
    OpenEditProject(ProjectId),
    DeleteProject(ProjectId),

    // templates -------------------------------------------------------
    TemplateCursor(isize),
    OpenCreateTemplate,
    OpenEditTemplate(TemplateId),
    DeleteTemplate(TemplateId),

    // tasks -----------------------------------------------------------
    TaskCursor(isize),
    ToggleTaskSelection(TaskId),
    ClearTaskSelection,
    OpenCreateTask,
    /// Open the task-creation form for a specific template, bypassing
    /// the picker. Used both as the picker's accept message and from
    /// the Templates screen where a template is already in focus.
    OpenCreateTaskWith(TemplateId),
    OpenEditTask(ProjectId, TaskId),
    // Generic picker overlay. Items each carry their own accept Msg,
    // so the picker itself is reusable for templates, projects, agents,
    // etc.
    PickerMove(isize),
    PickerAccept,
    PickerCancel,
    DeleteTask(ProjectId, TaskId),
    /// User-initiated dispatch ("x" on the Tasks screen). Branches on
    /// what's available: no task selected → warn toast, no installed
    /// agent → actionable toast, 1 agent → dispatch directly, >1
    /// agent → open the agent picker.
    RequestDispatch,
    DispatchSelected(AgentId),

    // forms -----------------------------------------------------------
    FormInputChar(char),
    FormBackspace,
    FormNext,
    FormPrev,
    FormCaretLeft,
    FormCaretRight,
    FormCaretUp,
    FormCaretDown,
    FormCaretHome,
    FormCaretEnd,
    FormSubmit,
    FormCancel,

    // sequence-gram editor --------------------------------------------
    OpenSequenceEditor,
    SeqEdCancel,
    SeqEdSave,
    SeqEdMove(isize),
    SeqEdMoveParticipant(isize),
    SeqEdMoveDiagram(isize),
    SeqEdInputChar(char),
    SeqEdBackspace,
    SeqEdStartAddParticipant,
    SeqEdStartAddMessage,
    SeqEdToggleDashed,
    SeqEdCycleFrom(isize),
    SeqEdCycleTo(isize),
    SeqEdConfirm,
    SeqEdDeleteEvent,
    SeqEdDeleteParticipant,
    SeqEdStartAddDiagram,
    SeqEdStartRenameDiagram,
    SeqEdDeleteDiagram,

    // runtime ---------------------------------------------------------
    RuntimeCursor(isize),
    InspectRun(RunId),
    KillRun(RunId),

    // history ---------------------------------------------------------
    RunCursor(isize),

    // results from async actions --------------------------------------
    ConfirmDeleteProject(ProjectId),
    ConfirmDeleteTemplate(TemplateId),
    ConfirmDeleteTask(ProjectId, TaskId),
    ConfirmKill(RunId),
}

/// Pure state transition. No I/O. Returns an optional "command" the
/// caller should schedule as an async action (loads, saves, etc.).
pub fn update(model: &mut Model, msg: Msg) -> Option<Cmd> {
    match msg {
        Msg::Quit => {
            model.quitting = true;
            None
        }
        Msg::GoScreen(s) => {
            model.prev_screen = model.screen;
            model.screen = s;
            if matches!(model.screen, Screen::History) {
                let run_id = model
                    .runs_for_selected_project()
                    .get(model.run_cursor)
                    .map(|r| r.id);
                run_id.map(Cmd::LoadHistoryLogs)
            } else {
                None
            }
        }
        Msg::NextTab => {
            let idx = Screen::TABS
                .iter()
                .position(|s| *s == model.screen)
                .unwrap_or(0);
            model.prev_screen = model.screen;
            model.screen = Screen::TABS[(idx + 1) % Screen::TABS.len()];
            None
        }
        Msg::PrevTab => {
            let idx = Screen::TABS
                .iter()
                .position(|s| *s == model.screen)
                .unwrap_or(0);
            model.prev_screen = model.screen;
            model.screen = Screen::TABS[(idx + Screen::TABS.len() - 1) % Screen::TABS.len()];
            None
        }
        Msg::ShowHelp => {
            model.prev_screen = model.screen;
            model.screen = Screen::Help;
            None
        }
        Msg::PalToggle => {
            model.palette_open = !model.palette_open;
            if !model.palette_open {
                model.palette_input.clear();
                model.palette_cursor = 0;
            }
            None
        }
        Msg::PalInput(c) => {
            if model.palette_open {
                model.palette_input.push(c);
            }
            None
        }
        Msg::PalBackspace => {
            if model.palette_open {
                model.palette_input.pop();
            }
            None
        }
        Msg::PalMove(d) => {
            if model.palette_open {
                model.palette_cursor = apply_delta(model.palette_cursor, d, usize::MAX);
            }
            None
        }
        Msg::PalAccept => {
            let action = crate::palette::resolve(&model.palette_input, model.palette_cursor);
            model.palette_open = false;
            model.palette_input.clear();
            model.palette_cursor = 0;
            action.map(Cmd::Dispatch)
        }
        Msg::DismissToast => {
            if !model.toasts.is_empty() {
                model.toasts.remove(0);
            }
            None
        }
        Msg::AckConfirm => {
            if let Some(p) = model.confirm.take() {
                return Some(Cmd::Dispatch(*p.accept));
            }
            None
        }
        Msg::CancelConfirm => {
            model.confirm = None;
            None
        }

        Msg::SetProjects(v) => {
            model.projects = v;
            model.ensure_selection_consistency();
            None
        }
        Msg::SetTemplates(v) => {
            model.templates = v;
            model.ensure_selection_consistency();
            None
        }
        Msg::SetTasks(pid, v) => {
            model.tasks_by_project.insert(pid, v);
            None
        }
        Msg::SetRuns(pid, v) => {
            model.runs_by_project.insert(pid, v);
            // Clamp cursor after refresh; if History is visible, also refresh tails.
            let len = model.runs_for_selected_project().len();
            model.run_cursor = model.run_cursor.min(len.saturating_sub(1));
            if matches!(model.screen, Screen::History) {
                let run_id = model
                    .runs_for_selected_project()
                    .get(model.run_cursor)
                    .map(|r| r.id);
                return run_id.map(Cmd::LoadHistoryLogs);
            }
            None
        }
        Msg::SetAgents(v) => {
            model.available_agents = v;
            None
        }
        Msg::SetRunning(list) => {
            model.running = list.into_iter().map(|r| (r.run_id, r)).collect();
            None
        }
        Msg::AppendInspectLine(line) => {
            model.inspect_tail.push(line);
            // Cap the in-memory tail to something sane.
            if model.inspect_tail.len() > 5_000 {
                let drop = model.inspect_tail.len() - 5_000;
                model.inspect_tail.drain(0..drop);
            }
            None
        }
        Msg::SetHistoryLogs {
            run_id,
            stdout_tail,
            stderr_tail,
        } => {
            model.history_run = Some(run_id);
            model.history_stdout_tail = stdout_tail;
            model.history_stderr_tail = stderr_tail;
            None
        }
        Msg::ToastInfo(t) => {
            model.toasts.push(Toast::new(ToastLevel::Info, t));
            None
        }
        Msg::ToastWarn(t) => {
            model.toasts.push(Toast::new(ToastLevel::Warn, t));
            None
        }
        Msg::ToastError(t) => {
            model.toasts.push(Toast::new(ToastLevel::Error, t));
            None
        }

        Msg::ProjectCursor(d) => {
            move_cursor(&mut model.project_cursor, d, model.projects.len());
            if let Some(p) = model.projects.get(model.project_cursor) {
                model.selected_project = Some(p.id);
            }
            None
        }
        Msg::SelectProject(id) => {
            model.selected_project = Some(id);
            None
        }
        Msg::SelectAndGoToTasks(id) => {
            model.selected_project = Some(id);
            let name = model
                .projects
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone());
            model.prev_screen = model.screen;
            model.screen = Screen::Tasks;
            if let Some(name) = name {
                model.toasts.push(Toast::new(
                    ToastLevel::Info,
                    format!("targeting project \"{name}\""),
                ));
            }
            None
        }
        Msg::OpenProjectPicker => {
            if model.projects.is_empty() {
                model.toasts.push(Toast::new(
                    ToastLevel::Warn,
                    "no projects yet (press n on the Projects tab)",
                ));
                return None;
            }
            let current = model.selected_project;
            let items: Vec<PickerItem> = model
                .projects
                .iter()
                .map(|p| {
                    let tag = if Some(p.id) == current { "● " } else { "  " };
                    PickerItem {
                        label: format!("{tag}{}  ({})", p.name, p.path.to_string_lossy()),
                        accept: Msg::SelectAndGoToTasks(p.id),
                    }
                })
                .collect();
            let cursor = current
                .and_then(|id| model.projects.iter().position(|p| p.id == id))
                .unwrap_or(model.project_cursor.min(items.len().saturating_sub(1)));
            model.picker = Some(Picker {
                title: "Pick target project".into(),
                items,
                cursor,
            });
            None
        }
        Msg::OpenCreateProject => {
            model.form = Some(FormState {
                title: "New project".into(),
                fields: vec![
                    FormField::plain("Name", "", true, false),
                    FormField::plain("Path", "", true, false),
                    FormField::plain("Description", "", false, true),
                ],
                cursor: 0,
                submit: FormSubmit::CreateProject,
            });
            None
        }
        Msg::OpenEditProject(id) => {
            let proj = model.projects.iter().find(|p| p.id == id).cloned()?;
            model.form = Some(FormState {
                title: format!("Edit project: {}", proj.name),
                fields: vec![
                    FormField::plain("Name", proj.name.clone(), true, false),
                    FormField::plain(
                        "Path",
                        proj.path.to_string_lossy().into_owned(),
                        true,
                        false,
                    ),
                    FormField::plain("Description", proj.description.clone(), false, true),
                ],
                cursor: 0,
                submit: FormSubmit::EditProject(id),
            });
            None
        }
        Msg::DeleteProject(id) => {
            let name = model
                .projects
                .iter()
                .find(|p| p.id == id)
                .map_or(String::from("<unknown>"), |p| p.name.clone());
            model.confirm = Some(ConfirmPrompt {
                title: "Delete project?".into(),
                message: format!(
                    "This will delete project `{name}` and all its tasks and runs. Proceed?"
                ),
                accept: Box::new(Msg::ConfirmDeleteProject(id)),
            });
            None
        }

        Msg::TemplateCursor(d) => {
            move_cursor(&mut model.template_cursor, d, model.templates.len());
            if let Some(t) = model.templates.get(model.template_cursor) {
                model.selected_template = Some(t.id);
            }
            None
        }
        Msg::OpenCreateTemplate => {
            model.form = Some(FormState {
                title: "New template".into(),
                fields: vec![
                    FormField::plain("Name", "", true, false),
                    FormField::plain("Context (markdown)", "", false, true),
                    FormField::plain(
                        "Fields (TOML — one [[fields]] block per field)",
                        default_fields_toml_hint(),
                        false,
                        true,
                    ),
                    FormField::plain(
                        "Prompt Jinja (optional — leave empty for canonical skeleton)",
                        String::new(),
                        false,
                        true,
                    ),
                ],
                cursor: 0,
                submit: FormSubmit::CreateTemplate,
            });
            None
        }
        Msg::OpenEditTemplate(id) => {
            let tpl = model.templates.iter().find(|t| t.id == id).cloned()?;
            let fields_toml = fields_to_toml(&tpl.fields);
            model.form = Some(FormState {
                title: format!("Edit template: {}", tpl.name),
                fields: vec![
                    FormField::plain("Name", tpl.name, true, false),
                    FormField::plain(
                        "Context (markdown)",
                        tpl.preamble.markdown.clone(),
                        false,
                        true,
                    ),
                    FormField::plain(
                        "Fields (TOML — one [[fields]] block per field)",
                        fields_toml,
                        false,
                        true,
                    ),
                    FormField::plain(
                        "Prompt Jinja (optional — leave empty for canonical skeleton)",
                        tpl.prompt.template.clone().unwrap_or_default(),
                        false,
                        true,
                    ),
                ],
                cursor: 0,
                submit: FormSubmit::EditTemplate(id),
            });
            None
        }
        Msg::DeleteTemplate(id) => {
            let name = model
                .templates
                .iter()
                .find(|t| t.id == id)
                .map_or(String::from("<unknown>"), |t| t.name.clone());
            model.confirm = Some(ConfirmPrompt {
                title: "Delete template?".into(),
                message: format!("Delete template `{name}`?"),
                accept: Box::new(Msg::ConfirmDeleteTemplate(id)),
            });
            None
        }

        Msg::TaskCursor(d) => {
            let len = model.tasks_for_selected_project().len();
            move_cursor(&mut model.task_cursor, d, len);
            None
        }
        Msg::ToggleTaskSelection(id) => {
            if let Some(pos) = model.task_multi_select.iter().position(|x| *x == id) {
                model.task_multi_select.remove(pos);
            } else {
                model.task_multi_select.push(id);
            }
            None
        }
        Msg::ClearTaskSelection => {
            model.task_multi_select.clear();
            None
        }
        Msg::OpenCreateTask => {
            let Some(pid) = model.selected_project else {
                model
                    .toasts
                    .push(Toast::new(ToastLevel::Warn, "select a project first"));
                return None;
            };
            if model.templates.is_empty() {
                model
                    .toasts
                    .push(Toast::new(ToastLevel::Warn, "create a template first"));
                return None;
            }
            // Single template? Skip the picker to save a click. With
            // multiple templates we always ask — blindly picking the
            // first template (like we used to) meant the user could
            // not choose what to base the task on.
            if model.templates.len() == 1 {
                let tid = model.templates[0].id;
                open_create_task_form(model, pid, tid);
                return None;
            }
            // Default the cursor to the currently-highlighted template
            // on the Templates screen, or the recently-used one, so
            // the picker opens on something sensible.
            let preferred = model
                .selected_template
                .or_else(|| model.templates.get(model.template_cursor).map(|t| t.id));
            let items: Vec<PickerItem> = model
                .templates
                .iter()
                .map(|t| PickerItem {
                    label: format!(
                        "{}  (v{}, {} field{})",
                        t.name,
                        t.version,
                        t.fields.len(),
                        if t.fields.len() == 1 { "" } else { "s" },
                    ),
                    accept: Msg::OpenCreateTaskWith(t.id),
                })
                .collect();
            let cursor = preferred
                .and_then(|id| model.templates.iter().position(|t| t.id == id))
                .unwrap_or(0);
            model.picker = Some(Picker {
                title: "Pick a template".into(),
                items,
                cursor,
            });
            None
        }
        Msg::OpenCreateTaskWith(tid) => {
            let Some(pid) = model.selected_project else {
                model
                    .toasts
                    .push(Toast::new(ToastLevel::Warn, "select a project first"));
                return None;
            };
            open_create_task_form(model, pid, tid);
            None
        }
        Msg::OpenEditTask(pid, task_id) => {
            model.picker = None;
            let Some(tasks) = model.tasks_by_project.get(&pid) else {
                return None;
            };
            let task = tasks.iter().find(|t| t.id == task_id).cloned()?;
            if !task.status.is_pending() {
                model.toasts.push(Toast::new(
                    ToastLevel::Warn,
                    "only Draft/Queued tasks can be edited",
                ));
                return None;
            }

            // We build the form from the current in-memory template schema.
            // If the template has changed since task creation, the edit UI
            // will reflect the latest version (and saving will re-render the
            // prompt against it).
            let tpl = model.templates.iter().find(|t| t.id == task.template_id);

            let title = format!("Edit task · {}", task.name);
            let mut fields: Vec<FormField> =
                vec![FormField::plain("Name", task.name.clone(), true, false)];
            if let Some(tpl) = tpl {
                for tf in &tpl.fields {
                    let mut f = form_field_for_template_field(tf);
                    if let Some(v) = task.fields.get(&tf.id) {
                        f.value = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        f.caret = f.value.len();
                    }
                    fields.push(f);
                }
            } else {
                // Template missing: fall back to showing existing field keys as read-only-ish rows.
                // We keep them editable as plain text but treat them as `Text` at submit time.
                for (k, v) in &task.fields {
                    let mut f =
                        FormField::plain(format!("{k} [unknown]"), v.to_string(), false, true);
                    f.field_id = Some(k.clone());
                    f.kind = Some(FieldKind::Text);
                    fields.push(f);
                }
            }

            model.form = Some(FormState {
                title,
                fields,
                cursor: 0,
                submit: FormSubmit::EditTask(pid, task_id),
            });
            None
        }
        Msg::PickerMove(d) => {
            if let Some(p) = model.picker.as_mut()
                && !p.items.is_empty()
            {
                // i128 arithmetic sidesteps the usize↔isize cast
                // lints and keeps wrap-around semantics clean.
                let n = i128::try_from(p.items.len()).unwrap_or(i128::MAX);
                let cur = i128::try_from(p.cursor).unwrap_or(0);
                let delta = i128::try_from(d).unwrap_or(0);
                let wrapped = ((cur + delta) % n + n) % n;
                p.cursor = usize::try_from(wrapped).unwrap_or(0);
            }
            None
        }
        Msg::PickerAccept => {
            // Take the picker so its `Msg` can be dispatched without
            // borrowing the model.
            let picker = model.picker.take()?;
            if let Some(item) = picker.items.into_iter().nth(picker.cursor) {
                return Some(Cmd::Dispatch(item.accept));
            }
            None
        }
        Msg::PickerCancel => {
            model.picker = None;
            None
        }
        Msg::DeleteTask(pid, tid) => {
            model.confirm = Some(ConfirmPrompt {
                title: "Delete task?".into(),
                message: "Delete this task? Its prompt/snapshot will be removed.".into(),
                accept: Box::new(Msg::ConfirmDeleteTask(pid, tid)),
            });
            None
        }
        Msg::RequestDispatch => {
            // Guard 1: we need a target project.
            if model.selected_project.is_none() {
                model.toasts.push(Toast::new(
                    ToastLevel::Warn,
                    "no project selected — press p to pick one",
                ));
                return None;
            }
            // Guard 2: something to dispatch. Either a multi-select,
            // or a single task under the cursor.
            let have_task = !model.task_multi_select.is_empty()
                || !model.tasks_for_selected_project().is_empty();
            if !have_task {
                model.toasts.push(Toast::new(
                    ToastLevel::Warn,
                    "no task to dispatch — press n to create one",
                ));
                return None;
            }
            // Guard 3: at least one installed agent.
            let installed: Vec<_> = model
                .available_agents
                .iter()
                .filter(|a| a.installed)
                .collect();
            if installed.is_empty() {
                let any = !model.available_agents.is_empty();
                let msg = if any {
                    "no installed agent — open Settings and install one (then press r to refresh)"
                } else {
                    "no agents registered — check your lattice install and press r in Settings"
                };
                model.toasts.push(Toast::new(ToastLevel::Error, msg));
                return None;
            }
            // One agent? Dispatch immediately. Several? Let the user
            // pick so the choice is never hidden.
            if installed.len() == 1 {
                let agent_id = installed[0].manifest.id.clone();
                return Some(Cmd::DispatchSelected(agent_id));
            }
            let items: Vec<PickerItem> = installed
                .iter()
                .map(|a| PickerItem {
                    label: format!("{}  ({})", a.manifest.display_name, a.manifest.id),
                    accept: Msg::DispatchSelected(a.manifest.id.clone()),
                })
                .collect();
            model.picker = Some(Picker {
                title: "Pick agent to dispatch".into(),
                items,
                cursor: 0,
            });
            None
        }
        Msg::DispatchSelected(agent_id) => Some(Cmd::DispatchSelected(agent_id)),

        Msg::FormInputChar(c) => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.insert_char(c);
            }
            None
        }
        Msg::FormBackspace => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.backspace();
            }
            None
        }
        Msg::FormNext => {
            if let Some(f) = model.form.as_mut() {
                f.cursor = (f.cursor + 1) % f.fields.len().max(1);
                // Land the caret at end-of-text so the user sees what
                // they previously typed instead of a caret at col 0.
                if let Some(field) = f.fields.get_mut(f.cursor) {
                    field.caret = field.value.len();
                }
            }
            None
        }
        Msg::FormPrev => {
            if let Some(f) = model.form.as_mut() {
                let n = f.fields.len().max(1);
                f.cursor = (f.cursor + n - 1) % n;
                if let Some(field) = f.fields.get_mut(f.cursor) {
                    field.caret = field.value.len();
                }
            }
            None
        }
        Msg::FormCaretLeft => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.caret_left();
            }
            None
        }
        Msg::FormCaretRight => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.caret_right();
            }
            None
        }
        Msg::FormCaretUp => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.caret_up();
            }
            None
        }
        Msg::FormCaretDown => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.caret_down();
            }
            None
        }
        Msg::FormCaretHome => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.caret_home();
            }
            None
        }
        Msg::FormCaretEnd => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                field.caret_end();
            }
            None
        }
        Msg::FormSubmit => {
            if let Some(f) = model.form.take() {
                return Some(Cmd::SubmitForm(f));
            }
            None
        }
        Msg::FormCancel => {
            model.form = None;
            None
        }

        // sequence-gram editor --------------------------------------------
        Msg::OpenSequenceEditor => {
            let Some(form) = &model.form else {
                return None;
            };
            let Some(field) = form.fields.get(form.cursor) else {
                return None;
            };
            if !matches!(field.kind, Some(FieldKind::SequenceGram)) {
                return None;
            }
            let diagrams = parse_sequence_gram(&field.value);
            model.sequence_editor = Some(SequenceEditorState {
                form_field_index: form.cursor,
                diagrams,
                diagram_cursor: 0,
                event_cursor: 0,
                participant_cursor: 0,
                mode: SequenceEditorMode::Browse,
            });
            None
        }
        Msg::SeqEdCancel => {
            model.sequence_editor = None;
            None
        }
        Msg::SeqEdSave => {
            let Some(ed) = model.sequence_editor.take() else {
                return None;
            };
            if let Some(form) = &mut model.form
                && let Some(f) = form.fields.get_mut(ed.form_field_index)
            {
                f.value = render_sequence_gram(&ed.diagrams);
                f.set_caret(f.value.len());
            }
            None
        }
        Msg::SeqEdMove(d) => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
            {
                let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
                    return None;
                };
                ed.event_cursor =
                    apply_delta(ed.event_cursor, d, diag.events.len().saturating_sub(1));
            }
            None
        }
        Msg::SeqEdMoveParticipant(d) => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
            {
                let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
                    return None;
                };
                if diag.participants.is_empty() {
                    return None;
                }
                ed.participant_cursor = apply_delta(
                    ed.participant_cursor,
                    d,
                    diag.participants.len().saturating_sub(1),
                );
            }
            None
        }
        Msg::SeqEdMoveDiagram(d) => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
                && !ed.diagrams.is_empty()
            {
                ed.diagram_cursor =
                    apply_delta(ed.diagram_cursor, d, ed.diagrams.len().saturating_sub(1));
                ed.event_cursor = 0;
                ed.participant_cursor = 0;
            }
            None
        }
        Msg::SeqEdStartAddParticipant => {
            if let Some(ed) = &mut model.sequence_editor {
                ed.mode = SequenceEditorMode::AddParticipant {
                    input: String::new(),
                };
            }
            None
        }
        Msg::SeqEdStartAddMessage => {
            if let Some(ed) = &mut model.sequence_editor {
                let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
                    return None;
                };
                let from = 0;
                let to = diag.participants.get(1).map_or(0, |_| 1);
                ed.mode = SequenceEditorMode::AddMessage {
                    from,
                    to,
                    dashed: false,
                    input: String::new(),
                };
            }
            None
        }
        Msg::SeqEdToggleDashed => {
            if let Some(ed) = &mut model.sequence_editor
                && let SequenceEditorMode::AddMessage { dashed, .. } = &mut ed.mode
            {
                *dashed = !*dashed;
            }
            None
        }
        Msg::SeqEdCycleFrom(d) => {
            if let Some(ed) = &mut model.sequence_editor
                && let SequenceEditorMode::AddMessage { from, .. } = &mut ed.mode
            {
                let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
                    return None;
                };
                if diag.participants.is_empty() {
                    return None;
                }
                *from = apply_delta(*from, d, diag.participants.len().saturating_sub(1));
            }
            None
        }
        Msg::SeqEdCycleTo(d) => {
            if let Some(ed) = &mut model.sequence_editor
                && let SequenceEditorMode::AddMessage { to, .. } = &mut ed.mode
            {
                let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
                    return None;
                };
                if diag.participants.is_empty() {
                    return None;
                }
                *to = apply_delta(*to, d, diag.participants.len().saturating_sub(1));
            }
            None
        }
        Msg::SeqEdInputChar(c) => {
            if let Some(ed) = &mut model.sequence_editor {
                match &mut ed.mode {
                    SequenceEditorMode::AddParticipant { input } => input.push(c),
                    SequenceEditorMode::AddDiagram { input } => input.push(c),
                    SequenceEditorMode::RenameDiagram { input } => input.push(c),
                    SequenceEditorMode::AddMessage { input, .. } => input.push(c),
                    SequenceEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::SeqEdBackspace => {
            if let Some(ed) = &mut model.sequence_editor {
                match &mut ed.mode {
                    SequenceEditorMode::AddParticipant { input } => {
                        input.pop();
                    }
                    SequenceEditorMode::AddDiagram { input } => {
                        input.pop();
                    }
                    SequenceEditorMode::RenameDiagram { input } => {
                        input.pop();
                    }
                    SequenceEditorMode::AddMessage { input, .. } => {
                        input.pop();
                    }
                    SequenceEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::SeqEdConfirm => {
            if let Some(ed) = &mut model.sequence_editor {
                match std::mem::replace(&mut ed.mode, SequenceEditorMode::Browse) {
                    SequenceEditorMode::AddParticipant { input } => {
                        let name = input.trim();
                        let Some(diag) = ed.diagrams.get_mut(ed.diagram_cursor) else {
                            return None;
                        };
                        if !name.is_empty() && !diag.participants.iter().any(|p| p == name) {
                            diag.participants.push(name.to_string());
                            ed.participant_cursor = diag.participants.len().saturating_sub(1);
                        }
                    }
                    SequenceEditorMode::AddDiagram { input } => {
                        let name = input.trim();
                        let name = if name.is_empty() {
                            "Diagram".to_string()
                        } else {
                            name.to_string()
                        };
                        ed.diagrams.push(SequenceDiagram {
                            name,
                            participants: Vec::new(),
                            events: Vec::new(),
                        });
                        ed.diagram_cursor = ed.diagrams.len().saturating_sub(1);
                        ed.event_cursor = 0;
                        ed.participant_cursor = 0;
                    }
                    SequenceEditorMode::RenameDiagram { input } => {
                        let name = input.trim();
                        if let Some(diag) = ed.diagrams.get_mut(ed.diagram_cursor)
                            && !name.is_empty()
                        {
                            diag.name = name.to_string();
                        }
                    }
                    SequenceEditorMode::AddMessage {
                        from,
                        to,
                        dashed,
                        input,
                    } => {
                        let text = input.trim();
                        let Some(diag) = ed.diagrams.get_mut(ed.diagram_cursor) else {
                            return None;
                        };
                        if !text.is_empty() && !diag.participants.is_empty() {
                            let from = diag.participants.get(from).cloned().unwrap_or_default();
                            let to = diag.participants.get(to).cloned().unwrap_or_default();
                            if !from.is_empty() && !to.is_empty() {
                                diag.events.push(SequenceEvent::Message {
                                    from,
                                    to,
                                    dashed,
                                    text: text.to_string(),
                                });
                                ed.event_cursor = diag.events.len().saturating_sub(1);
                            }
                        }
                    }
                    SequenceEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::SeqEdDeleteEvent => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
            {
                let Some(diag) = ed.diagrams.get_mut(ed.diagram_cursor) else {
                    return None;
                };
                if diag.events.is_empty() {
                    return None;
                }
                let idx = ed.event_cursor.min(diag.events.len().saturating_sub(1));
                diag.events.remove(idx);
                ed.event_cursor = ed.event_cursor.min(diag.events.len().saturating_sub(1));
            }
            None
        }
        Msg::SeqEdDeleteParticipant => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
            {
                let Some(diag) = ed.diagrams.get_mut(ed.diagram_cursor) else {
                    return None;
                };
                if diag.participants.is_empty() {
                    return None;
                }
                let idx = ed
                    .participant_cursor
                    .min(diag.participants.len().saturating_sub(1));
                let name = diag.participants.remove(idx);
                diag.events.retain(|ev| match ev {
                    SequenceEvent::Message { from, to, .. } => from != &name && to != &name,
                });
                ed.participant_cursor = ed
                    .participant_cursor
                    .min(diag.participants.len().saturating_sub(1));
                ed.event_cursor = ed.event_cursor.min(diag.events.len().saturating_sub(1));
            }
            None
        }
        Msg::SeqEdStartAddDiagram => {
            if let Some(ed) = &mut model.sequence_editor {
                ed.mode = SequenceEditorMode::AddDiagram {
                    input: String::new(),
                };
            }
            None
        }
        Msg::SeqEdStartRenameDiagram => {
            if let Some(ed) = &mut model.sequence_editor {
                let current = ed
                    .diagrams
                    .get(ed.diagram_cursor)
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                ed.mode = SequenceEditorMode::RenameDiagram { input: current };
            }
            None
        }
        Msg::SeqEdDeleteDiagram => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
                && !ed.diagrams.is_empty()
            {
                let idx = ed.diagram_cursor.min(ed.diagrams.len().saturating_sub(1));
                ed.diagrams.remove(idx);
                if ed.diagrams.is_empty() {
                    ed.diagrams.push(SequenceDiagram {
                        name: "Diagram".into(),
                        participants: Vec::new(),
                        events: Vec::new(),
                    });
                }
                ed.diagram_cursor = ed.diagram_cursor.min(ed.diagrams.len().saturating_sub(1));
                ed.event_cursor = 0;
                ed.participant_cursor = 0;
            }
            None
        }

        Msg::RuntimeCursor(d) => {
            move_cursor(&mut model.runtime_cursor, d, model.running.len());
            None
        }
        Msg::InspectRun(id) => {
            model.inspect_run = Some(id);
            model.inspect_tail.clear();
            Some(Cmd::SubscribeRunLogs(id))
        }
        Msg::KillRun(id) => {
            model.confirm = Some(ConfirmPrompt {
                title: "Kill run?".into(),
                message: "Send SIGTERM (then SIGKILL after grace) to this agent?".into(),
                accept: Box::new(Msg::ConfirmKill(id)),
            });
            None
        }

        Msg::RunCursor(d) => {
            let len = model.runs_for_selected_project().len();
            move_cursor(&mut model.run_cursor, d, len);
            let run_id = model
                .runs_for_selected_project()
                .get(model.run_cursor)
                .map(|r| r.id);
            run_id.map(Cmd::LoadHistoryLogs)
        }

        Msg::ConfirmDeleteProject(id) => Some(Cmd::DeleteProject(id)),
        Msg::ConfirmDeleteTemplate(id) => Some(Cmd::DeleteTemplate(id)),
        Msg::ConfirmDeleteTask(p, t) => Some(Cmd::DeleteTask(p, t)),
        Msg::ConfirmKill(id) => Some(Cmd::KillRun(id)),
    }
}

fn move_cursor(cur: &mut usize, d: isize, len: usize) {
    if len == 0 {
        *cur = 0;
        return;
    }
    *cur = apply_delta(*cur, d, len - 1);
}

/// Add `delta` to `cur`, clamped to `[0, cap]`. All arithmetic is
/// done in `i128` so we don't need `as` casts between `usize` and
/// `isize` (which clippy flags).
fn apply_delta(cur: usize, delta: isize, cap: usize) -> usize {
    let cur_i = i128::try_from(cur).unwrap_or(i128::MAX);
    let cap_i = i128::try_from(cap).unwrap_or(i128::MAX);
    let delta_i = i128::try_from(delta).unwrap_or(0);
    let clamped = (cur_i + delta_i).clamp(0, cap_i);
    usize::try_from(clamped).unwrap_or(0)
}

fn parse_sequence_gram(src: &str) -> Vec<SequenceDiagram> {
    // Supports two shapes:
    // - A single raw Mermaid body (no headings/fences)
    // - Multiple diagrams rendered by `render_sequence_gram`:
    //     "## Name" + ```mermaid ... ```
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return vec![SequenceDiagram {
            name: "Diagram".into(),
            participants: Vec::new(),
            events: Vec::new(),
        }];
    }

    let mut diagrams: Vec<SequenceDiagram> = Vec::new();
    let mut pending_name: Option<String> = None;
    let mut in_mermaid = false;
    let mut buf: Vec<String> = Vec::new();

    for line in src.lines() {
        let l = line.trim_end();
        if !in_mermaid {
            if let Some(h) = l.strip_prefix("## ").or_else(|| l.strip_prefix("### ")) {
                pending_name = Some(h.trim().to_string());
                continue;
            }
            if l.trim() == "```mermaid" {
                in_mermaid = true;
                buf.clear();
                continue;
            }
        } else if l.trim() == "```" {
            in_mermaid = false;
            let name = pending_name
                .take()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("Diagram {}", diagrams.len() + 1));
            diagrams.push(parse_mermaid_diagram(&name, &buf.join("\n")));
            buf.clear();
            continue;
        }

        if in_mermaid {
            buf.push(l.to_string());
        }
    }

    if !diagrams.is_empty() {
        return diagrams;
    }

    // Fallback: treat as a single diagram body.
    vec![parse_mermaid_diagram("Diagram", src)]
}

fn parse_mermaid_diagram(name: &str, body: &str) -> SequenceDiagram {
    let mut participants: Vec<String> = Vec::new();
    let mut events: Vec<SequenceEvent> = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("```") {
            continue;
        }
        if l.starts_with("sequenceDiagram") {
            continue;
        }
        if let Some(rest) = l.strip_prefix("participant ") {
            let pname = rest.trim();
            if !pname.is_empty() && !participants.iter().any(|p| p == pname) {
                participants.push(pname.to_string());
            }
            continue;
        }
        let (dashed, sep) = if l.contains("-->>") {
            (true, "-->>")
        } else if l.contains("->>") {
            (false, "->>")
        } else {
            continue;
        };
        let Some((lhs, rhs)) = l.split_once(sep) else {
            continue;
        };
        let from = lhs.trim();
        let Some((to, text)) = rhs.split_once(':') else {
            continue;
        };
        let to = to.trim();
        let text = text.trim();
        if from.is_empty() || to.is_empty() || text.is_empty() {
            continue;
        }
        for pname in [from, to] {
            if !participants.iter().any(|p| p == pname) {
                participants.push(pname.to_string());
            }
        }
        events.push(SequenceEvent::Message {
            from: from.to_string(),
            to: to.to_string(),
            dashed,
            text: text.to_string(),
        });
    }
    SequenceDiagram {
        name: name.to_string(),
        participants,
        events,
    }
}

fn render_sequence_gram(diagrams: &[SequenceDiagram]) -> String {
    let mut out = String::new();
    for (i, d) in diagrams.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str("## ");
        out.push_str(&d.name);
        out.push('\n');
        out.push_str("```mermaid\n");
        out.push_str(&render_mermaid_body(&d.participants, &d.events));
        out.push_str("```\n");
    }
    out
}

fn render_mermaid_body(participants: &[String], events: &[SequenceEvent]) -> String {
    let mut out = String::new();
    out.push_str("sequenceDiagram\n");
    for p in participants {
        out.push_str("    participant ");
        out.push_str(p);
        out.push('\n');
    }
    for ev in events {
        match ev {
            SequenceEvent::Message {
                from,
                to,
                dashed,
                text,
            } => {
                let arrow = if *dashed { "-->>" } else { "->>" };
                out.push_str("    ");
                out.push_str(from);
                out.push_str(arrow);
                out.push_str(to);
                out.push_str(": ");
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    out
}

/// Commands are side effects requested by `update`. The shell handles
/// them via `AppContext` and dispatches follow-up `Msg`s when done.
#[derive(Clone, Debug)]
pub enum Cmd {
    Dispatch(Msg),
    SubmitForm(FormState),
    DispatchSelected(AgentId),
    DeleteProject(ProjectId),
    DeleteTemplate(TemplateId),
    DeleteTask(ProjectId, TaskId),
    KillRun(RunId),
    SubscribeRunLogs(RunId),
    LoadHistoryLogs(RunId),
}

// Silence unused-import warnings in no-op builds where `TaskStatus` is
// only used through future extensions. We keep the import because
// downstream screens need it.
#[allow(dead_code)]
const _TS: [TaskStatus; 0] = [];

/// Friendly, multi-line example rendered into the `Fields (TOML)`
/// editor when the user creates a new template. We deliberately show
/// the common kinds so the UX doubles as documentation.
fn default_fields_toml_hint() -> String {
    "# one [[fields]] block per field. kinds: text, textarea, select,\n\
     # multiselect, number, boolean, file_picker, glob, markdown_note.\n\
     # Example:\n\
     # [[fields]]\n\
     # id = \"description\"\n\
     # kind = \"textarea\"\n\
     # label = \"What to do\"\n\
     # required = true\n"
        .into()
}

/// TOML wrapper used for the `Fields (TOML)` authoring shortcut. We
/// serialize/parse through this so the on-disk `[[fields]]` blocks
/// round-trip verbatim without exposing the user to crate-internal
/// types.
#[derive(serde::Serialize)]
struct FieldsTomlOut<'a> {
    fields: &'a [lattice_core::fields::Field],
}

#[derive(serde::Deserialize, Default)]
struct FieldsTomlIn {
    #[serde(default)]
    fields: Vec<lattice_core::fields::Field>,
}

/// Serialize `fields` as a sequence of `[[fields]]` TOML blocks that
/// round-trip through the template parser. Editing re-enters this
/// exact text so users can tweak a single attribute without losing
/// surrounding context.
fn fields_to_toml(fields: &[lattice_core::fields::Field]) -> String {
    if fields.is_empty() {
        return default_fields_toml_hint();
    }
    toml::to_string(&FieldsTomlOut { fields }).unwrap_or_default()
}

/// Parse the user-entered `[[fields]] …` TOML into a `Vec<Field>`.
/// Comment-only content returns an empty vector.
pub fn parse_fields_toml(src: &str) -> Result<Vec<lattice_core::fields::Field>, String> {
    let trimmed = src.trim();
    if trimmed.is_empty()
        || trimmed
            .lines()
            .all(|l| l.trim_start().starts_with('#') || l.trim().is_empty())
    {
        return Ok(Vec::new());
    }
    let w: FieldsTomlIn = toml::from_str(src).map_err(|e| e.to_string())?;
    Ok(w.fields)
}

/// Build the task-creation form for `(pid, tid)` and install it on
/// the model, clearing any open template picker. Extracted so both
/// the single-template shortcut and the picker-accept branch share
/// the same code path.
fn open_create_task_form(model: &mut Model, pid: ProjectId, tid: TemplateId) {
    model.picker = None;
    let tpl = model.templates.iter().find(|t| t.id == tid).cloned();
    let title = tpl.as_ref().map_or_else(
        || "New task".to_string(),
        |t| format!("New task · {}", t.name),
    );
    let mut fields: Vec<FormField> = vec![FormField::plain("Name", "", true, false)];
    if let Some(t) = tpl.as_ref() {
        for tf in &t.fields {
            fields.push(form_field_for_template_field(tf));
        }
    }
    model.form = Some(FormState {
        title,
        fields,
        cursor: 0,
        submit: FormSubmit::CreateTask(pid, tid),
    });
}

/// Build the form row that represents a single template-declared field
/// on the task-creation form. `kind` drives rendering (multiline for
/// `textarea`) and submit-time parsing of the typed value.
fn form_field_for_template_field(field: &lattice_core::fields::Field) -> FormField {
    let label = format_field_label(field);
    let multiline = matches!(field.kind, FieldKind::Textarea | FieldKind::SequenceGram);
    let value = match &field.default {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };
    let options = field
        .options
        .options
        .iter()
        .map(|o| o.id().to_string())
        .collect();
    let caret = value.len();
    FormField {
        label,
        value,
        required: field.required,
        multiline,
        caret,
        field_id: Some(field.id.clone()),
        kind: Some(field.kind),
        options,
    }
}

fn format_field_label(field: &lattice_core::fields::Field) -> String {
    let kind_label = match field.kind {
        FieldKind::Text => "text",
        FieldKind::Textarea => "textarea",
        FieldKind::Select => "select",
        FieldKind::Multiselect => "multiselect",
        FieldKind::Number => "number",
        FieldKind::Boolean => "boolean",
        FieldKind::FilePicker => "file",
        FieldKind::Glob => "glob",
        FieldKind::CmdOutput => "cmd_output",
        FieldKind::MarkdownNote => "note",
        FieldKind::Ref => "ref",
        FieldKind::Component => "component",
        FieldKind::SequenceGram => "sequence-gram",
    };
    format!("{} [{}]", field.label, kind_label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::entities::{Project, Template};
    use lattice_core::fields::{Field, FieldKind, FieldOptions, Validation};
    use lattice_core::ids::ProjectId;
    use lattice_core::time::Timestamp;

    #[test]
    fn update_tab_cycle() {
        let mut m = Model::new();
        assert_eq!(m.screen, Screen::Projects);
        update(&mut m, Msg::NextTab);
        assert_eq!(m.screen, Screen::Templates);
        update(&mut m, Msg::PrevTab);
        assert_eq!(m.screen, Screen::Projects);
    }

    #[test]
    fn parse_fields_toml_handles_comments_and_blocks() {
        assert!(parse_fields_toml("").unwrap().is_empty());
        assert!(parse_fields_toml("# only a comment\n").unwrap().is_empty());

        let src = r#"
[[fields]]
id = "description"
kind = "textarea"
label = "What to do"
required = true
"#;
        let v = parse_fields_toml(src).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "description");
        assert_eq!(v[0].kind, FieldKind::Textarea);
        assert!(v[0].required);
    }

    #[test]
    fn parse_fields_toml_surfaces_errors() {
        let src = "[[fields]]\nbogus = true\n";
        assert!(parse_fields_toml(src).is_err());
    }

    #[test]
    fn open_create_task_builds_one_row_per_template_field() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        let pid = ProjectId::new();
        m.selected_project = Some(pid);
        let mut tpl = Template::new("refactor", now);
        tpl.fields.push(Field {
            id: "module".into(),
            kind: FieldKind::Text,
            label: "Target module".into(),
            help: None,
            placeholder: None,
            required: true,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions::default(),
        });
        tpl.fields.push(Field {
            id: "description".into(),
            kind: FieldKind::Textarea,
            label: "Description".into(),
            help: None,
            placeholder: None,
            required: false,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions::default(),
        });
        let tid = tpl.id;
        m.templates.push(tpl);
        m.selected_template = Some(tid);

        update(&mut m, Msg::OpenCreateTask);
        let form = m.form.expect("form should open");
        // Name + one row per template field.
        assert_eq!(form.fields.len(), 3);
        assert_eq!(form.fields[0].label, "Name");
        assert_eq!(form.fields[1].field_id.as_deref(), Some("module"));
        assert_eq!(form.fields[1].kind, Some(FieldKind::Text));
        assert!(!form.fields[1].multiline);
        assert!(form.fields[1].required);
        assert_eq!(form.fields[2].field_id.as_deref(), Some("description"));
        assert_eq!(form.fields[2].kind, Some(FieldKind::Textarea));
        assert!(form.fields[2].multiline);
    }

    #[test]
    fn open_create_task_warns_without_project() {
        let mut m = Model::new();
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        m.templates.push(Template::new("t", now));
        update(&mut m, Msg::OpenCreateTask);
        assert!(m.form.is_none());
        assert!(m.picker.is_none());
        assert_eq!(m.toasts.len(), 1);
    }

    #[test]
    fn open_create_task_warns_without_templates() {
        let mut m = Model::new();
        m.selected_project = Some(ProjectId::new());
        update(&mut m, Msg::OpenCreateTask);
        assert!(m.form.is_none());
        assert!(m.picker.is_none());
        assert_eq!(m.toasts.len(), 1);
    }

    #[test]
    fn open_create_task_opens_picker_with_multiple_templates() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        m.selected_project = Some(ProjectId::new());
        m.templates.push(Template::new("alpha", now));
        m.templates.push(Template::new("beta", now));
        update(&mut m, Msg::OpenCreateTask);
        assert!(m.form.is_none(), "form must not open directly");
        let picker = m.picker.as_ref().expect("picker should be open");
        assert_eq!(picker.items.len(), 2);
        assert!(picker.items[0].label.starts_with("alpha"));
    }

    #[test]
    fn template_picker_accept_opens_form() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        m.selected_project = Some(ProjectId::new());
        m.templates.push(Template::new("alpha", now));
        m.templates.push(Template::new("beta", now));
        update(&mut m, Msg::OpenCreateTask);
        update(&mut m, Msg::PickerMove(1));
        // Picker accept dispatches a Cmd::Dispatch(accept). Run it.
        let cmd = update(&mut m, Msg::PickerAccept).expect("should dispatch");
        let Cmd::Dispatch(inner) = cmd else {
            panic!("expected Dispatch, got {cmd:?}");
        };
        let expected_tid = m.templates[1].id;
        update(&mut m, inner);
        assert!(m.picker.is_none());
        let form = m.form.as_ref().expect("form should open on accept");
        let FormSubmit::CreateTask(_pid, tid) = form.submit else {
            panic!("unexpected submit: {:?}", form.submit);
        };
        assert_eq!(tid, expected_tid);
    }

    #[test]
    fn template_picker_cancel_closes_without_form() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        m.selected_project = Some(ProjectId::new());
        m.templates.push(Template::new("alpha", now));
        m.templates.push(Template::new("beta", now));
        update(&mut m, Msg::OpenCreateTask);
        update(&mut m, Msg::PickerCancel);
        assert!(m.picker.is_none());
        assert!(m.form.is_none());
    }

    #[test]
    fn open_create_task_single_template_skips_picker() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        m.selected_project = Some(ProjectId::new());
        m.templates.push(Template::new("solo", now));
        update(&mut m, Msg::OpenCreateTask);
        assert!(m.picker.is_none());
        assert!(m.form.is_some(), "should open the form directly");
    }

    #[test]
    fn open_create_template_form_has_fields_block() {
        let mut m = Model::new();
        update(&mut m, Msg::OpenCreateTemplate);
        let form = m.form.expect("form should open");
        // Name, Context, Fields, Prompt.
        assert_eq!(form.fields.len(), 4);
        assert!(
            form.fields[2].label.starts_with("Fields"),
            "third field should be the Fields TOML editor, got: {}",
            form.fields[2].label
        );
        assert!(form.fields[2].multiline);
    }

    #[test]
    fn project_cursor_snaps_to_len() {
        let mut m = Model::new();
        m.projects = Vec::new();
        update(&mut m, Msg::ProjectCursor(5));
        assert_eq!(m.project_cursor, 0);
    }

    #[test]
    fn palette_accept_produces_command() {
        let mut m = Model::new();
        update(&mut m, Msg::PalToggle);
        for c in "quit".chars() {
            update(&mut m, Msg::PalInput(c));
        }
        let cmd = update(&mut m, Msg::PalAccept);
        assert!(matches!(cmd, Some(Cmd::Dispatch(Msg::Quit))));
        assert!(!m.palette_open);
    }

    #[test]
    fn delete_project_asks_for_confirm() {
        let mut m = Model::new();
        let p = Project::new(
            "x",
            "/x",
            lattice_core::time::Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
        );
        let pid = p.id;
        m.projects.push(p);
        update(&mut m, Msg::DeleteProject(pid));
        assert!(m.confirm.is_some());
        let cmd = update(&mut m, Msg::AckConfirm);
        match cmd {
            Some(Cmd::Dispatch(Msg::ConfirmDeleteProject(x))) => assert_eq!(x, pid),
            other => panic!("unexpected cmd: {other:?}"),
        }
    }

    fn multiline_field(value: &str, caret: usize) -> FormField {
        let mut f = FormField::plain("body", value, false, true);
        f.set_caret(caret);
        f
    }

    #[test]
    fn insert_char_places_at_caret_and_advances() {
        let mut f = multiline_field("hello", 2);
        f.insert_char('Z');
        assert_eq!(f.value, "heZllo");
        assert_eq!(f.caret, 3);
    }

    #[test]
    fn backspace_deletes_before_caret() {
        let mut f = multiline_field("hello", 3);
        f.backspace();
        assert_eq!(f.value, "helo");
        assert_eq!(f.caret, 2);
    }

    #[test]
    fn backspace_at_zero_is_noop() {
        let mut f = multiline_field("hi", 0);
        f.backspace();
        assert_eq!(f.value, "hi");
        assert_eq!(f.caret, 0);
    }

    #[test]
    fn caret_horizontal_crosses_multi_byte_scalars() {
        let mut f = multiline_field("aé", 3); // after 'é'
        f.caret_left();
        assert_eq!(f.caret, 1); // before 'é'
        f.caret_left();
        assert_eq!(f.caret, 0);
        f.caret_right();
        assert_eq!(f.caret, 1);
        f.caret_right();
        assert_eq!(f.caret, 3); // skipped the continuation byte
    }

    #[test]
    fn caret_up_keeps_column_on_longer_line() {
        // two lines, 5 chars each. Caret is at col 3 on line 2.
        let mut f = multiline_field("aaaaa\nbbbbb", 5 + 1 + 3);
        f.caret_up();
        assert_eq!(f.caret, 3);
    }

    #[test]
    fn caret_up_clamps_to_shorter_line_end() {
        // first line is "ab" (2 chars), second line has caret at col 4.
        let mut f = multiline_field("ab\ncdefg", 2 + 1 + 4);
        f.caret_up();
        assert_eq!(f.caret, 2); // end of "ab"
    }

    #[test]
    fn caret_down_moves_to_next_line_or_end() {
        let mut f = multiline_field("abc\ndef", 1);
        f.caret_down();
        assert_eq!(f.caret, 5); // col 1 on line 2 → 'e'
        f.caret_down();
        assert_eq!(f.caret, 7); // no next line → jump to end
    }

    #[test]
    fn caret_home_and_end_operate_per_line() {
        let mut f = multiline_field("abc\ndefg", 6);
        f.caret_home();
        assert_eq!(f.caret, 4); // start of "defg"
        f.caret_end();
        assert_eq!(f.caret, 8); // end of "defg"
    }

    #[test]
    fn input_char_via_msg_inserts_at_caret() {
        let mut m = Model::new();
        m.form = Some(FormState {
            title: "t".into(),
            fields: vec![multiline_field("hello", 2)],
            cursor: 0,
            submit: FormSubmit::CreateProject,
        });
        update(&mut m, Msg::FormInputChar('X'));
        let f = &m.form.as_ref().unwrap().fields[0];
        assert_eq!(f.value, "heXllo");
        assert_eq!(f.caret, 3);
    }

    #[test]
    fn form_next_lands_caret_at_end_of_new_field() {
        let mut m = Model::new();
        m.form = Some(FormState {
            title: "t".into(),
            fields: vec![
                FormField::plain("a", "", false, false),
                FormField::plain("b", "hello", false, true),
            ],
            cursor: 0,
            submit: FormSubmit::CreateProject,
        });
        update(&mut m, Msg::FormNext);
        let f = &m.form.as_ref().unwrap();
        assert_eq!(f.cursor, 1);
        assert_eq!(f.fields[1].caret, 5);
    }

    #[test]
    fn select_and_go_to_tasks_sets_selection_and_screen() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        let proj = Project::new("demo", std::path::PathBuf::from("/tmp/x"), now);
        let id = proj.id;
        m.projects.push(proj);
        update(&mut m, Msg::SelectAndGoToTasks(id));
        assert_eq!(m.selected_project, Some(id));
        assert_eq!(m.screen, Screen::Tasks);
        assert_eq!(m.toasts.len(), 1);
    }

    #[test]
    fn open_project_picker_warns_when_empty() {
        let mut m = Model::new();
        update(&mut m, Msg::OpenProjectPicker);
        assert!(m.picker.is_none());
        assert_eq!(m.toasts.len(), 1);
    }

    #[test]
    fn open_project_picker_preloads_selected_cursor() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        m.projects
            .push(Project::new("a", std::path::PathBuf::from("/a"), now));
        m.projects
            .push(Project::new("b", std::path::PathBuf::from("/b"), now));
        let target = m.projects[1].id;
        m.selected_project = Some(target);
        update(&mut m, Msg::OpenProjectPicker);
        let picker = m.picker.as_ref().expect("picker should open");
        assert_eq!(picker.cursor, 1);
        assert_eq!(picker.items.len(), 2);
    }

    #[test]
    fn request_dispatch_warns_without_project() {
        let mut m = Model::new();
        update(&mut m, Msg::RequestDispatch);
        assert!(m.picker.is_none());
        assert_eq!(m.toasts.len(), 1);
    }

    #[test]
    fn request_dispatch_errors_without_installed_agent() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        let proj = Project::new("demo", std::path::PathBuf::from("/tmp/x"), now);
        let pid = proj.id;
        m.projects.push(proj);
        m.selected_project = Some(pid);
        // Task under cursor so guard #2 passes.
        let mut t = lattice_core::entities::Task::new(pid, TemplateId::new(), 1, "t", now);
        t.status = lattice_core::entities::TaskStatus::Draft;
        m.tasks_by_project.insert(pid, vec![t]);
        update(&mut m, Msg::RequestDispatch);
        assert!(m.picker.is_none(), "no picker without installed agent");
        assert_eq!(m.toasts.len(), 1);
        assert_eq!(m.toasts[0].level, crate::toast::ToastLevel::Error);
    }
}
