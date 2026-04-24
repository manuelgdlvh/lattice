//! Tasks screen: list tasks for the selected project; multi-select
//! and dispatch to an agent.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::model::{Model, Msg};

pub fn handle_key(model: &Model, key: KeyEvent) -> Option<Msg> {
    let tasks = model.tasks_for_selected_project();
    match key.code {
        KeyCode::Up => Some(Msg::TaskCursor(-1)),
        KeyCode::Down => Some(Msg::TaskCursor(1)),
        KeyCode::Char('n') => Some(Msg::OpenCreateTask),
        KeyCode::Char('e') | KeyCode::Enter => {
            if let Some(t) = tasks.get(model.task_cursor) {
                Some(Msg::OpenEditTask(t.project_id, t.id))
            } else {
                Some(Msg::ToastWarn("no task selected".into()))
            }
        }
        KeyCode::Char(' ') => {
            if let Some(t) = tasks.get(model.task_cursor) {
                Some(Msg::ToggleTaskSelection(t.id))
            } else {
                Some(Msg::ToastWarn("no task selected".into()))
            }
        }
        KeyCode::Char('d') => {
            if let Some(t) = tasks.get(model.task_cursor) {
                Some(Msg::DeleteTask(t.project_id, t.id))
            } else {
                Some(Msg::ToastWarn("no task selected".into()))
            }
        }
        // Switch the "target project" without leaving the Tasks screen.
        // Mirrors the way Enter picks a project on the Projects tab.
        KeyCode::Char('p') => Some(Msg::OpenProjectPicker),
        // Dispatch. Msg::RequestDispatch handles all branching (no
        // selection, no agent, 1 agent, N agents) so the user always
        // gets visible feedback instead of silent no-ops.
        KeyCode::Char('x') => Some(Msg::RequestDispatch),
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let title_text = if let Some(pid) = model.selected_project {
        if let Some(p) = model.projects.iter().find(|p| p.id == pid) {
            format!(" Tasks · {} ", p.name)
        } else {
            " Tasks ".to_string()
        }
    } else {
        " Tasks · no project selected (go to Projects and press Enter) ".to_string()
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            title_text,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!("selected: {}", model.task_multi_select.len()),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("   "),
        Span::styled(
            "n=new  e=edit  p=pick project  space=multi-select  x=dispatch  d=delete",
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[1]);

    let tasks = model.tasks_for_selected_project();
    let items: Vec<ListItem<'_>> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let cursor = if i == model.task_cursor { "▶ " } else { "  " };
            let selected = model.task_multi_select.contains(&t.id);
            let check = if selected { "[x] " } else { "[ ] " };
            let status = format!("  [{:?}]", t.status);
            let status_style = match t.status {
                lattice_core::entities::TaskStatus::Succeeded => Style::default().fg(Color::Green),
                lattice_core::entities::TaskStatus::Failed
                | lattice_core::entities::TaskStatus::Killed
                | lattice_core::entities::TaskStatus::Interrupted => {
                    Style::default().fg(Color::Red)
                }
                lattice_core::entities::TaskStatus::Running => Style::default().fg(Color::Yellow),
                lattice_core::entities::TaskStatus::Queued => Style::default().fg(Color::Cyan),
                lattice_core::entities::TaskStatus::Draft => Style::default().fg(Color::DarkGray),
            };
            ListItem::new(Line::from(vec![
                Span::styled(cursor, Style::default().fg(Color::Cyan)),
                Span::styled(check, Style::default().fg(Color::Magenta)),
                Span::raw(t.name.clone()),
                Span::styled(status, status_style),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Tasks ({}) ", tasks.len())),
    );
    frame.render_widget(list, body[0]);

    let detail = if let Some(t) = tasks.get(model.task_cursor) {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                t.name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:?}", t.status)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Template: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} v{}", t.template_id, t.template_version)),
        ]));
        lines.push(Line::from(""));
        for (k, v) in &t.fields {
            lines.push(Line::from(vec![
                Span::styled(format!("{k}: "), Style::default().fg(Color::Cyan)),
                Span::raw(v.to_string()),
            ]));
        }
        lines
    } else {
        vec![Line::from(Span::styled(
            "No tasks. Press `n` to create one.",
            Style::default().fg(Color::DarkGray),
        ))]
    };
    let para = Paragraph::new(detail)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Detail "));
    frame.render_widget(para, body[1]);
}
