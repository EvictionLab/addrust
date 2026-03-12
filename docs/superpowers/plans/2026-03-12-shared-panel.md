# Shared Panel Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace dead `panel.rs` with a shared panel overlay system for editing steps and dictionary entries, with inline editing, dropdown sections, and step type selection.

**Architecture:** Single-column overlay (`panel.rs`) with two editing modes: inline (cursor on same row) and dropdown (expand below). `PanelKind` enum distinguishes step vs dict panels. All form rendering/handling extracted from `tabs.rs`.

**Tech Stack:** Rust, ratatui, crossterm

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/tui/panel.rs` | Panel overlay frame, field rendering, inline edit, dropdown, key handling. All form-related code lives here. |
| `src/tui/tabs.rs` | Tab-level table views and key handling (steps table, dict table, output table). Opens panels but doesn't render them. |
| `src/tui/mod.rs` | App state, event loop. `form_state` → `panel`, `InputMode` dict variants removed. Routes keys to panel when open. |
| `src/tui/meta.rs` | Unchanged — `STEP_TYPES`, `PROP_HELP`, `TABLE_DESCRIPTIONS` |
| `src/tui/widgets.rs` | Unchanged — `selected_style`, `focus_border`, `checkbox`, `cursor_line`, `truncate` |

## Chunk 1: Panel state types and auto-height frame

### Task 1: Define panel state types in `panel.rs`

**Files:**
- Rewrite: `src/tui/panel.rs`

- [ ] **Step 1: Clear `panel.rs` and define `PanelFocus` enum**

Replace entire file contents:

```rust
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Warnings about unused imports, but no errors (the old exports from panel.rs were unused anyway).

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "refactor: define panel state types in panel.rs"
```

### Task 2: Add auto-height centered overlay helper

**Files:**
- Modify: `src/tui/panel.rs`

- [ ] **Step 1: Add `centered_rect_auto` helper to `panel.rs`**

After the `mod types` block, add:

```rust
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: add centered_overlay auto-height helper"
```

## Chunk 2: Step panel rendering — field list and frame

### Task 3: Add step field helpers

**Files:**
- Modify: `src/tui/panel.rs`

- [ ] **Step 1: Add `visible_fields_for_type` and `step_field_display` functions**

These are adapted from the existing functions in `tabs.rs` (`visible_fields_for_type` at line 168, `form_field_display` at line 216). Add after `centered_overlay`:

```rust
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: step field helpers for panel"
```

### Task 4: Render the step panel frame and field list

**Files:**
- Modify: `src/tui/panel.rs`

- [ ] **Step 1: Add `render_step_panel` function**

```rust
/// Render the step editor panel overlay.
pub(crate) fn render_step_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let panel = match &app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    let step_state = panel.step_index.map(|i| &app.steps[i]);

    // Count content lines: type selector (1) + fields + any expanded dropdown
    let dropdown_lines = match &panel.focus {
        PanelFocus::Dropdown { .. } | PanelFocus::DropdownEdit { .. } => {
            dropdown_content_height(panel) as u16
        }
        _ => 0,
    };
    let content_lines = 1 + panel.visible_fields.len() as u16 + dropdown_lines;
    let overlay = centered_overlay(area, content_lines);
    frame.render_widget(Clear, overlay);

    // Layout: header, body, footer
    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ]).areas(overlay);

    // Header
    let origin = if step_state.map(|s| s.is_custom).unwrap_or(true) {
        "CUSTOM"
    } else {
        "DEFAULT"
    };
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

    // Body: type selector + field rows
    render_step_body(frame, app, body_area);

    // Footer
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

    // Discard prompt
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
            .count().max(1) + 2,  // prefix + groups + suffix
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

    // Field rows
    let expanded_field_idx = match &panel.focus {
        PanelFocus::Dropdown { .. } | PanelFocus::DropdownEdit { .. } => Some(panel.field_cursor),
        _ => None,
    };

    for (i, &field) in panel.visible_fields.iter().enumerate() {
        let is_selected = panel.focus == PanelFocus::Navigating && panel.field_cursor == i;
        let (label, value) = step_field_display(field, &panel.def);

        // Check if this field is being inline-edited
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

        // Right-align value by padding
        let label_str = format!("  {:16}", label);
        let avail = area.width.saturating_sub(label_str.len() as u16 + 2) as usize;
        let value_padded = format!("{:>width$}  ", value, width = avail);

        lines.push(Line::from(vec![
            Span::styled(label_str, style),
            Span::styled(value_padded, style),
        ]));

        // If this field is expanded as dropdown, render dropdown items
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
            // First option: working string (None)
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: May warn about unused functions (not called yet), no errors.

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: step panel rendering with auto-height frame"
```

## Chunk 3: Step panel key handling

### Task 5: Step panel key handling

**Files:**
- Modify: `src/tui/panel.rs`

- [ ] **Step 1: Add `handle_step_panel_key` and helper functions**

```rust
/// Handle a key event when the step panel is open.
pub(crate) fn handle_step_panel_key(app: &mut App, code: KeyCode) {
    let panel = match &mut app.panel {
        Some(PanelKind::Step(s)) => s,
        _ => return,
    };

    // Discard prompt
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
        KeyCode::Left => {
            // Cycle step type backward — updates def.step_type directly
            let types = meta::STEP_TYPES;
            let idx = types.iter().position(|t| t.name == panel.def.step_type).unwrap_or(0);
            let new_idx = if idx == 0 { types.len() - 1 } else { idx - 1 };
            panel.def.step_type = types[new_idx].name.to_string();
            panel.visible_fields = visible_fields_for_type(&panel.def.step_type);
            if panel.field_cursor >= panel.visible_fields.len() {
                panel.field_cursor = panel.visible_fields.len().saturating_sub(1);
            }
        }
        KeyCode::Right => {
            // Cycle step type forward — updates def.step_type directly
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
                // Inline edit
                let value = match field {
                    StepField::Label => panel.def.label.clone(),
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
            // Restore all fields to default
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
                    StepField::Pattern => {
                        // Enter group to see alternatives
                        // (keep existing pattern drill-down behavior)
                    }
                    StepField::Table => {
                        let table_name = TABLE_DESCRIPTIONS[*cursor].0;
                        panel.def.table = Some(table_name.to_string());
                        panel.focus = PanelFocus::Navigating;
                        app.dirty = true;
                    }
                    StepField::OutputCol => {
                        if !matches!(&panel.def.output_col, Some(OutputCol::Multi(_))) {
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
                        // Toggle alternation group — get selectable segments
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
                                // Toggle all alts in group
                                let all_enabled = alternatives.iter().all(|a| a.enabled);
                                for alt in alternatives.iter_mut() {
                                    alt.enabled = !all_enabled;
                                }
                                panel.def.pattern = Some(crate::pattern::rebuild_pattern(&panel.pattern_segments));
                                app.dirty = true;
                            }
                        }
                    }
                    StepField::OutputCol if matches!(&panel.def.output_col, Some(OutputCol::Multi(_))) => {
                        let fkey = COL_DEFS[*cursor].key.to_string();
                        if let Some(OutputCol::Multi(m)) = &mut panel.def.output_col {
                            if m.contains_key(&fkey) { m.remove(&fkey); }
                            else { let max = m.values().max().copied().unwrap_or(0); m.insert(fkey, max + 1); }
                        }
                        app.dirty = true;
                    }
                    _ => {}
                }
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
                // For now, commit text edits for pattern raw editing
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
                default_enabled: true,
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
            // def.step_type is already updated by Left/Right cycling
            app.steps[idx].def = panel.def.clone();
            app.dirty = true;
        }
        app.panel = None;
    }
}

