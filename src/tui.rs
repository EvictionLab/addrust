use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::config::{Config, DictEntry, DictOverrides};
use crate::tables::abbreviations::build_default_tables;

/// Which top-level tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Steps,
    Dictionaries,
    Output,
}

/// Input mode for dictionary and pattern editing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
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

use crate::address::COL_DEFS;

/// Fields in the step editor form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormField {
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
enum FormFocus {
    Left,          // navigating field list
    RightPattern,  // in pattern drill-down
    RightOutputCol, // in output column picker
    RightTable,    // in table picker
    EditingText(String, usize, String), // field being text-edited (field_name, cursor, text)
}

/// State for the step editor form.
#[derive(Debug, Clone)]
struct FormState {
    /// Index into App.steps of the step being edited, or None for new step.
    step_index: Option<usize>,
    /// Working copy of the StepDef being edited.
    def: crate::step::StepDef,
    /// Which fields are visible (computed from step type).
    visible_fields: Vec<FormField>,
    /// Cursor position in visible_fields.
    field_cursor: usize,
    /// Which panel has focus.
    focus: FormFocus,
    /// For right-panel list navigation (pattern segments, target fields, table list).
    right_cursor: usize,
    /// For pattern drill-down: which alternation group is expanded.
    right_alt_selected: Option<usize>,
    /// Parsed pattern segments for drill-down.
    pattern_segments: Vec<crate::pattern::PatternSegment>,
    /// Whether this is a new step (for cancel/discard behavior).
    is_new: bool,
    /// Show discard confirmation prompt.
    show_discard_prompt: bool,
}

fn visible_fields_for_type(step_type: &str, def: &crate::step::StepDef) -> Vec<FormField> {
    match step_type {
        "extract" => {
            let mut fields = vec![FormField::Pattern, FormField::OutputCol];
            fields.push(FormField::SkipIfFilled);
            fields.push(FormField::Replacement);
            fields.push(FormField::InputCol);
            fields.push(FormField::Label);
            fields
        }
        "rewrite" => {
            let mut fields = vec![FormField::Pattern];
            if def.table.is_some() {
                fields.push(FormField::Table);
            } else {
                fields.push(FormField::Replacement);
            }
            fields.push(FormField::InputCol);
            fields.push(FormField::Label);
            fields
        }
        "standardize" => {
            let mut fields = vec![];
            if def.pattern.is_some() {
                fields.push(FormField::Pattern);
                fields.push(FormField::Replacement);
            } else {
                fields.push(FormField::Table);
            }
            fields.push(FormField::OutputCol);
            fields.push(FormField::Mode);
            fields.push(FormField::Label);
            fields
        }
        _ => vec![FormField::Label],
    }
}

const TABLE_DESCRIPTIONS: &[(&str, &str)] = &[
    ("direction", "N/S/E/W, NORTH/SOUTH/EAST/WEST"),
    ("unit_type", "APT/SUITE/UNIT etc."),
    ("unit_location", "FRONT/REAR/BASEMENT etc."),
    ("suffix_all", "All suffix variants (AVE/AV/AVEN -> AVENUE)"),
    ("suffix_common", "Common suffixes only"),
    ("state", "State abbreviations"),
    ("street_name_abbr", "Street name abbreviations (MT->MOUNT)"),
    ("na_values", "NA/N/A values"),
    ("number_cardinal", "1->ONE, 42->FORTYTWO"),
    ("number_ordinal", "1->FIRST, 42->FORTYSECOND"),
];

/// A per-component output format setting.
#[derive(Debug, Clone)]
struct OutputSettingState {
    component: String,
    format: crate::config::OutputFormat,
    default_format: crate::config::OutputFormat,
    example_short: String,
    example_long: String,
}

