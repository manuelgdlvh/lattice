//! UI model, `Msg`, and the pure `update` function.
//!
//! This module deliberately depends only on the domain types — it
//! never touches crossterm or ratatui. That lets us test state
//! transitions without a terminal.

use lattice_core::entities::{Task, Template};
use lattice_core::fields::FieldKind;
use lattice_core::ids::{TaskId, TemplateId};

use crate::toast::{Toast, ToastLevel};

/// Top-level screens, in tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Templates,
    Tasks,
    Info,
    Help,
}

impl Screen {
    pub const TABS: &'static [Screen] = &[Screen::Templates, Screen::Tasks, Screen::Info];

    pub fn label(self) -> &'static str {
        match self {
            Self::Templates => "Templates",
            Self::Tasks => "Tasks",
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

    pub selected_template: Option<TemplateId>,

    pub templates: Vec<Template>,
    pub template_cursor: usize,

    pub tasks: Vec<Task>,
    pub task_cursor: usize,
    pub task_multi_select: Vec<TaskId>,
    /// Scroll offset for the Tasks screen prompt preview pane.
    pub task_prompt_scroll: usize,

    pub toasts: Vec<Toast>,
    pub status_message: Option<String>,

    pub palette_open: bool,
    pub palette_input: String,
    pub palette_cursor: usize,

    pub confirm: Option<ConfirmPrompt>,
    pub form: Option<FormState>,
    /// Generic modal list overlay. Used for the template picker during
    /// task creation. Each item carries its own accept message so the picker
    /// itself is message-agnostic.
    pub picker: Option<Picker>,
    /// Modal sequence diagram editor for `sequence-gram` fields.
    pub sequence_editor: Option<SequenceEditorState>,
    /// Modal code blocks editor for `code-blocks` fields.
    pub code_editor: Option<CodeEditorState>,
    /// Modal Gherkin test case editor for `gherkin` fields.
    pub gherkin_editor: Option<GherkinEditorState>,
    /// Modal OpenAPI contract editor for `openapi` fields.
    pub openapi_editor: Option<OpenApiEditorState>,
    // (json-schema removed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenApiEditorState {
    pub form_field_index: usize,
    pub title: String,
    pub version: String,
    pub base_url: String,
    pub endpoints: Vec<OpenApiEndpoint>,
    pub endpoint_cursor: usize,
    pub schemas: Vec<OpenApiSchema>,
    pub schema_cursor: usize,
    pub prop_cursor: usize,
    pub preview_scroll: usize,
    pub mode: OpenApiEditorMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenApiEndpoint {
    pub method: OpenApiMethod,
    pub path: String,
    pub request_content_type: String,
    pub request_body: OpenApiBody,
    pub response_status: u16,
    pub response_content_type: String,
    pub response_body: OpenApiBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenApiMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenApiBody {
    None,
    String,
    Integer,
    Number,
    Boolean,
    SchemaRef(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenApiSchema {
    pub name: String,
    pub additional_properties: bool,
    pub properties: Vec<OpenApiSchemaProp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenApiSchemaProp {
    pub name: String,
    pub kind: OpenApiPropType,
    pub required: bool,
    pub nullable: bool,
    pub format: OpenApiStringFormat,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub pattern: String,
    pub enum_values: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenApiPropType {
    String,
    Integer,
    Number,
    Boolean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenApiStringFormat {
    None,
    Email,
    Password,
    Uri,
}

impl Default for OpenApiEndpoint {
    fn default() -> Self {
        Self {
            method: OpenApiMethod::Get,
            path: "/".into(),
            request_content_type: "application/json".into(),
            request_body: OpenApiBody::None,
            response_status: 200,
            response_content_type: "application/json".into(),
            response_body: OpenApiBody::None,
        }
    }
}

impl Default for OpenApiSchema {
    fn default() -> Self {
        Self {
            name: "Schema".into(),
            additional_properties: false,
            properties: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenApiEditorMode {
    Browse,
    Schemas,
    AddEndpoint,
    AddSchema { input: String },
    RenameSchema { input: String },
    AddProp { input: String },
    RenameProp { input: String },
    EditPropPattern { input: String },
    EditPropEnum { input: String },
    EditPropMin { input: String },
    EditPropMax { input: String },
    EditPath { input: String },
    EditTitle { input: String },
    EditVersion { input: String },
    EditBaseUrl { input: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GherkinEditorState {
    /// Index into `form.fields` we are editing.
    pub form_field_index: usize,
    pub features: Vec<GherkinFeature>,
    pub feature_cursor: usize,
    pub scenario_cursor: usize,
    pub mode: GherkinEditorMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GherkinFeature {
    pub name: String,
    pub background: String,
    pub scenarios: Vec<GherkinScenario>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GherkinScenario {
    pub name: String,
    pub steps: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GherkinEditorMode {
    Browse,
    AddFeature { input: String },
    RenameFeature { input: String },
    EditFeature { input: String }, // rename current feature
    RenameScenario { input: String },
    EditBackground { editor: FormField },
    EditSteps { editor: FormField },
    AddScenario { input: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEditorState {
    /// Index into `form.fields` we are editing.
    pub form_field_index: usize,
    /// Multiple code blocks inside one field.
    pub blocks: Vec<CodeBlock>,
    /// Selected block.
    pub block_cursor: usize,
    pub mode: CodeEditorMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    pub name: String,
    pub language: String,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeEditorMode {
    Browse,
    AddBlock { input: String },
    RenameBlock { input: String },
    EditLanguage { input: String },
    EditCode { editor: FormField },
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
    EditEdgeContext {
        input: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceEvent {
    Message {
        from: String,
        to: String,
        dashed: bool,
        rel_id: String,
        text: String,
        edge_context: Option<String>,
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
    CreateTemplate,
    EditTemplate(TemplateId),
    CreateTask(TemplateId),
    EditTask(TaskId),
    SaveTaskPromptToFile(TaskId),
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            screen: Screen::Templates,
            prev_screen: Screen::Templates,
            quitting: false,
            selected_template: None,
            templates: Vec::new(),
            template_cursor: 0,
            tasks: Vec::new(),
            task_cursor: 0,
            task_multi_select: Vec::new(),
            task_prompt_scroll: 0,
            toasts: Vec::new(),
            status_message: None,
            palette_open: false,
            palette_input: String::new(),
            palette_cursor: 0,
            confirm: None,
            form: None,
            picker: None,
            sequence_editor: None,
            code_editor: None,
            gherkin_editor: None,
            openapi_editor: None,
        }
    }

    pub fn tasks_for_selected_project(&self) -> &[Task] {
        &self.tasks
    }

    pub fn ensure_selection_consistency(&mut self) {
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
    SetTemplates(Vec<Template>),
    SetTasks(Vec<Task>),
    ToastInfo(String),
    ToastWarn(String),
    ToastError(String),

    // templates -------------------------------------------------------
    TemplateCursor(isize),
    OpenCreateTemplate,
    OpenEditTemplate(TemplateId),
    DeleteTemplate(TemplateId),

    // tasks -----------------------------------------------------------
    TaskCursor(isize),
    TaskPromptScroll(isize),
    ToggleTaskSelection(TaskId),
    OpenCreateTask,
    /// Open the task-creation form for a specific template, bypassing
    /// the picker. Used both as the picker's accept message and from
    /// the Templates screen where a template is already in focus.
    OpenCreateTaskWith(TemplateId),
    OpenEditTask(TaskId),
    OpenSaveTaskPrompt(TaskId),
    // Generic picker overlay. Items each carry their own accept Msg,
    // so the picker itself is reusable for templates, projects, agents,
    // etc.
    PickerMove(isize),
    PickerAccept,
    PickerCancel,
    DeleteTask(TaskId),

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
    SeqEdStartEditEdgeContext,
    SeqEdConfirm,
    SeqEdDeleteEvent,
    SeqEdDeleteParticipant,
    SeqEdStartAddDiagram,
    SeqEdStartRenameDiagram,
    SeqEdDeleteDiagram,

    // code-blocks editor ----------------------------------------------
    OpenCodeEditor,
    CodeEdCancel,
    CodeEdSave,
    CodeEdMoveBlock(isize),
    CodeEdInputChar(char),
    CodeEdBackspace,
    CodeEdConfirm,
    CodeEdStartAddBlock,
    CodeEdStartRenameBlock,
    CodeEdStartEditLanguage,
    CodeEdStartEditCode,
    CodeEdDeleteBlock,
    CodeEdCaretLeft,
    CodeEdCaretRight,
    CodeEdCaretUp,
    CodeEdCaretDown,
    CodeEdCaretHome,
    CodeEdCaretEnd,

    // gherkin editor ---------------------------------------------------
    OpenGherkinEditor,
    GhEdCancel,
    GhEdSave,
    GhEdMoveScenario(isize),
    GhEdMoveFeature(isize),
    GhEdInputChar(char),
    GhEdBackspace,
    GhEdConfirm,
    GhEdStartAddScenario,
    GhEdStartRenameScenario,
    GhEdStartEditFeature,
    GhEdStartEditBackground,
    GhEdStartEditSteps,
    GhEdDeleteScenario,
    GhEdStartAddFeature,
    GhEdStartRenameFeature,
    GhEdDeleteFeature,
    GhEdCaretLeft,
    GhEdCaretRight,
    GhEdCaretUp,
    GhEdCaretDown,
    GhEdCaretHome,
    GhEdCaretEnd,

    // openapi editor ---------------------------------------------------
    OpenOpenApiEditor,
    OaEdCancel,
    OaEdSave,
    OaEdMoveEndpoint(isize),
    OaEdScrollPreview(isize),
    OaEdStartAddEndpoint,
    OaEdDeleteEndpoint,
    OaEdTogglePane,
    OaEdMoveSchema(isize),
    OaEdStartAddSchema,
    OaEdDeleteSchema,
    OaEdStartRenameSchema,
    OaEdStartAddProp,
    OaEdDeleteProp,
    OaEdTogglePropRequired,
    OaEdCyclePropType(isize),
    OaEdTogglePropNullable,
    OaEdCyclePropFormat(isize),
    OaEdStartRenameProp,
    OaEdStartEditPropMin,
    OaEdStartEditPropMax,
    OaEdStartEditPropPattern,
    OaEdStartEditPropEnum,
    OaEdMoveProp(isize),
    OaEdCycleMethod(isize),
    OaEdCycleReqBody(isize),
    OaEdCycleRespBody(isize),
    OaEdCycleStatus(isize),
    OaEdCycleReqContentType(isize),
    OaEdStartEditPath,
    OaEdStartEditTitle,
    OaEdStartEditVersion,
    OaEdStartEditBaseUrl,
    OaEdInputChar(char),
    OaEdBackspace,
    OaEdConfirm,

    // results from async actions --------------------------------------
    ConfirmDeleteTemplate(TemplateId),
    ConfirmDeleteTask(TaskId),
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
            None
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
                model.toasts.pop();
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

        Msg::SetTemplates(v) => {
            model.templates = v;
            model.ensure_selection_consistency();
            None
        }
        Msg::SetTasks(v) => {
            model.tasks = v;
            // Clamp cursor after refresh so keybinds don't silently no-op
            // when the list shrinks (e.g. after delete or external change).
            let len = model.tasks_for_selected_project().len();
            model.task_cursor = model.task_cursor.min(len.saturating_sub(1));
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
                    FormField::plain(
                        "Fields (TOML — one [[fields]] block per field)",
                        default_fields_toml_hint(),
                        false,
                        true,
                    ),
                    FormField::plain("Prompt Jinja", String::new(), true, true),
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
                        "Fields (TOML — one [[fields]] block per field)",
                        fields_toml,
                        false,
                        true,
                    ),
                    FormField::plain("Prompt Jinja", tpl.prompt.template.clone(), true, true),
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
            model.task_prompt_scroll = 0;
            None
        }
        Msg::TaskPromptScroll(d) => {
            // Clamping is done in the view (based on rendered height).
            // Here we just keep it non-negative.
            model.task_prompt_scroll = apply_delta(model.task_prompt_scroll, d, usize::MAX);
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
        Msg::OpenCreateTask => {
            if model.templates.is_empty() {
                model
                    .toasts
                    .push(Toast::new(ToastLevel::Warn, "create a template first"));
                return None;
            }
            // Single template? Skip the picker to save a click. With
            // multiple templates we always ask.
            if model.templates.len() == 1 {
                let tid = model.templates[0].id;
                open_create_task_form(model, tid);
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
            open_create_task_form(model, tid);
            None
        }
        Msg::OpenEditTask(task_id) => {
            model.picker = None;
            let task = model.tasks.iter().find(|t| t.id == task_id).cloned()?;

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
                    f.kind = Some(FieldKind::Textarea);
                    fields.push(f);
                }
            }

            model.form = Some(FormState {
                title,
                fields,
                cursor: 0,
                submit: FormSubmit::EditTask(task_id),
            });
            None
        }
        Msg::OpenSaveTaskPrompt(task_id) => {
            let Some(task) = model.tasks.iter().find(|t| t.id == task_id) else {
                model
                    .toasts
                    .push(Toast::new(ToastLevel::Warn, "task not found"));
                return None;
            };
            model.form = Some(FormState {
                title: format!("Save prompt · {}", task.name),
                fields: vec![FormField::plain(
                    "File name",
                    task.name.clone(),
                    true,
                    false,
                )],
                cursor: 0,
                submit: FormSubmit::SaveTaskPromptToFile(task_id),
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
        Msg::DeleteTask(tid) => {
            model.confirm = Some(ConfirmPrompt {
                title: "Delete task?".into(),
                message: "Delete this task and its files?".into(),
                accept: Box::new(Msg::ConfirmDeleteTask(tid)),
            });
            None
        }

        Msg::FormInputChar(c) => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                if matches!(field.kind, Some(FieldKind::SequenceGram)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "sequence-gram is read-only here; press F3 to edit",
                    ));
                    return None;
                }
                if matches!(field.kind, Some(FieldKind::CodeBlocks)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "code-blocks is read-only here; press F4 to edit",
                    ));
                    return None;
                }
                if matches!(field.kind, Some(FieldKind::Gherkin)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "gherkin is read-only here; press F5 to edit",
                    ));
                    return None;
                }
                if matches!(field.kind, Some(FieldKind::OpenApi)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "openapi is read-only here; press F6 to edit",
                    ));
                    return None;
                }
                field.insert_char(c);
            }
            None
        }
        Msg::FormBackspace => {
            if let Some(f) = model.form.as_mut()
                && let Some(field) = f.fields.get_mut(f.cursor)
            {
                if matches!(field.kind, Some(FieldKind::SequenceGram)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "sequence-gram is read-only here; press F3 to edit",
                    ));
                    return None;
                }
                if matches!(field.kind, Some(FieldKind::CodeBlocks)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "code-blocks is read-only here; press F4 to edit",
                    ));
                    return None;
                }
                if matches!(field.kind, Some(FieldKind::Gherkin)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "gherkin is read-only here; press F5 to edit",
                    ));
                    return None;
                }
                if matches!(field.kind, Some(FieldKind::OpenApi)) {
                    model.toasts.push(Toast::new(
                        ToastLevel::Info,
                        "openapi is read-only here; press F6 to edit",
                    ));
                    return None;
                }
                field.backspace();
            }
            None
        }
        Msg::FormNext => {
            if let Some(f) = model.form.as_mut() {
                f.cursor = (f.cursor + 1) % f.fields.len().max(1);
                // Land the caret at end-of-text so the user sees what
                // is already typed instead of a caret at col 0.
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
                // Circular with one key (Tab): wrap around.
                let n = i128::try_from(ed.diagrams.len()).unwrap_or(1).max(1);
                let cur = i128::try_from(ed.diagram_cursor).unwrap_or(0);
                let delta = i128::try_from(d).unwrap_or(0);
                let wrapped = ((cur + delta) % n + n) % n;
                ed.diagram_cursor = usize::try_from(wrapped).unwrap_or(0);
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
        Msg::SeqEdStartEditEdgeContext => {
            if let Some(ed) = &mut model.sequence_editor
                && matches!(ed.mode, SequenceEditorMode::Browse)
            {
                let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
                    return None;
                };
                let Some(ev) = diag.events.get(ed.event_cursor) else {
                    return None;
                };
                let current = match ev {
                    SequenceEvent::Message { edge_context, .. } => {
                        edge_context.as_deref().unwrap_or_default().to_string()
                    }
                };
                ed.mode = SequenceEditorMode::EditEdgeContext { input: current };
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
                    SequenceEditorMode::EditEdgeContext { input } => input.push(c),
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
                    SequenceEditorMode::EditEdgeContext { input } => {
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
                                let rel_id = next_relation_id(diag);
                                diag.events.push(SequenceEvent::Message {
                                    from,
                                    to,
                                    dashed,
                                    rel_id,
                                    text: text.to_string(),
                                    edge_context: None,
                                });
                                ed.event_cursor = diag.events.len().saturating_sub(1);
                            }
                        }
                    }
                    SequenceEditorMode::EditEdgeContext { input } => {
                        let Some(diag) = ed.diagrams.get_mut(ed.diagram_cursor) else {
                            return None;
                        };
                        let Some(ev) = diag.events.get_mut(ed.event_cursor) else {
                            return None;
                        };
                        let txt = input.trim();
                        match ev {
                            SequenceEvent::Message { edge_context, .. } => {
                                if txt.is_empty() {
                                    *edge_context = None;
                                } else {
                                    *edge_context = Some(txt.to_string());
                                }
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

        // code-blocks editor ----------------------------------------------
        Msg::OpenCodeEditor => {
            let Some(form) = &model.form else {
                return None;
            };
            let Some(field) = form.fields.get(form.cursor) else {
                return None;
            };
            if !matches!(field.kind, Some(FieldKind::CodeBlocks)) {
                return None;
            }
            let blocks = parse_code_blocks(&field.value);
            model.code_editor = Some(CodeEditorState {
                form_field_index: form.cursor,
                blocks,
                block_cursor: 0,
                mode: CodeEditorMode::Browse,
            });
            None
        }
        Msg::CodeEdCancel => {
            model.code_editor = None;
            None
        }
        Msg::CodeEdSave => {
            let Some(ed) = model.code_editor.take() else {
                return None;
            };
            if let Some(form) = &mut model.form
                && let Some(f) = form.fields.get_mut(ed.form_field_index)
            {
                f.value = render_code_blocks(&ed.blocks);
                f.set_caret(f.value.len());
            }
            None
        }
        Msg::CodeEdMoveBlock(d) => {
            if let Some(ed) = &mut model.code_editor
                && matches!(ed.mode, CodeEditorMode::Browse)
                && !ed.blocks.is_empty()
            {
                let n = i128::try_from(ed.blocks.len()).unwrap_or(1).max(1);
                let cur = i128::try_from(ed.block_cursor).unwrap_or(0);
                let delta = i128::try_from(d).unwrap_or(0);
                let wrapped = ((cur + delta) % n + n) % n;
                ed.block_cursor = usize::try_from(wrapped).unwrap_or(0);
            }
            None
        }
        Msg::CodeEdStartAddBlock => {
            if let Some(ed) = &mut model.code_editor {
                ed.mode = CodeEditorMode::AddBlock {
                    input: String::new(),
                };
            }
            None
        }
        Msg::CodeEdStartRenameBlock => {
            if let Some(ed) = &mut model.code_editor {
                let current = ed
                    .blocks
                    .get(ed.block_cursor)
                    .map(|b| b.name.clone())
                    .unwrap_or_default();
                ed.mode = CodeEditorMode::RenameBlock { input: current };
            }
            None
        }
        Msg::CodeEdStartEditLanguage => {
            if let Some(ed) = &mut model.code_editor {
                let current = ed
                    .blocks
                    .get(ed.block_cursor)
                    .map(|b| b.language.clone())
                    .unwrap_or_default();
                ed.mode = CodeEditorMode::EditLanguage { input: current };
            }
            None
        }
        Msg::CodeEdStartEditCode => {
            if let Some(ed) = &mut model.code_editor {
                let current = ed
                    .blocks
                    .get(ed.block_cursor)
                    .map(|b| b.code.clone())
                    .unwrap_or_default();
                ed.mode = CodeEditorMode::EditCode {
                    editor: FormField::plain("Code", current, false, true),
                };
            }
            None
        }
        Msg::CodeEdInputChar(c) => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::AddBlock { input }
                    | CodeEditorMode::RenameBlock { input }
                    | CodeEditorMode::EditLanguage { input } => input.push(c),
                    CodeEditorMode::EditCode { editor } => {
                        editor.insert_char(c);
                    }
                    CodeEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::CodeEdBackspace => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::AddBlock { input }
                    | CodeEditorMode::RenameBlock { input }
                    | CodeEditorMode::EditLanguage { input } => {
                        input.pop();
                    }
                    CodeEditorMode::EditCode { editor } => {
                        editor.backspace();
                    }
                    CodeEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::CodeEdCaretLeft => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::EditCode { editor } => {
                        editor.caret_left();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::CodeEdCaretRight => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::EditCode { editor } => {
                        editor.caret_right();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::CodeEdCaretUp => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::EditCode { editor } => {
                        editor.caret_up();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::CodeEdCaretDown => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::EditCode { editor } => {
                        editor.caret_down();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::CodeEdCaretHome => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::EditCode { editor } => {
                        editor.caret_home();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::CodeEdCaretEnd => {
            if let Some(ed) = &mut model.code_editor {
                match &mut ed.mode {
                    CodeEditorMode::EditCode { editor } => {
                        editor.caret_end();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::CodeEdConfirm => {
            if let Some(ed) = &mut model.code_editor {
                match std::mem::replace(&mut ed.mode, CodeEditorMode::Browse) {
                    CodeEditorMode::AddBlock { input } => {
                        let name = input.trim();
                        let name = if name.is_empty() {
                            format!("Block {}", ed.blocks.len() + 1)
                        } else {
                            name.to_string()
                        };
                        ed.blocks.push(CodeBlock {
                            name,
                            language: "rust".into(),
                            code: String::new(),
                        });
                        ed.block_cursor = ed.blocks.len().saturating_sub(1);
                    }
                    CodeEditorMode::RenameBlock { input } => {
                        let name = input.trim();
                        if let Some(b) = ed.blocks.get_mut(ed.block_cursor)
                            && !name.is_empty()
                        {
                            b.name = name.to_string();
                        }
                    }
                    CodeEditorMode::EditLanguage { input } => {
                        let lang = input.trim();
                        if let Some(b) = ed.blocks.get_mut(ed.block_cursor) {
                            b.language = if lang.is_empty() {
                                "text".into()
                            } else {
                                lang.into()
                            };
                        }
                    }
                    CodeEditorMode::EditCode { editor } => {
                        if let Some(b) = ed.blocks.get_mut(ed.block_cursor) {
                            b.code = editor.value.trim_end().to_string();
                        }
                    }
                    CodeEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::CodeEdDeleteBlock => {
            if let Some(ed) = &mut model.code_editor
                && matches!(ed.mode, CodeEditorMode::Browse)
                && !ed.blocks.is_empty()
            {
                let idx = ed.block_cursor.min(ed.blocks.len().saturating_sub(1));
                ed.blocks.remove(idx);
                if ed.blocks.is_empty() {
                    ed.blocks.push(CodeBlock {
                        name: "Block 1".into(),
                        language: "rust".into(),
                        code: String::new(),
                    });
                }
                ed.block_cursor = ed.block_cursor.min(ed.blocks.len().saturating_sub(1));
            }
            None
        }

        // gherkin editor ---------------------------------------------------
        Msg::OpenGherkinEditor => {
            let Some(form) = &model.form else {
                return None;
            };
            let Some(field) = form.fields.get(form.cursor) else {
                return None;
            };
            if !matches!(field.kind, Some(FieldKind::Gherkin)) {
                return None;
            }
            let features = parse_gherkin_features(&field.value);
            model.gherkin_editor = Some(GherkinEditorState {
                form_field_index: form.cursor,
                features,
                feature_cursor: 0,
                scenario_cursor: 0,
                mode: GherkinEditorMode::Browse,
            });
            None
        }
        Msg::GhEdCancel => {
            model.gherkin_editor = None;
            None
        }
        Msg::GhEdSave => {
            let Some(ed) = model.gherkin_editor.take() else {
                return None;
            };
            if let Some(form) = &mut model.form
                && let Some(f) = form.fields.get_mut(ed.form_field_index)
            {
                f.value = render_gherkin_features(&ed.features);
                f.set_caret(f.value.len());
            }
            None
        }
        Msg::GhEdMoveFeature(d) => {
            if let Some(ed) = &mut model.gherkin_editor
                && matches!(ed.mode, GherkinEditorMode::Browse)
                && !ed.features.is_empty()
            {
                let n = i128::try_from(ed.features.len()).unwrap_or(1).max(1);
                let cur = i128::try_from(ed.feature_cursor).unwrap_or(0);
                let delta = i128::try_from(d).unwrap_or(0);
                let wrapped = ((cur + delta) % n + n) % n;
                ed.feature_cursor = usize::try_from(wrapped).unwrap_or(0);
                ed.scenario_cursor = 0;
            }
            None
        }
        Msg::GhEdMoveScenario(d) => {
            if let Some(ed) = &mut model.gherkin_editor
                && matches!(ed.mode, GherkinEditorMode::Browse)
            {
                let Some(feat) = ed.features.get(ed.feature_cursor) else {
                    return None;
                };
                if feat.scenarios.is_empty() {
                    return None;
                }
                let n = i128::try_from(feat.scenarios.len()).unwrap_or(1).max(1);
                let cur = i128::try_from(ed.scenario_cursor).unwrap_or(0);
                let delta = i128::try_from(d).unwrap_or(0);
                let wrapped = ((cur + delta) % n + n) % n;
                ed.scenario_cursor = usize::try_from(wrapped).unwrap_or(0);
            }
            None
        }
        Msg::GhEdStartAddFeature => {
            if let Some(ed) = &mut model.gherkin_editor {
                ed.mode = GherkinEditorMode::AddFeature {
                    input: String::new(),
                };
            }
            None
        }
        Msg::GhEdStartRenameFeature => {
            if let Some(ed) = &mut model.gherkin_editor {
                let current = ed
                    .features
                    .get(ed.feature_cursor)
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                ed.mode = GherkinEditorMode::RenameFeature { input: current };
            }
            None
        }
        Msg::GhEdStartAddScenario => {
            if let Some(ed) = &mut model.gherkin_editor {
                ed.mode = GherkinEditorMode::AddScenario {
                    input: String::new(),
                };
            }
            None
        }
        Msg::GhEdStartRenameScenario => {
            if let Some(ed) = &mut model.gherkin_editor {
                let current = ed
                    .features
                    .get(ed.feature_cursor)
                    .and_then(|f| f.scenarios.get(ed.scenario_cursor))
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                ed.mode = GherkinEditorMode::RenameScenario { input: current };
            }
            None
        }
        Msg::GhEdStartEditFeature => {
            if let Some(ed) = &mut model.gherkin_editor {
                let current = ed
                    .features
                    .get(ed.feature_cursor)
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                ed.mode = GherkinEditorMode::EditFeature { input: current };
            }
            None
        }
        Msg::GhEdStartEditBackground => {
            if let Some(ed) = &mut model.gherkin_editor {
                let current = ed
                    .features
                    .get(ed.feature_cursor)
                    .map(|f| f.background.clone())
                    .unwrap_or_default();
                ed.mode = GherkinEditorMode::EditBackground {
                    editor: FormField::plain("Background", current, false, true),
                };
            }
            None
        }
        Msg::GhEdStartEditSteps => {
            if let Some(ed) = &mut model.gherkin_editor {
                let current = ed
                    .features
                    .get(ed.feature_cursor)
                    .and_then(|f| f.scenarios.get(ed.scenario_cursor))
                    .map(|s| s.steps.clone())
                    .unwrap_or_default();
                ed.mode = GherkinEditorMode::EditSteps {
                    editor: FormField::plain("Steps", current, false, true),
                };
            }
            None
        }
        Msg::GhEdInputChar(c) => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::AddFeature { input }
                    | GherkinEditorMode::RenameFeature { input }
                    | GherkinEditorMode::EditFeature { input }
                    | GherkinEditorMode::RenameScenario { input }
                    | GherkinEditorMode::AddScenario { input } => input.push(c),
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.insert_char(c),
                    GherkinEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::GhEdBackspace => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::AddFeature { input }
                    | GherkinEditorMode::RenameFeature { input }
                    | GherkinEditorMode::EditFeature { input }
                    | GherkinEditorMode::RenameScenario { input }
                    | GherkinEditorMode::AddScenario { input } => {
                        input.pop();
                    }
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.backspace(),
                    GherkinEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::GhEdCaretLeft => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.caret_left(),
                    _ => {}
                }
            }
            None
        }
        Msg::GhEdCaretRight => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.caret_right(),
                    _ => {}
                }
            }
            None
        }
        Msg::GhEdCaretUp => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.caret_up(),
                    _ => {}
                }
            }
            None
        }
        Msg::GhEdCaretDown => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.caret_down(),
                    _ => {}
                }
            }
            None
        }
        Msg::GhEdCaretHome => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.caret_home(),
                    _ => {}
                }
            }
            None
        }
        Msg::GhEdCaretEnd => {
            if let Some(ed) = &mut model.gherkin_editor {
                match &mut ed.mode {
                    GherkinEditorMode::EditBackground { editor }
                    | GherkinEditorMode::EditSteps { editor } => editor.caret_end(),
                    _ => {}
                }
            }
            None
        }
        Msg::GhEdConfirm => {
            if let Some(ed) = &mut model.gherkin_editor {
                // Special case: validate steps before accepting.
                if let GherkinEditorMode::EditSteps { editor } = &mut ed.mode {
                    match validate_gherkin_steps(&editor.value) {
                        Ok(()) => {
                            if let Some(feat) = ed.features.get_mut(ed.feature_cursor)
                                && let Some(sc) = feat.scenarios.get_mut(ed.scenario_cursor)
                            {
                                sc.steps = editor.value.trim_end().to_string();
                            }
                            ed.mode = GherkinEditorMode::Browse;
                        }
                        Err(msg) => {
                            model.toasts.push(Toast::new(ToastLevel::Warn, msg));
                        }
                    }
                    return None;
                }

                match std::mem::replace(&mut ed.mode, GherkinEditorMode::Browse) {
                    GherkinEditorMode::AddFeature { input } => {
                        let name = input.trim();
                        let name = if name.is_empty() {
                            format!("Feature {}", ed.features.len() + 1)
                        } else {
                            name.to_string()
                        };
                        ed.features.push(GherkinFeature {
                            name,
                            background: String::new(),
                            scenarios: vec![GherkinScenario {
                                name: "Scenario 1".into(),
                                steps: String::new(),
                            }],
                        });
                        ed.feature_cursor = ed.features.len().saturating_sub(1);
                        ed.scenario_cursor = 0;
                    }
                    GherkinEditorMode::RenameFeature { input }
                    | GherkinEditorMode::EditFeature { input } => {
                        let name = input.trim();
                        if let Some(f) = ed.features.get_mut(ed.feature_cursor)
                            && !name.is_empty()
                        {
                            f.name = name.to_string();
                        }
                    }
                    GherkinEditorMode::AddScenario { input } => {
                        let name = input.trim();
                        let name = if name.is_empty() {
                            let n = ed
                                .features
                                .get(ed.feature_cursor)
                                .map(|f| f.scenarios.len())
                                .unwrap_or(0);
                            format!("Scenario {}", n + 1)
                        } else {
                            name.to_string()
                        };
                        if let Some(f) = ed.features.get_mut(ed.feature_cursor) {
                            f.scenarios.push(GherkinScenario {
                                name,
                                steps: String::new(),
                            });
                            ed.scenario_cursor = f.scenarios.len().saturating_sub(1);
                        }
                    }
                    GherkinEditorMode::RenameScenario { input } => {
                        let name = input.trim();
                        if let Some(f) = ed.features.get_mut(ed.feature_cursor)
                            && let Some(s) = f.scenarios.get_mut(ed.scenario_cursor)
                            && !name.is_empty()
                        {
                            s.name = name.to_string();
                        }
                    }
                    GherkinEditorMode::EditBackground { editor } => {
                        if let Some(f) = ed.features.get_mut(ed.feature_cursor) {
                            f.background = editor.value.trim_end().to_string();
                        }
                    }
                    GherkinEditorMode::EditSteps { .. } => {
                        // Handled above (validated + saved) before we reach this match.
                    }
                    GherkinEditorMode::Browse => {}
                }
            }
            None
        }
        Msg::GhEdDeleteFeature => {
            if let Some(ed) = &mut model.gherkin_editor
                && matches!(ed.mode, GherkinEditorMode::Browse)
                && !ed.features.is_empty()
            {
                let idx = ed.feature_cursor.min(ed.features.len().saturating_sub(1));
                ed.features.remove(idx);
                if ed.features.is_empty() {
                    ed.features.push(GherkinFeature {
                        name: "Test cases".into(),
                        background: String::new(),
                        scenarios: vec![GherkinScenario {
                            name: "Scenario 1".into(),
                            steps: String::new(),
                        }],
                    });
                }
                ed.feature_cursor = ed.feature_cursor.min(ed.features.len().saturating_sub(1));
                ed.scenario_cursor = 0;
            }
            None
        }
        Msg::GhEdDeleteScenario => {
            if let Some(ed) = &mut model.gherkin_editor
                && matches!(ed.mode, GherkinEditorMode::Browse)
            {
                let Some(f) = ed.features.get_mut(ed.feature_cursor) else {
                    return None;
                };
                if f.scenarios.is_empty() {
                    return None;
                }
                let idx = ed.scenario_cursor.min(f.scenarios.len().saturating_sub(1));
                f.scenarios.remove(idx);
                if f.scenarios.is_empty() {
                    f.scenarios.push(GherkinScenario {
                        name: "Scenario 1".into(),
                        steps: String::new(),
                    });
                }
                ed.scenario_cursor = ed.scenario_cursor.min(f.scenarios.len().saturating_sub(1));
            }
            None
        }

        // openapi editor ---------------------------------------------------
        Msg::OpenOpenApiEditor => {
            let Some(form) = &model.form else {
                return None;
            };
            let Some(field) = form.fields.get(form.cursor) else {
                return None;
            };
            if !matches!(field.kind, Some(FieldKind::OpenApi)) {
                return None;
            }
            let (title, version, base_url, endpoints) = parse_openapi_contract(&field.value);
            model.openapi_editor = Some(OpenApiEditorState {
                form_field_index: form.cursor,
                title,
                version,
                base_url,
                endpoints,
                endpoint_cursor: 0,
                schemas: Vec::new(),
                schema_cursor: 0,
                prop_cursor: 0,
                preview_scroll: 0,
                mode: OpenApiEditorMode::Browse,
            });
            None
        }
        Msg::OaEdCancel => {
            model.openapi_editor = None;
            None
        }
        Msg::OaEdSave => {
            let Some(ed) = model.openapi_editor.take() else {
                return None;
            };
            if let Some(form) = &mut model.form
                && let Some(f) = form.fields.get_mut(ed.form_field_index)
            {
                f.value = render_openapi_contract(
                    &ed.title,
                    &ed.version,
                    &ed.base_url,
                    &ed.endpoints,
                    &ed.schemas,
                );
                f.set_caret(f.value.len());
            }
            None
        }
        Msg::OaEdMoveEndpoint(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
            {
                move_cursor(&mut ed.endpoint_cursor, d, ed.endpoints.len());
                ed.preview_scroll = 0;
            }
            None
        }
        Msg::OaEdTogglePane => {
            if let Some(ed) = &mut model.openapi_editor {
                ed.mode = match ed.mode.clone() {
                    OpenApiEditorMode::Browse => OpenApiEditorMode::Schemas,
                    OpenApiEditorMode::Schemas => OpenApiEditorMode::Browse,
                    other => other,
                };
                ed.preview_scroll = 0;
            }
            None
        }
        Msg::OaEdMoveSchema(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
            {
                move_cursor(&mut ed.schema_cursor, d, ed.schemas.len());
                ed.prop_cursor = 0;
                ed.preview_scroll = 0;
            }
            None
        }
        Msg::OaEdMoveProp(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get(ed.schema_cursor)
            {
                move_cursor(&mut ed.prop_cursor, d, s.properties.len());
            }
            None
        }
        Msg::OaEdStartAddSchema => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
            {
                ed.mode = OpenApiEditorMode::AddSchema {
                    input: String::new(),
                };
            }
            None
        }
        Msg::OaEdStartRenameSchema => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get(ed.schema_cursor)
            {
                ed.mode = OpenApiEditorMode::RenameSchema {
                    input: s.name.clone(),
                };
            }
            None
        }
        Msg::OaEdDeleteSchema => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && !ed.schemas.is_empty()
            {
                let idx = ed.schema_cursor.min(ed.schemas.len().saturating_sub(1));
                let removed = ed.schemas.remove(idx).name;
                for ep in &mut ed.endpoints {
                    if matches!(&ep.request_body, OpenApiBody::SchemaRef(name) if name == &removed)
                    {
                        ep.request_body = OpenApiBody::None;
                    }
                    if matches!(&ep.response_body, OpenApiBody::SchemaRef(name) if name == &removed)
                    {
                        ep.response_body = OpenApiBody::None;
                    }
                }
                ed.schema_cursor = ed.schema_cursor.min(ed.schemas.len().saturating_sub(1));
                ed.prop_cursor = 0;
            }
            None
        }
        Msg::OaEdStartAddProp => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && ed.schemas.get(ed.schema_cursor).is_some()
            {
                ed.mode = OpenApiEditorMode::AddProp {
                    input: String::new(),
                };
            }
            None
        }
        Msg::OaEdStartRenameProp => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get(ed.schema_cursor)
                && let Some(p) = s.properties.get(ed.prop_cursor)
            {
                ed.mode = OpenApiEditorMode::RenameProp {
                    input: p.name.clone(),
                };
            }
            None
        }
        Msg::OaEdDeleteProp => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                && !s.properties.is_empty()
            {
                let idx = ed.prop_cursor.min(s.properties.len().saturating_sub(1));
                s.properties.remove(idx);
                ed.prop_cursor = ed.prop_cursor.min(s.properties.len().saturating_sub(1));
            }
            None
        }
        Msg::OaEdTogglePropRequired => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                && let Some(p) = s.properties.get_mut(ed.prop_cursor)
            {
                p.required = !p.required;
            }
            None
        }
        Msg::OaEdCyclePropType(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                && let Some(p) = s.properties.get_mut(ed.prop_cursor)
            {
                p.kind = cycle_prop_type(p.kind, d);
            }
            None
        }
        Msg::OaEdTogglePropNullable => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                && let Some(p) = s.properties.get_mut(ed.prop_cursor)
            {
                p.nullable = !p.nullable;
            }
            None
        }
        Msg::OaEdCyclePropFormat(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
                && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                && let Some(p) = s.properties.get_mut(ed.prop_cursor)
                && p.kind == OpenApiPropType::String
            {
                p.format = cycle_string_format(p.format, d);
            }
            None
        }
        Msg::OaEdStartEditPropMin => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
            {
                ed.mode = OpenApiEditorMode::EditPropMin {
                    input: String::new(),
                };
            }
            None
        }
        Msg::OaEdStartEditPropMax => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
            {
                ed.mode = OpenApiEditorMode::EditPropMax {
                    input: String::new(),
                };
            }
            None
        }
        Msg::OaEdStartEditPropPattern => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
            {
                ed.mode = OpenApiEditorMode::EditPropPattern {
                    input: String::new(),
                };
            }
            None
        }
        Msg::OaEdStartEditPropEnum => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Schemas)
            {
                ed.mode = OpenApiEditorMode::EditPropEnum {
                    input: String::new(),
                };
            }
            None
        }
        Msg::OaEdScrollPreview(d) => {
            if let Some(ed) = &mut model.openapi_editor {
                ed.preview_scroll = apply_delta(ed.preview_scroll, d, usize::MAX);
            }
            None
        }
        Msg::OaEdStartAddEndpoint => {
            if let Some(ed) = &mut model.openapi_editor {
                ed.mode = OpenApiEditorMode::AddEndpoint;
            }
            None
        }
        Msg::OaEdDeleteEndpoint => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
                && !ed.endpoints.is_empty()
            {
                let idx = ed.endpoint_cursor.min(ed.endpoints.len().saturating_sub(1));
                ed.endpoints.remove(idx);
                if ed.endpoints.is_empty() {
                    ed.endpoints.push(OpenApiEndpoint::default());
                }
                ed.endpoint_cursor = ed.endpoint_cursor.min(ed.endpoints.len().saturating_sub(1));
            }
            None
        }
        Msg::OaEdCycleMethod(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
                && let Some(ep) = ed.endpoints.get_mut(ed.endpoint_cursor)
            {
                ep.method = cycle_method(ep.method, d);
            }
            None
        }
        Msg::OaEdCycleReqBody(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
                && let Some(ep) = ed.endpoints.get_mut(ed.endpoint_cursor)
            {
                ep.request_body = cycle_body_with_schemas(ep.request_body.clone(), d, &ed.schemas);
            }
            None
        }
        Msg::OaEdCycleRespBody(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
                && let Some(ep) = ed.endpoints.get_mut(ed.endpoint_cursor)
            {
                ep.response_body =
                    cycle_body_with_schemas(ep.response_body.clone(), d, &ed.schemas);
            }
            None
        }
        Msg::OaEdCycleStatus(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
                && let Some(ep) = ed.endpoints.get_mut(ed.endpoint_cursor)
            {
                ep.response_status = cycle_status(ep.response_status, d);
            }
            None
        }
        Msg::OaEdCycleReqContentType(d) => {
            if let Some(ed) = &mut model.openapi_editor
                && matches!(ed.mode, OpenApiEditorMode::Browse)
                && let Some(ep) = ed.endpoints.get_mut(ed.endpoint_cursor)
            {
                // c: keep req/resp in lockstep for speed; use r/p for body shape.
                let next = cycle_content_type(&ep.request_content_type, d);
                ep.request_content_type = next.clone();
                ep.response_content_type = next;
            }
            None
        }
        Msg::OaEdStartEditPath => {
            if let Some(ed) = &mut model.openapi_editor
                && let Some(ep) = ed.endpoints.get(ed.endpoint_cursor)
            {
                ed.mode = OpenApiEditorMode::EditPath {
                    input: ep.path.clone(),
                };
            }
            None
        }
        Msg::OaEdStartEditTitle => {
            if let Some(ed) = &mut model.openapi_editor {
                ed.mode = OpenApiEditorMode::EditTitle {
                    input: ed.title.clone(),
                };
            }
            None
        }
        Msg::OaEdStartEditVersion => {
            if let Some(ed) = &mut model.openapi_editor {
                ed.mode = OpenApiEditorMode::EditVersion {
                    input: ed.version.clone(),
                };
            }
            None
        }
        Msg::OaEdStartEditBaseUrl => {
            if let Some(ed) = &mut model.openapi_editor {
                ed.mode = OpenApiEditorMode::EditBaseUrl {
                    input: ed.base_url.clone(),
                };
            }
            None
        }
        Msg::OaEdInputChar(c) => {
            if let Some(ed) = &mut model.openapi_editor {
                match &mut ed.mode {
                    OpenApiEditorMode::AddSchema { input }
                    | OpenApiEditorMode::RenameSchema { input }
                    | OpenApiEditorMode::AddProp { input }
                    | OpenApiEditorMode::RenameProp { input }
                    | OpenApiEditorMode::EditPropPattern { input }
                    | OpenApiEditorMode::EditPropEnum { input }
                    | OpenApiEditorMode::EditPropMin { input }
                    | OpenApiEditorMode::EditPropMax { input }
                    | OpenApiEditorMode::EditPath { input }
                    | OpenApiEditorMode::EditTitle { input }
                    | OpenApiEditorMode::EditVersion { input }
                    | OpenApiEditorMode::EditBaseUrl { input } => input.push(c),
                    _ => {}
                }
            }
            None
        }
        Msg::OaEdBackspace => {
            if let Some(ed) = &mut model.openapi_editor {
                match &mut ed.mode {
                    OpenApiEditorMode::AddSchema { input }
                    | OpenApiEditorMode::RenameSchema { input }
                    | OpenApiEditorMode::AddProp { input }
                    | OpenApiEditorMode::RenameProp { input }
                    | OpenApiEditorMode::EditPropPattern { input }
                    | OpenApiEditorMode::EditPropEnum { input }
                    | OpenApiEditorMode::EditPropMin { input }
                    | OpenApiEditorMode::EditPropMax { input }
                    | OpenApiEditorMode::EditPath { input }
                    | OpenApiEditorMode::EditTitle { input }
                    | OpenApiEditorMode::EditVersion { input }
                    | OpenApiEditorMode::EditBaseUrl { input } => {
                        input.pop();
                    }
                    _ => {}
                }
            }
            None
        }
        Msg::OaEdConfirm => {
            if let Some(ed) = &mut model.openapi_editor {
                match &mut ed.mode {
                    OpenApiEditorMode::AddEndpoint => {
                        ed.endpoints.push(OpenApiEndpoint::default());
                        ed.endpoint_cursor = ed.endpoints.len().saturating_sub(1);
                    }
                    OpenApiEditorMode::AddSchema { input } => {
                        let name = input.trim();
                        if !name.is_empty() {
                            ed.schemas.push(OpenApiSchema {
                                name: name.to_string(),
                                ..OpenApiSchema::default()
                            });
                            ed.schema_cursor = ed.schemas.len().saturating_sub(1);
                            ed.prop_cursor = 0;
                        }
                    }
                    OpenApiEditorMode::RenameSchema { input } => {
                        let name = input.trim();
                        if !name.is_empty()
                            && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                        {
                            s.name = name.to_string();
                        }
                    }
                    OpenApiEditorMode::AddProp { input } => {
                        let name = input.trim();
                        if !name.is_empty()
                            && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                        {
                            s.properties.push(OpenApiSchemaProp {
                                name: name.to_string(),
                                kind: OpenApiPropType::String,
                                required: false,
                                nullable: false,
                                format: OpenApiStringFormat::None,
                                min: None,
                                max: None,
                                pattern: String::new(),
                                enum_values: Vec::new(),
                            });
                            ed.prop_cursor = s.properties.len().saturating_sub(1);
                        }
                    }
                    OpenApiEditorMode::RenameProp { input } => {
                        let name = input.trim();
                        if !name.is_empty()
                            && let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                            && let Some(p) = s.properties.get_mut(ed.prop_cursor)
                        {
                            p.name = name.to_string();
                        }
                    }
                    OpenApiEditorMode::EditPropPattern { input } => {
                        if let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                            && let Some(p) = s.properties.get_mut(ed.prop_cursor)
                        {
                            p.pattern = input.trim().to_string();
                        }
                    }
                    OpenApiEditorMode::EditPropEnum { input } => {
                        if let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                            && let Some(p) = s.properties.get_mut(ed.prop_cursor)
                        {
                            p.enum_values = input
                                .split(',')
                                .map(|x| x.trim().to_string())
                                .filter(|x| !x.is_empty())
                                .collect();
                        }
                    }
                    OpenApiEditorMode::EditPropMin { input } => {
                        if let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                            && let Some(p) = s.properties.get_mut(ed.prop_cursor)
                        {
                            p.min = input.trim().parse::<i64>().ok();
                        }
                    }
                    OpenApiEditorMode::EditPropMax { input } => {
                        if let Some(s) = ed.schemas.get_mut(ed.schema_cursor)
                            && let Some(p) = s.properties.get_mut(ed.prop_cursor)
                        {
                            p.max = input.trim().parse::<i64>().ok();
                        }
                    }
                    OpenApiEditorMode::EditPath { input } => {
                        if let Some(ep) = ed.endpoints.get_mut(ed.endpoint_cursor) {
                            let v = input.trim();
                            if !v.is_empty() {
                                ep.path = v.to_string();
                            }
                        }
                    }
                    OpenApiEditorMode::EditTitle { input } => {
                        let v = input.trim();
                        if !v.is_empty() {
                            ed.title = v.to_string();
                        }
                    }
                    OpenApiEditorMode::EditVersion { input } => {
                        let v = input.trim();
                        if !v.is_empty() {
                            ed.version = v.to_string();
                        }
                    }
                    OpenApiEditorMode::EditBaseUrl { input } => {
                        let v = input.trim();
                        if !v.is_empty() {
                            ed.base_url = v.to_string();
                        }
                    }
                    OpenApiEditorMode::Browse | OpenApiEditorMode::Schemas => {}
                }
                ed.mode = match ed.mode {
                    OpenApiEditorMode::Schemas
                    | OpenApiEditorMode::AddSchema { .. }
                    | OpenApiEditorMode::RenameSchema { .. }
                    | OpenApiEditorMode::AddProp { .. }
                    | OpenApiEditorMode::RenameProp { .. }
                    | OpenApiEditorMode::EditPropPattern { .. }
                    | OpenApiEditorMode::EditPropEnum { .. }
                    | OpenApiEditorMode::EditPropMin { .. }
                    | OpenApiEditorMode::EditPropMax { .. } => OpenApiEditorMode::Schemas,
                    _ => OpenApiEditorMode::Browse,
                };
            }
            None
        }
        Msg::ConfirmDeleteTemplate(id) => Some(Cmd::DeleteTemplate(id)),
        Msg::ConfirmDeleteTask(t) => Some(Cmd::DeleteTask(t)),
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

fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn cycle_method(cur: OpenApiMethod, d: isize) -> OpenApiMethod {
    let all = [
        OpenApiMethod::Get,
        OpenApiMethod::Post,
        OpenApiMethod::Put,
        OpenApiMethod::Patch,
        OpenApiMethod::Delete,
    ];
    cycle_wrap(&all, cur, d)
}

fn cycle_body_with_schemas(cur: OpenApiBody, d: isize, schemas: &[OpenApiSchema]) -> OpenApiBody {
    let mut all: Vec<OpenApiBody> = vec![
        OpenApiBody::None,
        OpenApiBody::String,
        OpenApiBody::Integer,
        OpenApiBody::Number,
        OpenApiBody::Boolean,
    ];
    for s in schemas {
        let name = s.name.trim();
        if !name.is_empty() {
            all.push(OpenApiBody::SchemaRef(name.to_string()));
        }
    }
    // If current schema ref was deleted/renamed, treat it as None for cycling.
    let cur_norm = match &cur {
        OpenApiBody::SchemaRef(name) if !all.iter().any(|x| x == &cur) => OpenApiBody::None,
        _ => cur,
    };
    cycle_wrap(&all, cur_norm, d)
}

fn cycle_prop_type(cur: OpenApiPropType, d: isize) -> OpenApiPropType {
    let all = [
        OpenApiPropType::String,
        OpenApiPropType::Integer,
        OpenApiPropType::Number,
        OpenApiPropType::Boolean,
    ];
    cycle_wrap(&all, cur, d)
}

