//! Keyboard → `Msg` translation.
//!
//! We keep the mapping centralized so the Help screen can render the
//! same list the shell consumes — no drift between docs and behavior.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{Model, Msg, Screen};

/// Convert a keystroke to an optional `Msg`, considering global
/// overlays first (palette / confirm / form / help).
pub fn translate(model: &Model, key: KeyEvent) -> Option<Msg> {
    // Help screen: any key returns to previous.
    if matches!(model.screen, Screen::Help) {
        return Some(Msg::GoScreen(model.prev_screen));
    }

    // Palette captures everything while open.
    if model.palette_open {
        return match key.code {
            KeyCode::Esc => Some(Msg::PalToggle),
            KeyCode::Enter => Some(Msg::PalAccept),
            KeyCode::Backspace => Some(Msg::PalBackspace),
            KeyCode::Up => Some(Msg::PalMove(-1)),
            KeyCode::Down => Some(Msg::PalMove(1)),
            KeyCode::Char(c) => Some(Msg::PalInput(c)),
            _ => None,
        };
    }

    // Confirm prompt: Enter = ack, Esc = cancel.
    if model.confirm.is_some() {
        return match key.code {
            KeyCode::Enter | KeyCode::Char('y' | 'Y') => Some(Msg::AckConfirm),
            KeyCode::Esc | KeyCode::Char('n' | 'N') => Some(Msg::CancelConfirm),
            _ => None,
        };
    }

    // Sequence diagram editor captures everything while open.
    if let Some(ed) = &model.sequence_editor {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // Keys that always work (even while typing).
        if matches!(key.code, KeyCode::Esc) {
            return Some(Msg::SeqEdCancel);
        }
        if matches!(key.code, KeyCode::F(2)) {
            return Some(Msg::SeqEdSave);
        }
        if matches!(key.code, KeyCode::Backspace) {
            return Some(Msg::SeqEdBackspace);
        }
        if matches!(key.code, KeyCode::Enter) {
            // For edge context, allow multiline input: Alt+Enter (or Ctrl+Enter)
            // inserts a newline; plain Enter saves.
            if matches!(
                ed.mode,
                crate::model::SequenceEditorMode::EditEdgeContext { .. }
            ) && (alt || ctrl)
            {
                return Some(Msg::SeqEdInputChar('\n'));
            }
            return Some(Msg::SeqEdConfirm);
        }

        // While in an input mode, **all** printable characters should type,
        // not trigger actions (so 'p' doesn't open "add participant", etc).
        if !matches!(ed.mode, crate::model::SequenceEditorMode::Browse) {
            return match (key.code, ctrl) {
                // While typing a message, allow participant cycling.
                (KeyCode::Left, true) => Some(Msg::SeqEdCycleFrom(-1)),
                (KeyCode::Right, true) => Some(Msg::SeqEdCycleFrom(1)),
                (KeyCode::Left, false) => Some(Msg::SeqEdCycleTo(-1)),
                (KeyCode::Right, false) => Some(Msg::SeqEdCycleTo(1)),
                (KeyCode::Char(c), _) => Some(Msg::SeqEdInputChar(c)),
                _ => None,
            };
        }

        // Browse mode hotkeys.
        return match key.code {
            KeyCode::Up => Some(Msg::SeqEdMove(-1)),
            KeyCode::Down => Some(Msg::SeqEdMove(1)),
            KeyCode::Left => Some(Msg::SeqEdMoveParticipant(-1)),
            KeyCode::Right => Some(Msg::SeqEdMoveParticipant(1)),
            KeyCode::Char('p') => Some(Msg::SeqEdStartAddParticipant),
            KeyCode::Char('m') => Some(Msg::SeqEdStartAddMessage),
            KeyCode::Char('c') => Some(Msg::SeqEdStartEditEdgeContext),
            KeyCode::Char('x') => Some(Msg::SeqEdDeleteEvent),
            KeyCode::Char('X') => Some(Msg::SeqEdDeleteParticipant),
            KeyCode::Char('n') => Some(Msg::SeqEdStartAddDiagram),
            KeyCode::Char('r') => Some(Msg::SeqEdStartRenameDiagram),
            KeyCode::Char('D') => Some(Msg::SeqEdDeleteDiagram),
            KeyCode::Tab => Some(Msg::SeqEdMoveDiagram(1)),
            _ => None,
        };
    }

    // Code blocks editor captures everything while open.
    if let Some(ed) = &model.code_editor {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if matches!(key.code, KeyCode::Esc) {
            return Some(Msg::CodeEdCancel);
        }
        if matches!(key.code, KeyCode::F(2)) {
            return Some(Msg::CodeEdSave);
        }
        if matches!(key.code, KeyCode::Backspace) {
            return Some(Msg::CodeEdBackspace);
        }
        if matches!(key.code, KeyCode::Enter) {
            // Match the "textarea" feel while editing code:
            // - Enter inserts a newline
            // - Alt/Ctrl+Enter confirms (done editing)
            if matches!(ed.mode, crate::model::CodeEditorMode::EditCode { .. }) {
                if alt || ctrl {
                    return Some(Msg::CodeEdConfirm);
                }
                return Some(Msg::CodeEdInputChar('\n'));
            }
            return Some(Msg::CodeEdConfirm);
        }

        // While typing/editing, printable characters should type.
        if !matches!(ed.mode, crate::model::CodeEditorMode::Browse) {
            return match key.code {
                KeyCode::Tab => Some(Msg::CodeEdInputChar('\t')),
                KeyCode::Left => Some(Msg::CodeEdCaretLeft),
                KeyCode::Right => Some(Msg::CodeEdCaretRight),
                KeyCode::Up => Some(Msg::CodeEdCaretUp),
                KeyCode::Down => Some(Msg::CodeEdCaretDown),
                KeyCode::Home => Some(Msg::CodeEdCaretHome),
                KeyCode::End => Some(Msg::CodeEdCaretEnd),
                KeyCode::Char(c) => Some(Msg::CodeEdInputChar(c)),
                _ => None,
            };
        }

        return match key.code {
            KeyCode::Tab => Some(Msg::CodeEdMoveBlock(1)),
            KeyCode::Up => Some(Msg::CodeEdMoveBlock(-1)),
            KeyCode::Down => Some(Msg::CodeEdMoveBlock(1)),
            KeyCode::Char('n') => Some(Msg::CodeEdStartAddBlock),
            KeyCode::Char('r') => Some(Msg::CodeEdStartRenameBlock),
            KeyCode::Char('l') => Some(Msg::CodeEdStartEditLanguage),
            KeyCode::Char('e') => Some(Msg::CodeEdStartEditCode),
            KeyCode::Char('D') => Some(Msg::CodeEdDeleteBlock),
            _ => None,
        };
    }

    // Gherkin editor captures everything while open.
    if let Some(ed) = &model.gherkin_editor {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if matches!(key.code, KeyCode::Esc) {
            return Some(Msg::GhEdCancel);
        }
        if matches!(key.code, KeyCode::F(2)) {
            return Some(Msg::GhEdSave);
        }
        if matches!(key.code, KeyCode::Backspace) {
            return Some(Msg::GhEdBackspace);
        }
        if matches!(key.code, KeyCode::Enter) {
            // While editing multiline text, Enter inserts newline and Alt/Ctrl+Enter confirms.
            if matches!(
                ed.mode,
                crate::model::GherkinEditorMode::EditBackground { .. }
                    | crate::model::GherkinEditorMode::EditSteps { .. }
            ) {
                if alt || ctrl {
                    return Some(Msg::GhEdConfirm);
                }
                return Some(Msg::GhEdInputChar('\n'));
            }
            return Some(Msg::GhEdConfirm);
        }

        if !matches!(ed.mode, crate::model::GherkinEditorMode::Browse) {
            return match key.code {
                KeyCode::Tab => Some(Msg::GhEdInputChar('\t')),
                KeyCode::Left => Some(Msg::GhEdCaretLeft),
                KeyCode::Right => Some(Msg::GhEdCaretRight),
                KeyCode::Up => Some(Msg::GhEdCaretUp),
                KeyCode::Down => Some(Msg::GhEdCaretDown),
                KeyCode::Home => Some(Msg::GhEdCaretHome),
                KeyCode::End => Some(Msg::GhEdCaretEnd),
                KeyCode::Char(c) => Some(Msg::GhEdInputChar(c)),
                _ => None,
            };
        }

        return match key.code {
            KeyCode::Tab => Some(Msg::GhEdMoveFeature(1)),
            KeyCode::Up => Some(Msg::GhEdMoveScenario(-1)),
            KeyCode::Down => Some(Msg::GhEdMoveScenario(1)),
            KeyCode::Char('n') => Some(Msg::GhEdStartAddScenario),
            KeyCode::Char('r') => Some(Msg::GhEdStartRenameScenario),
            KeyCode::Char('f') => Some(Msg::GhEdStartEditFeature),
            KeyCode::Char('b') => Some(Msg::GhEdStartEditBackground),
            KeyCode::Char('e') => Some(Msg::GhEdStartEditSteps),
            KeyCode::Char('N') => Some(Msg::GhEdStartAddFeature),
            KeyCode::Char('R') => Some(Msg::GhEdStartRenameFeature),
            KeyCode::Char('D') => Some(Msg::GhEdDeleteScenario),
            KeyCode::Char('X') => Some(Msg::GhEdDeleteFeature),
            _ => None,
        };
    }

    // OpenAPI editor captures everything while open.
    if let Some(ed) = &model.openapi_editor {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if matches!(key.code, KeyCode::Esc) {
            return Some(Msg::OaEdCancel);
        }
        if matches!(key.code, KeyCode::F(2)) {
            return Some(Msg::OaEdSave);
        }
        if matches!(key.code, KeyCode::Backspace) {
            return Some(Msg::OaEdBackspace);
        }
        if matches!(key.code, KeyCode::Enter) {
            return Some(Msg::OaEdConfirm);
        }

        if !matches!(ed.mode, crate::model::OpenApiEditorMode::Browse) {
            return match key.code {
                KeyCode::Tab => Some(Msg::OaEdInputChar('\t')),
                KeyCode::Char(c) => Some(Msg::OaEdInputChar(c)),
                _ => None,
            };
        }

        if matches!(ed.mode, crate::model::OpenApiEditorMode::Schemas) {
            return match key.code {
                KeyCode::PageUp => Some(Msg::OaEdScrollPreview(-6)),
                KeyCode::PageDown => Some(Msg::OaEdScrollPreview(6)),
                KeyCode::Up if ctrl => Some(Msg::OaEdScrollPreview(-1)),
                KeyCode::Down if ctrl => Some(Msg::OaEdScrollPreview(1)),
                KeyCode::Char('g') => Some(Msg::OaEdTogglePane),
                KeyCode::Tab => Some(Msg::OaEdMoveSchema(1)),
                KeyCode::Up => Some(Msg::OaEdMoveProp(-1)),
                KeyCode::Down => Some(Msg::OaEdMoveProp(1)),
                KeyCode::Char('n') => Some(Msg::OaEdStartAddSchema),
                KeyCode::Char('D') => Some(Msg::OaEdDeleteSchema),
                KeyCode::Char('h') => Some(Msg::OaEdStartRenameSchema),
                KeyCode::Char('p') => Some(Msg::OaEdStartAddProp),
                KeyCode::Char('x') => Some(Msg::OaEdDeleteProp),
                KeyCode::Char(' ') => Some(Msg::OaEdTogglePropRequired),
                KeyCode::Char('t') => Some(Msg::OaEdCyclePropType(1)),
                KeyCode::Char('a') => Some(Msg::OaEdTogglePropNullable),
                KeyCode::Char('f') => Some(Msg::OaEdCyclePropFormat(1)),
                KeyCode::Char('r') => Some(Msg::OaEdStartRenameProp),
                KeyCode::Char('i') => Some(Msg::OaEdStartEditPropMin),
                KeyCode::Char('k') => Some(Msg::OaEdStartEditPropMax),
                KeyCode::Char('/') => Some(Msg::OaEdStartEditPropPattern),
                KeyCode::Char('e') => Some(Msg::OaEdStartEditPropEnum),
                _ => None,
            };
        }

        return match key.code {
            KeyCode::PageUp => Some(Msg::OaEdScrollPreview(-6)),
            KeyCode::PageDown => Some(Msg::OaEdScrollPreview(6)),
            KeyCode::Up if ctrl => Some(Msg::OaEdScrollPreview(-1)),
            KeyCode::Down if ctrl => Some(Msg::OaEdScrollPreview(1)),
            KeyCode::Tab => Some(Msg::OaEdMoveEndpoint(1)),
            KeyCode::Up => Some(Msg::OaEdMoveEndpoint(-1)),
            KeyCode::Down => Some(Msg::OaEdMoveEndpoint(1)),
            KeyCode::Char('g') => Some(Msg::OaEdTogglePane),
            KeyCode::Char('n') => Some(Msg::OaEdStartAddEndpoint),
            KeyCode::Char('D') => Some(Msg::OaEdDeleteEndpoint),
            KeyCode::Char('m') => Some(Msg::OaEdCycleMethod(1)),
            KeyCode::Char('s') => Some(Msg::OaEdCycleStatus(1)),
            KeyCode::Char('c') => Some(Msg::OaEdCycleReqContentType(1)),
            KeyCode::Char('r') => Some(Msg::OaEdCycleReqBody(1)),
            KeyCode::Char('p') => Some(Msg::OaEdCycleRespBody(1)),
            KeyCode::Char('u') => Some(Msg::OaEdStartEditPath),
            KeyCode::Char('t') => Some(Msg::OaEdStartEditTitle),
            KeyCode::Char('v') => Some(Msg::OaEdStartEditVersion),
            KeyCode::Char('b') => Some(Msg::OaEdStartEditBaseUrl),
            _ => None,
        };
    }

    // Modal picker overlay (templates / projects / agents): arrow
    // keys + Enter / Esc. The picker itself is message-agnostic;
    // accepting runs the per-item `Msg` stored in the picker.
    if model.picker.is_some() {
        return match key.code {
            KeyCode::Esc => Some(Msg::PickerCancel),
            KeyCode::Enter => Some(Msg::PickerAccept),
            KeyCode::Up => Some(Msg::PickerMove(-1)),
            KeyCode::Down => Some(Msg::PickerMove(1)),
            _ => None,
        };
    }

    // Form dialog: capture input.
    if let Some(form) = &model.form {
        let focused_multiline = form.fields.get(form.cursor).is_some_and(|f| f.multiline);
        let focused_sequence = form
            .fields
            .get(form.cursor)
            .is_some_and(|f| matches!(f.kind, Some(lattice_core::fields::FieldKind::SequenceGram)));
        let focused_codeblocks = form
            .fields
            .get(form.cursor)
            .is_some_and(|f| matches!(f.kind, Some(lattice_core::fields::FieldKind::CodeBlocks)));
        let focused_gherkin = form
            .fields
            .get(form.cursor)
            .is_some_and(|f| matches!(f.kind, Some(lattice_core::fields::FieldKind::Gherkin)));
        let focused_openapi = form
            .fields
            .get(form.cursor)
            .is_some_and(|f| matches!(f.kind, Some(lattice_core::fields::FieldKind::OpenApi)));
        // Submit bindings, in order of robustness across terminals:
        //   * `F2`              — universally reliable
        //   * `Ctrl+S`          — works once raw mode disables IXON; most
        //                         terminals honor it
        //   * `Alt+Enter`       — works in every common terminal and is
        //                         the go-to "submit from multiline" key
        //   * `Ctrl+Enter`      — kitty keyboard protocol only
        //   * plain Enter       — submits only on single-line fields
        //                         (multiline: inserts newline instead)
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let is_submit = matches!(key.code, KeyCode::F(2))
            || (matches!(key.code, KeyCode::Char('s')) && ctrl)
            || (matches!(key.code, KeyCode::Enter) && (ctrl || alt))
            || (matches!(key.code, KeyCode::Enter) && !focused_multiline);
        if is_submit {
            return Some(Msg::FormSubmit);
        }
        // Arrow-key semantics depend on the focused field:
        //   * multiline: arrows move the caret within the text, like a
        //     regular text editor would, so typing long TOML or Jinja
        //     doesn't fight the UI.
        //   * single-line: Up/Down still jump between fields (it's the
        //     natural way to navigate short, stacked inputs) while
        //     Left/Right move the caret within that single line.
        //
        // The arm order encodes that dispatch — clippy flags a few
        // arms as having the same body as `Tab`/`BackTab`, but merging
        // them would defeat the `focused_multiline` guard above.
        #[allow(clippy::match_same_arms)]
        return match key.code {
            KeyCode::Esc => Some(Msg::FormCancel),
            KeyCode::Tab => Some(Msg::FormNext),
            KeyCode::BackTab => Some(Msg::FormPrev),
            KeyCode::F(3) if focused_sequence => Some(Msg::OpenSequenceEditor),
            KeyCode::F(4) if focused_codeblocks => Some(Msg::OpenCodeEditor),
            KeyCode::F(5) if focused_gherkin => Some(Msg::OpenGherkinEditor),
            KeyCode::F(6) if focused_openapi => Some(Msg::OpenOpenApiEditor),
            KeyCode::Up if focused_multiline => Some(Msg::FormCaretUp),
            KeyCode::Down if focused_multiline => Some(Msg::FormCaretDown),
            KeyCode::Up => Some(Msg::FormPrev),
            KeyCode::Down => Some(Msg::FormNext),
            KeyCode::Left => Some(Msg::FormCaretLeft),
            KeyCode::Right => Some(Msg::FormCaretRight),
            KeyCode::Home => Some(Msg::FormCaretHome),
            KeyCode::End => Some(Msg::FormCaretEnd),
            KeyCode::Backspace => Some(Msg::FormBackspace),
            KeyCode::Enter => Some(Msg::FormInputChar('\n')),
            KeyCode::Char(c) => Some(Msg::FormInputChar(c)),
            _ => None,
        };
    }

    // Global keys.
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Some(Msg::Quit);
        }
        (KeyCode::Char('k'), KeyModifiers::CONTROL) | (KeyCode::Char('/'), _) => {
            return Some(Msg::PalToggle);
        }
        (KeyCode::Char('?') | KeyCode::F(1), _) => return Some(Msg::ShowHelp),
        // Esc dismisses the top-most toast when no overlay is open.
        // Lets users clear a pinned error toast without switching screens.
        (KeyCode::Esc, _) if !model.toasts.is_empty() => return Some(Msg::DismissToast),
        (KeyCode::Tab, _) => return Some(Msg::NextTab),
        (KeyCode::BackTab, _) => return Some(Msg::PrevTab),
        (KeyCode::Char('1'), _) => return Some(Msg::GoScreen(Screen::Templates)),
        (KeyCode::Char('2'), _) => return Some(Msg::GoScreen(Screen::Tasks)),
        (KeyCode::Char('3'), _) => return Some(Msg::GoScreen(Screen::Info)),
        _ => {}
    }

    // Per-screen keys delegate.
    crate::screens::handle_key(model, key)
}

