//! Projects screen: list with add / edit / delete.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::model::{Model, Msg};

pub fn handle_key(model: &Model, key: KeyEvent) -> Option<Msg> {
    match key.code {
        KeyCode::Up => Some(Msg::ProjectCursor(-1)),
        KeyCode::Down => Some(Msg::ProjectCursor(1)),
        KeyCode::Char('n') => Some(Msg::OpenCreateProject),
        // Enter: set this project as the active "target" and jump to
        // its Tasks screen. This is how users pick what project
        // subsequent actions (create task, dispatch) apply to.
        KeyCode::Enter => model
            .projects
            .get(model.project_cursor)
            .map(|p| Msg::SelectAndGoToTasks(p.id)),
        KeyCode::Char('e') => model
            .projects
            .get(model.project_cursor)
            .map(|p| Msg::OpenEditProject(p.id)),
        KeyCode::Char('d') => model
            .projects
            .get(model.project_cursor)
            .map(|p| Msg::DeleteProject(p.id)),
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let items: Vec<ListItem<'_>> = model
        .projects
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let prefix = if i == model.project_cursor {
                "▶ "
            } else {
                "  "
            };
            let selected = model.selected_project == Some(p.id);
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::styled(p.name.clone(), style),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Projects ({}) ", model.projects.len())),
    );
    frame.render_widget(list, chunks[0]);

    let detail = if let Some(p) = model.projects.get(model.project_cursor) {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                p.name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
            Span::raw(p.path.to_string_lossy().into_owned()),
        ]));
        lines.push(Line::from(""));
        if p.description.is_empty() {
            lines.push(Line::from(Span::styled(
                "<no description>",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for l in p.description.lines() {
                lines.push(Line::from(l.to_string()));
            }
        }
        lines.push(Line::from(""));
        let task_count = model.tasks_by_project.get(&p.id).map_or(0, Vec::len);
        lines.push(Line::from(format!("Tasks: {task_count}")));
        lines
    } else {
        vec![Line::from(Span::styled(
            "No projects yet. Press `n` to add one.",
            Style::default().fg(Color::DarkGray),
        ))]
    };
    let para = Paragraph::new(detail)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Detail "));
    frame.render_widget(para, chunks[1]);
}
