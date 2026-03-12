use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::address::COL_DEFS;
use crate::pattern::{self, PatternSegment};
use crate::step::{OutputCol, StepDef};

use super::meta::{self, PropKey, TABLE_DESCRIPTIONS};
use super::widgets;
use super::App;

// Re-export types used by mod.rs and tabs.rs
pub(crate) use types::*;

mod types {
    /// Which panel is open.
    #[derive(Debug, Clone)]
    pub(crate) enum PanelKind {
        Step(StepPanelState),
        Dict(DictPanelState),
    }

    /// Focus state within the panel.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum PanelFocus {
        /// Navigating the field list with up/down.
        Navigating,
        /// Editing a single-value field inline (cursor position, buffer text).
        InlineEdit { cursor: usize, buffer: String },
        /// Navigating items in an expanded dropdown.
        Dropdown { cursor: usize },
        /// Editing an item within a dropdown (item index, cursor, buffer).
        DropdownEdit { item: usize, cursor: usize, buffer: String },
    }

    /// Step panel state.
    /// Note: step type is stored in `def.step_type` — no separate field.
    /// When cycling types with Left/Right, update `def.step_type` directly.
    #[derive(Debug, Clone)]
    pub(crate) struct StepPanelState {
        /// Index into App.steps, or None for new step.
        pub(crate) step_index: Option<usize>,
        /// Working copy of the step definition (includes step_type).
        pub(crate) def: crate::step::StepDef,
        /// Which fields are visible (computed from def.step_type).
        pub(crate) visible_fields: Vec<StepField>,
        /// Cursor position in visible_fields.
        pub(crate) field_cursor: usize,
        /// Current focus.
        pub(crate) focus: PanelFocus,
        /// Parsed pattern segments for drill-down.
        pub(crate) pattern_segments: Vec<crate::pattern::PatternSegment>,
        /// Whether this is a new step.
        pub(crate) is_new: bool,
        /// Show discard confirmation.
        pub(crate) show_discard_prompt: bool,
    }

    /// Dictionary panel state.
    #[derive(Debug, Clone)]
    pub(crate) struct DictPanelState {
        /// Index into the current dict_entries vec.
        pub(crate) entry_index: usize,
        /// Working copies.
        pub(crate) short: String,
        pub(crate) long: String,
        pub(crate) variants: Vec<(String, bool)>,  // (text, enabled)
        /// Which field is focused: 0=short, 1=long, 2=variants.
        pub(crate) field_cursor: usize,
        /// Current focus.
        pub(crate) focus: PanelFocus,
        /// Whether this is a new entry.
        pub(crate) is_new: bool,
    }

    /// Fields in the step panel.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum StepField {
        Label,
        Pattern,
        Table,
        OutputCol,
        SkipIfFilled,
        Replacement,
        InputCol,
        Mode,
    }
}