/// A dictionary group with its change status.
#[derive(Debug, Clone)]
struct DictGroupState {
    short: String,
    long: String,
    variants: Vec<String>,
    status: GroupStatus,
    /// Original values for tracking overrides.
    original_short: String,
    original_long: String,
    original_variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupStatus {
    Default,
    Added,
    Removed,
    Modified,
}

/// A step with its current and default state, carrying full definition.
#[derive(Debug, Clone)]
struct StepState {
    enabled: bool,
    default_enabled: bool,
    is_custom: bool,
    def: crate::step::StepDef,
    default_def: Option<crate::step::StepDef>,
}

impl StepState {
    fn label(&self) -> &str { &self.def.label }
    fn step_type(&self) -> &str { &self.def.step_type }
    fn pattern_template(&self) -> &str {
        self.def.pattern.as_deref().unwrap_or("")
    }
    fn is_modified(&self) -> bool {
        match &self.default_def {
            None => false,
            Some(default) => self.def != *default || self.enabled != self.default_enabled,
        }
    }
    fn is_field_modified(&self, field: &str) -> bool {
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

/// Full TUI application state.
struct App {
    /// Path to save config to.
    config_path: PathBuf,
    /// Whether there are unsaved changes.
    dirty: bool,
    /// Whether we're showing the quit confirmation prompt.
    show_quit_prompt: bool,

    // -- Top-level navigation --
    active_tab: Tab,

    // -- Rules tab --
    steps: Vec<StepState>,
    steps_list_state: ListState,
    /// If Some, we're in move mode — value is the index of the step being moved.
    moving_step: Option<usize>,
    /// Original index before move started, for Esc cancel.
    moving_step_origin: Option<usize>,

    // -- Dictionaries tab --
    table_names: Vec<String>,
    dict_tab_index: usize,
    /// Dictionary entries per table, with change tracking.
    dict_entries: Vec<Vec<DictGroupState>>,
    dict_list_state: ListState,
    input_mode: InputMode,

    // -- Output tab --
    output_settings: Vec<OutputSettingState>,
    output_list_state: ListState,

    // -- Step editor form --
    /// Step editor form state (when open).
    form_state: Option<FormState>,
    /// If Some, we're showing delete confirmation for a custom step at this index.
    confirm_delete: Option<usize>,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let config = Config::load(&config_path);
        let default_tables = build_default_tables();

        // Parse default step definitions
        let toml_str = include_str!("../data/defaults/steps.toml");
        let default_defs: crate::step::StepsDef = toml::from_str(toml_str)
            .expect("Failed to parse default steps.toml");

        // Build default StepDef map (before any overrides)
        let default_def_map: std::collections::HashMap<String, crate::step::StepDef> =
            default_defs.step.iter()
                .map(|d| (d.label.clone(), d.clone()))
                .collect();

        // Build current defs (with overrides applied)
        let mut current_defs: Vec<crate::step::StepDef> = default_defs.step.clone();
        for def in &mut current_defs {
            if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
                step_override.apply_to(def);
            }
        }

        // Append custom steps
        for custom_def in &config.steps.custom_steps {
            let mut def = custom_def.clone();
            if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
                step_override.apply_to(&mut def);
            }
            current_defs.push(def);
        }

        // Apply step_order reordering (same logic as pipeline.rs)
        if !config.steps.step_order.is_empty() {
            let order = &config.steps.step_order;
            let pos_map: std::collections::HashMap<&str, usize> = order
                .iter().enumerate().map(|(i, label)| (label.as_str(), i)).collect();
            let mut ordered = Vec::new();
            let mut unordered = Vec::new();
            for def in current_defs {
                if let Some(&pos) = pos_map.get(def.label.as_str()) {
                    ordered.push((pos, def));
                } else {
                    unordered.push(def);
                }
            }
            ordered.sort_by_key(|(pos, _)| *pos);
            current_defs = ordered.into_iter().map(|(_, d)| d).collect();
            current_defs.extend(unordered);
        }

        // Build StepState vec
        let steps: Vec<StepState> = current_defs.iter().map(|def| {
            let is_custom = !default_def_map.contains_key(&def.label);
            let default_enabled = true;
            let enabled = !config.steps.disabled.contains(&def.label);
            StepState {
                enabled,
                default_enabled,
                is_custom,
                def: def.clone(),
                default_def: default_def_map.get(&def.label).cloned(),
            }
        }).collect();

        let mut steps_list_state = ListState::default();
        if !steps.is_empty() {
            steps_list_state.select(Some(0));
        }

        // Build dictionary states
        let table_names: Vec<String> = default_tables
            .table_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let dict_entries: Vec<Vec<DictGroupState>> = table_names
            .iter()
            .map(|name| {
                let table = default_tables.get(name).unwrap();
                let overrides = config.dictionaries.get(name);

                let mut entries: Vec<DictGroupState> = table
                    .groups
                    .iter()
                    .map(|g| {
                        let mut status = GroupStatus::Default;
                        let mut long = g.long.clone();
                        let mut variants = g.variants.clone();

                        if let Some(ov) = overrides {
                            // Check if removed
                            let is_removed = ov.remove.iter().any(|r| {
                                let upper = r.to_uppercase();
                                g.short == upper || g.long == upper
                            });
                            if is_removed {
                                status = GroupStatus::Removed;
                            }

                            // Check if overridden
                            for o in &ov.override_entries {
                                if o.short.to_uppercase() == g.short {
                                    long = o.long.to_uppercase();
                                    variants = o.variants.clone();
                                    status = GroupStatus::Modified;
                                }
                            }
                        }

                        DictGroupState {
                            short: g.short.clone(),
                            long,
                            variants,
                            status,
                            original_short: g.short.clone(),
                            original_long: g.long.clone(),
                            original_variants: g.variants.clone(),
                        }
                    })
                    .collect();

                // Append added entries from config
                if let Some(ov) = overrides {
                    for add in &ov.add {
                        let short = add.short.to_uppercase();
                        let long = add.long.to_uppercase();
                        // Check if this add merges into an existing group
                        let existing = entries.iter_mut().find(|e| e.short == short);
                        if let Some(e) = existing {
                            // Merge variants
                            for v in &add.variants {
                                let vu = v.to_uppercase();
                                if !e.variants.contains(&vu) {
                                    e.variants.push(vu);
                                }
                            }
                            e.status = GroupStatus::Modified;
                        } else {
                            entries.push(DictGroupState {
                                short: short.clone(),
                                long: long.clone(),
                                variants: add.variants.iter().map(|v| v.to_uppercase()).collect(),
                                status: GroupStatus::Added,
                                original_short: short,
                                original_long: long,
                                original_variants: Vec::new(),
                            });
                        }
                    }
                }

                entries
            })
            .collect();

