use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::address::COL_DEFS;
use crate::step::{OutputCol, StepDef};

use super::meta::{self, TABLE_DESCRIPTIONS};
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

const DICT_FIELD_COUNT: u16 = 3; // Short form, Long form, Variants

impl PanelKind {
    /// Number of content lines for the panel body (fields + dropdown + border + spacing).
    fn content_lines(&self) -> u16 {
        match self {
            PanelKind::Step(panel) => {
                let dropdown = match &panel.focus {
                    PanelFocus::Dropdown { .. } | PanelFocus::DropdownEdit { .. } =>
                        dropdown_content_height(panel) as u16,
                    _ => 0,
                };
                // type selector (1) + blank line (1) + fields + dropdown + body border (2)
                2 + panel.visible_fields.len() as u16 + dropdown + 2
            }
            PanelKind::Dict(panel) => {
                let dropdown = if panel.field_cursor == 2 {
                    match &panel.focus {
                        PanelFocus::Dropdown { .. } => panel.variants.len().max(1) as u16,
                        PanelFocus::DropdownEdit { item, .. } => {
                            // +1 line if adding a new variant beyond existing
                            let extra = if *item >= panel.variants.len() { 1 } else { 0 };
                            (panel.variants.len() as u16 + extra).max(1)
                        }
                        _ => 0,
                    }
                } else { 0 };
                // fields + dropdown + body border (2) + blank line (1)
                DICT_FIELD_COUNT + dropdown + 2 + 1
            }
        }
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

/// Fields visible per step type.
pub(crate) fn visible_fields_for_type(step_type: &str) -> Vec<StepField> {
    match step_type {
        "extract" => vec![
            StepField::Label,
            StepField::InputCol,
            StepField::Pattern,
            StepField::Table,
            StepField::OutputCol,
            StepField::Replacement,
            StepField::SkipIfFilled,
        ],
        _ => vec![
            // "rewrite" (including former "standardize")
            StepField::Label,
            StepField::InputCol,
            StepField::Pattern,
            StepField::Table,
            StepField::Replacement,
            StepField::Mode,
        ],
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
            Some(OutputCol::Multi(m)) => {
                let mut pairs: Vec<_> = m.iter().collect();
                pairs.sort_by_key(|(_, v)| *v);
                pairs.iter().map(|(k, v)| format!("{}=${}", k, v)).collect::<Vec<_>>().join(", ")
            }
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
/// Only column pickers are dropdowns. Pattern/Table are inline text edits.
fn is_dropdown_field(field: StepField) -> bool {
    matches!(field, StepField::OutputCol | StepField::InputCol)
}

/// Render the step editor panel overlay.
pub(crate) fn render_step_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let panel = match &app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    let step_state = panel.step_index.map(|i| &app.steps[i]);

    let overlay = centered_overlay(area, app.panel.as_ref().unwrap().content_lines());
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
        PanelFocus::Navigating => "Enter: edit  Esc: close  Space: toggle  r: restore  t: change type",
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
                let prefix = format!("  {:16}", label);
                let avail = area.width.saturating_sub(prefix.len() as u16 + 2) as usize;
                let cursor_line = widgets::cursor_line(buffer, cursor, avail);
                let mut spans = vec![
                    Span::styled(prefix, Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
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

/// Handle a key event when the step panel is open.
pub(crate) fn handle_step_panel_key(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    if panel.show_discard_prompt {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => { app.panel = None; }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => { panel.show_discard_prompt = false; }
            _ => {}
        }
        return;
    }

    match panel.focus.clone() {
        PanelFocus::Navigating => handle_step_navigating(app, code),
        PanelFocus::InlineEdit { .. } => handle_step_inline_edit(app, code),
        PanelFocus::Dropdown { .. } => handle_step_dropdown(app, code),
        PanelFocus::DropdownEdit { .. } => handle_step_dropdown_edit(app, code),
    }
}

fn handle_step_navigating(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };
    let field_count = panel.visible_fields.len();

    match code {
        KeyCode::Down => {
            panel.field_cursor = (panel.field_cursor + 1) % field_count;
        }
        KeyCode::Up => {
            panel.field_cursor = if panel.field_cursor == 0 { field_count - 1 } else { panel.field_cursor - 1 };
        }
        KeyCode::Char('t') => {
            // Cycle step type forward
            let types = meta::STEP_TYPES;
            let idx = types.iter().position(|t| t.name == panel.def.step_type).unwrap_or(0);
            let new_idx = (idx + 1) % types.len();
            panel.def.step_type = types[new_idx].name.to_string();
            panel.visible_fields = visible_fields_for_type(&panel.def.step_type);
            if panel.field_cursor >= panel.visible_fields.len() {
                panel.field_cursor = panel.visible_fields.len().saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            let field = panel.visible_fields[panel.field_cursor];
            if is_dropdown_field(field) {
                panel.focus = PanelFocus::Dropdown { cursor: 0 };
            } else {
                let value = match field {
                    StepField::Label => panel.def.label.clone(),
                    StepField::Pattern => panel.def.pattern.clone().unwrap_or_default(),
                    StepField::Table => panel.def.table.clone().unwrap_or_default(),
                    StepField::Replacement => panel.def.replacement.clone().unwrap_or_default(),
                    _ => return,
                };
                let len = value.len();
                panel.focus = PanelFocus::InlineEdit { cursor: len, buffer: value };
            }
        }
        KeyCode::Char(' ') => {
            let field = panel.visible_fields[panel.field_cursor];
            match field {
                StepField::SkipIfFilled => {
                    let current = panel.def.skip_if_filled.unwrap_or(false);
                    panel.def.skip_if_filled = Some(!current);
                    app.dirty = true;
                }
                StepField::Mode => {
                    let current = panel.def.mode.as_deref();
                    panel.def.mode = if current == Some("per_word") { None } else { Some("per_word".to_string()) };
                    app.dirty = true;
                }
                _ => {}
            }
        }
        KeyCode::Char('r') => {
            if let Some(step_idx) = panel.step_index {
                if let Some(default) = &app.steps[step_idx].default_def {
                    panel.def = default.clone();
                    panel.visible_fields = visible_fields_for_type(&panel.def.step_type);
                    panel.pattern_segments = crate::pattern::parse_pattern(
                        panel.def.pattern.as_deref().unwrap_or(""));
                    if panel.field_cursor >= panel.visible_fields.len() {
                        panel.field_cursor = 0;
                    }
                    app.dirty = true;
                }
            }
        }
        KeyCode::Esc => {
            close_step_panel(app);
        }
        _ => {}
    }
}

fn handle_step_inline_edit(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };
    if let PanelFocus::InlineEdit { cursor, buffer } = &mut panel.focus {
        match code {
            KeyCode::Enter => {
                let value = buffer.clone();
                let field = panel.visible_fields[panel.field_cursor];
                match field {
                    StepField::Label => if !value.is_empty() { panel.def.label = value },
                    StepField::Pattern => {
                        panel.def.pattern = if value.is_empty() { None } else { Some(value) };
                        panel.pattern_segments = crate::pattern::parse_pattern(
                            panel.def.pattern.as_deref().unwrap_or(""));
                    }
                    StepField::Table => panel.def.table = if value.is_empty() { None } else { Some(value) },
                    StepField::Replacement => panel.def.replacement = if value.is_empty() { None } else { Some(value) },
                    _ => {}
                }
                panel.focus = PanelFocus::Navigating;
                app.dirty = true;
            }
            KeyCode::Esc => { panel.focus = PanelFocus::Navigating; }
            KeyCode::Backspace => {
                if *cursor > 0 { buffer.remove(*cursor - 1); *cursor -= 1; }
            }
            KeyCode::Left => { if *cursor > 0 { *cursor -= 1; } }
            KeyCode::Right => { if *cursor < buffer.len() { *cursor += 1; } }
            KeyCode::Char(c) => { buffer.insert(*cursor, c); *cursor += 1; }
            _ => {}
        }
    }
}

fn handle_step_dropdown(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };
    let field = panel.visible_fields[panel.field_cursor];
    let item_count = dropdown_item_count(panel, field);

    if let PanelFocus::Dropdown { cursor } = &mut panel.focus {
        match code {
            KeyCode::Down => { *cursor = (*cursor + 1) % item_count; }
            KeyCode::Up => { *cursor = if *cursor == 0 { item_count - 1 } else { *cursor - 1 }; }
            KeyCode::Enter => {
                match field {
                    StepField::Pattern => {}
                    StepField::Table => {
                        let table_name = TABLE_DESCRIPTIONS[*cursor].0;
                        panel.def.table = Some(table_name.to_string());
                        panel.focus = PanelFocus::Navigating;
                        app.dirty = true;
                    }
                    StepField::OutputCol => {
                        if matches!(&panel.def.output_col, Some(OutputCol::Multi(_))) {
                            // In multi mode, Enter closes the dropdown
                            panel.focus = PanelFocus::Navigating;
                        } else {
                            let fkey = COL_DEFS[*cursor].key;
                            panel.def.output_col = Some(OutputCol::Single(fkey.to_string()));
                            panel.focus = PanelFocus::Navigating;
                            app.dirty = true;
                        }
                    }
                    StepField::InputCol => {
                        if *cursor == 0 {
                            panel.def.input_col = None;
                        } else {
                            panel.def.input_col = Some(COL_DEFS[*cursor - 1].key.to_string());
                        }
                        panel.focus = PanelFocus::Navigating;
                        app.dirty = true;
                    }
                    _ => {}
                }
            }
            KeyCode::Char(' ') => {
                match field {
                    StepField::Pattern => {
                        let selectable: Vec<usize> = panel.pattern_segments.iter().enumerate()
                            .filter(|(_, s)| matches!(s,
                                crate::pattern::PatternSegment::AlternationGroup { .. } |
                                crate::pattern::PatternSegment::TableRef(_)))
                            .map(|(i, _)| i)
                            .collect();
                        if let Some(&seg_idx) = selectable.get(*cursor) {
                            if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                                panel.pattern_segments.get_mut(seg_idx)
                            {
                                let all_enabled = alternatives.iter().all(|a| a.enabled);
                                for alt in alternatives.iter_mut() {
                                    alt.enabled = !all_enabled;
                                }
                                panel.def.pattern = Some(crate::pattern::rebuild_pattern(&panel.pattern_segments));
                                app.dirty = true;
                            }
                        }
                    }
                    StepField::OutputCol => {
                        let fkey = COL_DEFS[*cursor].key.to_string();
                        match &mut panel.def.output_col {
                            Some(OutputCol::Multi(m)) => {
                                m.remove(&fkey);
                                if m.is_empty() {
                                    panel.def.output_col = None;
                                }
                            }
                            Some(OutputCol::Single(s)) if *s == fkey => {
                                panel.def.output_col = None;
                            }
                            _ => {}
                        }
                        app.dirty = true;
                    }
                    _ => {}
                }
            }
            KeyCode::Char(c @ '1'..='9') if field == StepField::OutputCol => {
                let group = (c as usize) - ('0' as usize);
                let fkey = COL_DEFS[*cursor].key.to_string();
                // Convert to Multi if needed, then assign group number
                let mut m = match panel.def.output_col.take() {
                    Some(OutputCol::Multi(m)) => m,
                    Some(OutputCol::Single(s)) => {
                        let mut m = std::collections::HashMap::new();
                        m.insert(s, 0); // existing single gets group 0 (whole match)
                        m
                    }
                    None => std::collections::HashMap::new(),
                };
                m.insert(fkey, group);
                panel.def.output_col = Some(OutputCol::Multi(m));
                app.dirty = true;
            }
            KeyCode::Char('e') if field == StepField::Pattern => {
                let text = panel.def.pattern.clone().unwrap_or_default();
                let len = text.len();
                panel.focus = PanelFocus::InlineEdit { cursor: len, buffer: text };
            }
            KeyCode::Esc => { panel.focus = PanelFocus::Navigating; }
            _ => {}
        }
    }
}

fn handle_step_dropdown_edit(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };
    if let PanelFocus::DropdownEdit { item: _, cursor, buffer } = &mut panel.focus {
        match code {
            KeyCode::Enter => {
                let value = buffer.clone();
                if !value.is_empty() {
                    panel.def.pattern = Some(value);
                    panel.pattern_segments = crate::pattern::parse_pattern(
                        panel.def.pattern.as_deref().unwrap_or(""));
                }
                panel.focus = PanelFocus::Dropdown { cursor: 0 };
                app.dirty = true;
            }
            KeyCode::Esc => { panel.focus = PanelFocus::Dropdown { cursor: 0 }; }
            KeyCode::Backspace => { if *cursor > 0 { buffer.remove(*cursor - 1); *cursor -= 1; } }
            KeyCode::Left => { if *cursor > 0 { *cursor -= 1; } }
            KeyCode::Right => { if *cursor < buffer.len() { *cursor += 1; } }
            KeyCode::Char(c) => { buffer.insert(*cursor, c); *cursor += 1; }
            _ => {}
        }
    }
}

fn dropdown_item_count(panel: &StepPanelState, field: StepField) -> usize {
    match field {
        StepField::Pattern => panel.pattern_segments.iter()
            .filter(|s| matches!(s,
                crate::pattern::PatternSegment::AlternationGroup { .. } |
                crate::pattern::PatternSegment::TableRef(_)))
            .count().max(1),
        StepField::Table => TABLE_DESCRIPTIONS.len(),
        StepField::OutputCol => COL_DEFS.len(),
        StepField::InputCol => COL_DEFS.len() + 1,
        _ => 0,
    }
}

fn close_step_panel(app: &mut App) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    if panel.is_new {
        let valid = validate_step_def(&panel.def);
        if valid {
            let def = panel.def.clone();
            let insert_idx = app.steps_list_state.selected().map(|i| i + 1).unwrap_or(app.steps.len());
            app.steps.insert(insert_idx, super::tabs::StepState {
                enabled: true,
                is_custom: true,
                def,
                default_def: None,
            });
            app.dirty = true;
            app.panel = None;
        } else {
            panel.show_discard_prompt = true;
        }
    } else {
        if let Some(idx) = panel.step_index {
            app.steps[idx].def = panel.def.clone();
            app.dirty = true;
        }
        app.panel = None;
    }
}

fn validate_step_def(def: &StepDef) -> bool {
    !def.label.is_empty() && (def.pattern.is_some() || def.table.is_some())
}

/// Render the dictionary entry panel overlay.
pub(crate) fn render_dict_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let panel = match &app.panel {
        Some(PanelKind::Dict(d)) => d,
        _ => return,
    };