fn cycle_string_format(cur: OpenApiStringFormat, d: isize) -> OpenApiStringFormat {
    let all = [
        OpenApiStringFormat::None,
        OpenApiStringFormat::Email,
        OpenApiStringFormat::Password,
        OpenApiStringFormat::Uri,
    ];
    cycle_wrap(&all, cur, d)
}

fn cycle_status(cur: u16, d: isize) -> u16 {
    let all: [u16; 11] = [200, 201, 204, 400, 401, 403, 404, 409, 422, 500, 502];
    cycle_wrap(&all, cur, d)
}

fn cycle_content_type(cur: &str, d: isize) -> String {
    let all = [
        "application/json",
        "text/plain",
        "application/xml",
        "text/markdown",
    ];
    let idx = all.iter().position(|x| *x == cur).unwrap_or(0);
    let picked = cycle_wrap(&all, all[idx], d);
    picked.to_string()
}

fn cycle_wrap<T: Clone + Eq>(all: &[T], cur: T, d: isize) -> T {
    if all.is_empty() {
        return cur;
    }
    let idx = all.iter().position(|x| *x == cur).unwrap_or(0);
    let n = i128::try_from(all.len()).unwrap_or(1).max(1);
    let cur_i = i128::try_from(idx).unwrap_or(0);
    let d_i = i128::try_from(d).unwrap_or(0);
    let wrapped = ((cur_i + d_i) % n + n) % n;
    let next = usize::try_from(wrapped).unwrap_or(0);
    all[next].clone()
}