#[derive(Clone, Copy, Debug)]
pub struct KeybindHelp {
    pub key: &'static str,
    pub description: &'static str,
}

pub const GLOBAL_KEYS: &[KeybindHelp] = &[
    KeybindHelp {
        key: "Tab / Shift+Tab",
        description: "Next/prev screen",
    },
    KeybindHelp {
        key: "1..3",
        description: "Jump to screen",
    },
    KeybindHelp {
        key: "q / Ctrl+C",
        description: "Quit",
    },
    KeybindHelp {
        key: "Ctrl+K or /",
        description: "Command palette",
    },
    KeybindHelp {
        key: "? / F1",
        description: "Help",
    },
    KeybindHelp {
        key: "Esc",
        description: "Cancel dialog / close palette",
    },
];

pub const FORM_KEYS: &[KeybindHelp] = &[
    KeybindHelp {
        key: "F2",
        description: "Submit form (universal)",
    },
    KeybindHelp {
        key: "Ctrl+S",
        description: "Submit form",
    },
    KeybindHelp {
        key: "Alt+Enter",
        description: "Submit form (works from multiline fields)",
    },
    KeybindHelp {
        key: "Enter",
        description: "Submit (single-line field) / newline (multiline)",
    },
    KeybindHelp {
        key: "Ctrl+Enter",
        description: "Submit form (Kitty-protocol terminals only)",
    },
    KeybindHelp {
        key: "Tab / Shift+Tab",
        description: "Next / previous field",
    },
    KeybindHelp {
        key: "↓ / ↑  (single-line)",
        description: "Next / previous field",
    },
    KeybindHelp {
        key: "↓ / ↑  (multiline)",
        description: "Move caret up/down within the text",
    },
    KeybindHelp {
        key: "← / →",
        description: "Move caret within the current line",
    },
    KeybindHelp {
        key: "Home / End",
        description: "Jump to start / end of current line",
    },
    KeybindHelp {
        key: "Esc",
        description: "Cancel form",
    },
];

