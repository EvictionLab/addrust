use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

use crate::address::COL_DEFS;
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

/// Fields in the step editor form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormField {
    Pattern,
    OutputCol,    // Output column picker (single or multi)
    SkipIfFilled, // Extract only
    Replacement,
    Table,
    InputCol,
    Mode,         // Standardize only: whole_field / per_word
    Label,
}

/// Which panel has focus in the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormFocus {
    Left,          // navigating field list
    RightPattern,  // in pattern drill-down
    RightOutputCol, // in output column picker
    RightTable,    // in table picker
    EditingText(String, usize, String), // field being text-edited (field_name, cursor, text)
}

/// State for the step editor form.
#[derive(Debug, Clone)]
pub(crate) struct FormState {
    /// Index into App.steps of the step being edited, or None for new step.
    pub(crate) step_index: Option<usize>,
    /// Working copy of the StepDef being edited.
    pub(crate) def: crate::step::StepDef,
    /// Which fields are visible (computed from step type).
    pub(crate) visible_fields: Vec<FormField>,
    /// Cursor position in visible_fields.
    pub(crate) field_cursor: usize,
    /// Which panel has focus.
    pub(crate) focus: FormFocus,
    /// For right-panel list navigation (pattern segments, target fields, table list).
    pub(crate) right_cursor: usize,
    /// For pattern drill-down: which alternation group is expanded.
    pub(crate) right_alt_selected: Option<usize>,
    /// Parsed pattern segments for drill-down.
    pub(crate) pattern_segments: Vec<crate::pattern::PatternSegment>,
    /// Whether this is a new step (for cancel/discard behavior).
    pub(crate) is_new: bool,
    /// Show discard confirmation prompt.
    pub(crate) show_discard_prompt: bool,
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
    pub(crate) fn is_field_modified(&self, field: &str) -> bool {
        let Some(default) = &self.default_def else { return false };
        match field {
            "pattern" => self.def.pattern != default.pattern,
            "output_col" => self.def.output_col != default.output_col,
            "replacement" => self.def.replacement != default.replacement,
            "skip_if_filled" => self.def.skip_if_filled != default.skip_if_filled,
            "table" => self.def.table != default.table,
            "input_col" => self.def.input_col != default.input_col,
            "mode" => self.def.mode != default.mode,
            "label" => false,
            _ => false,
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

use super::meta::TABLE_DESCRIPTIONS;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn visible_fields_for_type(step_type: &str, _def: &crate::step::StepDef) -> Vec<FormField> {
    match super::meta::find_step_type(step_type) {
        Some(meta) => meta.visible.iter().map(|pk| prop_key_to_form_field(*pk)).collect(),
        None => vec![FormField::Label],
    }
}

fn prop_key_to_form_field(pk: super::meta::PropKey) -> FormField {
    use super::meta::PropKey;
    match pk {
        PropKey::Pattern => FormField::Pattern,
        PropKey::Table => FormField::Table,
        PropKey::OutputCol => FormField::OutputCol,
        PropKey::Replacement => FormField::Replacement,
        PropKey::SkipIfFilled => FormField::SkipIfFilled,
        PropKey::Mode => FormField::Mode,
        PropKey::InputCol => FormField::InputCol,
        PropKey::Label => FormField::Label,
    }
}

fn form_field_to_prop_key(f: FormField) -> super::meta::PropKey {
    use super::meta::PropKey;
    match f {
        FormField::Pattern => PropKey::Pattern,
        FormField::Table => PropKey::Table,
        FormField::OutputCol => PropKey::OutputCol,
        FormField::Replacement => PropKey::Replacement,
        FormField::SkipIfFilled => PropKey::SkipIfFilled,
        FormField::Mode => PropKey::Mode,
        FormField::InputCol => PropKey::InputCol,
        FormField::Label => PropKey::Label,
    }
}

pub(crate) fn field_key(field: FormField) -> &'static str {
    match field {
        FormField::Pattern => "pattern",
        FormField::OutputCol => "output_col",
        FormField::SkipIfFilled => "skip_if_filled",
        FormField::Replacement => "replacement",
        FormField::Table => "table",
        FormField::InputCol => "input_col",
        FormField::Mode => "mode",
        FormField::Label => "label",
    }
}

pub(crate) fn form_field_display(field: FormField, def: &crate::step::StepDef) -> (&'static str, String) {
    match field {
        FormField::Pattern => ("Pattern", def.pattern.as_deref().unwrap_or("(none)").to_string()),
        FormField::OutputCol => {
            let val = match &def.output_col {
                Some(OutputCol::Single(s)) => s.clone(),
                Some(OutputCol::Multi(m)) => {
                    let mut pairs: Vec<_> = m.iter().collect();
                    pairs.sort_by_key(|(_, v)| *v);
                    pairs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
                }
                None => "(none)".to_string(),
            };
            ("Output col", val)
        }
        FormField::SkipIfFilled => ("Skip if filled", if def.skip_if_filled == Some(true) { "yes" } else { "no" }.to_string()),
        FormField::Replacement => ("Replacement", def.replacement.as_deref().unwrap_or("(none)").to_string()),
        FormField::Table => ("Table", def.table.as_deref().unwrap_or("(none)").to_string()),
        FormField::InputCol => ("Input col", def.input_col.as_deref().unwrap_or("working string").to_string()),
        FormField::Mode => ("Mode", def.mode.as_deref().unwrap_or("whole field").to_string()),
        FormField::Label => ("Label", def.label.clone()),
    }
}

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

pub(crate) fn validate_step_def(def: &crate::step::StepDef) -> bool {
    super::meta::find_step_type(&def.step_type)
        .map(|m| (m.required)(def))
        .unwrap_or(false)
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
            if let Some(selected) = app.steps_list_state.selected() {
                let step = &app.steps[selected];
                let def = step.def.clone();
                let visible = visible_fields_for_type(&def.step_type, &def);
                let segments = crate::pattern::parse_pattern(def.pattern.as_deref().unwrap_or(""));
                app.form_state = Some(FormState {
                    step_index: Some(selected),
                    def,
                    visible_fields: visible,
                    field_cursor: 0,
                    focus: FormFocus::Left,
                    right_cursor: 0,
                    right_alt_selected: None,
                    pattern_segments: segments,
                    is_new: false,
                    show_discard_prompt: false,
                });
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
                label: format!("custom_{}", app.steps.len()),
                step_type: "extract".to_string(),
                ..Default::default()
            };
            let visible = visible_fields_for_type(&def.step_type, &def);
            let segments = crate::pattern::parse_pattern(def.pattern.as_deref().unwrap_or(""));
            app.form_state = Some(FormState {
                step_index: None,
                def,
                visible_fields: visible,
                field_cursor: 0,
                focus: FormFocus::Left,
                right_cursor: 0,
                right_alt_selected: None,
                pattern_segments: segments,
                is_new: true,
                show_discard_prompt: false,
            });
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
        // Open EditVariants modal
        KeyCode::Enter => {
            if let Some(i) = app.dict_list_state.selected() {
                let entry = &app.current_dict_entries()[i];
                if entry.status != GroupStatus::Removed {
                    app.input_mode = InputMode::EditVariants(i, 0);
                }
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

pub(crate) fn handle_form_key(app: &mut App, code: KeyCode) {
    let form = match &mut app.form_state {
        Some(f) => f,
        None => return,
    };

    if form.show_discard_prompt {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.form_state = None;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                form.show_discard_prompt = false;
            }
            _ => {}
        }
        return;
    }

    match form.focus.clone() {
        FormFocus::Left => handle_form_left_key(app, code),
        FormFocus::RightPattern => handle_form_pattern_key(app, code),
        FormFocus::RightOutputCol => handle_form_targets_key(app, code),
        FormFocus::RightTable => handle_form_table_key(app, code),
        FormFocus::EditingText(_, _, _) => handle_form_text_edit(app, code),
    }
}

fn handle_form_left_key(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    let field_count = form.visible_fields.len();

    match code {
        KeyCode::Down => {
            form.field_cursor = (form.field_cursor + 1) % field_count;
        }
        KeyCode::Up => {
            form.field_cursor = if form.field_cursor == 0 { field_count - 1 } else { form.field_cursor - 1 };
        }
        KeyCode::Enter => {
            let field = form.visible_fields[form.field_cursor];
            match field {
                FormField::Pattern => {
                    form.focus = FormFocus::RightPattern;
                    form.right_cursor = 0;
                    form.right_alt_selected = None;
                }
                FormField::OutputCol | FormField::InputCol => {
                    form.focus = FormFocus::RightOutputCol;
                    form.right_cursor = 0;
                }
                FormField::Table => {
                    form.focus = FormFocus::RightTable;
                    form.right_cursor = 0;
                }
                FormField::Replacement | FormField::Label => {
                    let current = match field {
                        FormField::Replacement => form.def.replacement.clone().unwrap_or_default(),
                        FormField::Label => form.def.label.clone(),
                        _ => String::new(),
                    };
                    let len = current.len();
                    form.focus = FormFocus::EditingText(field_key(field).to_string(), len, current);
                }
                _ => {}
            }
        }
        KeyCode::Char(' ') => {
            let field = form.visible_fields[form.field_cursor];
            match field {
                FormField::SkipIfFilled => {
                    let current = form.def.skip_if_filled.unwrap_or(false);
                    form.def.skip_if_filled = Some(!current);
                    app.dirty = true;
                }
                FormField::Mode => {
                    let current = form.def.mode.as_deref();
                    form.def.mode = if current == Some("per_word") { None } else { Some("per_word".to_string()) };
                    app.dirty = true;
                }
                _ => {}
            }
        }
        KeyCode::Char('r') => {
            if let Some(step_idx) = form.step_index {
                let step = &app.steps[step_idx];
                if let Some(default) = &step.default_def {
                    let field = form.visible_fields[form.field_cursor];
                    match field {
                        FormField::Pattern => form.def.pattern = default.pattern.clone(),
                        FormField::OutputCol => form.def.output_col = default.output_col.clone(),
                        FormField::Replacement => form.def.replacement = default.replacement.clone(),
                        FormField::SkipIfFilled => form.def.skip_if_filled = default.skip_if_filled,
                        FormField::Table => form.def.table = default.table.clone(),
                        FormField::InputCol => form.def.input_col = default.input_col.clone(),
                        FormField::Mode => form.def.mode = default.mode.clone(),
                        _ => {}
                    }
                    app.dirty = true;
                }
            }
        }
        KeyCode::Esc | KeyCode::Left => {
            close_form(app);
        }
        _ => {}
    }
}

pub(crate) fn close_form(app: &mut App) {
    let form = app.form_state.as_mut().unwrap();

    if form.is_new {
        let valid = validate_step_def(&form.def);
        if valid {
            let def = form.def.clone();
            let insert_idx = app.steps_list_state.selected().map(|i| i + 1).unwrap_or(app.steps.len());
            app.steps.insert(insert_idx, StepState {
                enabled: true,
                default_enabled: true,
                is_custom: true,
                def,
                default_def: None,
            });
            app.dirty = true;
            app.form_state = None;
        } else {
            form.show_discard_prompt = true;
        }
    } else {
        if let Some(idx) = form.step_index {
            app.steps[idx].def = form.def.clone();
            app.dirty = true;
        }
        app.form_state = None;
    }
}

fn handle_form_pattern_key(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    // Count selectable segments (alternation groups + table refs)
    let selectable: Vec<usize> = form.pattern_segments.iter().enumerate()
        .filter(|(_, s)| matches!(s,
            crate::pattern::PatternSegment::AlternationGroup { .. } |
            crate::pattern::PatternSegment::TableRef(_)))
        .map(|(i, _)| i)
        .collect();
    let selectable_count = selectable.len();

    match code {
        KeyCode::Down => {
            if let Some(alt_idx) = form.right_alt_selected {
                // Inside alternation group — navigate alternatives
                if let Some(seg_real) = selectable.get(form.right_cursor) {
                    if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                        form.pattern_segments.get(*seg_real)
                    {
                        if alt_idx + 1 < alternatives.len() {
                            form.right_alt_selected = Some(alt_idx + 1);
                        }
                    }
                }
            } else if selectable_count > 0 {
                form.right_cursor = (form.right_cursor + 1) % selectable_count;
            }
        }
        KeyCode::Up => {
            if let Some(alt_idx) = form.right_alt_selected {
                if alt_idx > 0 {
                    form.right_alt_selected = Some(alt_idx - 1);
                }
            } else if selectable_count > 0 {
                form.right_cursor = if form.right_cursor == 0 { selectable_count - 1 } else { form.right_cursor - 1 };
            }
        }
        KeyCode::Enter => {
            if form.right_alt_selected.is_none() {
                if let Some(&seg_real) = selectable.get(form.right_cursor) {
                    if matches!(form.pattern_segments.get(seg_real),
                        Some(crate::pattern::PatternSegment::AlternationGroup { .. }))
                    {
                        form.right_alt_selected = Some(0);
                    }
                }
            }
        }
        KeyCode::Char(' ') => {
            if let Some(alt_idx) = form.right_alt_selected {
                if let Some(&seg_real) = selectable.get(form.right_cursor) {
                    if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                        form.pattern_segments.get_mut(seg_real)
                    {
                        alternatives[alt_idx].enabled = !alternatives[alt_idx].enabled;
                        form.def.pattern = Some(crate::pattern::rebuild_pattern(&form.pattern_segments));
                        app.dirty = true;
                    }
                }
            }
        }
        KeyCode::Char('a') => {
            if form.right_alt_selected.is_some() {
                form.focus = FormFocus::EditingText("add_alternative".to_string(), 0, String::new());
            }
        }
        KeyCode::Char('d') => {
            if let Some(alt_idx) = form.right_alt_selected {
                if let Some(&seg_real) = selectable.get(form.right_cursor) {
                    if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                        form.pattern_segments.get_mut(seg_real)
                    {
                        if alternatives.len() > 1 {
                            alternatives.remove(alt_idx);
                            if alt_idx >= alternatives.len() {
                                form.right_alt_selected = Some(alternatives.len() - 1);
                            }
                            form.def.pattern = Some(crate::pattern::rebuild_pattern(&form.pattern_segments));
                            app.dirty = true;
                        }
                    }
                }
            }
        }
        KeyCode::Char('e') => {
            let text = form.def.pattern.clone().unwrap_or_default();
            let len = text.len();
            form.focus = FormFocus::EditingText("pattern".to_string(), len, text);
        }
        KeyCode::Esc => {
            if form.right_alt_selected.is_some() {
                form.right_alt_selected = None;
            } else {
                form.focus = FormFocus::Left;
            }
        }
        _ => {}
    }
}

fn handle_form_targets_key(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    let current_field = form.visible_fields[form.field_cursor];
    let is_source = current_field == FormField::InputCol;
    let is_multi = current_field == FormField::OutputCol
        && matches!(&form.def.output_col, Some(OutputCol::Multi(_)));
    let item_count = if is_source { COL_DEFS.len() + 1 } else { COL_DEFS.len() };

    match code {
        KeyCode::Down => {
            form.right_cursor = (form.right_cursor + 1) % item_count;
        }
        KeyCode::Up => {
            form.right_cursor = if form.right_cursor == 0 { item_count - 1 } else { form.right_cursor - 1 };
        }
        KeyCode::Enter if !is_multi => {
            let offset = if is_source { 1 } else { 0 };
            if is_source && form.right_cursor == 0 {
                form.def.input_col = None;
            } else {
                let fkey = COL_DEFS[form.right_cursor - offset].key;
                if is_source {
                    form.def.input_col = Some(fkey.to_string());
                } else {
                    form.def.output_col = Some(OutputCol::Single(fkey.to_string()));
                }
            }
            form.focus = FormFocus::Left;
            app.dirty = true;
        }
        KeyCode::Char(' ') if is_multi => {
            let fkey = COL_DEFS[form.right_cursor].key.to_string();
            let targets = match &mut form.def.output_col {
                Some(OutputCol::Multi(m)) => m,
                _ => unreachable!(),
            };
            if targets.contains_key(&fkey) {
                targets.remove(&fkey);
            } else {
                let max = targets.values().max().copied().unwrap_or(0);
                targets.insert(fkey, max + 1);
            }
            app.dirty = true;
        }
        KeyCode::Char(c) if is_multi && c.is_ascii_digit() && c != '0' => {
            let group = (c as u8 - b'0') as usize;
            let fkey = COL_DEFS[form.right_cursor].key.to_string();
            let targets = match &mut form.def.output_col {
                Some(OutputCol::Multi(m)) => m,
                _ => unreachable!(),
            };
            targets.insert(fkey, group);
            app.dirty = true;
        }
        KeyCode::Char('d') if is_multi => {
            let fkey = COL_DEFS[form.right_cursor].key;
            if let Some(OutputCol::Multi(targets)) = &mut form.def.output_col {
                targets.remove(fkey);
            }
            app.dirty = true;
        }
        KeyCode::Esc => {
            form.focus = FormFocus::Left;
        }
        _ => {}
    }
}

fn handle_form_table_key(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    match code {
        KeyCode::Down => {
            form.right_cursor = (form.right_cursor + 1) % TABLE_DESCRIPTIONS.len();
        }
        KeyCode::Up => {
            form.right_cursor = if form.right_cursor == 0 { TABLE_DESCRIPTIONS.len() - 1 } else { form.right_cursor - 1 };
        }
        KeyCode::Enter => {
            let table_name = TABLE_DESCRIPTIONS[form.right_cursor].0;
            form.def.table = Some(table_name.to_string());
            form.focus = FormFocus::Left;
            app.dirty = true;
        }
        KeyCode::Esc => {
            form.focus = FormFocus::Left;
        }
        _ => {}
    }
}

fn handle_form_text_edit(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    if let FormFocus::EditingText(field_name, cursor, text) = &mut form.focus {
        match code {
            KeyCode::Enter => {
                let value = text.clone();
                let field = field_name.clone();
                match field.as_str() {
                    "replacement" => form.def.replacement = if value.is_empty() { None } else { Some(value) },
                    "label" => if !value.is_empty() { form.def.label = value },
                    "pattern" => {
                        form.def.pattern = if value.is_empty() { None } else { Some(value) };
                        form.pattern_segments = crate::pattern::parse_pattern(
                            form.def.pattern.as_deref().unwrap_or("")
                        );
                    }
                    "add_alternative" => {
                        if !value.is_empty() {
                            // Resolve selectable index to real segment index
                            let seg_real = form.pattern_segments.iter().enumerate()
                                .filter(|(_, s)| matches!(s,
                                    crate::pattern::PatternSegment::AlternationGroup { .. } |
                                    crate::pattern::PatternSegment::TableRef(_)))
                                .nth(form.right_cursor)
                                .map(|(i, _)| i);
                            if let Some(idx) = seg_real {
                                if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                                    form.pattern_segments.get_mut(idx)
                                {
                                    alternatives.push(crate::pattern::Alternative { text: value, enabled: true });
                                    form.def.pattern = Some(crate::pattern::rebuild_pattern(&form.pattern_segments));
                                }
                            }
                        }
                        form.focus = FormFocus::RightPattern;
                        app.dirty = true;
                        return;
                    }
                    _ => {}
                }
                form.focus = FormFocus::Left;
                app.dirty = true;
            }
            KeyCode::Esc => {
                form.focus = FormFocus::Left;
            }
            KeyCode::Backspace => {
                if *cursor > 0 {
                    text.remove(*cursor - 1);
                    *cursor -= 1;
                }
            }
            KeyCode::Left => { if *cursor > 0 { *cursor -= 1; } }
            KeyCode::Right => { if *cursor < text.len() { *cursor += 1; } }
            KeyCode::Char(c) => {
                text.insert(*cursor, c);
                *cursor += 1;
            }
            _ => {}
        }
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

pub(crate) fn render_step_form(frame: &mut Frame, app: &mut App, area: Rect) {
    let form = match &app.form_state {
        Some(f) => f,
        None => return,
    };

    // Centered overlay on top of the steps table
    let overlay = super::centered_rect_pct(80, 80, area);
    frame.render_widget(ratatui::widgets::Clear, overlay);

    let step_state = form.step_index.map(|i| &app.steps[i]);

    let [header_area, body_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
    ]).areas(overlay);

    // Header
    let type_str = form.def.step_type.to_uppercase();
    let origin = if step_state.map(|s| s.is_custom).unwrap_or(true) {
        "CUSTOM STEP"
    } else {
        "DEFAULT STEP"
    };
    let modified = if step_state.map(|s| s.is_modified()).unwrap_or(false) {
        Span::styled("  ● MODIFIED", Style::new().fg(Color::Yellow))
    } else {
        Span::raw("")
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" TYPE: {}     ", type_str), Style::new().fg(Color::Cyan)),
        Span::styled(origin, Style::new().fg(Color::DarkGray)),
        modified,
    ]))
    .block(Block::bordered().title(format!("Step: {}", form.def.label)));
    frame.render_widget(header, header_area);