fn parse_code_blocks(src: &str) -> Vec<CodeBlock> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return vec![CodeBlock {
            name: "Block 1".into(),
            language: "rust".into(),
            code: String::new(),
        }];
    }

    let mut out: Vec<CodeBlock> = Vec::new();
    let mut pending_name: Option<String> = None;
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut buf: Vec<String> = Vec::new();

    for line in src.lines() {
        let l = line.trim_end();
        let t = l.trim();

        if !in_fence {
            if let Some(h) = t.strip_prefix("## ").or_else(|| t.strip_prefix("### ")) {
                pending_name = Some(h.trim().to_string());
                continue;
            }

            if let Some(rest) = t.strip_prefix("```") {
                in_fence = true;
                fence_lang = rest.trim().to_string();
                buf.clear();
                continue;
            }
        } else if t == "```" {
            in_fence = false;
            let name = pending_name
                .take()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("Block {}", out.len() + 1));
            let language = if fence_lang.trim().is_empty() {
                "text".to_string()
            } else {
                fence_lang.trim().to_string()
            };
            out.push(CodeBlock {
                name,
                language,
                code: buf.join("\n").trim_end().to_string(),
            });
            fence_lang.clear();
            buf.clear();
            continue;
        }

        if in_fence {
            buf.push(l.to_string());
        }
    }

    if out.is_empty() {
        return vec![CodeBlock {
            name: pending_name
                .take()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Block 1".into()),
            language: "text".into(),
            code: src.trim_end().to_string(),
        }];
    }

    out
}

