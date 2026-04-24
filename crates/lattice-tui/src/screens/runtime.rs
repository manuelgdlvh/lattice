//! Runtime screen: list of running agents + live log tail.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::model::{Model, Msg};

pub fn handle_key(model: &Model, key: KeyEvent) -> Option<Msg> {
    let running: Vec<_> = model.running.values().collect();
    match key.code {
        KeyCode::Up => Some(Msg::RuntimeCursor(-1)),
        KeyCode::Down => Some(Msg::RuntimeCursor(1)),
        KeyCode::Enter => running
            .get(model.runtime_cursor)
            .map(|r| Msg::InspectRun(r.run_id)),
        KeyCode::Char('k') => running
            .get(model.runtime_cursor)
            .map(|r| Msg::KillRun(r.run_id)),
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let rows: Vec<_> = model.running.values().cloned().collect();
    let queued: Vec<&lattice_core::entities::Task> = model
        .tasks_by_project
        .values()
        .flatten()
        .filter(|t| t.status == lattice_core::entities::TaskStatus::Queued)
        .collect();

    let items: Vec<ListItem<'_>> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let prefix = if i == model.runtime_cursor {
                "▶ "
            } else {
                "  "
            };
            let pid = r.pid.map_or("?".into(), |p| p.to_string());
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::styled(r.agent_id.to_string(), Style::default().fg(Color::Yellow)),
                Span::raw(format!("  pid={pid}")),
                Span::styled(
                    format!("  run={}", &r.run_id.to_string()[..8]),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
        " Runtime · {} running · {} queued  [Enter=inspect, k=kill] ",
        rows.len(),
        queued.len()
    )));
    frame.render_widget(list, chunks[0]);

    let title = match model.inspect_run {
        Some(id) => format!(" Log · {} ", &id.to_string()[..8]),
        None => " Log ".into(),
    };
    let body: Vec<Line<'_>> = if model.inspect_tail.is_empty() {
        let mut hints = vec![Line::from(Span::styled(
            "Press Enter on a running entry to inspect its live log.",
            Style::default().fg(Color::DarkGray),
        ))];
        if !rows.is_empty() && model.inspect_run.is_none() {
            hints.push(Line::from(Span::styled(
                "No entry selected yet — ↑/↓ then Enter.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        if rows.is_empty() && !queued.is_empty() {
            hints.push(Line::from(Span::styled(
                format!(
                    "{} task(s) queued. If this stays stuck, check for an error toast.\nThe queue will also mark tasks Interrupted on abort (missing prompt, unknown agent, etc.).",
                    queued.len()
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }
        hints
    } else {
        // Show last page of lines.
        let area_h = chunks[1].height.saturating_sub(2) as usize;
        let start = model.inspect_tail.len().saturating_sub(area_h);
        model.inspect_tail[start..]
            .iter()
            .map(|l| Line::from(l.clone()))
            .collect()
    };
    let p = Paragraph::new(body).wrap(Wrap { trim: false }).block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(p, chunks[1]);
}