        let mut dict_list_state = ListState::default();
        if !dict_entries.is_empty() && !dict_entries[0].is_empty() {
            dict_list_state.select(Some(0));
        }

        // Build output settings
        use crate::config::OutputFormat;
        let output_settings = vec![
            OutputSettingState {
                component: "suffix".to_string(),
                format: config.output.suffix,
                default_format: OutputFormat::Long,
                example_short: "DR".to_string(),
                example_long: "DRIVE".to_string(),
            },
            OutputSettingState {
                component: "direction".to_string(),
                format: config.output.direction,
                default_format: OutputFormat::Short,
                example_short: "N".to_string(),
                example_long: "NORTH".to_string(),
            },
            OutputSettingState {
                component: "unit_type".to_string(),
                format: config.output.unit_type,
                default_format: OutputFormat::Long,
                example_short: "APT".to_string(),
                example_long: "APARTMENT".to_string(),
            },
            OutputSettingState {
                component: "unit_location".to_string(),
                format: config.output.unit_location,
                default_format: OutputFormat::Long,
                example_short: "UPPR".to_string(),
                example_long: "UPPER".to_string(),
            },
            OutputSettingState {
                component: "state".to_string(),
                format: config.output.state,
                default_format: OutputFormat::Short,
                example_short: "NY".to_string(),
                example_long: "NEW YORK".to_string(),
            },
        ];
        let mut output_list_state = ListState::default();
        output_list_state.select(Some(0));

        App {
            config_path,
            dirty: false,
            show_quit_prompt: false,
            active_tab: Tab::Steps,
            steps,
            steps_list_state,
            moving_step: None,
            moving_step_origin: None,
            table_names,
            dict_tab_index: 0,
            dict_entries,
            dict_list_state,
            input_mode: InputMode::Normal,
            output_settings,
            output_list_state,
            confirm_delete: None,
            form_state: None,
        }
    }

    /// Build a Config from current TUI state (diff from defaults only).
    fn to_config(&self) -> Config {
        let mut config = Config::default();

        let mut disabled = Vec::new();
        let mut step_overrides = HashMap::new();
        let mut custom_steps = Vec::new();

        // Parse default step order for comparison
        let toml_str = include_str!("../data/defaults/steps.toml");
        let default_defs: crate::step::StepsDef = toml::from_str(toml_str).unwrap();
        let default_order: Vec<&str> = default_defs.step.iter().map(|d| d.label.as_str()).collect();

        for step in &self.steps {
            if !step.enabled && step.default_enabled {
                disabled.push(step.label().to_string());
            }

            if step.is_custom {
                custom_steps.push(step.def.clone());
            } else if let Some(default) = &step.default_def {
                // Diff against default, produce StepOverride with only changed fields
                let mut ovr = crate::config::StepOverride::default();
                let mut has_changes = false;
                if step.def.pattern != default.pattern {
                    ovr.pattern = step.def.pattern.clone(); has_changes = true;
                }
                if step.def.output_col != default.output_col {
                    ovr.output_col = step.def.output_col.clone(); has_changes = true;
                }
                if step.def.replacement != default.replacement {
                    ovr.replacement = step.def.replacement.clone(); has_changes = true;
                }
                if step.def.skip_if_filled != default.skip_if_filled {
                    ovr.skip_if_filled = step.def.skip_if_filled; has_changes = true;
                }
                if step.def.table != default.table {
                    ovr.table = step.def.table.clone(); has_changes = true;
                }
                if step.def.input_col != default.input_col {
                    ovr.input_col = step.def.input_col.clone(); has_changes = true;
                }
                if step.def.mode != default.mode {
                    ovr.mode = step.def.mode.clone(); has_changes = true;
                }
                if has_changes {
                    step_overrides.insert(step.label().to_string(), ovr);
                }
            }
        }

        // Only emit step_order if it differs from default
        let current_order: Vec<&str> = self.steps.iter().map(|s| s.label()).collect();
        let emit_order = current_order != default_order;

        config.steps = crate::config::StepsConfig {
            disabled,
            step_overrides,
            step_order: if emit_order { self.steps.iter().map(|s| s.label().to_string()).collect() } else { Vec::new() },
            custom_steps,
        };

        // Dictionaries: collect changes per table
        for (i, name) in self.table_names.iter().enumerate() {
            let entries = &self.dict_entries[i];
            let mut overrides = DictOverrides::default();

            for entry in entries {
                match entry.status {
                    GroupStatus::Added => {
                        overrides.add.push(DictEntry {
                            short: entry.short.clone(),
                            long: entry.long.clone(),
                            variants: entry.variants.clone(),
                            canonical: None,
                        });
                    }
                    GroupStatus::Removed => {
                        overrides.remove.push(entry.short.clone());
                    }
                    GroupStatus::Modified => {
                        overrides.add.push(DictEntry {
                            short: entry.short.clone(),
                            long: entry.long.clone(),
                            variants: entry.variants.clone(),
                            canonical: Some(true),
                        });
                    }
                    GroupStatus::Default => {}
                }
            }

            if !overrides.add.is_empty()
                || !overrides.remove.is_empty()
                || !overrides.override_entries.is_empty()
            {
                config.dictionaries.insert(name.clone(), overrides);
            }
        }

        // Output settings
        let mut output = crate::config::OutputConfig::default();
        for setting in &self.output_settings {
            let format = setting.format;
            match setting.component.as_str() {
                "suffix" => output.suffix = format,
                "direction" => output.direction = format,
                "unit_type" => output.unit_type = format,
                "unit_location" => output.unit_location = format,
                "state" => output.state = format,
                _ => {}
            }
        }
        config.output = output;

        config
    }