    let overlay = centered_overlay(area, app.panel.as_ref().unwrap().content_lines());
    frame.render_widget(Clear, overlay);

    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ]).areas(overlay);

    let title = if panel.is_new { "New Entry" } else { "Edit Entry" };
    let header = Paragraph::new(Line::from(
        Span::styled(format!(" {}", panel.short), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ))
    .block(Block::bordered().title(title));
    frame.render_widget(header, header_area);

    render_dict_body(frame, panel, body_area);

    let hints = match &panel.focus {
        PanelFocus::Navigating => "Enter: edit  Esc: close",
        PanelFocus::InlineEdit { .. } => "Enter: confirm  Esc: cancel",
        PanelFocus::Dropdown { .. } => "Space: toggle  a: add  d: delete  Esc: collapse",
        PanelFocus::DropdownEdit { .. } => "Enter: confirm  Esc: cancel",
    };
    let footer = Paragraph::new(Line::from(Span::styled(
        format!(" {}", hints), Style::new().fg(Color::DarkGray),
    )))
    .block(Block::new().borders(ratatui::widgets::Borders::TOP).border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(footer, footer_area);
}

fn render_dict_body(frame: &mut Frame, panel: &DictPanelState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    let fields = ["Short form", "Long form", "Variants"];

    for (i, &label) in fields.iter().enumerate() {
        let is_selected = panel.focus == PanelFocus::Navigating && panel.field_cursor == i;

        if i == panel.field_cursor {
            if let PanelFocus::InlineEdit { cursor, ref buffer } = panel.focus {
                let prefix = format!("  {:16}", label);
                let avail = area.width.saturating_sub(prefix.len() as u16 + 2) as usize;
                let cursor_line = widgets::cursor_line(buffer, cursor, avail);
                let mut spans = vec![
                    Span::styled(prefix, Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
                ];
                spans.extend(cursor_line.spans);
                lines.push(Line::from(spans));
                continue;
            }
        }

        let value = match i {
            0 => panel.short.clone(),
            1 => panel.long.clone(),
            2 => if panel.variants.is_empty() {
                "(none)".to_string()
            } else {
                panel.variants.iter()
                    .map(|(t, _)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            _ => String::new(),
        };

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

        if i == 2 {
            if let PanelFocus::Dropdown { cursor } | PanelFocus::DropdownEdit { item: cursor, .. } = &panel.focus {
                if panel.field_cursor == 2 {
                    for (vi, (text, enabled)) in panel.variants.iter().enumerate() {
                        let is_sel = vi == *cursor;
                        let check = widgets::checkbox(*enabled);

                        if let PanelFocus::DropdownEdit { item, cursor: c, buffer } = &panel.focus {
                            if vi == *item {
                                let prefix = format!("    {} ", check);
                                let avail = area.width.saturating_sub(prefix.len() as u16 + 2) as usize;
                                let cursor_line = widgets::cursor_line(buffer, *c, avail);
                                let mut spans = vec![
                                    Span::styled(prefix, Style::new().fg(Color::Cyan)),
                                ];
                                spans.extend(cursor_line.spans);
                                lines.push(Line::from(spans));
                                continue;
                            }
                        }

                        let display = if *enabled {
                            Span::styled(text.clone(), if is_sel {
                                Style::new().fg(Color::White).bg(Color::DarkGray)
                            } else {
                                Style::new().fg(Color::DarkGray)
                            })
                        } else {
                            Span::styled(text.clone(), Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM))
                        };
                        let label = format!("    {} ", check);
                        lines.push(Line::from(vec![
                            Span::styled(label, if is_sel {
                                Style::new().fg(Color::Cyan).bg(Color::DarkGray)
                            } else {
                                Style::new().fg(Color::Cyan)
                            }),
                            display,
                        ]));
                    }
                    // New variant being typed (item index beyond existing variants)
                    if let PanelFocus::DropdownEdit { item, cursor: c, buffer } = &panel.focus {
                        if *item >= panel.variants.len() {
                            let prefix = format!("    {} ", widgets::checkbox(true));
                            let avail = area.width.saturating_sub(prefix.len() as u16 + 2) as usize;
                            let cursor_line = widgets::cursor_line(buffer, *c, avail);
                            let mut spans = vec![
                                Span::styled(prefix, Style::new().fg(Color::Cyan)),
                            ];
                            spans.extend(cursor_line.spans);
                            lines.push(Line::from(spans));
                        }
                    }
                }
            }
        }
    }

    let body = Paragraph::new(lines)
        .block(Block::bordered().border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(body, area);
}

/// Handle a key event when the dict panel is open.
pub(crate) fn handle_dict_panel_key(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Dict(d)) => d,
        _ => return,
    };

    match panel.focus.clone() {
        PanelFocus::Navigating => {
            match code {
                KeyCode::Down => { panel.field_cursor = (panel.field_cursor + 1) % 3; }
                KeyCode::Up => { panel.field_cursor = if panel.field_cursor == 0 { 2 } else { panel.field_cursor - 1 }; }
                KeyCode::Enter => {
                    match panel.field_cursor {
                        0 => {
                            let len = panel.short.len();
                            panel.focus = PanelFocus::InlineEdit { cursor: len, buffer: panel.short.clone() };
                        }
                        1 => {
                            let len = panel.long.len();
                            panel.focus = PanelFocus::InlineEdit { cursor: len, buffer: panel.long.clone() };
                        }
                        2 => {
                            if panel.variants.is_empty() {
                                // Go straight to adding a new variant
                                panel.focus = PanelFocus::DropdownEdit {
                                    item: 0, cursor: 0, buffer: String::new(),
                                };
                            } else {
                                panel.focus = PanelFocus::Dropdown { cursor: 0 };
                            }
                        }
                        _ => {}
                    }
                }
                KeyCode::Esc => {
                    close_dict_panel(app);
                }
                _ => {}
            }
        }
        PanelFocus::InlineEdit { .. } => {
            if let Some(PanelKind::Dict(panel)) = &mut app.panel {
                if let PanelFocus::InlineEdit { cursor, buffer } = &mut panel.focus {
                    match code {
                        KeyCode::Enter => {
                            let value = buffer.clone();
                            match panel.field_cursor {
                                0 => panel.short = value.to_uppercase(),
                                1 => panel.long = value.to_uppercase(),
                                _ => {}
                            }
                            panel.focus = PanelFocus::Navigating;
                            app.dirty = true;
                        }
                        KeyCode::Esc => { panel.focus = PanelFocus::Navigating; }
                        KeyCode::Backspace => { if *cursor > 0 { buffer.remove(*cursor - 1); *cursor -= 1; } }
                        KeyCode::Left => { if *cursor > 0 { *cursor -= 1; } }
                        KeyCode::Right => { if *cursor < buffer.len() { *cursor += 1; } }
                        KeyCode::Char(c) => { buffer.insert(*cursor, c); *cursor += 1; }
                        _ => {}
                    }
                }
            }
        }
        PanelFocus::Dropdown { .. } => {
            if let Some(PanelKind::Dict(panel)) = &mut app.panel {
                if let PanelFocus::Dropdown { cursor } = &mut panel.focus {
                    let count = panel.variants.len().max(1);
                    match code {
                        KeyCode::Down => { *cursor = (*cursor + 1) % count; }
                        KeyCode::Up => { *cursor = if *cursor == 0 { count - 1 } else { *cursor - 1 }; }
                        KeyCode::Char(' ') => {
                            if *cursor < panel.variants.len() {
                                panel.variants[*cursor].1 = !panel.variants[*cursor].1;
                                app.dirty = true;
                            }
                        }
                        KeyCode::Enter => {
                            if *cursor < panel.variants.len() {
                                let text = panel.variants[*cursor].0.clone();
                                let len = text.len();
                                panel.focus = PanelFocus::DropdownEdit {
                                    item: *cursor, cursor: len, buffer: text,
                                };
                            }
                        }
                        KeyCode::Char('a') => {
                            panel.focus = PanelFocus::DropdownEdit {
                                item: panel.variants.len(), cursor: 0, buffer: String::new(),
                            };
                        }
                        KeyCode::Char('d') => {
                            if *cursor < panel.variants.len() {
                                panel.variants.remove(*cursor);
                                if *cursor >= panel.variants.len() && !panel.variants.is_empty() {
                                    *cursor = panel.variants.len() - 1;
                                }
                                app.dirty = true;
                            }
                        }
                        KeyCode::Esc => { panel.focus = PanelFocus::Navigating; }
                        _ => {}
                    }
                }
            }
        }
        PanelFocus::DropdownEdit { .. } => {
            if let Some(PanelKind::Dict(panel)) = &mut app.panel {
                if let PanelFocus::DropdownEdit { item, cursor, buffer } = &mut panel.focus {
                    match code {
                        KeyCode::Enter => {
                            let value = buffer.clone();
                            if !value.is_empty() {
                                if *item >= panel.variants.len() {
                                    panel.variants.push((value, true));
                                } else {
                                    panel.variants[*item].0 = value;
                                }
                                app.dirty = true;
                            }
                            panel.focus = PanelFocus::Dropdown { cursor: panel.variants.len().saturating_sub(1) };
                        }
                        KeyCode::Esc => {
                            panel.focus = PanelFocus::Dropdown { cursor: (*item).min(panel.variants.len().saturating_sub(1)) };
                        }
                        KeyCode::Backspace => { if *cursor > 0 { buffer.remove(*cursor - 1); *cursor -= 1; } }
                        KeyCode::Left => { if *cursor > 0 { *cursor -= 1; } }
                        KeyCode::Right => { if *cursor < buffer.len() { *cursor += 1; } }
                        KeyCode::Char(c) => { buffer.insert(*cursor, c); *cursor += 1; }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn close_dict_panel(app: &mut App) {
    use super::tabs::{DictGroupState, GroupStatus};

    // Clone fields out of panel to avoid borrow conflict with app mutation
    let (entry_index, is_new, short, long, variants) = match &app.panel {
        Some(PanelKind::Dict(d)) => (
            d.entry_index,
            d.is_new,
            d.short.clone(),
            d.long.clone(),
            d.variants.iter()
                .filter(|(_, enabled)| *enabled)
                .map(|(text, _)| text.clone())
                .collect::<Vec<_>>(),
        ),
        _ => return,
    };

    if is_new {
        if !short.is_empty() {
            let entries = app.current_dict_entries_mut();
            entries.push(DictGroupState {
                short: short.clone(),
                long: long.clone(),
                variants: variants.clone(),
                status: GroupStatus::Added,
                original_short: short,
                original_long: long,
                original_variants: variants,
            });
            let len = entries.len();
            app.dict_list_state.select(Some(len - 1));
            app.dirty = true;
        }
    } else {
        let entry = &mut app.current_dict_entries_mut()[entry_index];
        entry.short = short;
        entry.long = long;
        entry.variants = variants;

        if entry.status == GroupStatus::Default {
            if entry.short != entry.original_short
                || entry.long != entry.original_long
                || entry.variants != entry.original_variants
            {
                entry.status = GroupStatus::Modified;
            }
        }
        app.dirty = true;
    }

    app.panel = None;
}