fn parse_openapi_contract(src: &str) -> (String, String, String, Vec<OpenApiEndpoint>) {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return (
            "API Contract".into(),
            "1.0.0".into(),
            "https://api.example.com".into(),
            vec![OpenApiEndpoint::default()],
        );
    }

    // Best-effort parser for the YAML we render. If it doesn't match,
    // we fall back to defaults but preserve the field as editable.
    let mut title = "API Contract".to_string();
    let mut version = "1.0.0".to_string();
    let mut base_url = "https://api.example.com".to_string();
    let mut endpoints: Vec<OpenApiEndpoint> = Vec::new();

    let mut cur_path: Option<String> = None;
    let mut cur_method: Option<OpenApiMethod> = None;
    let mut cur_status: u16 = 200;
    let mut seen_req_ct: Option<String> = None;
    let mut seen_resp_ct: Option<String> = None;

    for line in src.lines() {
        let l = line.trim_end();
        let t = l.trim_start();

        if t.starts_with("title:") {
            title = t
                .trim_start_matches("title:")
                .trim()
                .trim_matches('"')
                .to_string();
            continue;
        }
        if t.starts_with("version:") {
            version = t
                .trim_start_matches("version:")
                .trim()
                .trim_matches('"')
                .to_string();
            continue;
        }
        if t.starts_with("- url:") {
            base_url = t
                .trim_start_matches("- url:")
                .trim()
                .trim_matches('"')
                .to_string();
            continue;
        }

        if l.starts_with("  /") && l.ends_with(':') {
            if let (Some(p), Some(m)) = (cur_path.take(), cur_method.take()) {
                endpoints.push(OpenApiEndpoint {
                    method: m,
                    path: p,
                    request_content_type: seen_req_ct
                        .clone()
                        .unwrap_or_else(|| "application/json".into()),
                    request_body: OpenApiBody::None,
                    response_status: cur_status,
                    response_content_type: seen_resp_ct
                        .clone()
                        .unwrap_or_else(|| "application/json".into()),
                    response_body: OpenApiBody::None,
                });
            }
            cur_path = Some(l.trim().trim_end_matches(':').to_string());
            cur_method = None;
            cur_status = 200;
            seen_req_ct = None;
            seen_resp_ct = None;
            continue;
        }

        if l.starts_with("    get:") {
            cur_method = Some(OpenApiMethod::Get);
        } else if l.starts_with("    post:") {
            cur_method = Some(OpenApiMethod::Post);
        } else if l.starts_with("    put:") {
            cur_method = Some(OpenApiMethod::Put);
        } else if l.starts_with("    patch:") {
            cur_method = Some(OpenApiMethod::Patch);
        } else if l.starts_with("    delete:") {
            cur_method = Some(OpenApiMethod::Delete);
        } else if t.starts_with("'") && t.ends_with("':") {
            let code = t.trim_matches('\'').trim_end_matches(':');
            if let Ok(n) = code.parse::<u16>() {
                cur_status = n;
            }
        } else if t == "application/json:"
            || t == "text/plain:"
            || t == "application/xml:"
            || t == "text/markdown:"
        {
            let ct = t.trim_end_matches(':').to_string();
            // heuristic: first seen is request, second is response
            if seen_req_ct.is_none() {
                seen_req_ct = Some(ct);
            } else {
                seen_resp_ct = Some(ct);
            }
        }
    }

    if let (Some(p), Some(m)) = (cur_path.take(), cur_method.take()) {
        endpoints.push(OpenApiEndpoint {
            method: m,
            path: p,
            request_content_type: seen_req_ct.unwrap_or_else(|| "application/json".into()),
            request_body: OpenApiBody::None,
            response_status: cur_status,
            response_content_type: seen_resp_ct.unwrap_or_else(|| "application/json".into()),
            response_body: OpenApiBody::None,
        });
    }

    if endpoints.is_empty() {
        endpoints.push(OpenApiEndpoint::default());
    }
    (title, version, base_url, endpoints)
}

