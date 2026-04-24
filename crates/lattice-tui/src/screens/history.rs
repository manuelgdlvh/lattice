//! History screen: past runs for the selected project.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::model::{Model, Msg};

pub fn handle_key(_model: &Model, key: KeyEvent) -> Option<Msg> {
    match key.code {
        KeyCode::Up => Some(Msg::RunCursor(-1)),
        KeyCode::Down => Some(Msg::RunCursor(1)),
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let runs = model.runs_for_selected_project();
    let items: Vec<ListItem<'_>> = runs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let prefix = if i == model.run_cursor { "▶ " } else { "  " };
            let status_style = match r.status {
                lattice_core::entities::TaskStatus::Succeeded => Style::default().fg(Color::Green),
                lattice_core::entities::TaskStatus::Failed
                | lattice_core::entities::TaskStatus::Killed
                | lattice_core::entities::TaskStatus::Interrupted => {
                    Style::default().fg(Color::Red)
                }
                _ => Style::default().fg(Color::Yellow),
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::styled(r.agent_id.to_string(), Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(format!("{:?}", r.status), status_style),
                Span::styled(
                    format!("  {}", &r.id.to_string()[..8]),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" History ({}) ", runs.len())),
    );
    frame.render_widget(list, chunks[0]);

    let right_lines: Vec<Line<'_>> = if let Some(r) = runs.get(model.run_cursor) {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Run id: ", Style::default().fg(Color::DarkGray)),
            Span::raw(r.id.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Task id: ", Style::default().fg(Color::DarkGray)),
            Span::raw(r.task_id.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:?}", r.status)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Agent: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} {}", r.agent_id, r.agent_version)),
        ]));
        if let Some(t) = r.started_at {
            lines.push(Line::from(vec![
                Span::styled("Started: ", Style::default().fg(Color::DarkGray)),
                Span::raw(t.to_rfc3339()),
            ]));
        }
        if let Some(t) = r.finished_at {
            lines.push(Line::from(vec![
                Span::styled("Finished: ", Style::default().fg(Color::DarkGray)),
                Span::raw(t.to_rfc3339()),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("stdout: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} B", r.log.stdout_bytes)),
            Span::raw(" · "),
            Span::styled("stderr: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} B", r.log.stderr_bytes)),
            Span::raw(if r.log.truncated { " (truncated)" } else { "" }),
        ]));
        lines.push(Line::from(""));

        let have_cache = model.history_run == Some(r.id);
        if !have_cache {
            lines.push(Line::from(Span::styled(
                "Loading logs…",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "stderr (tail)",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            if model.history_stderr_tail.is_empty() {
                lines.push(Line::from(Span::styled(
                    "<empty>",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for l in &model.history_stderr_tail {
                    lines.push(Line::from(Span::raw(l.clone())));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "stdout (tail)",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            if model.history_stdout_tail.is_empty() {
                lines.push(Line::from(Span::styled(
                    "<empty>",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for l in &model.history_stdout_tail {
                    lines.push(Line::from(Span::raw(l.clone())));
                }
            }
        }
        lines
    } else {
        vec![Line::from(Span::styled(
            "No runs for this project yet.",
            Style::default().fg(Color::DarkGray),
        ))]
    };
    let p = Paragraph::new(right_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                " Detail ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
        );
    frame.render_widget(p, chunks[1]);
}