fn validate_step_def(def: &StepDef) -> bool {
    !def.label.is_empty() && (def.pattern.is_some() || def.table.is_some())
}

/// Get step type from a StepDef. Falls back to "extract" if empty.
pub(crate) fn get_step_type(def: &StepDef) -> String {
    if def.step_type.is_empty() {
        "extract".to_string()
    } else {
        def.step_type.clone()
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: step panel key handling"
```

## Chunk 4: Dictionary panel

### Task 6: Dictionary panel rendering and key handling

**Files:**
- Modify: `src/tui/panel.rs`

- [ ] **Step 1: Add `render_dict_panel` and `handle_dict_panel_key`**

```rust
/// Render the dictionary entry panel overlay.
pub(crate) fn render_dict_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let panel = match &app.panel {
        Some(PanelKind::Dict(d)) => d,
        _ => return,
    };

    let dropdown_lines = if panel.field_cursor == 2 {
        match &panel.focus {
            PanelFocus::Dropdown { .. } | PanelFocus::DropdownEdit { .. } =>
                panel.variants.len().max(1) as u16,
            _ => 0,
        }
    } else { 0 };
    let content_lines = 3 + dropdown_lines;  // short + long + variants header
    let overlay = centered_overlay(area, content_lines);
    frame.render_widget(Clear, overlay);

    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ]).areas(overlay);

    // Header
    let title = if panel.is_new { "New Entry" } else { "Edit Entry" };
    let header = Paragraph::new(Line::from(
        Span::styled(format!(" {}", panel.short), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ))
    .block(Block::bordered().title(title));
    frame.render_widget(header, header_area);

    // Body
    render_dict_body(frame, panel, body_area);

    // Footer
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

        // Inline edit check
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

        let value = match i {
            0 => panel.short.clone(),
            1 => panel.long.clone(),
            2 => format!("{} variants", panel.variants.len()),
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

        // Variants dropdown
        if i == 2 {
            if let PanelFocus::Dropdown { cursor } | PanelFocus::DropdownEdit { item: cursor, .. } = &panel.focus {
                if panel.field_cursor == 2 {
                    for (vi, (text, enabled)) in panel.variants.iter().enumerate() {
                        let is_sel = vi == *cursor;
                        let check = widgets::checkbox(*enabled);

                        // Check if editing this variant
                        if let PanelFocus::DropdownEdit { item, cursor: c, buffer } = &panel.focus {
                            if vi == *item {
                                let cursor_line = widgets::cursor_line(buffer, *c);
                                let mut spans = vec![
                                    Span::styled(format!("    {} ", check), Style::new().fg(Color::Cyan)),
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
                            panel.focus = PanelFocus::Dropdown { cursor: 0 };
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
        PanelFocus::Dropdown { .. } => {
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
        PanelFocus::DropdownEdit { .. } => {
            if let PanelFocus::DropdownEdit { item, cursor, buffer } = &mut panel.focus {
                match code {
                    KeyCode::Enter => {
                        let value = buffer.clone().to_uppercase();
                        if !value.is_empty() {
                            if *item >= panel.variants.len() {
                                // Adding new variant
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

fn close_dict_panel(app: &mut App) {
    // Clone fields out of panel to avoid borrow conflict with app mutation
    let (entry_index, short, long, variants) = match &app.panel {
        Some(PanelKind::Dict(d)) => (
            d.entry_index,
            d.short.clone(),
            d.long.clone(),
            d.variants.iter()
                .filter(|(_, enabled)| *enabled)
                .map(|(text, _)| text.clone())
                .collect::<Vec<_>>(),
        ),
        _ => return,
    };

    // Now safe to mutate app
    let entry = &mut app.current_dict_entries_mut()[entry_index];
    entry.short = short;
    entry.long = long;
    entry.variants = variants;

    // Update status
    if entry.status == super::tabs::GroupStatus::Default {
        if entry.short != entry.original_short
            || entry.long != entry.original_long
            || entry.variants != entry.original_variants
        {
            entry.status = super::tabs::GroupStatus::Modified;
        }
    }

    app.dirty = true;
    app.panel = None;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: dictionary panel rendering and key handling"
```

## Chunk 5: Wire panel into App and remove old form code

### Task 7: Update `mod.rs` — replace `form_state` with `panel`

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Replace `form_state: Option<FormState>` with `panel: Option<PanelKind>`**

In the `App` struct, change:
```rust
    pub(crate) form_state: Option<FormState>,
```
to:
```rust
    pub(crate) panel: Option<panel::PanelKind>,
```

Update the `App::new` constructor: change `form_state: None` to `panel: None`.

- [ ] **Step 2: Update imports in `mod.rs`**

Remove `FormState` from the `use tabs::{ ... }` import. Remove `handle_form_key` and `handle_input_mode` from that import (they'll be handled by panel). Add `InputMode` still needed for now (can be removed later if all dict editing goes through panel).

Actually: keep `InputMode` import for now — the small popup flows (AddShort, AddLong) will remain for adding brand new dict entries from the table view. The panel opens only when editing an existing entry. Remove `handle_form_key` from import, add panel imports.

Update the imports to:
```rust
use tabs::{
    DictGroupState, GroupStatus, InputMode, OutputSettingState, StepState,
    handle_dict_key, handle_input_mode, handle_output_key, handle_rules_key,
    render_text_with_cursor,
};
use panel::{PanelKind, handle_step_panel_key, handle_dict_panel_key};
```

- [ ] **Step 3: Update event loop to route to panel**

In `run_loop`, replace:
```rust
            // Form mode: form consumes all keys (including Esc, Tab, s)
            if app.form_state.is_some() {
                handle_form_key(app, key.code);
                continue;
            }
```
with:
```rust
            // Panel mode: panel consumes all keys
            if let Some(panel_kind) = &app.panel {
                match panel_kind {
                    PanelKind::Step(_) => handle_step_panel_key(app, key.code),
                    PanelKind::Dict(_) => handle_dict_panel_key(app, key.code),
                }
                continue;
            }
```

- [ ] **Step 4: Update render function**

In the `render` function, replace:
```rust
        Tab::Steps => {
            tabs::render_steps(frame, app, content_area);
            if app.form_state.is_some() {
                tabs::render_step_form(frame, app, content_area);
            }
        }
```
with:
```rust
        Tab::Steps => {
            tabs::render_steps(frame, app, content_area);
            if matches!(&app.panel, Some(PanelKind::Step(_))) {
                panel::render_step_panel(frame, app, content_area);
            }
        }
        Tab::Dictionaries => {
            tabs::render_dict(frame, app, content_area);
            if matches!(&app.panel, Some(PanelKind::Dict(_))) {
                panel::render_dict_panel(frame, app, content_area);
            }
        }
```
(The Dictionaries arm needs updating to render the dict panel overlay on top of the dict table.)

- [ ] **Step 5: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Errors about `form_state` references in tabs.rs — that's expected, we fix those next.

- [ ] **Step 6: Commit (even if not compiling — checkpoint)**

```bash
git add src/tui/mod.rs
git commit -m "refactor: wire panel into App, replace form_state"
```

### Task 8: Update `tabs.rs` — use panel for step editing, remove old form code

**Files:**
- Modify: `src/tui/tabs.rs`

- [ ] **Step 1: Update `handle_rules_key` to open step panel instead of form**

In `handle_rules_key` (around line 310), find where it opens the form on `KeyCode::Enter` and `KeyCode::Char('a')`. Replace those to open a `PanelKind::Step` instead.

For Enter (editing existing step):
```rust
KeyCode::Enter => {
    if let Some(i) = app.steps_list_state.selected() {
        let step = &app.steps[i];
        let visible = super::panel::visible_fields_for_type(&step.def.step_type);
        let segments = crate::pattern::parse_pattern(
            step.def.pattern.as_deref().unwrap_or(""));
        app.panel = Some(super::panel::PanelKind::Step(super::panel::StepPanelState {
            step_index: Some(i),
            def: step.def.clone(),
            visible_fields: visible,
            field_cursor: 0,
            focus: super::panel::PanelFocus::Navigating,
            pattern_segments: segments,
            is_new: false,
            show_discard_prompt: false,
        }));
    }
}
```

For 'a' (adding new step):
```rust
KeyCode::Char('a') => {
    let def = crate::step::StepDef {
        label: String::new(),
        step_type: "extract".to_string(),
        ..Default::default()
    };
    let visible = super::panel::visible_fields_for_type("extract");
    app.panel = Some(super::panel::PanelKind::Step(super::panel::StepPanelState {
        step_index: None,
        def,
        visible_fields: visible,
        field_cursor: 0,
        focus: super::panel::PanelFocus::Navigating,
        pattern_segments: Vec::new(),
        is_new: true,
        show_discard_prompt: false,
    }));
}
```

- [ ] **Step 2: Update `handle_dict_key` to open dict panel on Enter**

In `handle_dict_key` (around line 431), add an Enter handler:
```rust
KeyCode::Enter => {
    if let Some(i) = app.dict_list_state.selected() {
        let entry = &app.current_dict_entries()[i];
        let variants: Vec<(String, bool)> = entry.variants.iter()
            .map(|v| (v.clone(), true))
            .collect();
        app.panel = Some(super::panel::PanelKind::Dict(super::panel::DictPanelState {
            entry_index: i,
            short: entry.short.clone(),
            long: entry.long.clone(),
            variants,
            field_cursor: 0,
            focus: super::panel::PanelFocus::Navigating,
            is_new: false,
        }));
    }
}
```

- [ ] **Step 3: Remove old form code from `tabs.rs`**

Delete the following functions (they're now in `panel.rs`):
- `visible_fields_for_type` (line 168)
- `form_field_display` (line 216, renamed to `step_field_display` in panel)
- `handle_form_key` (line 748)
- `handle_form_left_key` (line 776)
- `close_form` (line 857)
- `handle_form_pattern_key` (line 886)
- `handle_form_targets_key` (line 986)
- `handle_form_table_key` (line 1054)
- `handle_form_text_edit` (line 1076)
- `render_step_form` (line 1452)
- `render_form_left_panel` (line 1515)
- `render_form_right_panel` (line 1554)
- `render_form_text_edit_panel` (line 1579)
- `render_form_help_panel` (line 1612)
- `render_form_pattern_panel` (around 1660+)
- `render_form_targets_panel` (around 1760+)
- `render_form_table_panel` (around 1850+)

Also remove:
- `FormField` enum (line 36)
- `FormFocus` enum (line 49)
- `FormState` struct (line 59)

Keep:
- `InputMode` — still used for dict add-new-entry flow
- `text_edit` — used by `handle_input_mode`
- `render_text_with_cursor` — used by mod.rs for InputMode popups
- `validate_step_def` — moved to panel.rs, remove from tabs.rs
- `field_key` — only if still needed; if not, remove
- `form_field_to_prop_key` — remove (was bridge between FormField and PropKey)

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Fix any remaining references to old types.

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: All 130 tests pass. The TUI tests in `mod.rs` don't test form rendering directly, so they should still work.

- [ ] **Step 6: Commit**

```bash
git add src/tui/tabs.rs src/tui/mod.rs src/tui/panel.rs
git commit -m "refactor: migrate step/dict editing to shared panel, remove old form code"
```

## Chunk 6: Clean up and manual testing

### Task 9: Remove `InputMode` dict variants (if all dict editing goes through panel)

**Files:**
- Modify: `src/tui/tabs.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Evaluate whether `InputMode` can be simplified**

The `InputMode::AddShort`/`AddLong` flow is for adding *brand new* dict entries from the table view (pressing `a`). This is separate from the panel which edits existing entries. Decide: keep the small popup flow for adding new entries, or route `a` through a new dict panel with `is_new: true`.

If routing through panel: update `handle_dict_key` for `KeyCode::Char('a')` to open a `PanelKind::Dict` with `is_new: true` and empty fields. Then remove `InputMode` entirely (or reduce to just `Normal`). Remove `handle_input_mode` from tabs.rs and the InputMode rendering from mod.rs.

If keeping popup flow: leave `InputMode` as-is. Both approaches work. Routing through panel is more consistent but slightly more work.

- [ ] **Step 2: If simplifying, remove InputMode dict variants and update mod.rs rendering**

Remove the `InputMode` rendering block in `mod.rs` `render()` (lines 621-691). Remove `handle_input_mode` from tabs.rs. Simplify or remove `InputMode` enum.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo check && cargo test 2>&1 | tail -20`

- [ ] **Step 4: Commit**

```bash
git add src/tui/tabs.rs src/tui/mod.rs
git commit -m "refactor: route dict add-new through panel, remove InputMode"
```

### Task 10: Remove `widgets.rs` dead code marker and verify

**Files:**
- Modify: `src/tui/widgets.rs`

- [ ] **Step 1: Remove `#![allow(dead_code)]` from widgets.rs**

The widgets should now all be used by panel.rs. Remove the allow attribute and verify no warnings.

- [ ] **Step 2: Verify**

Run: `cargo check 2>&1 | grep "warning.*dead_code"` — should be empty.

- [ ] **Step 3: Commit**

```bash
git add src/tui/widgets.rs
git commit -m "chore: remove dead_code allow from widgets.rs"
```

### Task 11: Manual smoke test

- [ ] **Step 1: Run the TUI and test step editing**

Run: `cargo run -- configure`

Test:
- Navigate to Steps tab, press Enter on a step
- Panel opens with correct type shown
- Left/Right cycles type, fields update
- Enter on Label → inline edit works
- Enter on Pattern → dropdown with checkboxes
- Space toggles groups
- `r` restores defaults
- Esc closes panel

- [ ] **Step 2: Test dictionary editing**

- Navigate to Dictionaries tab, press Enter on an entry
- Panel opens with Short, Long, Variants
- Enter on Short/Long → inline edit
- Enter on Variants → dropdown with checkboxes
- Space toggles variants, `a` adds, `d` deletes
- Esc closes and saves changes

- [ ] **Step 3: Test adding new entries**

- Steps tab: `a` opens panel with empty fields, type selector defaults to extract
- Dict tab: `a` opens panel with empty fields

- [ ] **Step 4: Final commit if any fixes needed**

```bash
git add -A && git commit -m "fix: panel smoke test fixes"
```