    // Two panels
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Percentage(44),
        Constraint::Percentage(56),
    ]).areas(body_area);

    render_form_left_panel(frame, app, left_area);
    render_form_right_panel(frame, app, right_area);

    // Discard confirmation overlay
    if form.show_discard_prompt {
        let popup = super::centered_rect(50, 5, overlay);
        frame.render_widget(ratatui::widgets::Clear, popup);
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                " Missing required fields. Discard step? (y/n) ",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
        ])
        .block(Block::bordered().title("Confirm"));
        frame.render_widget(msg, popup);
    }
}

fn render_form_left_panel(frame: &mut Frame, app: &App, area: Rect) {
    use super::widgets;

    let form = app.form_state.as_ref().unwrap();
    let step_state = form.step_index.map(|i| &app.steps[i]);

    let mut items: Vec<ListItem> = Vec::new();

    for (i, field) in form.visible_fields.iter().enumerate() {
        let is_selected = form.focus == FormFocus::Left && form.field_cursor == i;
        let is_modified = step_state.map(|s| s.is_field_modified(field_key(*field))).unwrap_or(false);

        let prefix = if is_selected { "▸ " } else { "  " };
        let mod_marker = if is_modified { "* " } else { "  " };
        let (label, value) = form_field_display(*field, &form.def);

        let style = widgets::selected_style(is_selected).fg(if is_selected { Color::White } else { Color::DarkGray });

        let mod_style = if is_modified {
            Style::new().fg(Color::Yellow)
        } else {
            style
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(prefix, if is_selected { Style::new().fg(Color::Magenta) } else { Style::new() }),
            Span::styled(mod_marker, mod_style),
            Span::styled(format!("{:16}", label), style),
            Span::styled(value, style),
        ])));
    }

    let list = List::new(items)
        .block(Block::bordered().border_style(
            widgets::focus_border(form.focus == FormFocus::Left)
        ));
    frame.render_widget(list, area);
}

