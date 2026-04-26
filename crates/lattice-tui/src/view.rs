//! Top-level view helpers: tab bar, status footer, overlays.
//!
//! The shell calls [`render`] on every frame; it composes the active
//! screen with the chrome. Screens only render into the inner rect.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap};

use crate::keybind::{FORM_KEYS, GLOBAL_KEYS, SCREEN_KEYS};
use crate::model::{Model, Screen};
use crate::palette;
use crate::toast::ToastLevel;

pub fn render(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    let status_height = status_height_for(model);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),             // tabs
            Constraint::Min(0),                // content
            Constraint::Length(status_height), // status / toast
        ])
        .split(area);

    render_tabs(frame, chunks[0], model);
    crate::screens::draw(frame, chunks[1], model);
    render_status(frame, chunks[2], model);

    if model.palette_open {
        render_palette(frame, area, model);
    }
    if let Some(picker) = &model.picker {
        render_picker(frame, area, picker);
    }
    if let Some(form) = &model.form {
        render_form(frame, area, form);
    }
    if let Some(ed) = &model.sequence_editor {
        render_sequence_editor(frame, area, ed);
    }
    if let Some(ed) = &model.code_editor {
        render_code_editor(frame, area, ed);
    }
    if let Some(ed) = &model.gherkin_editor {
        render_gherkin_editor(frame, area, ed);
    }
    if let Some(ed) = &model.openapi_editor {
        render_openapi_editor(frame, area, ed);
    }
    if let Some(confirm) = &model.confirm {
        render_confirm(frame, area, confirm);
    }
}

fn render_openapi_editor(frame: &mut Frame<'_>, area: Rect, ed: &crate::model::OpenApiEditorState) {
    let width = ((area.width as u32) * 86 / 100)
        .try_into()
        .unwrap_or(area.width)
        .clamp(78, 220);
    let height = area.height.saturating_sub(4).clamp(18, 36);
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);

    let footer_h = match ed.mode {
        crate::model::OpenApiEditorMode::Browse => 3,
        _ => 4,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " openapi editor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(vec![
            Span::styled("F2", Style::default().fg(Color::Green)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" next  "),
            Span::styled("g", Style::default().fg(Color::Cyan)),
            Span::raw(" toggle endpoints/schemas  "),
            Span::styled("n", Style::default().fg(Color::Cyan)),
            Span::raw(" new  "),
            Span::styled("D", Style::default().fg(Color::Cyan)),
            Span::raw(" delete  "),
            Span::styled("m", Style::default().fg(Color::Cyan)),
            Span::raw(" method  "),
            Span::styled("s", Style::default().fg(Color::Cyan)),
            Span::raw(" status  "),
            Span::styled("c", Style::default().fg(Color::Cyan)),
            Span::raw(" content-type  "),
            Span::styled("r/p", Style::default().fg(Color::Cyan)),
            Span::raw(" req/resp body shape  "),
            Span::styled("u", Style::default().fg(Color::Cyan)),
            Span::raw(" path  "),
            Span::styled("t/v/b", Style::default().fg(Color::Cyan)),
            Span::raw(" title/version/base-url"),
        ]));

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(footer_h),
        ])
        .split(inner);

    // Header
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("title: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&ed.title),
            Span::styled("   version: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&ed.version),
        ]),
        Line::from(vec![
            Span::styled("base: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&ed.base_url),
        ]),
    ])
    .wrap(Wrap { trim: true });
    frame.render_widget(header, chunks[0]);

    // Left list + right preview
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(56), Constraint::Min(0)])
        .split(chunks[1]);

    if matches!(ed.mode, crate::model::OpenApiEditorMode::Schemas) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Length(28)])
            .split(split[0]);

        let schema_items: Vec<ListItem> = ed
            .schemas
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let sel = i == ed.schema_cursor;
                let style = if sel {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(s.name.clone(), style)))
            })
            .collect();
        frame.render_widget(
            List::new(schema_items)
                .block(Block::default().borders(Borders::ALL).title(" schemas ")),
            cols[0],
        );

        let props: Vec<ListItem> = ed
            .schemas
            .get(ed.schema_cursor)
            .map(|s| {
                s.properties
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let sel = i == ed.prop_cursor;
                        let style = if sel {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        ListItem::new(Line::from(Span::styled(
                            format!(
                                "{}{} ({})",
                                p.name,
                                if p.required { " *" } else { "" },
                                crate::model::openapi_prop_type_str(p.kind)
                            ),
                            style,
                        )))
                    })
                    .collect()
            })
            .unwrap_or_default();
        frame.render_widget(
            List::new(props).block(Block::default().borders(Borders::ALL).title(" properties ")),
            cols[1],
        );
    } else {
        let items: Vec<ListItem> = ed
            .endpoints
            .iter()
            .enumerate()
            .map(|(i, ep)| {
                let sel = i == ed.endpoint_cursor;
                let style = if sel {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(
                    format!(
                        "{} {} {}",
                        openapi_method(ep.method),
                        ep.path,
                        ep.response_status
                    ),
                    style,
                )))
            })
            .collect();
        frame.render_widget(
            List::new(items).block(Block::default().borders(Borders::ALL).title(" endpoints ")),
            split[0],
        );
    }

    let preview = crate::model::render_openapi_preview(ed);
    let scroll_y: u16 = ed.preview_scroll.try_into().unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(preview)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" preview (OpenAPI YAML) "),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0)),
        split[1],
    );

    // Footer editor
    let footer_block = Block::default().borders(Borders::ALL).title(" edit ");
    let footer_inner = footer_block.inner(chunks[2]);
    frame.render_widget(footer_block, chunks[2]);
    let footer_text = crate::model::render_openapi_footer(ed);
    frame.render_widget(
        Paragraph::new(footer_text).wrap(Wrap { trim: false }),
        footer_inner,
    );
}

