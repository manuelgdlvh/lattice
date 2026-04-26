//! Tasks screen: list tasks for the selected project; multi-select
//! and manage prompts.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use lattice_core::time::Timestamp;

use crate::model::{Model, Msg};

pub fn handle_key(model: &Model, key: KeyEvent) -> Option<Msg> {
    let tasks = model.tasks_for_selected_project();
    match key.code {
        KeyCode::Up => Some(Msg::TaskCursor(-1)),
        KeyCode::Down => Some(Msg::TaskCursor(1)),
        KeyCode::PageUp => Some(Msg::TaskPromptScroll(-8)),
        KeyCode::PageDown => Some(Msg::TaskPromptScroll(8)),
        KeyCode::Char('n') => Some(Msg::OpenCreateTask),
        KeyCode::Char('w') => {
            if let Some(t) = tasks.get(model.task_cursor) {
                Some(Msg::OpenSaveTaskPrompt(t.id))
            } else {
                Some(Msg::ToastWarn("no task selected".into()))
            }
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            if let Some(t) = tasks.get(model.task_cursor) {
                Some(Msg::OpenEditTask(t.id))
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
                Some(Msg::DeleteTask(t.id))
            } else {
                Some(Msg::ToastWarn("no task selected".into()))
            }
        }
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let title_text = " Tasks ".to_string();

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
            "n=new  e=edit  w=write prompt  space=multi-select  PgUp/PgDn=scroll  d=delete",
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
            ListItem::new(Line::from(vec![
                Span::styled(cursor, Style::default().fg(Color::Cyan)),
                Span::styled(check, Style::default().fg(Color::Magenta)),
                Span::raw(t.name.clone()),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Tasks ({}) ", tasks.len())),
    );
    frame.render_widget(list, body[0]);

    if let Some(t) = tasks.get(model.task_cursor) {
        let detail_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(0)])
            .split(body[1]);

        let details = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    t.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Template: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{} v{}", t.template_id, t.template_version)),
            ]),
        ])
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Detail "));
        frame.render_widget(details, detail_chunks[0]);

        let prompt = model
            .templates
            .iter()
            .find(|tpl| tpl.id == t.template_id)
            .and_then(|tpl| lattice_core::prompt::render(tpl, t, Timestamp::now()).ok())
            .unwrap_or_else(|| {
                "Prompt preview unavailable.\n\n- Ensure the template exists.\n- Ensure the template prompt Jinja renders (no undefined vars)."
                    .to_string()
            });

        let prompt_lines: Vec<Line<'_>> =
            prompt.lines().map(|l| Line::from(l.to_string())).collect();
        let inner_h = detail_chunks[1].height.saturating_sub(2).max(1) as usize;
        let max_scroll = prompt_lines.len().saturating_sub(inner_h);
        let scroll = model.task_prompt_scroll.min(max_scroll);
        let scroll_u16 = u16::try_from(scroll).unwrap_or(0);

        let preview = Paragraph::new(prompt_lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll_u16, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Prompt preview "),
            );
        frame.render_widget(preview, detail_chunks[1]);
    } else {
        let para = Paragraph::new(vec![Line::from(Span::styled(
            "No tasks. Press `n` to create one.",
            Style::default().fg(Color::DarkGray),
        ))])
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(" Detail "));
        frame.render_widget(para, body[1]);
    }
}