fn render_openapi_contract(
    title: &str,
    version: &str,
    base_url: &str,
    endpoints: &[OpenApiEndpoint],
    schemas: &[OpenApiSchema],
) -> String {
    let mut out = String::new();
    out.push_str("openapi: \"3.1.0\"\n");
    out.push_str("info:\n");
    out.push_str(&format!("  title: \"{}\"\n", yaml_escape(title)));
    out.push_str(&format!("  version: \"{}\"\n", yaml_escape(version)));
    out.push_str("servers:\n");
    out.push_str(&format!("  - url: \"{}\"\n", yaml_escape(base_url)));
    if !schemas.is_empty() {
        out.push_str("components:\n");
        out.push_str("  schemas:\n");
        for s in schemas {
            out.push_str(&render_openapi_schema(s));
        }
    }
    out.push_str("paths:\n");

    let mut by_path: std::collections::BTreeMap<&str, Vec<&OpenApiEndpoint>> =
        std::collections::BTreeMap::new();
    for ep in endpoints {
        by_path.entry(ep.path.as_str()).or_default().push(ep);
    }

    for (path, eps) in by_path {
        out.push_str(&format!("  {}:\n", path));
        for ep in eps {
            out.push_str(&format!("    {}:\n", openapi_method_str(ep.method)));
            if !matches!(ep.request_body, OpenApiBody::None) {
                out.push_str("      requestBody:\n");
                out.push_str("        required: true\n");
                out.push_str("        content:\n");
                out.push_str(&format!("          {}:\n", ep.request_content_type));
                out.push_str("            schema:\n");
                out.push_str(&render_openapi_body_schema(
                    &ep.request_body,
                    "              ",
                ));
            }
            out.push_str("      responses:\n");
            out.push_str(&format!("        '{}':\n", ep.response_status));
            if matches!(ep.response_body, OpenApiBody::None) {
                out.push_str("          description: No response body\n");
            } else {
                out.push_str("          description: OK\n");
                out.push_str("          content:\n");
                out.push_str(&format!("            {}:\n", ep.response_content_type));
                out.push_str("              schema:\n");
                out.push_str(&render_openapi_body_schema(
                    &ep.response_body,
                    "                ",
                ));
            }
        }
    }

    out
}

fn render_openapi_schema(s: &OpenApiSchema) -> String {
    let mut out = String::new();
    let name = s.name.trim();
    if name.is_empty() {
        return out;
    }
    out.push_str(&format!("    {}:\n", name));
    out.push_str("      type: object\n");
    out.push_str(&format!(
        "      additionalProperties: {}\n",
        if s.additional_properties {
            "true"
        } else {
            "false"
        }
    ));
    let required: Vec<String> = s
        .properties
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    if !required.is_empty() {
        out.push_str("      required:\n");
        for r in required {
            out.push_str(&format!("        - {}\n", r));
        }
    }
    out.push_str("      properties:\n");
    for p in &s.properties {
        let pname = p.name.trim();
        if pname.is_empty() {
            continue;
        }
        out.push_str(&format!("        {}:\n", pname));
        out.push_str(&format!(
            "          type: {}\n",
            openapi_prop_type_str(p.kind)
        ));
        if p.nullable {
            out.push_str("          nullable: true\n");
        }
        if p.kind == OpenApiPropType::String {
            if let Some(fmt) = openapi_string_format_str(p.format) {
                out.push_str(&format!("          format: {}\n", fmt));
            }
            if let Some(min) = p.min {
                out.push_str(&format!("          minLength: {}\n", min));
            }
            if let Some(max) = p.max {
                out.push_str(&format!("          maxLength: {}\n", max));
            }
            if !p.pattern.trim().is_empty() {
                out.push_str(&format!(
                    "          pattern: \"{}\"\n",
                    yaml_escape(p.pattern.trim())
                ));
            }
        } else if matches!(p.kind, OpenApiPropType::Integer | OpenApiPropType::Number) {
            if let Some(min) = p.min {
                out.push_str(&format!("          minimum: {}\n", min));
            }
            if let Some(max) = p.max {
                out.push_str(&format!("          maximum: {}\n", max));
            }
        }
        if !p.enum_values.is_empty() {
            out.push_str("          enum:\n");
            for e in &p.enum_values {
                let v = e.trim();
                if !v.is_empty() {
                    out.push_str(&format!("            - {}\n", v));
                }
            }
        }
    }
    out
}

pub(crate) fn openapi_prop_type_str(t: OpenApiPropType) -> &'static str {
    match t {
        OpenApiPropType::String => "string",
        OpenApiPropType::Integer => "integer",
        OpenApiPropType::Number => "number",
        OpenApiPropType::Boolean => "boolean",
    }
}

fn openapi_string_format_str(f: OpenApiStringFormat) -> Option<&'static str> {
    match f {
        OpenApiStringFormat::None => None,
        OpenApiStringFormat::Email => Some("email"),
        OpenApiStringFormat::Password => Some("password"),
        OpenApiStringFormat::Uri => Some("uri"),
    }
}

fn openapi_method_str(m: OpenApiMethod) -> &'static str {
    match m {
        OpenApiMethod::Get => "get",
        OpenApiMethod::Post => "post",
        OpenApiMethod::Put => "put",
        OpenApiMethod::Patch => "patch",
        OpenApiMethod::Delete => "delete",
    }
}

fn render_openapi_body_schema(body: &OpenApiBody, indent: &str) -> String {
    match body {
        OpenApiBody::None => format!("{indent}type: object\n"),
        OpenApiBody::String => format!("{indent}type: string\n"),
        OpenApiBody::Integer => format!("{indent}type: integer\n"),
        OpenApiBody::Number => format!("{indent}type: number\n"),
        OpenApiBody::Boolean => format!("{indent}type: boolean\n"),
        OpenApiBody::SchemaRef(name) => {
            format!(
                "{indent}$ref: \"#/components/schemas/{}\"\n",
                yaml_escape(name)
            )
        }
    }
}

pub(crate) fn render_openapi_preview(ed: &OpenApiEditorState) -> String {
    render_openapi_contract(
        &ed.title,
        &ed.version,
        &ed.base_url,
        &ed.endpoints,
        &ed.schemas,
    )
}

pub(crate) fn render_openapi_footer(ed: &OpenApiEditorState) -> String {
    match &ed.mode {
        OpenApiEditorMode::Browse => {
            let Some(ep) = ed.endpoints.get(ed.endpoint_cursor) else {
                return "No endpoints. Press `n` to add one.".into();
            };
            format!(
                "Selected: {} {}\nreq: {} {}\nresp: {} {} {}\n(edit: u path, m method, s status, c content-type, r req body, p resp body)",
                openapi_method_str(ep.method).to_uppercase(),
                ep.path,
                ep.request_content_type,
                render_openapi_body_label(&ep.request_body),
                ep.response_status,
                ep.response_content_type,
                render_openapi_body_label(&ep.response_body),
            )
        }
        OpenApiEditorMode::Schemas => {
            let Some(s) = ed.schemas.get(ed.schema_cursor) else {
                return "Schemas: none. Press `n` to add one. (g toggles back to endpoints)".into();
            };
            let prop = s.properties.get(ed.prop_cursor).map(|p| {
                format!(
                    "prop: {}{} type={} nullable={} fmt={}",
                    p.name,
                    if p.required { " *" } else { "" },
                    openapi_prop_type_str(p.kind),
                    if p.nullable { "true" } else { "false" },
                    openapi_string_format_str(p.format).unwrap_or("-")
                )
            });
            format!(
                "Schema: {} (props: {})\n{}\n(keys: Tab schema · Up/Down prop · p add prop · x del prop · Space required · t type · f format · a nullable · / pattern · e enum · i/k min/max)",
                s.name,
                s.properties.len(),
                prop.unwrap_or_else(|| "no properties".into())
            )
        }
        OpenApiEditorMode::AddEndpoint => "Press Enter to create a new endpoint.".into(),
        OpenApiEditorMode::AddSchema { input } => {
            format!("New schema name:\n{input}\n(Enter to create)")
        }
        OpenApiEditorMode::RenameSchema { input } => {
            format!("Rename schema:\n{input}\n(Enter to confirm)")
        }
        OpenApiEditorMode::AddProp { input } => {
            format!("New property name:\n{input}\n(Enter to add)")
        }
        OpenApiEditorMode::RenameProp { input } => {
            format!("Rename property:\n{input}\n(Enter to confirm)")
        }
        OpenApiEditorMode::EditPropPattern { input } => {
            format!("Pattern (regex):\n{input}\n(Enter to set)")
        }
        OpenApiEditorMode::EditPropEnum { input } => {
            format!("Enum values (comma-separated):\n{input}\n(Enter to set)")
        }
        OpenApiEditorMode::EditPropMin { input } => {
            format!("Min (minLength/minimum):\n{input}\n(Enter to set, blank clears)")
        }
        OpenApiEditorMode::EditPropMax { input } => {
            format!("Max (maxLength/maximum):\n{input}\n(Enter to set, blank clears)")
        }
        OpenApiEditorMode::EditPath { input } => format!("Edit path:\n{input}\n(Enter to confirm)"),
        OpenApiEditorMode::EditTitle { input } => {
            format!("Edit API title:\n{input}\n(Enter to confirm)")
        }
        OpenApiEditorMode::EditVersion { input } => {
            format!("Edit version (e.g. 1.0.0):\n{input}\n(Enter to confirm)")
        }
        OpenApiEditorMode::EditBaseUrl { input } => {
            format!("Edit base URL:\n{input}\n(Enter to confirm)")
        }
    }
}

fn render_openapi_body_label(body: &OpenApiBody) -> String {
    match body {
        OpenApiBody::None => "none".into(),
        OpenApiBody::String => "string".into(),
        OpenApiBody::Integer => "integer".into(),
        OpenApiBody::Number => "number".into(),
        OpenApiBody::Boolean => "boolean".into(),
        OpenApiBody::SchemaRef(name) => format!("$ref({name})"),
    }
}

fn render_code_blocks(blocks: &[CodeBlock]) -> String {
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let name = b.name.trim();
        out.push_str("## ");
        out.push_str(if name.is_empty() { "Block" } else { name });
        out.push('\n');
        out.push_str("```");
        let lang = b.language.trim();
        if !lang.is_empty() {
            out.push_str(lang);
        }
        out.push('\n');
        if !b.code.is_empty() {
            out.push_str(b.code.trim_end());
            out.push('\n');
        }
        out.push_str("```\n");
    }
    out
}

fn parse_gherkin_features(src: &str) -> Vec<GherkinFeature> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return vec![GherkinFeature {
            name: "Test cases".into(),
            background: String::new(),
            scenarios: vec![GherkinScenario {
                name: "Scenario 1".into(),
                steps: String::new(),
            }],
        }];
    }

    let mut features: Vec<GherkinFeature> = Vec::new();
    let mut current: Option<GherkinFeature> = None;

    let mut i = 0usize;
    let lines: Vec<&str> = src.lines().collect();
    while i < lines.len() {
        let line = lines[i].trim_end();
        let t = line.trim();

        if let Some(rest) = t.strip_prefix("Feature:") {
            if let Some(f) = current.take() {
                features.push(f);
            }
            current = Some(GherkinFeature {
                name: rest.trim().to_string(),
                background: String::new(),
                scenarios: Vec::new(),
            });
            i += 1;
            continue;
        }

        if current.is_none() {
            i += 1;
            continue;
        }

        if t.starts_with("Background:") {
            i += 1;
            let mut bg: Vec<String> = Vec::new();
            while i < lines.len() {
                let tt = lines[i].trim();
                if tt.starts_with("Scenario:")
                    || tt.starts_with("Scenario Outline:")
                    || tt.starts_with("Feature:")
                {
                    break;
                }
                if !tt.is_empty() {
                    bg.push(tt.to_string());
                }
                i += 1;
            }
            if let Some(f) = current.as_mut() {
                f.background = bg.join("\n");
            }
            continue;
        }

        if t.starts_with("Scenario:") || t.starts_with("Scenario Outline:") {
            let head = t;
            let name = head
                .strip_prefix("Scenario:")
                .or_else(|| head.strip_prefix("Scenario Outline:"))
                .map(|s| s.trim())
                .unwrap_or("")
                .to_string();
            i += 1;
            let mut step_lines: Vec<String> = Vec::new();
            while i < lines.len() {
                let tt = lines[i].trim();
                if tt.starts_with("Scenario:")
                    || tt.starts_with("Scenario Outline:")
                    || tt.starts_with("Feature:")
                {
                    break;
                }
                if tt.starts_with('@') {
                    // Tags not supported in this field type; ignore.
                    i += 1;
                    continue;
                }
                if !tt.is_empty() {
                    step_lines.push(tt.to_string());
                }
                i += 1;
            }
            if let Some(f) = current.as_mut() {
                let idx = f.scenarios.len() + 1;
                f.scenarios.push(GherkinScenario {
                    name: if name.is_empty() {
                        format!("Scenario {idx}")
                    } else {
                        name
                    },
                    steps: step_lines.join("\n"),
                });
            }
            continue;
        }

        i += 1;
    }

    if let Some(f) = current.take() {
        features.push(f);
    }

    if features.is_empty() {
        features.push(GherkinFeature {
            name: "Test cases".into(),
            background: String::new(),
            scenarios: vec![GherkinScenario {
                name: "Scenario 1".into(),
                steps: String::new(),
            }],
        });
    }

    for f in &mut features {
        if f.name.trim().is_empty() {
            f.name = "Test cases".into();
        }
        if f.scenarios.is_empty() {
            f.scenarios.push(GherkinScenario {
                name: "Scenario 1".into(),
                steps: String::new(),
            });
        }
    }

    features
}