fn render_form_right_panel(frame: &mut Frame, app: &App, area: Rect) {
    let form = app.form_state.as_ref().unwrap();
    let step_state = form.step_index.map(|i| &app.steps[i]);
    let current_field = form.visible_fields.get(form.field_cursor).copied();

    match &form.focus {
        FormFocus::RightPattern => { render_form_pattern_panel(frame, app, area); return; }
        FormFocus::RightOutputCol => { render_form_targets_panel(frame, app, area); return; }
        FormFocus::RightTable => { render_form_table_panel(frame, app, area); return; }
        FormFocus::EditingText(field_name, cursor, text) => {
            render_form_text_edit_panel(frame, field_name, text, *cursor, area);
            return;
        }
        FormFocus::Left => {}
    }

    match current_field {
        Some(FormField::Pattern) => render_form_pattern_panel(frame, app, area),
        Some(FormField::OutputCol) | Some(FormField::InputCol) => render_form_targets_panel(frame, app, area),
        Some(FormField::Table) => render_form_table_panel(frame, app, area),
        Some(field) => render_form_help_panel(frame, field, &form.def, step_state, area),
        None => {}
    }
}

fn render_form_text_edit_panel(
    frame: &mut Frame,
    field_name: &str,
    text: &str,
    cursor: usize,
    area: Rect,
) {
    let title = match field_name {
        "replacement" => "Editing Replacement",
        "label" => "Editing Label",
        "pattern" => "Editing Pattern",
        "add_alternative" => "Adding Alternative",
        _ => "Editing",
    };
    let (before, after) = text.split_at(cursor.min(text.len()));
    let cursor_char = if after.is_empty() { "_".to_string() } else { after[..1].to_string() };
    let after_cursor = if after.len() > 1 { &after[1..] } else { "" };

    let lines = vec![
        Line::from(Span::styled(title, Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Enter: confirm   Esc: cancel", Style::new().fg(Color::DarkGray))),
        Line::from(""),
        Line::from(vec![
            Span::styled(before, Style::new().fg(Color::White)),
            Span::styled(cursor_char, Style::new().fg(Color::Black).bg(Color::White)),
            Span::styled(after_cursor, Style::new().fg(Color::White)),
        ]),
    ];
    let panel = Paragraph::new(lines)
        .block(Block::bordered().border_style(super::widgets::focus_border(true)));
    frame.render_widget(panel, area);
}

fn render_form_help_panel(
    frame: &mut Frame,
    field: FormField,
    def: &crate::step::StepDef,
    step_state: Option<&StepState>,
    area: Rect,
) {
    let help = super::meta::help_text(form_field_to_prop_key(field));

    let (title, current_value, edit_hint): (&str, String, &str) = match field {
        FormField::SkipIfFilled => (
            "Skip If Filled",
            (if def.skip_if_filled == Some(true) { "yes" } else { "no" }).to_string(),
            "Space to toggle",
        ),
        FormField::Replacement => (
            "Replacement",
            def.replacement.as_deref().unwrap_or("(none)").to_string(),
            "Enter to edit",
        ),
        FormField::InputCol => (
            "Input Column",
            def.input_col.as_deref().unwrap_or("working string").to_string(),
            "Enter to pick",
        ),
        FormField::Mode => (
            "Mode",
            def.mode.as_deref().unwrap_or("whole field").to_string(),
            "Space to toggle",
        ),
        FormField::Label => (
            "Label",
            def.label.clone(),
            "Enter to edit",
        ),
        FormField::OutputCol => {
            let val = match &def.output_col {
                Some(OutputCol::Single(s)) => s.clone(),
                Some(OutputCol::Multi(m)) => {
                    let mut pairs: Vec<_> = m.iter().collect();
                    pairs.sort_by_key(|(_, v)| *v);
                    pairs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
                }
                None => "(none)".to_string(),
            };
            ("Output Column", val, "Enter to pick")
        }
        _ => return,
    };

    let help_text = help;

    let is_modified = step_state.map(|s| s.is_field_modified(field_key(field))).unwrap_or(false);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(title, Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            if is_modified {
                Span::styled("  ● modified", Style::new().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ]),
        Line::from(Span::styled(edit_hint, Style::new().fg(Color::DarkGray))),
        Line::from(""),
    ];

    for para in help_text.split("\n\n") {
        for line in para.lines() {
            lines.push(Line::from(Span::styled(line, Style::new().fg(Color::White))));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("Current: ", Style::new().fg(Color::DarkGray)),
        Span::styled(current_value, Style::new().fg(Color::Yellow)),
    ]));

    if is_modified {
        if let Some(step_state) = step_state {
            if let Some(default_def) = &step_state.default_def {
                let default_val: String = match field {
                    FormField::Replacement => default_def.replacement.as_deref().unwrap_or("(none)").to_string(),
                    FormField::InputCol => default_def.input_col.as_deref().unwrap_or("working string").to_string(),
                    FormField::OutputCol => match &default_def.output_col {
                        Some(OutputCol::Single(s)) => s.clone(),
                        Some(OutputCol::Multi(m)) => {
                            let mut pairs: Vec<_> = m.iter().collect();
                            pairs.sort_by_key(|(_, v)| *v);
                            pairs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
                        }
                        None => "(none)".to_string(),
                    },
                    FormField::SkipIfFilled => (if default_def.skip_if_filled == Some(true) { "yes" } else { "no" }).to_string(),
                    FormField::Mode => default_def.mode.as_deref().unwrap_or("whole field").to_string(),
                    _ => String::new(),
                };
                if !default_val.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("Default: ", Style::new().fg(Color::DarkGray)),
                        Span::styled(default_val, Style::new().fg(Color::DarkGray)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("r to reset to default", Style::new().fg(Color::DarkGray))));
                }
            }
        }
    }

    let panel = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::bordered().border_style(super::widgets::focus_border(false)));
    frame.render_widget(panel, area);
}