fn openapi_method(m: crate::model::OpenApiMethod) -> &'static str {
    match m {
        crate::model::OpenApiMethod::Get => "GET",
        crate::model::OpenApiMethod::Post => "POST",
        crate::model::OpenApiMethod::Put => "PUT",
        crate::model::OpenApiMethod::Patch => "PATCH",
        crate::model::OpenApiMethod::Delete => "DELETE",
    }
}

fn render_gherkin_editor(frame: &mut Frame<'_>, area: Rect, ed: &crate::model::GherkinEditorState) {
    let width = ((area.width as u32) * 80 / 100)
        .try_into()
        .unwrap_or(area.width)
        .clamp(70, 200);
    let height = area.height.saturating_sub(4).clamp(16, 34);
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " gherkin editor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(vec![
            Span::styled("F2", Style::default().fg(Color::Green)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" next feature  "),
            Span::styled("f", Style::default().fg(Color::Cyan)),
            Span::raw(" rename feature  "),
            Span::styled("b", Style::default().fg(Color::Cyan)),
            Span::raw(" background  "),
            Span::styled("n", Style::default().fg(Color::Cyan)),
            Span::raw(" new scenario  "),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" rename scenario  "),
            Span::styled("N", Style::default().fg(Color::Cyan)),
            Span::raw(" new feature  "),
            Span::styled("R", Style::default().fg(Color::Cyan)),
            Span::raw(" rename feature  "),
            Span::styled("e", Style::default().fg(Color::Cyan)),
            Span::raw(" edit steps  "),
            Span::styled("D", Style::default().fg(Color::Cyan)),
            Span::raw(" del scenario  "),
            Span::styled("X", Style::default().fg(Color::Cyan)),
            Span::raw(" del feature"),
        ]));

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let footer_h = match ed.mode {
        crate::model::GherkinEditorMode::EditBackground { .. }
        | crate::model::GherkinEditorMode::EditSteps { .. } => 6,
        _ => 3,
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(footer_h),
        ])
        .split(inner);

    let feature_name = ed
        .features
        .get(ed.feature_cursor)
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "Test cases".into());
    let scenario_count = ed
        .features
        .get(ed.feature_cursor)
        .map(|f| f.scenarios.len())
        .unwrap_or(0);
    let title_line = Line::from(vec![
        Span::styled("Feature: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(feature_name, Style::default().fg(Color::Cyan)),
        Span::styled(
            format!(
                "  ({}/{})  Scenario ({}/{})",
                ed.feature_cursor + 1,
                ed.features.len().max(1),
                ed.scenario_cursor + 1,
                scenario_count.max(1)
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(vec![title_line]).wrap(Wrap { trim: false }),
        chunks[0],
    );

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(chunks[1]);

    let preview = crate::model::render_gherkin_features(&ed.features);
    let preview_lines: Vec<Line<'static>> =
        preview.lines().map(|l| Line::from(l.to_string())).collect();
    frame.render_widget(
        Paragraph::new(preview_lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Preview ")),
        mid[0],
    );

    let items: Vec<ListItem<'_>> = ed
        .features
        .get(ed.feature_cursor)
        .map(|f| {
            f.scenarios
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let marker = if i == ed.scenario_cursor {
                        "▶ "
                    } else {
                        "  "
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, Style::default().fg(Color::Cyan)),
                        Span::styled(s.name.clone(), Style::default()),
                    ]))
                })
                .collect()
        })
        .unwrap_or_default();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Scenarios ")),
        mid[1],
    );

    match &ed.mode {
        crate::model::GherkinEditorMode::EditBackground { editor }
        | crate::model::GherkinEditorMode::EditSteps { editor } => {
            let title = match &ed.mode {
                crate::model::GherkinEditorMode::EditBackground { .. } => {
                    " Edit background (Enter=newline · Alt/Ctrl+Enter=done · Esc=cancel) "
                }
                _ => " Edit steps (Enter=newline · Alt/Ctrl+Enter=done · Esc=cancel) ",
            };
            let chunk = chunks[2];
            let caret = editor.caret.min(editor.value.len());
            let body = insert_caret(&editor.value, caret);
            let inner_w = chunk.width.saturating_sub(2);
            let inner_h = chunk.height.saturating_sub(2).max(1);
            let scroll_y = {
                let caret_row =
                    wrapped_row_count(&editor.value[..caret], inner_w).saturating_sub(1);
                caret_row.saturating_sub(inner_h.saturating_sub(1))
            };
            frame.render_widget(
                Paragraph::new(body)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_y, 0))
                    .block(Block::default().borders(Borders::ALL).title(title)),
                chunk,
            );
        }
        _ => {
            let footer_lines: Vec<Line<'static>> = match &ed.mode {
                crate::model::GherkinEditorMode::Browse => vec![Line::from(Span::styled(
                    "Tip: Tab cycles features; Up/Down picks scenarios; e edits steps.",
                    Style::default().fg(Color::DarkGray),
                ))],
                crate::model::GherkinEditorMode::AddScenario { input } => vec![Line::from(vec![
                    Span::styled(
                        "New scenario: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=create", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::GherkinEditorMode::RenameScenario { input } => {
                    vec![Line::from(vec![
                        Span::styled("Rename: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(input.clone()),
                        Span::styled("▌", Style::default().fg(Color::Cyan)),
                        Span::styled("  Enter=save", Style::default().fg(Color::DarkGray)),
                    ])]
                }
                crate::model::GherkinEditorMode::EditFeature { input } => vec![Line::from(vec![
                    Span::styled("Feature: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=save", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::GherkinEditorMode::AddFeature { input } => vec![Line::from(vec![
                    Span::styled(
                        "New feature: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=create", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::GherkinEditorMode::RenameFeature { input } => vec![Line::from(vec![
                    Span::styled(
                        "Rename feature: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=save", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::GherkinEditorMode::EditBackground { .. }
                | crate::model::GherkinEditorMode::EditSteps { .. } => vec![Line::from("")],
            };
            frame.render_widget(
                Paragraph::new(footer_lines)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL)),
                chunks[2],
            );
        }
    }
}

fn render_code_editor(frame: &mut Frame<'_>, area: Rect, ed: &crate::model::CodeEditorState) {
    let width = ((area.width as u32) * 80 / 100)
        .try_into()
        .unwrap_or(area.width)
        .clamp(70, 200);
    let height = area.height.saturating_sub(4).clamp(16, 34);
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " code-blocks editor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(vec![
            Span::styled("F2", Style::default().fg(Color::Green)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" next block  "),
            Span::styled("n", Style::default().fg(Color::Cyan)),
            Span::raw(" new  "),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" rename  "),
            Span::styled("l", Style::default().fg(Color::Cyan)),
            Span::raw(" language  "),
            Span::styled("e", Style::default().fg(Color::Cyan)),
            Span::raw(" edit code  "),
            Span::styled("D", Style::default().fg(Color::Cyan)),
            Span::raw(" delete"),
        ]));

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let footer_h = match ed.mode {
        crate::model::CodeEditorMode::EditCode { .. } => 6,
        _ => 3,
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(footer_h),
        ])
        .split(inner);

    let cur = ed.blocks.get(ed.block_cursor);
    let title_line = if let Some(b) = cur {
        Line::from(vec![
            Span::styled("Block: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(b.name.clone(), Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("  ({}/{})", ed.block_cursor + 1, ed.blocks.len().max(1)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled("lang: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(b.language.clone(), Style::default().fg(Color::Magenta)),
        ])
    } else {
        Line::from("No blocks.")
    };
    frame.render_widget(
        Paragraph::new(vec![title_line]).wrap(Wrap { trim: false }),
        chunks[0],
    );

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(chunks[1]);

    let preview_lines: Vec<Line<'static>> = if let Some(b) = cur {
        let mut v = Vec::new();
        v.push(Line::from(Span::styled(
            format!("```{}", b.language.trim()),
            Style::default().fg(Color::DarkGray),
        )));
        for l in b.code.lines() {
            v.push(Line::from(l.to_string()));
        }
        v.push(Line::from(Span::styled(
            "```",
            Style::default().fg(Color::DarkGray),
        )));
        if v.is_empty() {
            vec![Line::from("Empty.")]
        } else {
            v
        }
    } else {
        vec![Line::from("No block selected.")]
    };
    frame.render_widget(
        Paragraph::new(preview_lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Preview ")),
        mid[0],
    );

    let items: Vec<ListItem<'_>> = ed
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let marker = if i == ed.block_cursor { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(b.name.clone(), Style::default()),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Blocks ")),
        mid[1],
    );

    match &ed.mode {
        crate::model::CodeEditorMode::EditCode { editor } => {
            let title = " Edit code (Enter=newline · Alt/Ctrl+Enter=done · Esc=cancel) ";
            let chunk = chunks[2];
            let caret = editor.caret.min(editor.value.len());
            let body = insert_caret(&editor.value, caret);

            // Scroll so the caret stays visible (same strategy as the main form).
            let inner_w = chunk.width.saturating_sub(2);
            let inner_h = chunk.height.saturating_sub(2).max(1);
            let scroll_y = {
                let caret_row =
                    wrapped_row_count(&editor.value[..caret], inner_w).saturating_sub(1);
                caret_row.saturating_sub(inner_h.saturating_sub(1))
            };

            frame.render_widget(
                Paragraph::new(body)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_y, 0))
                    .block(Block::default().borders(Borders::ALL).title(title)),
                chunk,
            );
        }
        _ => {
            let footer_lines: Vec<Line<'static>> = match &ed.mode {
                crate::model::CodeEditorMode::Browse => vec![Line::from(Span::styled(
                    "Tip: press e to edit code.",
                    Style::default().fg(Color::DarkGray),
                ))],
                crate::model::CodeEditorMode::AddBlock { input } => vec![Line::from(vec![
                    Span::styled(
                        "New block name: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=create", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::CodeEditorMode::RenameBlock { input } => vec![Line::from(vec![
                    Span::styled(
                        "Rename block: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=save", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::CodeEditorMode::EditLanguage { input } => vec![Line::from(vec![
                    Span::styled("Language: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(input.clone()),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                    Span::styled("  Enter=save", Style::default().fg(Color::DarkGray)),
                ])],
                crate::model::CodeEditorMode::EditCode { .. } => {
                    vec![Line::from("")]
                }
            };
            frame.render_widget(
                Paragraph::new(footer_lines)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL)),
                chunks[2],
            );
        }
    }
}

fn insert_caret(s: &str, caret: usize) -> String {
    let caret = caret.min(s.len());
    let (before, after) = s.split_at(caret);
    format!("{before}▌{after}")
}

fn render_sequence_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    ed: &crate::model::SequenceEditorState,
) {
    // Use a % of the terminal width so diagrams stay readable.
    // Clamp to keep it usable on both tiny and huge terminals.
    let width = ((area.width as u32) * 70 / 100)
        .try_into()
        .unwrap_or(area.width)
        .clamp(70, 180);
    let height = area.height.saturating_sub(4).clamp(14, 30);
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " sequence-gram editor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(vec![
            Span::styled("F2", Style::default().fg(Color::Green)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" next diagram  "),
            Span::styled("n", Style::default().fg(Color::Cyan)),
            Span::raw(" new diagram  "),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" rename  "),
            Span::styled("D", Style::default().fg(Color::Cyan)),
            Span::raw(" del diagram"),
            Span::styled("p", Style::default().fg(Color::Cyan)),
            Span::raw(" add participant  "),
            Span::styled("m", Style::default().fg(Color::Cyan)),
            Span::raw(" add message  "),
            Span::styled("c", Style::default().fg(Color::Cyan)),
            Span::raw(" add notes  "),
            Span::styled("x", Style::default().fg(Color::Cyan)),
            Span::raw(" del event  "),
            Span::styled("X", Style::default().fg(Color::Cyan)),
            Span::raw(" del participant"),
        ]));

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let footer_h = match ed.mode {
        crate::model::SequenceEditorMode::EditEdgeContext { .. } => 6,
        _ => 3,
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(footer_h),
        ])
        .split(inner);

    // Diagram + participants row.
    let diag_name = ed
        .diagrams
        .get(ed.diagram_cursor)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "Diagram".into());
    let diag_line = Line::from(vec![
        Span::styled("Diagram: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(diag_name, Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("  ({}/{})", ed.diagram_cursor + 1, ed.diagrams.len().max(1)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let parts = if ed
        .diagrams
        .get(ed.diagram_cursor)
        .is_none_or(|d| d.participants.is_empty())
    {
        Line::from(Span::styled(
            "No participants yet. Press 'p' to add one.",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let diag = ed.diagrams.get(ed.diagram_cursor).unwrap();
        let mut spans = vec![Span::styled(
            "Participants: ",
            Style::default().add_modifier(Modifier::BOLD),
        )];
        for (i, p) in diag.participants.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let selected = i == ed.participant_cursor;
            spans.push(Span::styled(
                p.clone(),
                if selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).bg(Color::DarkGray)
                },
            ));
        }
        Line::from(spans)
    };
    let p = Paragraph::new(vec![diag_line, parts]).wrap(Wrap { trim: false });
    frame.render_widget(p, chunks[0]);

    // Diagram + events list side-by-side.
    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(chunks[1]);

    let diagram_lines = render_sequence_preview(ed, mid[0].width as usize);
    let diagram = Paragraph::new(diagram_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Diagram preview "),
        );
    frame.render_widget(diagram, mid[0]);

    let diag = ed.diagrams.get(ed.diagram_cursor);
    let events = diag.map(|d| d.events.as_slice()).unwrap_or(&[]);
    let rows: Vec<ListItem<'_>> = events
        .iter()
        .enumerate()
        .map(|(i, ev)| {
            let (text, style) = match ev {
                crate::model::SequenceEvent::Message {
                    from,
                    to,
                    dashed,
                    rel_id: _,
                    text,
                    edge_context,
                } => {
                    let arrow = if *dashed { "-->>" } else { "->>" };
                    let marker = if edge_context
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|s| !s.is_empty())
                    {
                        "  [ctx]"
                    } else {
                        ""
                    };
                    (
                        format!("{from} {arrow} {to}: {text}{marker}"),
                        Style::default(),
                    )
                }
            };
            let marker = if i == ed.event_cursor { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(text, style),
            ]))
        })
        .collect();
    let list = List::new(rows).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Events (Up/Down) "),
    );
    frame.render_widget(list, mid[1]);

    // Footer/input.
    let footer_width = chunks[2].width.saturating_sub(2) as usize;
    fn tail_window(s: &str, max_chars: usize) -> String {
        if max_chars <= 1 {
            return "…".into();
        }
        let n = s.chars().count();
        if n <= max_chars {
            return s.to_string();
        }
        let keep = max_chars.saturating_sub(1);
        let tail: String = s.chars().skip(n.saturating_sub(keep)).collect();
        format!("…{tail}")
    }

    let footer_lines: Vec<Line<'static>> = match &ed.mode {
        crate::model::SequenceEditorMode::Browse => vec![Line::from(Span::styled(
            "Tip: press c to add notes for selected message.",
            Style::default().fg(Color::DarkGray),
        ))],
        crate::model::SequenceEditorMode::AddParticipant { input } => vec![Line::from(vec![
            Span::styled(
                "Add participant: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(input.clone()),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
            Span::styled("  Enter=add", Style::default().fg(Color::DarkGray)),
        ])],
        crate::model::SequenceEditorMode::AddDiagram { input } => vec![Line::from(vec![
            Span::styled(
                "New diagram name: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(input.clone()),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
            Span::styled("  Enter=create", Style::default().fg(Color::DarkGray)),
        ])],
        crate::model::SequenceEditorMode::RenameDiagram { input } => vec![Line::from(vec![
            Span::styled(
                "Rename diagram: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(input.clone()),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
            Span::styled("  Enter=save", Style::default().fg(Color::DarkGray)),
        ])],
        crate::model::SequenceEditorMode::AddMessage {
            from,
            to,
            dashed,
            input,
        } => {
            let diag = ed.diagrams.get(ed.diagram_cursor);
            let from_name = diag
                .and_then(|d| d.participants.get(*from).cloned())
                .unwrap_or_default();
            let to_name = diag
                .and_then(|d| d.participants.get(*to).cloned())
                .unwrap_or_default();
            let arrow = if *dashed { "-->>" } else { "->>" };
            vec![Line::from(vec![
                Span::styled(
                    "Add message: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(from_name, Style::default().fg(Color::Magenta)),
                Span::raw(format!(" {arrow} ")),
                Span::styled(to_name, Style::default().fg(Color::Magenta)),
                Span::raw(": "),
                Span::raw(input.clone()),
                Span::styled("▌", Style::default().fg(Color::Cyan)),
                Span::styled(
                    "  Ctrl+←/→=from  ←/→=to  Enter=add",
                    Style::default().fg(Color::DarkGray),
                ),
            ])]
        }
        crate::model::SequenceEditorMode::EditEdgeContext { input } => {
            // Keep the editor compact (3 footer lines total) but still make typing visible:
            // show a single-line window of the full content with newlines rendered as " ↩ ".
            // The saved value is still multiline; newlines become `<br/>` in Mermaid output.
            let flat = input.replace('\n', " ↩ ");
            let visible = tail_window(&flat, footer_width.saturating_sub(2).max(8));
            vec![
                Line::from(vec![
                    Span::styled("Notes: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        "Alt+Enter=newline  Enter=save  Esc=cancel",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(vec![
                    Span::raw(visible),
                    Span::styled("▌", Style::default().fg(Color::Cyan)),
                ]),
                Line::from(""),
            ]
        }
    };
    let foot = Paragraph::new(footer_lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(foot, chunks[2]);
}

fn render_sequence_preview(
    ed: &crate::model::SequenceEditorState,
    max_width: usize,
) -> Vec<Line<'static>> {
    let Some(diag) = ed.diagrams.get(ed.diagram_cursor) else {
        return vec![Line::from("No diagram.")];
    };
    // Very small widths degrade to a simple hint.
    if max_width < 10 {
        return vec![Line::from("…")];
    }
    if diag.participants.is_empty() {
        return vec![Line::from("No participants. Press 'p' to add one.")];
    }

    // Column sizing: derive from available width and participant count.
    // Keep it bounded so it doesn't explode on wide terminals but stays readable.
    let gap = 2usize;
    let n = diag.participants.len();
    let width = max_width.saturating_sub(2).max(1);
    let usable = width.saturating_sub((n - 1) * gap);
    // Don't force a large minimum width; on narrow terminals or many participants,
    // we'd rather shrink columns than overflow and misalign lifelines.
    let col_w = (usable / n).clamp(3, 14);

    let col_x = |idx: usize| -> usize { idx * (col_w + gap) + (col_w / 2) };

    // Header: participant names centered in their columns.
    let mut header = vec![' '; width];
    for (i, name) in diag.participants.iter().enumerate() {
        let x0 = i * (col_w + gap);
        let label: String = name.chars().take(col_w).collect();
        let start = x0 + col_w.saturating_sub(label.chars().count()) / 2;
        for (k, ch) in label.chars().enumerate() {
            let pos = start + k;
            if pos < header.len() {
                header[pos] = ch;
            }
        }
    }

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(header.into_iter().collect::<String>()));

    // Render each event as one row with lifelines.
    for (i, ev) in diag.events.iter().enumerate() {
        let mut row = vec![' '; width];

        // Lifelines.
        for p in 0..n {
            let x = col_x(p);
            if x < row.len() {
                row[x] = '│';
            }
        }

        // Highlight selected event with a marker at start.
        if i == ed.event_cursor && !row.is_empty() {
            row[0] = '▶';
        }

        match ev {
            crate::model::SequenceEvent::Message {
                from,
                to,
                dashed,
                rel_id,
                text: _,
                ..
            } => {
                let from_i = diag.participants.iter().position(|p| p == from);
                let to_i = diag.participants.iter().position(|p| p == to);
                if let (Some(a), Some(b)) = (from_i, to_i) {
                    let xa = col_x(a);
                    let xb = col_x(b);
                    if xa == xb {
                        // Self message: render as a small loop to the right of the lifeline.
                        let loop_start = xa.saturating_add(1).min(row.len().saturating_sub(1));
                        if loop_start < row.len() {
                            row[loop_start] = '↺';
                        }
                        let label_max = 18usize;
                        let label = if rel_id.chars().count() > label_max {
                            let mut s = rel_id
                                .chars()
                                .take(label_max.saturating_sub(1))
                                .collect::<String>();
                            s.push('…');
                            s
                        } else {
                            rel_id.clone()
                        };
                        for (k, ch) in label.chars().enumerate() {
                            let pos = loop_start.saturating_add(2 + k);
                            if pos < row.len() && row[pos] == ' ' {
                                row[pos] = ch;
                            }
                        }
                        continue;
                    }
                    let (l, r) = if xa <= xb { (xa, xb) } else { (xb, xa) };
                    let stroke = if *dashed { '┄' } else { '─' };
                    for x in l.saturating_add(1)..r {
                        if x < row.len() && row[x] == ' ' {
                            row[x] = stroke;
                        }
                    }
                    // Arrowhead: place adjacent to the target lifeline so we don't overwrite `│`.
                    if xa <= xb {
                        if r > 0 && r - 1 < row.len() && row[r - 1] == stroke {
                            row[r - 1] = '►';
                        }
                    } else if r + 1 < row.len() && row[r + 1] == ' ' {
                        row[r + 1] = '◄';
                    }

                    // Inline label: place near the midpoint.
                    // Keep label short: prefer relation id so it doesn't overflow.
                    let label = rel_id;
                    let mid = l + (r - l) / 2;
                    let label_max = 18usize;
                    let label = if label.chars().count() > label_max {
                        let mut s = label
                            .chars()
                            .take(label_max.saturating_sub(1))
                            .collect::<String>();
                        s.push('…');
                        s
                    } else {
                        label.clone()
                    };
                    let start = mid.saturating_sub(label.chars().count() / 2);
                    for (k, ch) in label.chars().enumerate() {
                        let pos = start + k;
                        if pos < row.len() && row[pos] == ' ' {
                            row[pos] = ch;
                        }
                    }
                }
            }
        }

        out.push(Line::from(row.into_iter().collect::<String>()));
    }

    if diag.events.is_empty() {
        out.push(Line::from(""));
        out.push(Line::from("No events. Press 'm' to add a message."));
    }

    out
}

/// Height of the bottom status strip. Multi-line toasts (e.g. stderr
/// tails on failed runs) get enough rows to actually display the
/// information instead of being silently truncated.
fn status_height_for(model: &Model) -> u16 {
    if let Some(t) = model.toasts.last() {
        let lines = 1 + t.text.chars().filter(|c| *c == '\n').count();
        let clamped = u16::try_from(lines.min(16)).unwrap_or(1);
        clamped.max(1)
    } else {
        1
    }
}

fn render_tabs(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let titles: Vec<Line<'_>> = Screen::TABS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Line::from(vec![
                Span::styled(format!(" {} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(s.label(), Style::default().fg(Color::White)),
            ])
        })
        .collect();
    let selected = Screen::TABS
        .iter()
        .position(|s| *s == model.screen)
        .unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(selected)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(Span::styled(
                    "lattice",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, area);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let lines: Vec<Line<'_>> = if let Some(toast) = model.toasts.last() {
        let (prefix, style) = match toast.level {
            ToastLevel::Info => ("[info] ", Style::default().fg(Color::Green)),
            ToastLevel::Warn => ("[warn] ", Style::default().fg(Color::Yellow)),
            ToastLevel::Error => ("[error] ", Style::default().fg(Color::Red)),
        };
        let mut iter = toast.text.lines();
        let first = iter.next().unwrap_or("");
        let mut out = vec![Line::from(vec![
            Span::styled(prefix, style),
            Span::raw(first.to_string()),
        ])];
        for extra in iter {
            out.push(Line::from(Span::raw(extra.to_string())));
        }
        out
    } else if let Some(msg) = &model.status_message {
        vec![Line::from(Span::raw(msg.clone()))]
    } else {
        vec![Line::from(Span::styled(
            "q=quit  Tab=next  ?=help  Ctrl+K=palette",
            Style::default().fg(Color::DarkGray),
        ))]
    };
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, area);
}

fn render_palette(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let width = area.width.saturating_sub(20).clamp(40, 80);
    let height = 12u16;
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(rect);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("› ", Style::default().fg(Color::Cyan)),
        Span::raw(model.palette_input.clone()),
        Span::styled("▌", Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Command Palette (Enter to run, Esc to close) "),
    );
    frame.render_widget(input, chunks[0]);

    let items = palette::candidates(&model.palette_input);
    let cursor = model.palette_cursor.min(items.len().saturating_sub(1));
    let rows: Vec<ListItem<'_>> = items
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let marker = if i == cursor { "▶" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::Cyan)),
                Span::styled(c.label, Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  {}", c.hint),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let list = List::new(rows).block(Block::default().borders(Borders::ALL));
    frame.render_widget(list, chunks[1]);
}

fn render_form(frame: &mut Frame<'_>, area: Rect, form: &crate::model::FormState) {
    let width = area.width.saturating_sub(10).clamp(50, 100);
    let height = area.height.saturating_sub(6).clamp(10, 24);
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    // The top bar shows the form title on the left and the submit
    // keybinding on the right so the user can always see how to save
    // even when the bottom hint scrolls off a tall modal.
    let save_badge = |s: &'static str| {
        Span::styled(
            s,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" {} ", form.title),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .title_top(
            Line::from(vec![
                Span::raw(" save: "),
                save_badge(" F2 "),
                Span::raw(" "),
                save_badge(" Ctrl+S "),
                Span::raw(" "),
                save_badge(" Alt+Enter "),
                Span::raw(" "),
            ])
            .right_aligned(),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    // One row per field; the focused one expands for multi-line fields.
    // The trailing `3` rows are reserved for the footer hint, which can
    // wrap onto two lines on narrow modals.
    let base_rows: Vec<Constraint> = form
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let focused = i == form.cursor;
            if f.multiline && focused {
                Constraint::Min(5)
            } else {
                Constraint::Length(3)
            }
        })
        .chain(std::iter::once(Constraint::Length(3)))
        .collect();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(base_rows)
        .split(inner);

    for (i, field) in form.fields.iter().enumerate() {
        let focused = i == form.cursor;
        let style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let is_seq = matches!(
            field.kind,
            Some(lattice_core::fields::FieldKind::SequenceGram)
        );
        let is_code = matches!(
            field.kind,
            Some(lattice_core::fields::FieldKind::CodeBlocks)
        );
        let is_gherkin = matches!(field.kind, Some(lattice_core::fields::FieldKind::Gherkin));
        let is_openapi = matches!(field.kind, Some(lattice_core::fields::FieldKind::OpenApi));
        let editor_badge = if is_seq {
            "  [F3 open editor]"
        } else if is_code {
            "  [F4 open editor]"
        } else if is_gherkin {
            "  [F5 open editor]"
        } else if is_openapi {
            "  [F6 open editor]"
        } else {
            ""
        };
        let title = if field.required {
            format!(" {} *{editor_badge} ", field.label)
        } else {
            format!(" {}{editor_badge} ", field.label)
        };
        let caret = field.caret.min(field.value.len());
        let body = if is_seq {
            if field.value.trim().is_empty() {
                "<sequence diagram — press F3 to edit>".to_string()
            } else {
                // Keep the raw stored content visible, but make it clear it's not editable here.
                field.value.clone()
            }
        } else if is_code {
            if field.value.trim().is_empty() {
                "<code blocks — press F4 to edit>".to_string()
            } else {
                field.value.clone()
            }
        } else if is_gherkin {
            if field.value.trim().is_empty() {
                "<gherkin test cases — press F5 to edit>".to_string()
            } else {
                field.value.clone()
            }
        } else if is_openapi {
            if field.value.trim().is_empty() {
                "<openapi contract — press F6 to edit>".to_string()
            } else {
                field.value.clone()
            }
        } else if focused {
            // Insert the caret at its real byte offset so the user sees
            // exactly where the next character will land. This plays
            // nicely with arrow-key navigation inside multiline text.
            let (before, after) = field.value.split_at(caret);
            format!("{before}▌{after}")
        } else {
            field.value.clone()
        };
        let chunk = chunks[i];
        // Scroll so that the caret's wrapped row is visible. For
        // multi-line fields this prevents the caret from falling off
        // the bottom of the box as the user types beyond the visible
        // area. For single-line fields the math simplifies to 0.
        let scroll_y = if focused {
            let inner_w = chunk.width.saturating_sub(2); // borders
            let inner_h = chunk.height.saturating_sub(2).max(1);
            let caret_row = wrapped_row_count(&field.value[..caret], inner_w).saturating_sub(1);
            // If the caret is past the visible window, scroll so the
            // caret ends up on the last visible row.
            caret_row.saturating_sub(inner_h.saturating_sub(1))
        } else {
            0
        };
        let para = Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .style(style),
            );
        frame.render_widget(para, chunk);
    }
    // Keep the submit keybindings highlighted so they don't blend
    // into the other hint text.
    let submit_style = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::DarkGray);
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("F2", submit_style),
        Span::styled(" · ", hint_style),
        Span::styled("Ctrl+S", submit_style),
        Span::styled(" · ", hint_style),
        Span::styled("Alt+Enter", submit_style),
        Span::styled(" save · Tab field · ←→ caret · ↑↓ ", hint_style),
        Span::styled("caret (multiline) / field (single-line)", hint_style),
        Span::styled(" · Esc cancel", hint_style),
    ]))
    .wrap(Wrap { trim: false });
    frame.render_widget(hint, chunks[form.fields.len()]);
}