    fn save(&self) -> io::Result<()> {
        let config = self.to_config();
        config.save(&self.config_path)
    }

    fn current_dict_entries(&self) -> &[DictGroupState] {
        &self.dict_entries[self.dict_tab_index]
    }

    fn current_dict_entries_mut(&mut self) -> &mut Vec<DictGroupState> {
        &mut self.dict_entries[self.dict_tab_index]
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Launch the TUI configuration editor. Returns Ok(()) on clean exit.
pub fn run(config_path: PathBuf) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new(config_path);
    let result = run_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render(frame, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Global: Ctrl-C always exits immediately
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }

            // Quit prompt takes priority
            if app.show_quit_prompt {
                match key.code {
                    KeyCode::Char('s') | KeyCode::Char('y') => {
                        let _ = app.save();
                        return Ok(());
                    }
                    KeyCode::Char('n') | KeyCode::Char('q') => {
                        return Ok(());
                    }
                    KeyCode::Esc => {
                        app.show_quit_prompt = false;
                    }
                    _ => {}
                }
                continue;
            }

            // Delete confirmation takes priority
            if let Some(del_idx) = app.confirm_delete {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        app.steps.remove(del_idx);
                        let len = app.steps.len();
                        if len == 0 {
                            app.steps_list_state.select(None);
                        } else if del_idx >= len {
                            app.steps_list_state.select(Some(len - 1));
                        }
                        app.dirty = true;
                        app.confirm_delete = None;
                    }
                    _ => {
                        app.confirm_delete = None;
                    }
                }
                continue;
            }

            // Input mode for dictionary editing
            if app.input_mode != InputMode::Normal {
                handle_input_mode(app, key.code);
                continue;
            }

            // Move mode: only step handler processes keys
            if app.moving_step.is_some() && app.active_tab == Tab::Steps {
                handle_rules_key(app, key.code);
                continue;
            }

            // Form mode: form consumes all keys (including Esc, Tab, s)
            if app.form_state.is_some() {
                handle_form_key(app, key.code);
                continue;
            }

            // Normal mode
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    if app.dirty {
                        app.show_quit_prompt = true;
                    } else {
                        return Ok(());
                    }
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    app.active_tab = match app.active_tab {
                        Tab::Steps => Tab::Dictionaries,
                        Tab::Dictionaries => Tab::Output,
                        Tab::Output => Tab::Steps,
                    };
                }
                KeyCode::Char('s') => {
                    if let Err(_e) = app.save() {
                        // TODO: show error in status bar
                    } else {
                        app.dirty = false;
                    }
                }
                _ => match app.active_tab {
                    Tab::Steps => handle_rules_key(app, key.code),
                    Tab::Dictionaries => handle_dict_key(app, key.code),
                    Tab::Output => handle_output_key(app, key.code),
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Key handlers
// ---------------------------------------------------------------------------

fn handle_rules_key(app: &mut App, code: KeyCode) {
    let len = app.steps.len();
    if len == 0 {
        return;
    }

    // Move mode: step is grabbed, arrow keys reposition it
    if let Some(moving_idx) = app.moving_step {
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                if moving_idx + 1 < len {
                    app.steps.swap(moving_idx, moving_idx + 1);
                    let new_idx = moving_idx + 1;
                    app.moving_step = Some(new_idx);
                    app.steps_list_state.select(Some(new_idx));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
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
        KeyCode::Down | KeyCode::Char('j') => {
            let i = app.steps_list_state.selected().unwrap_or(0);
            app.steps_list_state.select(Some((i + 1) % len));
        }
        KeyCode::Up | KeyCode::Char('k') => {
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


fn handle_dict_key(app: &mut App, code: KeyCode) {
    let num_tables = app.table_names.len();
    match code {
        // Sub-tab navigation
        KeyCode::Right | KeyCode::Char('l') => {
            app.dict_tab_index = (app.dict_tab_index + 1) % num_tables;
            let len = app.current_dict_entries().len();
            app.dict_list_state
                .select(if len > 0 { Some(0) } else { None });
        }
        KeyCode::Left | KeyCode::Char('h') => {
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
        KeyCode::Down | KeyCode::Char('j') => {
            let len = app.current_dict_entries().len();
            if len > 0 {
                let i = app.dict_list_state.selected().unwrap_or(0);
                app.dict_list_state.select(Some((i + 1) % len));
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
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
        // Drill into group to view/edit variants
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

/// Result of a text editing keystroke.
enum TextEditResult {
    /// Text was modified or cursor moved — continue editing.
    Continue,
    /// Enter was pressed — return the final text.
    Submit(String),
    /// Esc was pressed — cancel.
    Cancel,
}

/// Handle a keystroke for cursor-aware text editing.
/// Returns the action to take. Mutates text and cursor in place for Continue.
fn text_edit(text: &mut String, cursor: &mut usize, code: KeyCode) -> TextEditResult {
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
fn render_text_with_cursor(text: &str, cursor: usize) -> String {
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

fn handle_input_mode(app: &mut App, code: KeyCode) {
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
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = app.current_dict_entries()[group_idx].variants.len();
                    if len > 0 {
                        app.input_mode = InputMode::EditVariants(group_idx, (cursor + 1) % len);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
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


// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(frame: &mut Frame, app: &mut App) {
    let [tabs_area, content_area, status_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    // Top-level tabs
    let tab_titles = vec!["Steps", "Dictionaries", "Output"];
    let selected_tab = match app.active_tab {
        Tab::Steps => 0,
        Tab::Dictionaries => 1,
        Tab::Output => 2,
    };
    let tabs = Tabs::new(tab_titles)
        .block(Block::bordered().title("addrust configure"))
        .select(selected_tab)
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ");
    frame.render_widget(tabs, tabs_area);

    // Content
    match app.active_tab {
        Tab::Steps => {
            if app.form_state.is_some() {
                render_step_form(frame, app, content_area);
            } else {
                render_steps(frame, app, content_area);
            }
        }
        Tab::Dictionaries => render_dict(frame, app, content_area),
        Tab::Output => render_output(frame, app, content_area),
    }

    // Status bar
    let dirty_indicator = if app.dirty { " [modified]" } else { "" };
    let status_text = if app.moving_step.is_some() {
        format!(" ↑↓: move | Enter: confirm | Esc: cancel{}", dirty_indicator)
    } else {
        format!(
            " Tab: switch | j/k: navigate | Space: toggle | m: move | a: add | d: delete | Enter: edit | s: save | q: quit{}",
            dirty_indicator
        )
    };
    let status = Paragraph::new(status_text)
        .style(Style::new().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(status, status_area);

    // Quit prompt overlay
    if app.show_quit_prompt {
        let popup_area = centered_rect(50, 5, frame.area());
        let popup = Paragraph::new(
            "Unsaved changes. Save before quitting? (s)ave / (n)o / (Esc) cancel",
        )
        .block(Block::bordered().title("Unsaved Changes"))
        .style(Style::new().bg(Color::Black).fg(Color::Yellow));
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        frame.render_widget(popup, popup_area);
    }

    // Input mode overlay
    match &app.input_mode {
        InputMode::Normal => {}
        InputMode::AddShort(s, cursor) => {
            let popup_area = centered_rect(50, 5, frame.area());
            let display = render_text_with_cursor(s, *cursor);
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Add entry — short form (Enter to continue, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::AddLong(short, long, cursor) => {
            let popup_area = centered_rect(50, 5, frame.area());
            let display = format!("{} -> {}", short, render_text_with_cursor(long, *cursor));
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Add entry — long form (Enter to confirm, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::EditLong(_, text, cursor) => {
            let popup_area = centered_rect(50, 5, frame.area());
            let display = render_text_with_cursor(text, *cursor);
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Edit long form (Enter to confirm, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::EditVariants(group_idx, cursor) => {
            let entry = &app.current_dict_entries()[*group_idx];
            let height = (entry.variants.len() as u16 + 4).max(6).min(20);
            let popup_area = centered_rect(50, height, frame.area());
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(&entry.short, Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" → "),
                    Span::styled(&entry.long, Style::new().fg(Color::Cyan)),
                ]),
                Line::raw(""),
            ];
            if entry.variants.is_empty() {
                lines.push(Line::styled("  (no variants)", Style::new().fg(Color::DarkGray)));
            } else {
                for (i, v) in entry.variants.iter().enumerate() {
                    let marker = if i == *cursor { "> " } else { "  " };
                    let style = if i == *cursor {
                        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    lines.push(Line::styled(format!("{}{}", marker, v), style));
                }
            }
            let popup = Paragraph::new(lines)
                .block(Block::bordered().title("Variants (a: add, d: delete, Esc: back)"))
                .style(Style::new().bg(Color::Black));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::AddVariant(group_idx, text, cursor) => {
            let entry = &app.current_dict_entries()[*group_idx];
            let popup_area = centered_rect(50, 5, frame.area());
            let display = format!("{} → {} : {}", entry.short, entry.long, render_text_with_cursor(text, *cursor));
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Add variant (Enter to confirm, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
    }

    // Delete confirmation overlay
    if let Some(del_idx) = app.confirm_delete {
        if del_idx < app.steps.len() {
            let label = app.steps[del_idx].label();
            let popup_area = centered_rect(50, 5, frame.area());
            let popup = Paragraph::new(format!("Delete custom step '{}'? (y/n)", label))
                .block(Block::bordered().title("Confirm Delete"))
                .style(Style::new().bg(Color::Black).fg(Color::Yellow));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
    }
}

fn render_steps(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .steps
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let is_moving = app.moving_step == Some(idx);
            let check = if r.enabled { " " } else { "x" };
            let style = if is_moving {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if !r.enabled {
                Style::new().fg(Color::DarkGray)
            } else if r.enabled != r.default_enabled {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            let check_style = if is_moving {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if r.enabled {
                Style::new().fg(Color::Green)
            } else {
                Style::new().fg(Color::Red)
            };
            let pattern_style = if is_moving {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new().fg(Color::DarkGray)
            };
            let label_display = if r.is_custom {
                format!("[+] {:27} ", r.label())
            } else {
                format!("{:30} ", r.label())
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", check), check_style),
                Span::styled(label_display, style),
                Span::styled(format!("{:8} ", r.step_type()), if is_moving { style } else { Style::new().fg(Color::DarkGray) }),
                Span::styled(r.pattern_template(), pattern_style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title("Pipeline Rules"))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.steps_list_state);
}

fn render_dict(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let [subtab_area, entries_area] = Layout::vertical([
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

    // Entries — build items from immutable borrow, then drop it before mutable borrow
    let is_value_list = {
        let tables = build_default_tables();
        tables
            .get(&app.table_names[app.dict_tab_index])
            .map(|t| t.is_value_list())
            .unwrap_or(false)
    };

    let (items, table_name) = {
        let entries = app.current_dict_entries();
        let items: Vec<ListItem> = entries
            .iter()
            .map(|e| {
                let (marker, style) = match e.status {
                    GroupStatus::Default => ("\u{2605} ", Style::new()),
                    GroupStatus::Added => ("\u{2605} ", Style::new().fg(Color::Green)),
                    GroupStatus::Removed => ("\u{2605} ", Style::new().fg(Color::Red).add_modifier(Modifier::CROSSED_OUT)),
                    GroupStatus::Modified => ("\u{2605} ", Style::new().fg(Color::Yellow)),
                };
                let variants_str = if e.variants.is_empty() {
                    String::new()
                } else {
                    format!("    {}", e.variants.join(", "))
                };
                if is_value_list {
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(e.short.clone(), style),
                    ]))
                } else {
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(format!("{:12}", e.short), style),
                        Span::styled(format!("{:20}", e.long), style),
                        Span::styled(variants_str, Style::new().fg(Color::DarkGray)),
                    ]))
                }
            })
            .collect();
        let table_name = app.table_names[app.dict_tab_index].clone();
        (items, table_name)
    };

    let list = List::new(items)
        .block(Block::bordered().title(format!("{} entries", table_name)))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, entries_area, &mut app.dict_list_state);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate a pattern template by expanding table placeholders and compiling.
// ---------------------------------------------------------------------------
// Step editor form — rendering
// ---------------------------------------------------------------------------

fn render_step_form(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let form = match &app.form_state {
        Some(f) => f,
        None => return,
    };

    let step_state = form.step_index.map(|i| &app.steps[i]);

    let [header_area, body_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
    ]).areas(area);

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
        let popup = centered_rect(50, 5, area);
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

fn render_form_left_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let form = app.form_state.as_ref().unwrap();
    let step_state = form.step_index.map(|i| &app.steps[i]);

    let mut items: Vec<ListItem> = Vec::new();

    for (i, field) in form.visible_fields.iter().enumerate() {
        let is_selected = form.focus == FormFocus::Left && form.field_cursor == i;
        let is_modified = step_state.map(|s| s.is_field_modified(field_key(*field))).unwrap_or(false);

        let prefix = if is_selected { "▸ " } else { "  " };
        let mod_marker = if is_modified { "* " } else { "  " };
        let (label, value) = form_field_display(*field, &form.def);

        let style = if is_selected {
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };

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
            if form.focus == FormFocus::Left {
                Style::new().fg(Color::Cyan)
            } else {
                Style::new().fg(Color::DarkGray)
            }
        ));
    frame.render_widget(list, area);
}

fn field_key(field: FormField) -> &'static str {
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

fn form_field_display(field: FormField, def: &crate::step::StepDef) -> (&'static str, String) {
    use crate::step::OutputCol;
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

fn render_form_right_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
    area: ratatui::layout::Rect,
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
        .block(Block::bordered().border_style(Style::new().fg(Color::Cyan)));
    frame.render_widget(panel, area);
}

fn render_form_help_panel(
    frame: &mut Frame,
    field: FormField,
    def: &crate::step::StepDef,
    step_state: Option<&StepState>,
    area: ratatui::layout::Rect,
) {
    use crate::step::OutputCol;
    let (title, help_text, current_value, edit_hint): (&str, &str, String, &str) = match field {
        FormField::SkipIfFilled => (
            "Skip If Filled",
            "When yes, this step is skipped if the target field(s) already have a value from a previous step.\n\nUse this for extraction steps that should only fire once.",
            (if def.skip_if_filled == Some(true) { "yes" } else { "no" }).to_string(),
            "Space to toggle",
        ),
        FormField::Replacement => (
            "Replacement",
            "Text that replaces the matched pattern. Supports backreferences:\n\n  $1        - capture group 1\n  ${N:table} - look up group N in a table\n  ${N/M:fraction} - fraction (group N / group M)",
            def.replacement.as_deref().unwrap_or("(none)").to_string(),
            "Enter to edit",
        ),
        FormField::InputCol => (
            "Input Column",
            "Which text this step operates on.\n\n'working string' is the main address being parsed. Selecting a field makes the step operate on that extracted field instead.",
            def.input_col.as_deref().unwrap_or("working string").to_string(),
            "Enter to pick",
        ),
        FormField::Mode => (
            "Mode",
            "'Whole field' standardizes the entire field value as one lookup.\n\n'Per word' splits on spaces and standardizes each word independently.",
            def.mode.as_deref().unwrap_or("whole field").to_string(),
            "Space to toggle",
        ),
        FormField::Label => (
            "Label",
            "Unique identifier for this step. Used in config files for overrides, ordering, and disable lists.",
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
            ("Output Column", "The address field(s) where the extracted value is stored.", val, "Enter to pick")
        }
        _ => return,
    };

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
        .block(Block::bordered().border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(panel, area);
}

fn render_form_pattern_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
        "j/k: navigate  Enter: expand  e: edit raw  Esc: back"
    };
    let list = List::new(items).block(
        Block::bordered()
            .title("Pattern")
            .title_bottom(hints)
            .border_style(Style::new().fg(if focused { Color::Cyan } else { Color::DarkGray }))
    );
    frame.render_widget(list, area);
}

fn render_form_targets_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use crate::step::OutputCol;
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
                    .border_style(Style::new().fg(if focused { Color::Cyan } else { Color::DarkGray }))
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
                    .border_style(Style::new().fg(if focused { Color::Cyan } else { Color::DarkGray }))
            );
            frame.render_widget(list, area);
        }
        _ => {}
    }
}

fn render_form_table_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
            .border_style(Style::new().fg(if focused { Color::Cyan } else { Color::DarkGray }))
    );
    frame.render_widget(list, area);
}

// ---------------------------------------------------------------------------
// Step editor form — key handling
// ---------------------------------------------------------------------------

fn handle_form_key(app: &mut App, code: KeyCode) {
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
        KeyCode::Down | KeyCode::Char('j') => {
            form.field_cursor = (form.field_cursor + 1) % field_count;
        }
        KeyCode::Up | KeyCode::Char('k') => {
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

fn close_form(app: &mut App) {
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

fn validate_step_def(def: &crate::step::StepDef) -> bool {
    use crate::step::OutputCol;
    match def.step_type.as_str() {
        "extract" => {
            def.pattern.is_some()
                && match &def.output_col {
                    Some(OutputCol::Single(_)) => true,
                    Some(OutputCol::Multi(m)) => !m.is_empty(),
                    None => false,
                }
        }
        "rewrite" => {
            def.pattern.is_some()
                && (def.replacement.is_some() || def.table.is_some())
        }
        "standardize" => {
            def.output_col.is_some()
                && (def.table.is_some() || (def.pattern.is_some() && def.replacement.is_some()))
        }
        _ => false,
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
        KeyCode::Down | KeyCode::Char('j') => {
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
        KeyCode::Up | KeyCode::Char('k') => {
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
    use crate::step::OutputCol;
    let form = app.form_state.as_mut().unwrap();
    let current_field = form.visible_fields[form.field_cursor];
    let is_source = current_field == FormField::InputCol;
    let is_multi = current_field == FormField::OutputCol
        && matches!(&form.def.output_col, Some(OutputCol::Multi(_)));
    let item_count = if is_source { COL_DEFS.len() + 1 } else { COL_DEFS.len() };

    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            form.right_cursor = (form.right_cursor + 1) % item_count;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            form.right_cursor = if form.right_cursor == 0 { item_count - 1 } else { form.right_cursor - 1 };
        }
        KeyCode::Enter if !is_multi => {
            let offset = if is_source { 1 } else { 0 };
            if is_source && form.right_cursor == 0 {
                form.def.input_col = None;
            } else {
                let field_key = COL_DEFS[form.right_cursor - offset].key;
                if is_source {
                    form.def.input_col = Some(field_key.to_string());
                } else {
                    form.def.output_col = Some(OutputCol::Single(field_key.to_string()));
                }
            }
            form.focus = FormFocus::Left;
            app.dirty = true;
        }
        KeyCode::Char(' ') if is_multi => {
            let field_key = COL_DEFS[form.right_cursor].key.to_string();
            let targets = match &mut form.def.output_col {
                Some(OutputCol::Multi(m)) => m,
                _ => unreachable!(),
            };
            if targets.contains_key(&field_key) {
                targets.remove(&field_key);
            } else {
                let max = targets.values().max().copied().unwrap_or(0);
                targets.insert(field_key, max + 1);
            }
            app.dirty = true;
        }
        KeyCode::Char(c) if is_multi && c.is_ascii_digit() && c != '0' => {
            let group = (c as u8 - b'0') as usize;
            let field_key = COL_DEFS[form.right_cursor].key.to_string();
            let targets = match &mut form.def.output_col {
                Some(OutputCol::Multi(m)) => m,
                _ => unreachable!(),
            };
            targets.insert(field_key, group);
            app.dirty = true;
        }
        KeyCode::Char('d') if is_multi => {
            let field_key = COL_DEFS[form.right_cursor].key;
            if let Some(OutputCol::Multi(targets)) = &mut form.def.output_col {
                targets.remove(field_key);
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
        KeyCode::Down | KeyCode::Char('j') => {
            form.right_cursor = (form.right_cursor + 1) % TABLE_DESCRIPTIONS.len();
        }
        KeyCode::Up | KeyCode::Char('k') => {
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

fn centered_rect(
    percent_x: u16,
    height: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center_h, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(center_v);
    center_h
}

// ---------------------------------------------------------------------------
// Output tab
// ---------------------------------------------------------------------------

fn handle_output_key(app: &mut App, code: KeyCode) {
    use crate::config::OutputFormat;
    let len = app.output_settings.len();
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            if len > 0 {
                let i = app.output_list_state.selected().unwrap_or(0);
                app.output_list_state.select(Some((i + 1) % len));
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
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

fn render_output(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    use crate::config::OutputFormat;

    let items: Vec<ListItem> = app
        .output_settings
        .iter()
        .map(|s| {
            let is_modified = s.format != s.default_format;
            let format_str = match s.format {
                OutputFormat::Short => "short",
                OutputFormat::Long => "long",
            };
            let example = match s.format {
                OutputFormat::Short => &s.example_short,
                OutputFormat::Long => &s.example_long,
            };
            let marker = if is_modified { "~ " } else { "  " };
            let style = if is_modified {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(format!("{:20}", s.component), style),
                Span::styled(
                    format!("{:8}", format_str),
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("({})", example), Style::new().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title("Output Format (Space to toggle)"))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.output_list_state);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_config_no_changes() {
        let app = App::new(PathBuf::from("nonexistent.toml"));
        let config = app.to_config();
        assert!(config.steps.disabled.is_empty());
        assert!(config.dictionaries.is_empty());
    }

    #[test]
    fn test_to_config_disabled_rule() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        // Disable first step
        if !app.steps.is_empty() {
            app.steps[0].enabled = false;
            let config = app.to_config();
            assert!(config.steps.disabled.contains(&app.steps[0].label().to_string()));
        }
    }

    #[test]
    fn test_to_config_dict_add() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.table_names.is_empty() {
            app.dict_entries[0].push(DictGroupState {
                short: "TEST".to_string(),
                long: "TESTING".to_string(),
                variants: vec!["TST".to_string()],
                status: GroupStatus::Added,
                original_short: "TEST".to_string(),
                original_long: "TESTING".to_string(),
                original_variants: Vec::new(),
            });
            let config = app.to_config();
            let name = &app.table_names[0];
            let overrides = config.dictionaries.get(name).unwrap();
            assert_eq!(overrides.add.len(), 1);
            assert_eq!(overrides.add[0].short, "TEST");
            assert_eq!(overrides.add[0].variants, vec!["TST".to_string()]);
        }
    }

    #[test]
    fn test_to_config_dict_remove() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.dict_entries[0].is_empty() {
            app.dict_entries[0][0].status = GroupStatus::Removed;
            let config = app.to_config();
            let name = &app.table_names[0];
            let overrides = config.dictionaries.get(name).unwrap();
            assert_eq!(overrides.remove.len(), 1);
            // Remove now stores the short form (canonical key)
            assert_eq!(overrides.remove[0], app.dict_entries[0][0].short);
        }
    }

    #[test]
    fn test_to_config_step_override() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.steps.is_empty() {
            let label = app.steps[0].label().to_string();
            // Modify a step's pattern
            let original = app.steps[0].def.pattern.clone();
            app.steps[0].def.pattern = Some("MODIFIED_PATTERN".to_string());
            let config = app.to_config();
            assert!(config.steps.step_overrides.contains_key(&label));
            assert_eq!(
                config.steps.step_overrides.get(&label).unwrap().pattern.as_deref(),
                Some("MODIFIED_PATTERN")
            );

            // Restore to default — should NOT appear in overrides
            app.steps[0].def.pattern = original;
            let config = app.to_config();
            assert!(!config.steps.step_overrides.contains_key(&label));
        }
    }

    #[test]
    fn test_add_entry_flow() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        // Start on Dictionaries tab, select suffix_all (first non-value-list table)
        app.active_tab = Tab::Dictionaries;

        // Press 'a' to start adding
        handle_dict_key(&mut app, KeyCode::Char('a'));
        assert!(matches!(app.input_mode, InputMode::AddShort(_, _)));

        // Type "TST"
        handle_input_mode(&mut app, KeyCode::Char('T'));
        handle_input_mode(&mut app, KeyCode::Char('S'));
        handle_input_mode(&mut app, KeyCode::Char('T'));
        if let InputMode::AddShort(ref s, _) = app.input_mode {
            assert_eq!(s, "TST");
        } else {
            panic!("Expected AddShort mode");
        }

        // Press Enter to move to long form
        handle_input_mode(&mut app, KeyCode::Enter);
        assert!(matches!(app.input_mode, InputMode::AddLong(_, _, _)));

        // Type "TESTING"
        for c in "TESTING".chars() {
            handle_input_mode(&mut app, KeyCode::Char(c));
        }

        // Press Enter to confirm
        handle_input_mode(&mut app, KeyCode::Enter);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.dirty);

        // Verify entry was added
        let entries = app.current_dict_entries();
        let last = entries.last().unwrap();
        assert_eq!(last.short, "TST");
        assert_eq!(last.long, "TESTING");
        assert_eq!(last.status, GroupStatus::Added);
    }

    #[test]
    fn test_to_config_dict_override() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.dict_entries[0].is_empty() {
            app.dict_entries[0][0].long = "CHANGED".to_string();
            app.dict_entries[0][0].status = GroupStatus::Modified;
            let config = app.to_config();
            let name = &app.table_names[0];
            let overrides = config.dictionaries.get(name).unwrap();
            // Modified entries go to add with canonical: Some(true)
            assert_eq!(overrides.add.len(), 1);
            assert_eq!(overrides.add[0].long, "CHANGED");
            assert_eq!(overrides.add[0].canonical, Some(true));
        }
    }
}