fn render_form_pattern_panel(frame: &mut Frame, app: &App, area: Rect) {
    let form = app.form_state.as_ref().unwrap();
    let focused = matches!(form.focus, FormFocus::RightPattern);
    let mut items: Vec<ListItem> = Vec::new();

    if form.pattern_segments.is_empty() {
        let text = form.def.pattern.as_deref().unwrap_or("(no pattern)");
        items.push(ListItem::new(Line::from(Span::styled(text, Style::new().fg(Color::DarkGray)))));
    } else {
        // Track which selectable index we're on (only alternation groups and table refs)
        let mut selectable_idx = 0usize;
        for segment in &form.pattern_segments {
            match segment {
                crate::pattern::PatternSegment::Literal(text) => {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled("  ", Style::new()),
                        Span::styled(text.as_str(), Style::new().fg(Color::DarkGray)),
                    ])));
                }
                crate::pattern::PatternSegment::TableRef(name) => {
                    let is_selected = focused && form.right_alt_selected.is_none()
                        && form.right_cursor == selectable_idx;
                    let style = if is_selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(Color::Cyan)
                    };
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(if is_selected { "> " } else { "  " }, style),
                        Span::styled(format!("{{{}}}", name), style),
                    ])));
                    selectable_idx += 1;
                }
                crate::pattern::PatternSegment::AlternationGroup { alternatives, .. } => {
                    let is_group_selected = focused && form.right_alt_selected.is_none()
                        && form.right_cursor == selectable_idx;
                    let group_style = if is_group_selected {
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(Color::Yellow)
                    };
                    let enabled_count = alternatives.iter().filter(|a| a.enabled).count();
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(if is_group_selected { "> " } else { "  " }, group_style),
                        Span::styled(
                            format!("Match group ({}/{} enabled)", enabled_count, alternatives.len()),
                            group_style,
                        ),
                    ])));

                    // Show alternatives when drilled into this group
                    if focused && form.right_cursor == selectable_idx && form.right_alt_selected.is_some() {
                        for (alt_idx, alt) in alternatives.iter().enumerate() {
                            let is_alt_selected = form.right_alt_selected == Some(alt_idx);
                            let (marker, style) = if !alt.enabled {
                                ("x", Style::new().fg(Color::Red))
                            } else {
                                (" ", Style::new().fg(Color::Green))
                            };
                            let prefix = if is_alt_selected { "  > " } else { "    " };
                            items.push(ListItem::new(Line::from(vec![
                                Span::styled(prefix, style),
                                Span::styled(format!("[{}] ", marker), style),
                                Span::styled(
                                    alt.text.as_str(),
                                    if is_alt_selected { style.add_modifier(Modifier::BOLD) } else { style },
                                ),
                            ])));
                        }
                    }
                    selectable_idx += 1;
                }
            }
        }
    }

    let hints = if form.right_alt_selected.is_some() {
        "Space: toggle  a: add  d: delete  Esc: collapse"
    } else {
        "Up/Down: navigate  Enter: expand  e: edit raw  Esc: back"
    };
    let list = List::new(items).block(
        Block::bordered()
            .title("Pattern")
            .title_bottom(hints)
            .border_style(super::widgets::focus_border(focused))
    );
    frame.render_widget(list, area);
}