fn render_picker(frame: &mut Frame<'_>, area: Rect, picker: &crate::model::Picker) {
    let width = area.width.saturating_sub(10).clamp(40, 80);
    let visible_rows: u16 = u16::try_from(picker.items.len().min(12))
        .unwrap_or(1)
        .max(1);
    // Border (2) + hint line (1).
    let height = visible_rows.saturating_add(3).clamp(6, 18);
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);

    let outer = Block::default().borders(Borders::ALL).title(Span::styled(
        format!(" {} ", picker.title),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    let inner = outer.inner(rect);
    frame.render_widget(outer, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem<'_>> = picker
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let marker = if i == picker.cursor { "▶ " } else { "  " };
            let style = if i == picker.cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(item.label.clone(), style),
            ]))
        })
        .collect();
    let list = List::new(items);
    frame.render_widget(list, chunks[0]);

    let hint = Paragraph::new("↑/↓ select · Enter confirm · Esc cancel")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(hint, chunks[1]);
}

fn render_confirm(frame: &mut Frame<'_>, area: Rect, confirm: &crate::model::ConfirmPrompt) {
    let width = area.width.saturating_sub(20).clamp(40, 70);
    let height = 7u16;
    let left = (area.width.saturating_sub(width)) / 2 + area.x;
    let top = (area.height.saturating_sub(height)) / 2 + area.y;
    let rect = Rect {
        x: left,
        y: top,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let body = format!(
        "{}\n\n[Enter / y] Confirm   [Esc / n] Cancel",
        confirm.message
    );
    let para = Paragraph::new(body).wrap(Wrap { trim: true }).block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            format!(" {} ", confirm.title),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(para, rect);
}

/// Rendered by [`crate::screens::help::draw`] but exposed here so
/// other screens can embed an abridged hint strip.
pub fn help_lines() -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        "Global",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan),
    )));
    for k in GLOBAL_KEYS {
        out.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}", k.key),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(k.description),
        ]));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "Screens",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan),
    )));
    for k in SCREEN_KEYS {
        out.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}", k.key),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(k.description),
        ]));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "Forms",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan),
    )));
    for k in FORM_KEYS {
        out.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}", k.key),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(k.description),
        ]));
    }
    out
}