/// Compute a centered overlay rect with auto-height.
/// Width is 70% of area, height is `content_lines` + 5 (header 3 + footer 2),
/// clamped to area height - 4.
pub(crate) fn centered_overlay(area: Rect, content_lines: u16) -> Rect {
    let width = (area.width * 70 / 100).max(50).min(100).min(area.width);
    let height = (content_lines + 5).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

/// Compute visible fields for a step type.
pub(crate) fn visible_fields_for_type(step_type: &str) -> Vec<StepField> {
    let meta = meta::find_step_type(step_type);
    match meta {
        Some(m) => m.visible.iter().filter_map(|pk| match pk {
            PropKey::Label => Some(StepField::Label),
            PropKey::Pattern => Some(StepField::Pattern),
            PropKey::Table => Some(StepField::Table),
            PropKey::OutputCol => Some(StepField::OutputCol),
            PropKey::SkipIfFilled => Some(StepField::SkipIfFilled),
            PropKey::Replacement => Some(StepField::Replacement),
            PropKey::InputCol => Some(StepField::InputCol),
            PropKey::Mode => Some(StepField::Mode),
        }).collect(),
        None => vec![StepField::Label, StepField::Pattern, StepField::Replacement],
    }
}

/// Get display label and current value for a step field.
fn step_field_display(field: StepField, def: &StepDef) -> (&'static str, String) {
    match field {
        StepField::Label => ("Label", def.label.clone()),
        StepField::Pattern => ("Pattern", widgets::truncate(
            def.pattern.as_deref().unwrap_or("(none)"), 40)),
        StepField::Table => ("Table", def.table.as_deref().unwrap_or("(none)").to_string()),
        StepField::OutputCol => ("Output", match &def.output_col {
            Some(OutputCol::Single(s)) => s.clone(),
            Some(OutputCol::Multi(m)) => format!("multi ({})", m.len()),
            None => "(none)".to_string(),
        }),
        StepField::SkipIfFilled => ("Skip if filled",
            if def.skip_if_filled == Some(true) { "yes".to_string() } else { "no".to_string() }),
        StepField::Replacement => ("Replacement",
            def.replacement.as_deref().unwrap_or("(none)").to_string()),
        StepField::InputCol => ("Input column",
            def.input_col.as_deref().unwrap_or("(working string)").to_string()),
        StepField::Mode => ("Mode",
            def.mode.as_deref().unwrap_or("whole field").to_string()),
    }
}

/// Whether a field uses dropdown (vs inline edit).
fn is_dropdown_field(field: StepField) -> bool {
    matches!(field, StepField::Pattern | StepField::Table
        | StepField::OutputCol | StepField::InputCol)
}

/// Render the step editor panel overlay.
pub(crate) fn render_step_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let panel = match &app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    let step_state = panel.step_index.map(|i| &app.steps[i]);

    let dropdown_lines = match &panel.focus {
        PanelFocus::Dropdown { .. } | PanelFocus::DropdownEdit { .. } => {
            dropdown_content_height(panel) as u16
        }
        _ => 0,
    };
    let content_lines = 1 + panel.visible_fields.len() as u16 + dropdown_lines;
    let overlay = centered_overlay(area, content_lines);
    frame.render_widget(Clear, overlay);

    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ]).areas(overlay);

    // Header
    let origin = if step_state.map(|s| s.is_custom).unwrap_or(true) { "CUSTOM" } else { "DEFAULT" };
    let modified = if step_state.map(|s| s.is_modified()).unwrap_or(false) {
        Span::styled("  * MODIFIED", Style::new().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {}", panel.def.label), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(origin, Style::new().fg(Color::DarkGray)),
        modified,
    ]))
    .block(Block::bordered().title("Step Editor"));
    frame.render_widget(header, header_area);

    render_step_body(frame, app, body_area);

    let hints = match &panel.focus {
        PanelFocus::Navigating => "Enter: edit  Esc: close  Space: toggle  r: restore default  Left/Right: change type",
        PanelFocus::InlineEdit { .. } => "Enter: confirm  Esc: cancel",
        PanelFocus::Dropdown { .. } => "Space: toggle  Enter: edit  Esc: collapse  a: add  d: delete",
        PanelFocus::DropdownEdit { .. } => "Enter: confirm  Esc: cancel",
    };
    let footer = Paragraph::new(Line::from(Span::styled(
        format!(" {}", hints), Style::new().fg(Color::DarkGray),
    )))
    .block(Block::new().borders(ratatui::widgets::Borders::TOP).border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(footer, footer_area);

    if panel.show_discard_prompt {
        let popup = super::centered_rect(50, 5, overlay);
        frame.render_widget(Clear, popup);
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                " Missing required fields. Discard step? (y/n) ",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
        ]).block(Block::bordered().title("Confirm"));
        frame.render_widget(msg, popup);
    }
}

fn dropdown_content_height(panel: &StepPanelState) -> usize {
    let field = panel.visible_fields.get(panel.field_cursor).copied();
    match field {
        Some(StepField::Pattern) => panel.pattern_segments.iter()
            .filter(|s| matches!(s,
                crate::pattern::PatternSegment::AlternationGroup { .. } |
                crate::pattern::PatternSegment::TableRef(_)))
            .count().max(1) + 2,
        Some(StepField::Table) => TABLE_DESCRIPTIONS.len(),
        Some(StepField::OutputCol) => COL_DEFS.len(),
        Some(StepField::InputCol) => COL_DEFS.len() + 1,
        _ => 0,
    }
}

