//! Templates screen: list + context + prompt jinja preview.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::model::{Model, Msg};

pub fn handle_key(model: &Model, key: KeyEvent) -> Option<Msg> {
    match key.code {
        KeyCode::Up => Some(Msg::TemplateCursor(-1)),
        KeyCode::Down => Some(Msg::TemplateCursor(1)),
        KeyCode::Char('n') => Some(Msg::OpenCreateTemplate),
        KeyCode::Char('e') | KeyCode::Enter => model
            .templates
            .get(model.template_cursor)
            .map(|t| Msg::OpenEditTemplate(t.id)),
        KeyCode::Char('d') => model
            .templates
            .get(model.template_cursor)
            .map(|t| Msg::DeleteTemplate(t.id)),
        _ => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let items: Vec<ListItem<'_>> = model
        .templates
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let prefix = if i == model.template_cursor {
                "▶ "
            } else {
                "  "
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::raw(t.name.clone()),
                Span::styled(
                    format!("  v{}", t.version),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Templates ({}) ", model.templates.len())),
    );
    frame.render_widget(list, chunks[0]);

    if let Some(t) = model.templates.get(model.template_cursor) {
        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ])
            .split(chunks[1]);
        let ctx = Paragraph::new(t.preamble.markdown.clone())
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Context "));
        frame.render_widget(ctx, v[0]);

        let field_lines: Vec<Line<'_>> = if t.fields.is_empty() {
            vec![Line::from(Span::styled(
                "(no fields — tasks will only carry a name)",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            t.fields
                .iter()
                .map(|f| {
                    let req = if f.required { "*" } else { " " };
                    Line::from(vec![
                        Span::styled(format!(" {req} "), Style::default().fg(Color::Magenta)),
                        Span::styled(
                            format!("{:<18}", f.id),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{:?}", f.kind).to_ascii_lowercase(),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(format!("  {}", f.label)),
                    ])
                })
                .collect()
        };
        let fields = Paragraph::new(field_lines)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Fields ({}) ", t.fields.len())),
            );
        frame.render_widget(fields, v[1]);

        let prompt_body = t.prompt.template.clone().unwrap_or_else(|| {
            "(uses canonical skeleton: Context / Inputs / Constraints / Acceptance / Deliverables / References)".to_string()
        });
        let prompt = Paragraph::new(prompt_body)
            .wrap(Wrap { trim: false })
            .block(
                Block::default().borders(Borders::ALL).title(Span::styled(
                    " Prompt (Jinja) ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
            );
        frame.render_widget(prompt, v[2]);
    } else {
        let lines = vec![Line::from(Span::styled(
            "No template selected. Press `n` to create one.",
            Style::default().fg(Color::DarkGray),
        ))];
        let p =
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Detail "));
        frame.render_widget(p, chunks[1]);
    }
}