/// How many terminal rows the text will occupy when rendered with
/// word-wrap inside a region `width` columns wide. Empty lines still
/// consume one row so the returned value matches what `Paragraph`
/// produces with `Wrap { trim: false }`.
///
/// We count Unicode scalar values rather than display width; that
/// under-counts for double-wide CJK glyphs but is good enough for
/// TOML-ish form input and keeps us free of an extra dependency.
fn wrapped_row_count(text: &str, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    let w = usize::from(width);
    let mut rows: usize = 0;
    for line in text.split('\n') {
        let chars = line.chars().count();
        let this = if chars == 0 { 1 } else { chars.div_ceil(w) };
        rows = rows.saturating_add(this);
    }
    u16::try_from(rows.max(1)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::wrapped_row_count;

    #[test]
    fn wrapped_row_count_counts_blank_lines_as_one() {
        assert_eq!(wrapped_row_count("", 20), 1);
        assert_eq!(wrapped_row_count("\n\n", 20), 3);
    }

    #[test]
    fn wrapped_row_count_breaks_long_lines() {
        // 21 chars into a 10-wide region → 3 rows.
        assert_eq!(wrapped_row_count(&"x".repeat(21), 10), 3);
        // Two distinct lines, each wrapping twice.
        assert_eq!(
            wrapped_row_count(&format!("{a}\n{a}", a = "x".repeat(15)), 10),
            4
        );
    }

    #[test]
    fn wrapped_row_count_survives_zero_width() {
        assert_eq!(wrapped_row_count("hello", 0), 1);
    }
}