pub(crate) fn render_gherkin_features(features: &[GherkinFeature]) -> String {
    fn push_indented(out: &mut String, indent: &str, text: &str) {
        for l in text.lines() {
            let t = l.trim_end();
            if t.is_empty() {
                continue;
            }
            out.push_str(indent);
            out.push_str(t);
            out.push('\n');
        }
    }

    let mut out = String::new();
    for (fi, f) in features.iter().enumerate() {
        if fi > 0 {
            out.push('\n');
            out.push('\n');
        }
        let name = f.name.trim();
        out.push_str("Feature: ");
        out.push_str(if name.is_empty() { "Test cases" } else { name });
        out.push('\n');
        out.push('\n');

        let bg = f.background.trim_end();
        if !bg.trim().is_empty() {
            out.push_str("  Background:\n");
            push_indented(&mut out, "    ", bg);
            out.push('\n');
        }

        for (si, sc) in f.scenarios.iter().enumerate() {
            if si > 0 {
                out.push('\n');
            }
            out.push_str("  Scenario: ");
            let name = sc.name.trim();
            out.push_str(if name.is_empty() { "Scenario" } else { name });
            out.push('\n');
            push_indented(&mut out, "    ", sc.steps.trim_end());
        }
    }

    out.trim_end().to_string()
}

fn validate_gherkin_steps(src: &str) -> Result<(), String> {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Seen {
        None,
        Given,
        When,
        Then,
    }

    let mut seen = Seen::None;
    let mut has_given = false;
    let mut has_when = false;
    let mut has_then = false;

    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }

        let (kw, _rest) = t.split_once(' ').map(|(a, b)| (a, b)).unwrap_or((t, ""));

        match kw {
            "Given" => {
                has_given = true;
                seen = Seen::Given;
            }
            "When" => {
                if !has_given {
                    return Err(format!(
                        "steps line {line_no}: `When` cannot appear before `Given`"
                    ));
                }
                has_when = true;
                seen = Seen::When;
            }
            "Then" => {
                if !has_when {
                    return Err(format!(
                        "steps line {line_no}: `Then` cannot appear before `When`"
                    ));
                }
                has_then = true;
                seen = Seen::Then;
            }
            "And" | "But" => {
                if seen == Seen::None {
                    return Err(format!(
                        "steps line {line_no}: `And/But` must follow Given/When/Then"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "steps line {line_no}: must start with Given/When/Then/And/But (got `{other}`)"
                ));
            }
        }
    }

    if src.trim().is_empty() {
        return Ok(());
    }
    if !(has_given && has_when && has_then) {
        return Err("steps must include at least one Given, one When, and one Then".into());
    }

    Ok(())
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
    // Edge context emitted outside Mermaid fences (our UI-friendly format).
    // Applies to the next fenced Mermaid block we parse.
    let mut pending_edge_contexts: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut in_edge_context = false;
    let mut edge_ctx_current_id: Option<String> = None;

    for line in src.lines() {
        let l = line.trim_end();
        if !in_mermaid {
            if let Some(h) = l.strip_prefix("## ").or_else(|| l.strip_prefix("### ")) {
                pending_name = Some(h.trim().to_string());
                in_edge_context = false;
                continue;
            }
            let t = l.trim();
            let t_unbold = t.trim_matches('*').trim();
            if t_unbold.eq_ignore_ascii_case("edgecontext:")
                || t.eq_ignore_ascii_case("edgecontext")
                || t.eq_ignore_ascii_case("context:")
                || t.eq_ignore_ascii_case("context")
            {
                in_edge_context = true;
                continue;
            }
            if in_edge_context {
                // Stop the edgeContext block when the Mermaid fence begins.
                // We must not consume the fence line here, otherwise we'd treat
                // the entire Mermaid body as edge context.
                if t == "```mermaid" {
                    in_edge_context = false;
                    edge_ctx_current_id = None;
                } else {
                    if t.is_empty() {
                        in_edge_context = false;
                        edge_ctx_current_id = None;
                        continue;
                    }
                    // Lines like:
                    // - "[R1] -> some text" (preferred)
                    // - "[R1]: some text" (legacy)
                    // - "[R1]" + newline + "some text" (preferred multi-line)
                    // - "- [R1] -> some text"
                    // - "R1: some text"
                    let rest = t.trim_start_matches('-').trim();
                    let rest_unbold = rest.trim_matches('*').trim();
                    // Multi-line format: a bare "[R1]" sets the current relation id,
                    // and subsequent lines become its context until another id/fence/heading.
                    if rest_unbold.starts_with('[')
                        && rest_unbold.ends_with(']')
                        && !rest_unbold.contains("->")
                    {
                        let id = rest_unbold
                            .trim()
                            .trim_start_matches('[')
                            .trim_end_matches(']');
                        if !id.is_empty() {
                            edge_ctx_current_id = Some(id.to_string());
                            pending_edge_contexts.entry(id.to_string()).or_default();
                            continue;
                        }
                    }
                    let pair = rest_unbold
                        .split_once("->")
                        .or_else(|| rest.split_once(':'))
                        .map(|(lhs, rhs)| (lhs.trim(), rhs.trim()));
                    if let Some((lhs, rhs)) = pair {
                        let id = lhs.trim().trim_start_matches('[').trim_end_matches(']');
                        let txt = rhs.trim();
                        if !id.is_empty() {
                            if txt.is_empty() {
                                pending_edge_contexts.remove(id);
                            } else {
                                pending_edge_contexts.insert(id.to_string(), txt.to_string());
                            }
                        }
                        edge_ctx_current_id = None;
                        continue;
                    }
                    if let Some(id) = edge_ctx_current_id.clone() {
                        let txt = rest;
                        if !txt.is_empty() {
                            pending_edge_contexts
                                .entry(id)
                                .and_modify(|cur| {
                                    if !cur.is_empty() {
                                        cur.push('\n');
                                    }
                                    cur.push_str(txt);
                                })
                                .or_insert_with(|| txt.to_string());
                            continue;
                        }
                    }
                    // Not an edge-context mapping line; stop treating subsequent lines
                    // as part of the block.
                    in_edge_context = false;
                    edge_ctx_current_id = None;
                }
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
            let mut parsed = parse_mermaid_diagram(&name, &buf.join("\n"));
            // Apply any outside-the-fence edge context we collected.
            if !pending_edge_contexts.is_empty() {
                attach_edge_contexts(&mut parsed, &pending_edge_contexts);
                pending_edge_contexts.clear();
            }
            // Drop empty placeholder diagrams (e.g. a fenced block that only
            // contains `sequenceDiagram`). These often appear as “source of truth”
            // stubs in prompts and should not round-trip into an extra diagram.
            if !(parsed.participants.is_empty() && parsed.events.is_empty()) {
                diagrams.push(parsed);
            }
            buf.clear();
            continue;
        }

        if in_mermaid {
            buf.push(l.to_string());
        }
    }

    if !diagrams.is_empty() {
        // Extra safety: if any empty diagrams slipped through (e.g. weirdly
        // formatted input), drop them.
        diagrams.retain(|d| !(d.participants.is_empty() && d.events.is_empty()));
        if !diagrams.is_empty() {
            return diagrams;
        }
    }

    // Fallback: treat as a single diagram body.
    let one = parse_mermaid_diagram("Diagram", src);
    if one.participants.is_empty() && one.events.is_empty() {
        // Keep exactly one empty diagram so the editor still has something to show.
        vec![SequenceDiagram {
            name: "Diagram".into(),
            participants: Vec::new(),
            events: Vec::new(),
        }]
    } else {
        vec![one]
    }
}

