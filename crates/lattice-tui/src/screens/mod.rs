//! Per-screen drawing and keybind handlers.
//!
//! Each submodule exports:
//! - `draw(frame, area, model)` — renders the screen content into
//!   the given rect.
//! - `handle_key(model, key)` — returns `Option<Msg>`; this is what
//!   gets called by the global keybind translator when no overlay is
//!   open and the key is not a global shortcut.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::model::{Model, Msg, Screen};

pub mod help;
pub mod history;
pub mod projects;
pub mod runtime;
pub mod settings;
pub mod tasks;
pub mod templates;

/// Dispatches key events to the screen-specific handler. Used after
/// the global keybind layer has ruled the key out.
pub fn handle_key(model: &Model, key: KeyEvent) -> Option<Msg> {
    match model.screen {
        Screen::Projects => projects::handle_key(model, key),
        Screen::Templates => templates::handle_key(model, key),
        Screen::Tasks => tasks::handle_key(model, key),
        Screen::Runtime => runtime::handle_key(model, key),
        Screen::History => history::handle_key(model, key),
        Screen::Info => settings::handle_key(model, key),
        Screen::Help => None,
    }
}

/// Draw the active screen into `area`.
pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    match model.screen {
        Screen::Projects => projects::draw(frame, area, model),
        Screen::Templates => templates::draw(frame, area, model),
        Screen::Tasks => tasks::draw(frame, area, model),
        Screen::Runtime => runtime::draw(frame, area, model),
        Screen::History => history::draw(frame, area, model),
        Screen::Info => settings::draw(frame, area, model),
        Screen::Help => help::draw(frame, area, model),
    }
}
