//! Settings screen: read-only view of app-wide preferences + the
//! list of detected agents.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::model::{Model, Msg};

pub fn handle_key(_model: &Model, key: KeyEvent) -> Option<Msg> {
    // No-op for now; settings.toml is user-editable on disk.
    match key.code {
        KeyCode::Char('r') => {
            // Reload detection in the future; no-op for v0.1.
            None
        }
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(13),
            Constraint::Min(0),
        ])
        .split(area);

    let top_lines = vec![
        Line::from(Span::styled(
            "App Info",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("config: ", Style::default().fg(Color::DarkGray)),
            Span::raw(
                model
                    .status_message
                    .clone()
                    .unwrap_or_else(|| "(see ~/.config/lattice/settings.toml)".into()),
            ),
        ]),
        Line::from(Span::styled(
            "Edit `settings.toml` on disk and restart to apply. Press `r` to refresh agents.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(top_lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Info "));
    frame.render_widget(p, chunks[0]);

    let field_lines = vec![
        Line::from(Span::styled(
            "Field types (template `[[fields]] kind = ...`)",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("text", Style::default().fg(Color::Yellow)),
            Span::raw(" — single-line string"),
        ]),
        Line::from(vec![
            Span::styled("textarea", Style::default().fg(Color::Yellow)),
            Span::raw(" — multi-line string"),
        ]),
        Line::from(vec![
            Span::styled("select", Style::default().fg(Color::Yellow)),
            Span::raw(" — one option id from `options = [...]`"),
        ]),
        Line::from(vec![
            Span::styled("multiselect", Style::default().fg(Color::Yellow)),
            Span::raw(" — comma-separated option ids (stored as JSON array)"),
        ]),
        Line::from(vec![
            Span::styled("number", Style::default().fg(Color::Yellow)),
            Span::raw(" — numeric input (supports integer-only via `integer = true`)"),
        ]),
        Line::from(vec![
            Span::styled("boolean", Style::default().fg(Color::Yellow)),
            Span::raw(" — true/false (also accepts yes/no, y/n, 1/0)"),
        ]),
        Line::from(vec![
            Span::styled("file_picker", Style::default().fg(Color::Yellow)),
            Span::raw(" — project-relative path string"),
        ]),
        Line::from(vec![
            Span::styled("glob", Style::default().fg(Color::Yellow)),
            Span::raw(" — glob pattern string"),
        ]),
        Line::from(vec![
            Span::styled("cmd_output", Style::default().fg(Color::Yellow)),
            Span::raw(" — captured at task creation time (read-only)"),
        ]),
        Line::from(vec![
            Span::styled("markdown_note", Style::default().fg(Color::Yellow)),
            Span::raw(" — documentation only (no stored value)"),
        ]),
        Line::from(vec![
            Span::styled("ref", Style::default().fg(Color::Yellow)),
            Span::raw(" — JSON ref object (v0.1: `{ kind: \"run\", id }`)"),
        ]),
        Line::from(vec![
            Span::styled("component", Style::default().fg(Color::Yellow)),
            Span::raw(" — JSON value consumed by a UI component (v0.2 stub)"),
        ]),
    ];
    let p = Paragraph::new(field_lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Fields "));
    frame.render_widget(p, chunks[1]);

    let items: Vec<ListItem<'_>> = model
        .available_agents
        .iter()
        .map(|a| {
            let status = if a.installed {
                Span::styled("installed", Style::default().fg(Color::Green))
            } else {
                Span::styled("not found ", Style::default().fg(Color::Red))
            };
            let ver = a
                .version
                .as_deref()
                .map_or(String::new(), |v| format!("  v{v}"));
            let warn = a
                .detection_warning
                .as_deref()
                .map_or(String::new(), |w| format!("  ({w})"));
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<24}", a.manifest.display_name), Style::default()),
                status,
                Span::raw(ver),
                Span::styled(warn, Style::default().fg(Color::Yellow)),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Agents ({}) ", model.available_agents.len())),
    );
    frame.render_widget(list, chunks[2]);
}