fn parse_mermaid_diagram(name: &str, body: &str) -> SequenceDiagram {
    let mut participants: Vec<String> = Vec::new();
    let mut events: Vec<SequenceEvent> = Vec::new();
    let mut edge_contexts: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut in_edge_context = false;
    let mut edge_ctx_current_id: Option<String> = None;
    let mut last_message_idx: Option<usize> = None;

    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("```") {
            continue;
        }
        if l.starts_with("sequenceDiagram") {
            continue;
        }
        // EdgeContext block parsing: supports both legacy `%%`-commented
        // lines and the new plain-text block.
        let (is_comment, rest) = if let Some(r) = l.strip_prefix("%%") {
            (true, r.trim())
        } else {
            (false, l)
        };
        let rest_unbold = rest.trim_matches('*').trim();
        if rest_unbold.eq_ignore_ascii_case("edgecontext:")
            || rest_unbold.eq_ignore_ascii_case("edgecontext")
            || rest_unbold.eq_ignore_ascii_case("context:")
            || rest_unbold.eq_ignore_ascii_case("context")
        {
            in_edge_context = true;
            edge_ctx_current_id = None;
            continue;
        }
        if in_edge_context {
            // Lines like:
            // - "[R1] -> some text" (preferred)
            // - "[R1]: some text" (legacy)
            // Brackets optional, text may be blank.
            let rest = rest.trim_start_matches('-').trim();
            let rest_unbold = rest.trim_matches('*').trim();
            // Multi-line format: "[R1]" alone, followed by one or more lines.
            if rest_unbold.starts_with('[')
                && rest_unbold.ends_with(']')
                && !rest_unbold.contains("->")
            {
                let id = rest_unbold
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']');
                if !id.is_empty() {
                    edge_ctx_current_id = Some(id.to_string());
                    edge_contexts.entry(id.to_string()).or_default();
                    // Always consume this line as an id marker.
                    continue;
                }
            }
            let pair = rest_unbold
                .split_once("->")
                .or_else(|| rest.split_once(':'))
                .map(|(lhs, rhs)| (lhs.trim(), rhs.trim()));
            if let Some((lhs, rhs)) = pair {
                let id = lhs.trim().trim_start_matches('[').trim_end_matches(']');
                let txt = rhs.trim();
                if !id.is_empty() {
                    if !txt.is_empty() {
                        edge_contexts.insert(id.to_string(), txt.to_string());
                    } else {
                        edge_contexts.remove(id);
                    }
                }
                edge_ctx_current_id = None;
            }
            if let Some(id) = edge_ctx_current_id.clone() {
                // Treat any other line as part of the current id's context.
                if !rest.is_empty() && !rest.contains(':') && !rest.contains("->") {
                    edge_contexts
                        .entry(id)
                        .and_modify(|cur| {
                            if !cur.is_empty() {
                                cur.push('\n');
                            }
                            cur.push_str(rest);
                        })
                        .or_insert_with(|| rest.to_string());
                }
            }
            // Only treat comment lines as part of the block unconditionally;
            // for plain-text, we stay in the block until EOF (good enough for
            // our generated format, which places EdgeContext at the end).
            if is_comment {
                continue;
            }
            // If this is not a comment line and doesn't match our `id:` shape,
            // fall through so it can be parsed as a message line.
        }
        // Mermaid notes (preferred output): attach to the previous message.
        // Example:
        //   Note over Alice,John: A typical interaction<br/>two lines
        //   Note over Alice: self interaction
        if let Some(rest) = l.strip_prefix("Note over ") {
            if let Some((who, text)) = rest.split_once(':') {
                let who = who.trim();
                let text = text.trim();
                if let Some(idx) = last_message_idx {
                    // Convert <br/> back to literal newlines for the editor/storage.
                    let normalized = text.replace("<br/>", "\n").trim().to_string();
                    if !normalized.is_empty() {
                        // Basic sanity: only attach if participants match the previous message
                        // (prevents notes from accidentally sticking to the wrong edge).
                        let matches = match events.get(idx) {
                            Some(SequenceEvent::Message { from, to, .. }) => {
                                if from == to {
                                    who == from
                                } else {
                                    who == format!("{from},{to}") || who == format!("{to},{from}")
                                }
                            }
                            _ => false,
                        };
                        if matches {
                            if let Some(SequenceEvent::Message { edge_context, .. }) =
                                events.get_mut(idx)
                            {
                                *edge_context = Some(normalized);
                            }
                        }
                    }
                }
                continue;
            }
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
        let mut lhs = lhs.trim().to_string();
        let mut rel_id_from_lhs: Option<String> = None;
        // New format: "[R12] A->>B: msg" (rel id prefixed on the left)
        if let Some(rest) = lhs.strip_prefix('[') {
            if let Some((id, tail)) = rest.split_once(']') {
                let id = id.trim();
                if !id.is_empty() && !id.contains(' ') {
                    rel_id_from_lhs = Some(id.to_string());
                    lhs = tail.trim().to_string();
                }
            }
        }
        let from = lhs.trim();
        let Some((to, text)) = rhs.split_once(':') else {
            continue;
        };
        let to = to.trim();
        let mut text = text.trim().to_string();
        let mut rel_id: Option<String> = None;
        // Legacy format: "A->>B: [R12] actual message"
        if let Some(rest) = text.strip_prefix('[') {
            if let Some((id, tail)) = rest.split_once(']') {
                let id = id.trim();
                if !id.is_empty() && !id.contains(' ') {
                    rel_id = Some(id.to_string());
                    text = tail.trim().to_string();
                }
            }
        }
        if from.is_empty() || to.is_empty() || text.is_empty() {
            continue;
        }
        for pname in [from, to] {
            if !participants.iter().any(|p| p == pname) {
                participants.push(pname.to_string());
            }
        }
        let rel_id = rel_id
            .or(rel_id_from_lhs)
            .unwrap_or_else(|| format!("R{}", events.len() + 1));
        events.push(SequenceEvent::Message {
            from: from.to_string(),
            to: to.to_string(),
            dashed,
            rel_id,
            text: text.to_string(),
            edge_context: None,
        });
        last_message_idx = Some(events.len().saturating_sub(1));
    }
    // Attach parsed edge context to matching rel_ids.
    for ev in &mut events {
        let SequenceEvent::Message {
            rel_id,
            edge_context,
            ..
        } = ev;
        if let Some(txt) = edge_contexts.get(rel_id) {
            *edge_context = Some(txt.clone());
        }
    }
    SequenceDiagram {
        name: name.to_string(),
        participants,
        events,
    }
}

fn attach_edge_contexts(
    diag: &mut SequenceDiagram,
    edge_contexts: &std::collections::BTreeMap<String, String>,
) {
    for ev in &mut diag.events {
        let SequenceEvent::Message {
            rel_id,
            edge_context,
            ..
        } = ev;
        if let Some(txt) = edge_contexts.get(rel_id) {
            *edge_context = Some(txt.clone());
        }
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

fn next_relation_id(diag: &SequenceDiagram) -> String {
    // Default format: R1, R2, ... unique within a diagram.
    let mut max_n: u64 = 0;
    for ev in &diag.events {
        let SequenceEvent::Message { rel_id, .. } = ev;
        if let Some(rest) = rel_id.strip_prefix('R') {
            if let Ok(n) = rest.parse::<u64>() {
                max_n = max_n.max(n);
            }
        }
    }
    format!("R{}", max_n.saturating_add(1))
}

fn render_mermaid_body(participants: &[String], events: &[SequenceEvent]) -> String {
    fn mermaid_text_inline(s: &str) -> String {
        // Mermaid sequenceDiagram supports `<br/>` in messages/notes to wrap.
        // Keep output strictly one line per Mermaid statement (no literal newlines).
        s.replace('\n', "<br/>").trim().to_string()
    }

    fn mermaid_note_text(s: &str) -> Option<String> {
        // Like `mermaid_text_inline`, but treat "only line breaks / whitespace" as empty
        // so we never emit `Note ...:` with no actual content.
        let cooked = mermaid_text_inline(s);
        if cooked.is_empty() {
            return None;
        }
        let without_breaks = cooked.replace("<br/>", "");
        if without_breaks.trim().is_empty() {
            return None;
        }
        Some(cooked)
    }

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
                edge_context,
                rel_id: _,
            } => {
                let arrow = if *dashed { "-->>" } else { "->>" };
                out.push_str("    ");
                out.push_str(from);
                out.push_str(arrow);
                out.push_str(to);
                out.push_str(": ");
                out.push_str(&mermaid_text_inline(text));
                out.push('\n');
                if let Some(ctx) = edge_context
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let Some(ctx) = mermaid_note_text(ctx) else {
                        continue;
                    };
                    out.push_str("    Note over ");
                    if from == to {
                        out.push_str(from);
                    } else {
                        out.push_str(from);
                        out.push(',');
                        out.push_str(to);
                    }
                    out.push_str(": ");
                    out.push_str(&ctx);
                    out.push('\n');
                }
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
    DeleteTemplate(TemplateId),
    DeleteTask(TaskId),
}

/// Friendly, multi-line example rendered into the `Fields (TOML)`
/// editor when the user creates a new template. We deliberately show
/// the common kinds so the UX doubles as documentation.
fn default_fields_toml_hint() -> String {
    "# one [[fields]] block per field. kinds: textarea, select,\n\
     # multiselect, sequence-gram, code-blocks.\n\
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
fn open_create_task_form(model: &mut Model, tid: TemplateId) {
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
        submit: FormSubmit::CreateTask(tid),
    });
}

/// Build the form row that represents a single template-declared field
/// on the task-creation form. `kind` drives rendering (multiline for
/// `textarea`) and submit-time parsing of the typed value.
fn form_field_for_template_field(field: &lattice_core::fields::Field) -> FormField {
    let label = format_field_label(field);
    let multiline = matches!(
        field.kind,
        FieldKind::Textarea
            | FieldKind::SequenceGram
            | FieldKind::CodeBlocks
            | FieldKind::Gherkin
            | FieldKind::OpenApi
    );
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
        FieldKind::Textarea => "textarea",
        FieldKind::Select => "select",
        FieldKind::Multiselect => "multiselect",
        FieldKind::SequenceGram => "sequence-gram",
        FieldKind::CodeBlocks => "code-blocks",
        FieldKind::Gherkin => "gherkin",
        FieldKind::OpenApi => "openapi",
    };
    format!("{} [{}]", field.label, kind_label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::entities::Template;
    use lattice_core::fields::{Field, FieldKind, FieldOptions, Validation};
    use lattice_core::time::Timestamp;

    #[test]
    fn update_tab_cycle() {
        let mut m = Model::new();
        assert_eq!(m.screen, Screen::Templates);
        update(&mut m, Msg::NextTab);
        assert_eq!(m.screen, Screen::Tasks);
        update(&mut m, Msg::PrevTab);
        assert_eq!(m.screen, Screen::Templates);
    }

    #[test]
    fn sequence_gram_roundtrips_with_relation_ids_and_edge_context() {
        let src = r#"sequenceDiagram
participant A
participant B
A->>B: [R1] Do the thing
**edgeContext:**
**[R1]**
Must be idempotent
"#;
        let d = parse_mermaid_diagram("D", src);
        assert_eq!(d.events.len(), 1);
        let SequenceEvent::Message {
            rel_id,
            edge_context,
            text,
            ..
        } = &d.events[0];
        assert_eq!(rel_id, "R1");
        assert_eq!(text, "Do the thing");
        assert_eq!(edge_context.as_deref(), Some("Must be idempotent"));

        let rendered = render_sequence_gram(&[d.clone()]);
        // Mermaid block should be standard-compliant (no [R*] prefix, no edgeContext inside).
        assert!(rendered.contains("```mermaid\nsequenceDiagram\n"));
        assert!(rendered.contains("A->>B: Do the thing"));
        assert!(!rendered.contains("[R1] A->>B"));
        assert!(!rendered.contains("edgeContext:\nsequenceDiagram"));
        // Edge context should be rendered as a Mermaid note.
        assert!(rendered.contains("Note over A,B: Must be idempotent"));
        // And it should parse back.
        let parsed_back = parse_sequence_gram(&rendered);
        assert_eq!(parsed_back.len(), 1);
        assert_eq!(parsed_back[0].events.len(), 1);
        let SequenceEvent::Message {
            rel_id,
            edge_context,
            text,
            ..
        } = &parsed_back[0].events[0];
        assert_eq!(rel_id, "R1");
        assert_eq!(text, "Do the thing");
        assert_eq!(edge_context.as_deref(), Some("Must be idempotent"));
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
        let mut tpl = Template::new("refactor", now);
        tpl.fields.push(Field {
            id: "module".into(),
            kind: FieldKind::Textarea,
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
        assert_eq!(form.fields[1].kind, Some(FieldKind::Textarea));
        assert!(form.fields[1].multiline);
        assert!(form.fields[1].required);
        assert_eq!(form.fields[2].field_id.as_deref(), Some("description"));
        assert_eq!(form.fields[2].kind, Some(FieldKind::Textarea));
        assert!(form.fields[2].multiline);
    }

    #[test]
    fn open_create_task_warns_without_templates() {
        let mut m = Model::new();
        update(&mut m, Msg::OpenCreateTask);
        assert!(m.form.is_none());
        assert!(m.picker.is_none());
        assert_eq!(m.toasts.len(), 1);
    }

    #[test]
    fn open_create_task_opens_picker_with_multiple_templates() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
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
        let FormSubmit::CreateTask(tid) = form.submit else {
            panic!("unexpected submit: {:?}", form.submit);
        };
        assert_eq!(tid, expected_tid);
    }

    #[test]
    fn template_picker_cancel_closes_without_form() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
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
        // Name, Fields, Prompt.
        assert_eq!(form.fields.len(), 3);
        assert!(
            form.fields[1].label.starts_with("Fields"),
            "second field should be the Fields TOML editor, got: {}",
            form.fields[1].label
        );
        assert!(form.fields[1].multiline);
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
            submit: FormSubmit::CreateTemplate,
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
            submit: FormSubmit::CreateTemplate,
        });
        update(&mut m, Msg::FormNext);
        let f = &m.form.as_ref().unwrap();
        assert_eq!(f.cursor, 1);
        assert_eq!(f.fields[1].caret, 5);
    }

    #[test]
    fn open_save_task_prompt_opens_form() {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut m = Model::new();
        let t = lattice_core::entities::Task::new(TemplateId::new(), 1, "demo", now);
        let tid = t.id;
        m.tasks = vec![t];

        update(&mut m, Msg::OpenSaveTaskPrompt(tid));
        let form = m.form.as_ref().expect("save form should open");
        assert!(form.title.contains("Save prompt"));
        assert_eq!(form.fields.len(), 1);
        assert_eq!(form.fields[0].label, "File name");
        assert_eq!(form.fields[0].value, "demo");
        assert!(matches!(
            form.submit,
            FormSubmit::SaveTaskPromptToFile(t) if t == tid
        ));
    }
}