pub const SCREEN_KEYS: &[KeybindHelp] = &[
    KeybindHelp {
        key: "↑ / ↓",
        description: "Move cursor",
    },
    KeybindHelp {
        key: "Enter",
        description: "Primary action (edit / inspect / submit)",
    },
    KeybindHelp {
        key: "n",
        description: "New (on Projects / Templates / Tasks)",
    },
    KeybindHelp {
        key: "e",
        description: "Edit (on Projects / Templates / Tasks)",
    },
    KeybindHelp {
        key: "d",
        description: "Delete (with confirmation)",
    },
    KeybindHelp {
        key: "Space",
        description: "Toggle multi-select (Tasks)",
    },
    KeybindHelp {
        key: "w",
        description: "Write selected task prompt to markdown (Tasks)",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FormField, FormState, FormSubmit, Model, Msg, Picker, PickerItem};

    fn form_with_cursor(multiline_flags: &[bool], cursor: usize) -> Model {
        let mut m = Model::new();
        m.form = Some(FormState {
            title: "t".into(),
            fields: multiline_flags
                .iter()
                .enumerate()
                .map(|(i, &ml)| FormField::plain(format!("f{i}"), "", false, ml))
                .collect(),
            cursor,
            submit: FormSubmit::CreateTemplate,
        });
        m
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    #[test]
    fn plain_enter_submits_on_single_line_field() {
        let m = form_with_cursor(&[false, false], 0);
        assert!(matches!(
            translate(&m, key(KeyCode::Enter)),
            Some(Msg::FormSubmit)
        ));
    }

    #[test]
    fn plain_enter_inserts_newline_on_multiline_field() {
        let m = form_with_cursor(&[false, true], 1);
        assert!(matches!(
            translate(&m, key(KeyCode::Enter)),
            Some(Msg::FormInputChar('\n'))
        ));
    }

    #[test]
    fn ctrl_s_submits_everywhere() {
        let m = form_with_cursor(&[true], 0);
        assert!(matches!(
            translate(&m, ctrl(KeyCode::Char('s'))),
            Some(Msg::FormSubmit)
        ));
    }

    #[test]
    fn f2_submits() {
        let m = form_with_cursor(&[true], 0);
        assert!(matches!(
            translate(&m, key(KeyCode::F(2))),
            Some(Msg::FormSubmit)
        ));
    }

    #[test]
    fn ctrl_enter_still_submits_on_kitty_protocol() {
        let m = form_with_cursor(&[true], 0);
        assert!(matches!(
            translate(&m, ctrl(KeyCode::Enter)),
            Some(Msg::FormSubmit)
        ));
    }

    #[test]
    fn alt_enter_submits_on_multiline_field() {
        // Alt+Enter is the "universally works across terminals" way
        // out of a multiline field; without it, users sometimes can't
        // figure out how to save a form they've filled in.
        let m = form_with_cursor(&[false, true], 1);
        assert!(matches!(
            translate(&m, alt(KeyCode::Enter)),
            Some(Msg::FormSubmit)
        ));
    }

    #[test]
    fn alt_enter_submits_on_single_line_field_too() {
        let m = form_with_cursor(&[false, true], 0);
        assert!(matches!(
            translate(&m, alt(KeyCode::Enter)),
            Some(Msg::FormSubmit)
        ));
    }

    #[test]
    fn arrow_down_moves_to_next_field_on_single_line() {
        let m = form_with_cursor(&[false, true, false], 0);
        assert!(matches!(
            translate(&m, key(KeyCode::Down)),
            Some(Msg::FormNext)
        ));
    }

    #[test]
    fn arrow_up_moves_to_prev_field_on_single_line() {
        let m = form_with_cursor(&[false, true, false], 2);
        assert!(matches!(
            translate(&m, key(KeyCode::Up)),
            Some(Msg::FormPrev)
        ));
    }

    #[test]
    fn arrow_down_moves_caret_within_multiline_field() {
        let m = form_with_cursor(&[false, true, false], 1);
        assert!(matches!(
            translate(&m, key(KeyCode::Down)),
            Some(Msg::FormCaretDown)
        ));
    }

    #[test]
    fn arrow_up_moves_caret_within_multiline_field() {
        let m = form_with_cursor(&[false, true, false], 1);
        assert!(matches!(
            translate(&m, key(KeyCode::Up)),
            Some(Msg::FormCaretUp)
        ));
    }

    #[test]
    fn tab_still_switches_fields_from_multiline() {
        let m = form_with_cursor(&[false, true, false], 1);
        assert!(matches!(
            translate(&m, key(KeyCode::Tab)),
            Some(Msg::FormNext)
        ));
        assert!(matches!(
            translate(&m, key(KeyCode::BackTab)),
            Some(Msg::FormPrev)
        ));
    }

    #[test]
    fn horizontal_arrows_move_caret() {
        let m = form_with_cursor(&[false, true, false], 1);
        assert!(matches!(
            translate(&m, key(KeyCode::Left)),
            Some(Msg::FormCaretLeft)
        ));
        assert!(matches!(
            translate(&m, key(KeyCode::Right)),
            Some(Msg::FormCaretRight)
        ));
    }

    fn model_with_picker() -> Model {
        let mut m = Model::new();
        m.picker = Some(Picker {
            title: "Pick".into(),
            items: vec![
                PickerItem {
                    label: "alpha".into(),
                    accept: Msg::Quit,
                },
                PickerItem {
                    label: "beta".into(),
                    accept: Msg::Quit,
                },
            ],
            cursor: 0,
        });
        m
    }

    #[test]
    fn picker_routes_arrows_and_enter() {
        let m = model_with_picker();
        assert!(matches!(
            translate(&m, key(KeyCode::Down)),
            Some(Msg::PickerMove(1))
        ));
        assert!(matches!(
            translate(&m, key(KeyCode::Up)),
            Some(Msg::PickerMove(-1))
        ));
        assert!(matches!(
            translate(&m, key(KeyCode::Enter)),
            Some(Msg::PickerAccept)
        ));
        assert!(matches!(
            translate(&m, key(KeyCode::Esc)),
            Some(Msg::PickerCancel)
        ));
    }

    #[test]
    fn picker_blocks_global_tab() {
        let m = model_with_picker();
        assert!(translate(&m, key(KeyCode::Tab)).is_none());
        assert!(translate(&m, key(KeyCode::Char('1'))).is_none());
    }
}