fn render_form_targets_panel(frame: &mut Frame, app: &App, area: Rect) {
    let form = app.form_state.as_ref().unwrap();
    let current_form_field = form.visible_fields[form.field_cursor];
    let focused = form.focus == FormFocus::RightOutputCol;

    match current_form_field {
        FormField::OutputCol if matches!(&form.def.output_col, Some(OutputCol::Multi(_))) => {
            let targets = match &form.def.output_col {
                Some(OutputCol::Multi(m)) => Some(m),
                _ => None,
            };
            let mut items = Vec::new();
            for (i, col_def) in COL_DEFS.iter().enumerate() {
                let is_selected = focused && form.right_cursor == i;
                let group_num = targets.and_then(|t| t.get(col_def.key)).copied();
                let marker = match group_num {
                    Some(n) => format!("[{}]", n),
                    None => "[ ]".to_string(),
                };
                let detail = group_num.map(|n| format!(" = capture group {}", n)).unwrap_or_default();
                let style = if is_selected {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                } else if group_num.is_some() {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                let prefix = if is_selected { "> " } else { "  " };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(format!("{} {:16}", marker, col_def.label), style),
                    Span::styled(detail, Style::new().fg(Color::DarkGray)),
                ])));
            }
            let list = List::new(items).block(
                Block::bordered()
                    .title("Targets")
                    .title_bottom("Space: toggle  1-9: set group  d: remove  Esc: back")
                    .border_style(super::widgets::focus_border(focused))
            );
            frame.render_widget(list, area);
        }
        FormField::OutputCol | FormField::InputCol => {
            let is_source = current_form_field == FormField::InputCol;
            let current = if is_source {
                form.def.input_col.as_deref()
            } else {
                match &form.def.output_col {
                    Some(OutputCol::Single(s)) => Some(s.as_str()),
                    _ => None,
                }
            };
            let mut items = Vec::new();
            if is_source {
                let is_selected = focused && form.right_cursor == 0;
                let is_current = current.is_none();
                let style = if is_selected {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(if is_selected { "> " } else { "  " }, style),
                    Span::styled(if is_current { "[x] " } else { "[ ] " }, style),
                    Span::styled("working string", style),
                ])));
            }
            let offset = if is_source { 1 } else { 0 };
            for (i, col_def) in COL_DEFS.iter().enumerate() {
                let list_idx = i + offset;
                let is_selected = focused && form.right_cursor == list_idx;
                let is_current = current == Some(col_def.key);
                let style = if is_selected {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(if is_selected { "> " } else { "  " }, style),
                    Span::styled(if is_current { "[x] " } else { "[ ] " }, style),
                    Span::styled(col_def.label.to_string(), style),
                ])));
            }
            let title = if is_source { "Source" } else { "Target" };
            let list = List::new(items).block(
                Block::bordered()
                    .title(title)
                    .title_bottom("Enter: select  Esc: back")
                    .border_style(super::widgets::focus_border(focused))
            );
            frame.render_widget(list, area);
        }
        _ => {}
    }
}

fn render_form_table_panel(frame: &mut Frame, app: &App, area: Rect) {
    let form = app.form_state.as_ref().unwrap();
    let focused = form.focus == FormFocus::RightTable;
    let current_table = form.def.table.as_deref();
    let mut items = Vec::new();

    for (i, (name, desc)) in TABLE_DESCRIPTIONS.iter().enumerate() {
        let is_selected = focused && form.right_cursor == i;
        let is_current = current_table == Some(*name);
        let style = if is_selected {
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::new().fg(Color::Green)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(if is_selected { "> " } else { "  " }, style),
            Span::styled(if is_current { "[x] " } else { "[ ] " }, style),
            Span::styled(format!("{:20}", name), style),
            Span::styled(*desc, Style::new().fg(Color::DarkGray)),
        ])));
    }

    let list = List::new(items).block(
        Block::bordered()
            .title("Table")
            .title_bottom("Enter: select  Esc: back")
            .border_style(super::widgets::focus_border(focused))
    );
    frame.render_widget(list, area);
}