fn render_step_body(frame: &mut Frame, app: &App, area: Rect) {
    let panel = match &app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    let mut lines: Vec<Line> = Vec::new();

    // Type selector row
    let type_spans: Vec<Span> = meta::STEP_TYPES.iter().map(|t| {
        if t.name == panel.def.step_type {
            Span::styled(
                format!(" {} ", t.display),
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {} ", t.display), Style::new().fg(Color::DarkGray))
        }
    }).collect();
    lines.push(Line::from(type_spans));
    lines.push(Line::from(""));

    let expanded_field_idx = match &panel.focus {
        PanelFocus::Dropdown { .. } | PanelFocus::DropdownEdit { .. } => Some(panel.field_cursor),
        _ => None,
    };

    for (i, &field) in panel.visible_fields.iter().enumerate() {
        let is_selected = panel.focus == PanelFocus::Navigating && panel.field_cursor == i;
        let (label, value) = step_field_display(field, &panel.def);

        if i == panel.field_cursor {
            if let PanelFocus::InlineEdit { cursor, ref buffer } = panel.focus {
                let cursor_line = widgets::cursor_line(buffer, cursor);
                let mut spans = vec![
                    Span::styled(format!("  {:16}", label), Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
                ];
                spans.extend(cursor_line.spans);
                lines.push(Line::from(spans));
                continue;
            }
        }

        let style = if is_selected {
            Style::new().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };

        let label_str = format!("  {:16}", label);
        let avail = area.width.saturating_sub(label_str.len() as u16 + 2) as usize;
        let value_padded = format!("{:>width$}  ", value, width = avail);

        lines.push(Line::from(vec![
            Span::styled(label_str, style),
            Span::styled(value_padded, style),
        ]));

        if expanded_field_idx == Some(i) {
            render_step_dropdown_lines(&mut lines, panel, field, area.width);
        }
    }

    let body = Paragraph::new(lines)
        .block(Block::bordered().border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(body, area);
}

fn render_step_dropdown_lines(
    lines: &mut Vec<Line>,
    panel: &StepPanelState,
    field: StepField,
    _width: u16,
) {
    let dropdown_cursor = match &panel.focus {
        PanelFocus::Dropdown { cursor } => Some(*cursor),
        PanelFocus::DropdownEdit { item, .. } => Some(*item),
        _ => None,
    };

    match field {
        StepField::Pattern => {
            let selectable: Vec<(usize, &crate::pattern::PatternSegment)> =
                panel.pattern_segments.iter().enumerate()
                    .filter(|(_, s)| matches!(s,
                        crate::pattern::PatternSegment::AlternationGroup { .. } |
                        crate::pattern::PatternSegment::TableRef(_)))
                    .collect();
            for (sel_i, (_, seg)) in selectable.iter().enumerate() {
                let is_selected = dropdown_cursor == Some(sel_i);
                match seg {
                    crate::pattern::PatternSegment::AlternationGroup { alternatives, .. } => {
                        let enabled_count = alternatives.iter().filter(|a| a.enabled).count();
                        let label = format!("    {} Group {} ({}/{})",
                            widgets::checkbox(enabled_count == alternatives.len()),
                            sel_i + 1, enabled_count, alternatives.len());
                        let style = if is_selected {
                            Style::new().fg(Color::White).bg(Color::DarkGray)
                        } else {
                            Style::new().fg(Color::DarkGray)
                        };
                        lines.push(Line::from(Span::styled(label, style)));
                    }
                    crate::pattern::PatternSegment::TableRef(name) => {
                        let label = format!("    Table: {}", name);
                        let style = if is_selected {
                            Style::new().fg(Color::White).bg(Color::DarkGray)
                        } else {
                            Style::new().fg(Color::DarkGray)
                        };
                        lines.push(Line::from(Span::styled(label, style)));
                    }
                    _ => {}
                }
            }
        }
        StepField::Table => {
            for (i, (name, desc)) in TABLE_DESCRIPTIONS.iter().enumerate() {
                let is_current = panel.def.table.as_deref() == Some(*name);
                let is_selected = dropdown_cursor == Some(i);
                let check = widgets::checkbox(is_current);
                let label = format!("    {} {:20} {}", check, name, desc);
                let style = if is_selected {
                    Style::new().fg(Color::White).bg(Color::DarkGray)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                lines.push(Line::from(Span::styled(label, style)));
            }
        }
        StepField::OutputCol => {
            let is_multi = matches!(&panel.def.output_col, Some(OutputCol::Multi(_)));
            for (i, col) in COL_DEFS.iter().enumerate() {
                let is_selected_col = match &panel.def.output_col {
                    Some(OutputCol::Single(s)) => s == col.key,
                    Some(OutputCol::Multi(m)) => m.contains_key(col.key),
                    None => false,
                };
                let is_selected = dropdown_cursor == Some(i);
                let check = widgets::checkbox(is_selected_col);
                let group_info = if is_multi {
                    if let Some(OutputCol::Multi(m)) = &panel.def.output_col {
                        m.get(col.key).map(|g| format!(" ${}",g)).unwrap_or_default()
                    } else { String::new() }
                } else { String::new() };
                let label = format!("    {} {:16}{}", check, col.key, group_info);
                let style = if is_selected {
                    Style::new().fg(Color::White).bg(Color::DarkGray)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                lines.push(Line::from(Span::styled(label, style)));
            }
        }
        StepField::InputCol => {
            let ws_selected = panel.def.input_col.is_none();
            let is_selected = dropdown_cursor == Some(0);
            let check = widgets::checkbox(ws_selected);
            let label = format!("    {} (working string)", check);
            let style = if is_selected {
                Style::new().fg(Color::White).bg(Color::DarkGray)
            } else {
                Style::new().fg(Color::DarkGray)
            };
            lines.push(Line::from(Span::styled(label, style)));
            for (i, col) in COL_DEFS.iter().enumerate() {
                let is_current = panel.def.input_col.as_deref() == Some(col.key);
                let is_selected = dropdown_cursor == Some(i + 1);
                let check = widgets::checkbox(is_current);
                let label = format!("    {} {}", check, col.key);
                let style = if is_selected {
                    Style::new().fg(Color::White).bg(Color::DarkGray)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                lines.push(Line::from(Span::styled(label, style)));
            }
        }
        _ => {}
    }
}
