//! Help screen: lists global + per-screen keybindings.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::model::Model;
use crate::view::help_lines;

pub fn draw(frame: &mut Frame<'_>, area: Rect, _model: &Model) {
    let lines = help_lines();
    let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Keybindings — press any key to dismiss "),
    );
    frame.render_widget(p, area);
}
