use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Row, Table, Tabs};
use ratatui::Frame;

use crate::step::OutputCol;
use crate::tables::abbreviations::build_default_tables;

use super::App;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Input mode for dictionary and pattern editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InputMode {
    Normal,
    /// Adding a new entry: typing the short form (text, cursor).
    AddShort(String, usize),
    /// Adding a new entry: short form done, typing the long form (short, long, cursor).
    AddLong(String, String, usize),
    /// Editing an existing entry's long form: (index, text, cursor).
    EditLong(usize, String, usize),
    /// Viewing/editing a group's variants: (group_index, variant_cursor).
    EditVariants(usize, usize),
    /// Adding a new variant to a group: (group_index, text, cursor).
    AddVariant(usize, String, usize),
}

/// A per-component output format setting.
#[derive(Debug, Clone)]
pub(crate) struct OutputSettingState {
    pub(crate) component: String,
    pub(crate) format: crate::config::OutputFormat,
    pub(crate) default_format: crate::config::OutputFormat,
    pub(crate) example_short: String,
    pub(crate) example_long: String,
}

/// A dictionary group with its change status.
#[derive(Debug, Clone)]
pub(crate) struct DictGroupState {
    pub(crate) short: String,
    pub(crate) long: String,
    pub(crate) variants: Vec<String>,
    pub(crate) status: GroupStatus,
    /// Original values for tracking overrides.
    pub(crate) original_short: String,
    pub(crate) original_long: String,
    pub(crate) original_variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GroupStatus {
    Default,
    Added,
    Removed,
    Modified,
}

/// A step with its current and default state, carrying full definition.
#[derive(Debug, Clone)]
pub(crate) struct StepState {
    pub(crate) enabled: bool,
    pub(crate) default_enabled: bool,
    pub(crate) is_custom: bool,
    pub(crate) def: crate::step::StepDef,
    pub(crate) default_def: Option<crate::step::StepDef>,
}

impl StepState {
    pub(crate) fn label(&self) -> &str { &self.def.label }
    pub(crate) fn step_type(&self) -> &str { &self.def.step_type }
    pub(crate) fn is_modified(&self) -> bool {
        match &self.default_def {
            None => false,
            Some(default) => self.def != *default || self.enabled != self.default_enabled,
        }
    }
}

/// Result of a text editing keystroke.
pub(crate) enum TextEditResult {
    /// Text was modified or cursor moved — continue editing.
    Continue,
    /// Enter was pressed — return the final text.
    Submit(String),
    /// Esc was pressed — cancel.
    Cancel,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Handle a keystroke for cursor-aware text editing.
/// Returns the action to take. Mutates text and cursor in place for Continue.
pub(crate) fn text_edit(text: &mut String, cursor: &mut usize, code: KeyCode) -> TextEditResult {
    match code {
        KeyCode::Enter => TextEditResult::Submit(text.clone()),
        KeyCode::Esc => TextEditResult::Cancel,
        KeyCode::Left => {
            if *cursor > 0 { *cursor -= 1; }
            TextEditResult::Continue
        }
        KeyCode::Right => {
            if *cursor < text.len() { *cursor += 1; }
            TextEditResult::Continue
        }
        KeyCode::Home => {
            *cursor = 0;
            TextEditResult::Continue
        }
        KeyCode::End => {
            *cursor = text.len();
            TextEditResult::Continue
        }
        KeyCode::Char(c) => {
            text.insert(*cursor, c);
            *cursor += 1;
            TextEditResult::Continue
        }
        KeyCode::Backspace => {
            if *cursor > 0 {
                *cursor -= 1;
                text.remove(*cursor);
            }
            TextEditResult::Continue
        }
        KeyCode::Delete => {
            if *cursor < text.len() {
                text.remove(*cursor);
            }
            TextEditResult::Continue
        }
        _ => TextEditResult::Continue,
    }
}

/// Render text with a cursor indicator at the given position.
pub(crate) fn render_text_with_cursor(text: &str, cursor: usize) -> String {
    let mut display = String::with_capacity(text.len() + 1);
    for (i, c) in text.chars().enumerate() {
        if i == cursor {
            // Use combining underline to show cursor position
            display.push('|');
        }
        display.push(c);
    }
    if cursor >= text.len() {
        display.push('_');
    }
    display
}

// ---------------------------------------------------------------------------
// Key handlers
// ---------------------------------------------------------------------------

pub(crate) fn handle_rules_key(app: &mut App, code: KeyCode) {
    let len = app.steps.len();
    if len == 0 {
        return;
    }

    // Move mode: step is grabbed, arrow keys reposition it
    if let Some(moving_idx) = app.moving_step {
        match code {
            KeyCode::Down => {
                if moving_idx + 1 < len {
                    app.steps.swap(moving_idx, moving_idx + 1);
                    let new_idx = moving_idx + 1;
                    app.moving_step = Some(new_idx);
                    app.steps_list_state.select(Some(new_idx));
                }
            }
            KeyCode::Up => {
                if moving_idx > 0 {
                    app.steps.swap(moving_idx, moving_idx - 1);
                    let new_idx = moving_idx - 1;
                    app.moving_step = Some(new_idx);
                    app.steps_list_state.select(Some(new_idx));
                }
            }
            KeyCode::Enter => {
                app.moving_step = None;
                app.moving_step_origin = None;
                app.dirty = true;
            }
            KeyCode::Esc => {
                // Cancel: remove from current position, re-insert at origin
                if let Some(origin) = app.moving_step_origin {
                    let step = app.steps.remove(moving_idx);
                    app.steps.insert(origin, step);
                    app.steps_list_state.select(Some(origin));
                }
                app.moving_step = None;
                app.moving_step_origin = None;
            }
            _ => {} // All other keys ignored in move mode
        }
        return;
    }

    // Normal mode
    match code {
        KeyCode::Down => {
            let i = app.steps_list_state.selected().unwrap_or(0);
            app.steps_list_state.select(Some((i + 1) % len));
        }
        KeyCode::Up => {
            let i = app.steps_list_state.selected().unwrap_or(0);
            app.steps_list_state
                .select(Some(if i == 0 { len - 1 } else { i - 1 }));
        }
        KeyCode::Char(' ') => {
            if let Some(i) = app.steps_list_state.selected() {
                app.steps[i].enabled = !app.steps[i].enabled;
                app.dirty = true;
            }
        }
        KeyCode::Enter => {
            if let Some(i) = app.steps_list_state.selected() {
                let step = &app.steps[i];
                let step_type = if step.def.step_type.is_empty() {
                    "extract".to_string()
                } else {
                    step.def.step_type.clone()
                };
                let visible = super::panel::visible_fields_for_type(&step_type);
                let segments = crate::pattern::parse_pattern(
                    step.def.pattern.as_deref().unwrap_or(""));
                let mut def = step.def.clone();
                def.step_type = step_type;
                app.panel = Some(super::panel::PanelKind::Step(super::panel::StepPanelState {
                    step_index: Some(i),
                    def,
                    visible_fields: visible,
                    field_cursor: 0,
                    focus: super::panel::PanelFocus::Navigating,
                    pattern_segments: segments,
                    is_new: false,
                    show_discard_prompt: false,
                }));
            }
        }
        KeyCode::Char('m') => {
            if let Some(i) = app.steps_list_state.selected() {
                app.moving_step = Some(i);
                app.moving_step_origin = Some(i);
            }
        }
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
        KeyCode::Char('d') => {
            if let Some(i) = app.steps_list_state.selected() {
                if app.steps[i].is_custom {
                    app.confirm_delete = Some(i);
                }
            }
        }
        _ => {}
    }
}


pub(crate) fn handle_dict_key(app: &mut App, code: KeyCode) {
    let num_tables = app.table_names.len();

    match code {
        // Sub-tab navigation
        KeyCode::Right => {
            app.dict_tab_index = (app.dict_tab_index + 1) % num_tables;
            let len = app.current_dict_entries().len();
            app.dict_list_state
                .select(if len > 0 { Some(0) } else { None });
        }
        KeyCode::Left => {
            app.dict_tab_index = if app.dict_tab_index == 0 {
                num_tables - 1
            } else {
                app.dict_tab_index - 1
            };
            let len = app.current_dict_entries().len();
            app.dict_list_state
                .select(if len > 0 { Some(0) } else { None });
        }
        // Entry navigation
        KeyCode::Down => {
            let len = app.current_dict_entries().len();
            if len > 0 {
                let i = app.dict_list_state.selected().unwrap_or(0);
                app.dict_list_state.select(Some((i + 1) % len));
            }
        }
        KeyCode::Up => {
            let len = app.current_dict_entries().len();
            if len > 0 {
                let i = app.dict_list_state.selected().unwrap_or(0);
                app.dict_list_state
                    .select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
        }
        // Toggle removal
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(i) = app.dict_list_state.selected() {
                let entry = &mut app.current_dict_entries_mut()[i];
                match entry.status {
                    GroupStatus::Default => {
                        entry.status = GroupStatus::Removed;
                        app.dirty = true;
                    }
                    GroupStatus::Removed => {
                        entry.status = GroupStatus::Default;
                        app.dirty = true;
                    }
                    GroupStatus::Added => {
                        // Remove the added entry entirely
                        app.current_dict_entries_mut().remove(i);
                        let len = app.current_dict_entries().len();
                        if len == 0 {
                            app.dict_list_state.select(None);
                        } else if i >= len {
                            app.dict_list_state.select(Some(len - 1));
                        }
                        app.dirty = true;
                    }
                    GroupStatus::Modified => {
                        // Revert to original values
                        entry.short = entry.original_short.clone();
                        entry.long = entry.original_long.clone();
                        entry.variants = entry.original_variants.clone();
                        entry.status = GroupStatus::Default;
                        app.dirty = true;
                    }
                }
            }
        }
        // Add new entry
        KeyCode::Char('a') => {
            app.input_mode = InputMode::AddShort(String::new(), 0);
        }
        // Toggle removal (like steps tab Space toggle)
        KeyCode::Char(' ') => {
            if let Some(i) = app.dict_list_state.selected() {
                let entry = &mut app.current_dict_entries_mut()[i];
                match entry.status {
                    GroupStatus::Default | GroupStatus::Modified => {
                        entry.status = GroupStatus::Removed;
                        app.dirty = true;
                    }
                    GroupStatus::Removed => {
                        // Restore: if it was modified before removal, go back to Modified
                        if entry.short != entry.original_short
                            || entry.long != entry.original_long
                            || entry.variants != entry.original_variants
                        {
                            entry.status = GroupStatus::Modified;
                        } else {
                            entry.status = GroupStatus::Default;
                        }
                        app.dirty = true;
                    }
                    GroupStatus::Added => {
                        // Remove the added entry entirely
                        app.current_dict_entries_mut().remove(i);
                        let len = app.current_dict_entries().len();
                        if len == 0 {
                            app.dict_list_state.select(None);
                        } else if i >= len {
                            app.dict_list_state.select(Some(len - 1));
                        }
                        app.dirty = true;
                    }
                }
            }
        }
        // Open dict panel
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
        // Edit long form directly
        KeyCode::Char('e') => {
            if let Some(i) = app.dict_list_state.selected() {
                let entry = &app.current_dict_entries()[i];
                if entry.status != GroupStatus::Removed {
                    let cursor = entry.long.len();
                    app.input_mode = InputMode::EditLong(i, entry.long.clone(), cursor);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn handle_input_mode(app: &mut App, code: KeyCode) {
    match &mut app.input_mode {
        InputMode::AddShort(short, cursor) => {
            match text_edit(short, cursor, code) {
                TextEditResult::Submit(s) if !s.is_empty() => {
                    let s = s.to_uppercase();
                    let is_vl = {
                        let tables = build_default_tables();
                        tables
                            .get(&app.table_names[app.dict_tab_index])
                            .map(|t| t.is_value_list())
                            .unwrap_or(false)
                    };
                    if is_vl {
                        let new_entry = DictGroupState {
                            short: s.clone(),
                            long: String::new(),
                            variants: Vec::new(),
                            status: GroupStatus::Added,
                            original_short: s,
                            original_long: String::new(),
                            original_variants: Vec::new(),
                        };
                        app.current_dict_entries_mut().push(new_entry);
                        let len = app.current_dict_entries().len();
                        app.dict_list_state.select(Some(len - 1));
                        app.dirty = true;
                        app.input_mode = InputMode::Normal;
                    } else {
                        app.input_mode = InputMode::AddLong(s, String::new(), 0);
                    }
                }
                TextEditResult::Submit(_) => {} // empty — ignore
                TextEditResult::Cancel => { app.input_mode = InputMode::Normal; }
                TextEditResult::Continue => {}
            }
        }
        InputMode::AddLong(short, long, cursor) => {
            let short_snapshot = short.clone();
            match text_edit(long, cursor, code) {
                TextEditResult::Submit(l) => {
                    if !l.is_empty() {
                        let s = short_snapshot.to_uppercase();
                        let l = l.to_uppercase();
                        let new_entry = DictGroupState {
                            short: s.clone(),
                            long: l.clone(),
                            variants: Vec::new(),
                            status: GroupStatus::Added,
                            original_short: s,
                            original_long: l,
                            original_variants: Vec::new(),
                        };
                        app.current_dict_entries_mut().push(new_entry);
                        let len = app.current_dict_entries().len();
                        app.dict_list_state.select(Some(len - 1));
                        app.dirty = true;
                    }
                    app.input_mode = InputMode::Normal;
                }
                TextEditResult::Cancel => { app.input_mode = InputMode::Normal; }
                TextEditResult::Continue => {}
            }
        }
        InputMode::EditLong(idx, text, cursor) => {
            let idx_val = *idx;
            match text_edit(text, cursor, code) {
                TextEditResult::Submit(new_long) => {
                    let new_long = new_long.to_uppercase();
                    let entry = &mut app.current_dict_entries_mut()[idx_val];
                    if entry.status == GroupStatus::Default {
                        entry.status = GroupStatus::Modified;
                    }
                    entry.long = new_long;
                    app.dirty = true;
                    app.input_mode = InputMode::Normal;
                }
                TextEditResult::Cancel => { app.input_mode = InputMode::Normal; }
                TextEditResult::Continue => {}
            }
        }
        InputMode::EditVariants(group_idx, cursor) => {
            let group_idx = *group_idx;
            let cursor = *cursor;
            match code {
                KeyCode::Esc => {
                    app.input_mode = InputMode::Normal;
                }
                KeyCode::Down => {
                    let len = app.current_dict_entries()[group_idx].variants.len();
                    if len > 0 {
                        app.input_mode = InputMode::EditVariants(group_idx, (cursor + 1) % len);
                    }
                }
                KeyCode::Up => {
                    let len = app.current_dict_entries()[group_idx].variants.len();
                    if len > 0 {
                        app.input_mode = InputMode::EditVariants(
                            group_idx,
                            if cursor == 0 { len - 1 } else { cursor - 1 },
                        );
                    }
                }
                KeyCode::Char('a') => {
                    app.input_mode = InputMode::AddVariant(group_idx, String::new(), 0);
                }
                KeyCode::Char('d') | KeyCode::Delete => {
                    let entry = &mut app.current_dict_entries_mut()[group_idx];
                    if !entry.variants.is_empty() {
                        entry.variants.remove(cursor);
                        if entry.status == GroupStatus::Default {
                            entry.status = GroupStatus::Modified;
                        }
                        app.dirty = true;
                        let new_len = app.current_dict_entries()[group_idx].variants.len();
                        let new_cursor = if new_len == 0 { 0 } else { cursor.min(new_len - 1) };
                        app.input_mode = InputMode::EditVariants(group_idx, new_cursor);
                    }
                }
                _ => {}
            }
        }
        InputMode::AddVariant(group_idx, text, cursor) => {
            let gidx = *group_idx;
            let back_to_variants = |app: &mut App, gidx: usize| {
                let len = app.current_dict_entries()[gidx].variants.len();
                app.input_mode = InputMode::EditVariants(gidx, if len > 0 { len - 1 } else { 0 });
            };
            match text_edit(text, cursor, code) {
                TextEditResult::Submit(v) => {
                    if !v.is_empty() {
                        let v = v.to_uppercase();
                        let entry = &mut app.current_dict_entries_mut()[gidx];
                        if !entry.variants.contains(&v) {
                            entry.variants.push(v);
                            if entry.status == GroupStatus::Default {
                                entry.status = GroupStatus::Modified;
                            }
                            app.dirty = true;
                        }
                    }
                    back_to_variants(app, gidx);
                }
                TextEditResult::Cancel => { back_to_variants(app, gidx); }
                TextEditResult::Continue => {}
            }
        }
        InputMode::Normal => unreachable!(),
    }
}

pub(crate) fn handle_output_key(app: &mut App, code: KeyCode) {
    use crate::config::OutputFormat;
    let len = app.output_settings.len();

    match code {
        KeyCode::Down => {
            if len > 0 {
                let i = app.output_list_state.selected().unwrap_or(0);
                app.output_list_state.select(Some((i + 1) % len));
            }
        }
        KeyCode::Up => {
            if len > 0 {
                let i = app.output_list_state.selected().unwrap_or(0);
                app.output_list_state
                    .select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
        }
        KeyCode::Char(' ') => {
            if let Some(i) = app.output_list_state.selected() {
                let setting = &mut app.output_settings[i];
                setting.format = match setting.format {
                    OutputFormat::Short => OutputFormat::Long,
                    OutputFormat::Long => OutputFormat::Short,
                };
                app.dirty = true;
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub(crate) fn render_steps(frame: &mut Frame, app: &mut App, area: Rect) {
    let rows: Vec<Row> = app
        .steps
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let is_moving = app.moving_step == Some(idx);
            let style = if is_moving {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if !r.enabled {
                Style::new().fg(Color::DarkGray)
            } else if r.enabled != r.default_enabled {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };

            // Enabled indicator
            let check = if is_moving {
                Span::styled("~", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else if r.enabled {
                Span::styled("✓", Style::new().fg(Color::Green))
            } else {
                Span::styled("✗", Style::new().fg(Color::Red))
            };

            // Label (with custom marker)
            let label = if r.is_custom {
                format!("[+] {}", r.label())
            } else {
                r.label().to_string()
            };

            // Type — capitalize via meta lookup
            let type_display = super::meta::find_step_type(r.step_type())
                .map(|m| m.display)
                .unwrap_or(r.step_type());

            // Input column
            let input = r.def.input_col.as_deref().unwrap_or("(working)");

            // Output column
            let output = match &r.def.output_col {
                Some(OutputCol::Single(name)) => name.clone(),
                Some(OutputCol::Multi(map)) => {
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(_, v)| *v);
                    pairs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
                }
                None => "\u{2014}".to_string(), // em-dash
            };

            // Pattern/table (truncated)
            let pattern = if let Some(tbl) = &r.def.table {
                format!("{{{}}}", tbl)
            } else {
                r.def.pattern.as_deref().unwrap_or("").to_string()
            };
            let pattern_truncated = super::widgets::truncate(&pattern, 30);

            Row::new(vec![
                Cell::from(Line::from(check)),
                Cell::from(label).style(style),
                Cell::from(type_display),
                Cell::from(input.to_string()),
                Cell::from(output),
                Cell::from(pattern_truncated).style(
                    if is_moving { style } else { Style::new().fg(Color::DarkGray) }
                ),
            ])
        })
        .collect();

    let header = Row::new(vec!["", "Label", "Type", "Input", "Output", "Pattern"])
        .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let widths = [
        Constraint::Length(1),   // check
        Constraint::Min(16),     // label
        Constraint::Length(11),  // type
        Constraint::Length(12),  // input
        Constraint::Length(14),  // output
        Constraint::Fill(1),     // pattern
    ];

    let table = Table::new(rows, widths)
        .block(Block::bordered().title("Pipeline Steps"))
        .header(header)
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(table, area, &mut app.steps_list_state);
}

pub(crate) fn render_dict(frame: &mut Frame, app: &mut App, area: Rect) {
    let [subtab_area, panel_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(area);

    // Sub-tabs for table names — windowed to keep selected tab visible
    let all_titles: Vec<String> = app
        .table_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let count = app.dict_entries[i].len();
            format!("{} ({})", name, count)
        })
        .collect();

    // Available width inside the bordered block (subtract 2 for borders)
    let avail_width = subtab_area.width.saturating_sub(2) as usize;
    let divider_len = 3; // " | "

    // Find a window of tabs that fits, centered on the selected tab
    let idx = app.dict_tab_index;
    let mut win_start = idx;
    let mut win_end = idx + 1; // exclusive
    let mut total_width = all_titles[idx].len();

    // Expand window outward, alternating right then left
    loop {
        let mut expanded = false;
        if win_end < all_titles.len() {
            let next_w = all_titles[win_end].len() + divider_len;
            if total_width + next_w <= avail_width {
                total_width += next_w;
                win_end += 1;
                expanded = true;
            }
        }
        if win_start > 0 {
            let prev_w = all_titles[win_start - 1].len() + divider_len;
            if total_width + prev_w <= avail_width {
                total_width += prev_w;
                win_start -= 1;
                expanded = true;
            }
        }
        if !expanded {
            break;
        }
    }

    let visible_titles: Vec<String> = all_titles[win_start..win_end].to_vec();
    let selected_in_window = idx - win_start;

    // Add scroll indicators
    let mut title = String::from("Tables (left/right to switch)");
    if win_start > 0 || win_end < all_titles.len() {
        title = format!(
            "Tables [{}/{}] (left/right to switch)",
            idx + 1,
            all_titles.len()
        );
    }

    let subtabs = Tabs::new(visible_titles)
        .block(Block::bordered().title(title))
        .select(selected_in_window)
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ");
    frame.render_widget(subtabs, subtab_area);

    // Check if this is a value-list table
    let is_value_list = {
        let tables = build_default_tables();
        tables
            .get(&app.table_names[app.dict_tab_index])
            .map(|t| t.is_value_list())
            .unwrap_or(false)
    };

    // Build table rows from entries
    let rows: Vec<Row> = {
        let entries = app.current_dict_entries();

        entries
            .iter()
            .map(|e| {
                let style = match e.status {
                    GroupStatus::Default => Style::new(),
                    GroupStatus::Added => Style::new().fg(Color::Green),
                    GroupStatus::Removed => Style::new().fg(Color::Red).add_modifier(Modifier::CROSSED_OUT),
                    GroupStatus::Modified => Style::new().fg(Color::Yellow),
                };
                let check = match e.status {
                    GroupStatus::Removed => Cell::from(Span::styled("✗", Style::new().fg(Color::Red))),
                    _ => Cell::from(Span::styled("✓", Style::new().fg(Color::Green))),
                };
                let variants_str = if e.variants.is_empty() {
                    String::new()
                } else {
                    e.variants.join(", ")
                };
                if is_value_list {
                    Row::new(vec![
                        check,
                        Cell::from(e.short.clone()).style(style),
                        Cell::from("".to_string()),
                        Cell::from(variants_str).style(Style::new().fg(Color::DarkGray)),
                    ])
                } else {
                    Row::new(vec![
                        check,
                        Cell::from(e.short.clone()).style(style),
                        Cell::from(e.long.clone()).style(style),
                        Cell::from(variants_str).style(Style::new().fg(Color::DarkGray)),
                    ])
                }
            })
            .collect()
    };

    let table_name = &app.table_names[app.dict_tab_index];

    let widths = [
        Constraint::Length(1),    // check
        Constraint::Length(12),   // short
        Constraint::Length(20),   // long
        Constraint::Fill(1),      // variants
    ];

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Short").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Long").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Variants").style(Style::new().add_modifier(Modifier::BOLD)),
    ]).style(Style::new().fg(Color::Cyan));

    let table_widget = Table::new(rows, widths)
        .block(Block::bordered().title(format!("{} entries", table_name)))
        .header(header)
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(table_widget, panel_area, &mut app.dict_list_state);
}

pub(crate) fn render_output(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::config::OutputFormat;

    // Build table rows
    let rows: Vec<Row> = app
        .output_settings
        .iter()
        .map(|s| {
            let is_modified = s.format != s.default_format;
            let format_str = match s.format {
                OutputFormat::Short => "Short",
                OutputFormat::Long => "Long",
            };
            let example = match s.format {
                OutputFormat::Short => &s.example_short,
                OutputFormat::Long => &s.example_long,
            };
            let style = if is_modified {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            Row::new(vec![
                Cell::from(s.component.clone()),
                Cell::from(format_str),
                Cell::from(example.clone()),
            ]).style(style)
        })
        .collect();

    let widths = [
        Constraint::Percentage(40),
        Constraint::Percentage(25),
        Constraint::Percentage(35),
    ];

    let header = Row::new(vec![
        Cell::from("Component").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Format").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Example").style(Style::new().add_modifier(Modifier::BOLD)),
    ]).style(Style::new().fg(Color::Cyan));

    let table_widget = Table::new(rows, widths)
        .block(Block::bordered().title("Output Format"))
        .header(header)
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(table_widget, area, &mut app.output_list_state);
}




