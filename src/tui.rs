use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs};
use ratatui::{DefaultTerminal, Frame};

use crate::config::{Config, DictEntry, DictOverrides};
use crate::pattern::PatternSegment;
use crate::pipeline::Pipeline;
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
    /// Editing a step's pattern template: (text, cursor position, optional validation error).
    EditPattern(String, usize, Option<String>),
    /// Add-step wizard: (wizard state, insertion index).
    AddStep(WizardState, usize),
}

/// Accumulated wizard choices for building a new custom step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WizardAccumulator {
    step_type: String,
    pattern: String,
    target: Option<String>,
    targets: Option<HashMap<String, usize>>,
    replacement: Option<String>,
    table: Option<String>,
    skip_if_filled: Option<bool>,
    mode: Option<String>,
}

/// State for the add-step wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WizardState {
    PickType(usize),
    Pattern(String, usize, Option<String>),
    /// For extract: choose single target vs. multiple targets
    ExtractMode(usize),
    PickTarget(usize),
    /// Multi-target select: (cursor, selections as field_name → group_number)
    /// Group number 0 means not selected; 1+ means capture group N.
    PickTargets(usize, Vec<usize>),
    /// Text input for capture group number when assigning a multi-target field
    TargetGroup(usize, Vec<usize>, String, usize),
    SkipIfFilled,
    Replacement(String, usize),
    RewriteMode(usize),
    TableName(String, usize),
    StandardizeMode(usize),
    /// Pick table for standardization: (cursor, table_names)
    PickTable(usize, Vec<String>),
    WordMode(usize),
    Label(String, usize),
}

const TARGET_FIELDS: &[(&str, &str)] = &[
    ("street_number", "Street Number"),
    ("pre_direction", "Pre-Direction"),
    ("street_name", "Street Name"),
    ("suffix", "Suffix"),
    ("post_direction", "Post-Direction"),
    ("unit", "Unit"),
    ("unit_type", "Unit Type"),
    ("po_box", "PO Box"),
    ("building", "Building"),
    ("extra_front", "Extra Front"),
    ("extra_back", "Extra Back"),
];

const STEP_TYPES: &[&str] = &["extract", "rewrite", "standardize"];

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

/// A step with its original and current enabled state.
#[derive(Debug, Clone)]
struct StepState {
    label: String,
    group: String,
    action_desc: String,
    pattern_template: String,
    enabled: bool,
    default_enabled: bool,
    is_custom: bool,
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

    // -- Step detail view --
    /// If Some, we're viewing/editing a step's detail (index into steps vec).
    step_detail_index: Option<usize>,
    /// Parsed pattern segments for the step being viewed.
    step_detail_segments: Vec<PatternSegment>,
    /// Which segment is selected (only alternation groups are selectable).
    step_detail_selected: usize,
    /// If viewing inside an alternation group, which alternative is selected.
    step_detail_alt_selected: Option<usize>,

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

    // -- Custom steps --
    /// Original StepDef for each custom step, keyed by label.
    custom_step_defs: std::collections::HashMap<String, crate::step::StepDef>,
    /// Wizard accumulator for building a new custom step.
    wizard_acc: WizardAccumulator,
    /// If Some, we're showing delete confirmation for a custom step at this index.
    confirm_delete: Option<usize>,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let config = Config::load(&config_path);
        let default_tables = build_default_tables();
        let pipeline = Pipeline::from_config(&config);

        // Build step states from step summaries
        let default_pipeline = Pipeline::default();
        let default_summaries = default_pipeline.step_summaries();
        let config_summaries = pipeline.step_summaries();

        // Use label-based lookup for default_enabled (not positional zip)
        let default_enabled_map: std::collections::HashMap<&str, bool> = default_summaries
            .iter()
            .map(|s| (s.label.as_str(), s.enabled))
            .collect();

        let steps: Vec<StepState> = config_summaries
            .iter()
            .map(|current| {
                let is_custom = !default_enabled_map.contains_key(current.label.as_str());
                let default_enabled = default_enabled_map
                    .get(current.label.as_str())
                    .copied()
                    .unwrap_or(true);
                StepState {
                    label: current.label.clone(),
                    group: current.step_type.clone(),
                    action_desc: current.step_type.clone(),
                    pattern_template: current.pattern_template.clone().unwrap_or_default(),
                    enabled: current.enabled,
                    default_enabled,
                    is_custom,
                }
            })
            .collect();

        let custom_step_defs: std::collections::HashMap<String, crate::step::StepDef> = config
            .steps
            .custom_steps
            .iter()
            .map(|d| (d.label.clone(), d.clone()))
            .collect();

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
            step_detail_index: None,
            step_detail_segments: Vec::new(),
            step_detail_selected: 0,
            step_detail_alt_selected: None,
            table_names,
            dict_tab_index: 0,
            dict_entries,
            dict_list_state,
            input_mode: InputMode::Normal,
            output_settings,
            output_list_state,
            custom_step_defs,
            wizard_acc: WizardAccumulator::default(),
            confirm_delete: None,
        }
    }

    /// Build a Config from current TUI state (diff from defaults only).
    fn to_config(&self) -> Config {
        let mut config = Config::default();

        // Steps: collect individually disabled labels
        let disabled: Vec<String> = self
            .steps
            .iter()
            .filter(|r| !r.enabled && r.default_enabled)
            .map(|r| r.label.clone())
            .collect();
        config.steps.disabled = disabled;

        // Pattern overrides: compare by label (not position) since steps may be reordered
        let default_pipeline = Pipeline::default();
        let default_summaries = default_pipeline.step_summaries();
        let default_patterns: std::collections::HashMap<&str, &str> = default_summaries
            .iter()
            .map(|s| (s.label.as_str(), s.pattern_template.as_deref().unwrap_or("")))
            .collect();

        for step in &self.steps {
            if step.is_custom {
                continue; // Custom step patterns stored in custom_steps, not overrides
            }
            let default_template = default_patterns
                .get(step.label.as_str())
                .copied()
                .unwrap_or("");
            if step.pattern_template != default_template {
                config.steps.pattern_overrides.insert(
                    step.label.clone(),
                    step.pattern_template.clone(),
                );
            }
        }

        // Custom steps: serialize in current step order
        config.steps.custom_steps = self
            .steps
            .iter()
            .filter(|s| s.is_custom)
            .filter_map(|s| {
                let mut def = self.custom_step_defs.get(&s.label)?.clone();
                if !s.pattern_template.is_empty() {
                    def.pattern = Some(s.pattern_template.clone());
                }
                Some(def)
            })
            .collect();

        // Step order: only store if different from default
        let default_order: Vec<&str> = default_summaries.iter().map(|s| s.label.as_str()).collect();
        let current_order: Vec<&str> = self.steps.iter().map(|s| s.label.as_str()).collect();
        if current_order != default_order {
            config.steps.step_order = self.steps.iter().map(|s| s.label.clone()).collect();
        }

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
                        let label = app.steps[del_idx].label.clone();
                        app.steps.remove(del_idx);
                        app.custom_step_defs.remove(&label);
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

            // Input mode for dictionary editing and wizard
            if app.input_mode != InputMode::Normal {
                if matches!(app.input_mode, InputMode::AddStep(_, _)) {
                    handle_wizard_key(app, key.code);
                } else {
                    handle_input_mode(app, key.code);
                }
                continue;
            }

            // Move mode: only step handler processes keys
            if app.moving_step.is_some() && app.active_tab == Tab::Steps {
                handle_rules_key(app, key.code);
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
                    Tab::Steps => {
                        if app.step_detail_index.is_some() {
                            handle_step_detail_key(app, key.code);
                        } else {
                            handle_rules_key(app, key.code);
                        }
                    }
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
            if let Some(i) = app.steps_list_state.selected() {
                let segments = crate::pattern::parse_pattern(&app.steps[i].pattern_template);
                app.step_detail_index = Some(i);
                app.step_detail_segments = segments;
                app.step_detail_selected = 0;
                app.step_detail_alt_selected = None;
            }
        }
        KeyCode::Char('m') => {
            if let Some(i) = app.steps_list_state.selected() {
                app.moving_step = Some(i);
                app.moving_step_origin = Some(i);
            }
        }
        KeyCode::Char('a') => {
            let insert_after = app.steps_list_state.selected().unwrap_or(0);
            app.wizard_acc = WizardAccumulator::default();
            app.input_mode = InputMode::AddStep(
                WizardState::PickType(0),
                insert_after + 1,
            );
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

fn handle_step_detail_key(app: &mut App, code: KeyCode) {
    match code {
        // Back to steps list
        KeyCode::Esc | KeyCode::Left => {
            if app.step_detail_alt_selected.is_some() {
                app.step_detail_alt_selected = None;
            } else {
                app.step_detail_index = None;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(alt_idx) = app.step_detail_alt_selected {
                // Navigate within alternation group
                if let PatternSegment::AlternationGroup { alternatives, .. } =
                    &app.step_detail_segments[app.step_detail_selected]
                {
                    if alt_idx + 1 < alternatives.len() {
                        app.step_detail_alt_selected = Some(alt_idx + 1);
                    }
                }
            } else {
                // Navigate between segments (skip non-actionable ones)
                let len = app.step_detail_segments.len();
                let mut next = app.step_detail_selected + 1;
                while next < len {
                    match &app.step_detail_segments[next] {
                        PatternSegment::AlternationGroup { .. } | PatternSegment::TableRef(_) => {
                            break
                        }
                        _ => next += 1,
                    }
                }
                if next < len {
                    app.step_detail_selected = next;
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(alt_idx) = app.step_detail_alt_selected {
                if alt_idx > 0 {
                    app.step_detail_alt_selected = Some(alt_idx - 1);
                }
            } else {
                // Navigate between segments (skip non-actionable ones)
                let mut prev = app.step_detail_selected;
                while prev > 0 {
                    prev -= 1;
                    match &app.step_detail_segments[prev] {
                        PatternSegment::AlternationGroup { .. } | PatternSegment::TableRef(_) => {
                            app.step_detail_selected = prev;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        // Enter/Right to drill into alternation group
        KeyCode::Enter | KeyCode::Right => {
            if app.step_detail_alt_selected.is_none() {
                if let PatternSegment::AlternationGroup { .. } =
                    &app.step_detail_segments[app.step_detail_selected]
                {
                    app.step_detail_alt_selected = Some(0);
                }
            }
        }
        // Edit the full pattern template
        KeyCode::Char('e') => {
            if let Some(step_idx) = app.step_detail_index {
                let template = app.steps[step_idx].pattern_template.clone();
                let len = template.len();
                app.input_mode = InputMode::EditPattern(template, len, None);
            }
        }
        // Space to toggle alternative
        KeyCode::Char(' ') => {
            if let Some(alt_idx) = app.step_detail_alt_selected {
                if let PatternSegment::AlternationGroup { alternatives, .. } =
                    &mut app.step_detail_segments[app.step_detail_selected]
                {
                    // Don't allow disabling the last enabled alternative
                    let enabled_count = alternatives.iter().filter(|a| a.enabled).count();
                    if alternatives[alt_idx].enabled && enabled_count <= 1 {
                        return;
                    }
                    alternatives[alt_idx].enabled = !alternatives[alt_idx].enabled;
                    // Update the step's pattern_template from the modified segments
                    let new_template =
                        crate::pattern::rebuild_pattern(&app.step_detail_segments);
                    if let Some(step_idx) = app.step_detail_index {
                        app.steps[step_idx].pattern_template = new_template;
                    }
                    app.dirty = true;
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
        InputMode::EditPattern(text, cursor, error) => {
            match text_edit(text, cursor, code) {
                TextEditResult::Submit(_) => {
                    match validate_pattern_template(text) {
                        Ok(()) => {
                            let new_template = text.clone();
                            if let Some(step_idx) = app.step_detail_index {
                                app.steps[step_idx].pattern_template = new_template;
                                app.step_detail_segments = crate::pattern::parse_pattern(
                                    &app.steps[step_idx].pattern_template,
                                );
                                app.step_detail_selected = 0;
                                app.step_detail_alt_selected = None;
                                app.dirty = true;
                            }
                            app.input_mode = InputMode::Normal;
                        }
                        Err(msg) => {
                            *error = Some(msg);
                        }
                    }
                }
                TextEditResult::Cancel => { app.input_mode = InputMode::Normal; }
                TextEditResult::Continue => { *error = None; }
            }
        }
        InputMode::Normal | InputMode::AddStep(_, _) => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Wizard handler
// ---------------------------------------------------------------------------

/// Handle text editing keys for wizard text-input states.
/// Returns Some(final_text) if Enter was pressed on valid input, None otherwise.
fn handle_wizard_text_edit(
    app: &mut App,
    code: KeyCode,
    text: &str,
    cursor: usize,
    insert_idx: usize,
    make_state: impl Fn(String, usize, Option<String>) -> WizardState,
    validate: bool,
) -> Option<String> {
    match code {
        KeyCode::Enter => {
            if validate {
                match validate_pattern_template(text) {
                    Ok(()) => return Some(text.to_string()),
                    Err(msg) => {
                        app.input_mode = InputMode::AddStep(
                            make_state(text.to_string(), cursor, Some(msg)),
                            insert_idx,
                        );
                        return None;
                    }
                }
            }
            return Some(text.to_string());
        }
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Left => {
            let new_cursor = if cursor > 0 { cursor - 1 } else { 0 };
            app.input_mode = InputMode::AddStep(make_state(text.to_string(), new_cursor, None), insert_idx);
        }
        KeyCode::Right => {
            let new_cursor = if cursor < text.len() { cursor + 1 } else { cursor };
            app.input_mode = InputMode::AddStep(make_state(text.to_string(), new_cursor, None), insert_idx);
        }
        KeyCode::Char(c) => {
            let mut t = text.to_string();
            t.insert(cursor, c);
            app.input_mode = InputMode::AddStep(make_state(t, cursor + 1, None), insert_idx);
        }
        KeyCode::Backspace => {
            if cursor > 0 {
                let mut t = text.to_string();
                t.remove(cursor - 1);
                app.input_mode = InputMode::AddStep(make_state(t, cursor - 1, None), insert_idx);
            }
        }
        _ => {}
    }
    None
}

fn handle_wizard_key(app: &mut App, code: KeyCode) {
    let (wizard, insert_idx) = match &app.input_mode {
        InputMode::AddStep(w, idx) => (w.clone(), *idx),
        _ => return,
    };

    match wizard {
        WizardState::PickType(selected) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickType((selected + 1) % STEP_TYPES.len()),
                    insert_idx,
                );
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickType(if selected == 0 { STEP_TYPES.len() - 1 } else { selected - 1 }),
                    insert_idx,
                );
            }
            KeyCode::Enter => {
                app.wizard_acc.step_type = STEP_TYPES[selected].to_string();
                let next = match STEP_TYPES[selected] {
                    "extract" | "rewrite" => WizardState::Pattern(String::new(), 0, None),
                    "standardize" => WizardState::PickTarget(0),
                    _ => return,
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::Pattern(text, cursor, _) => {
            if let Some(pattern) = handle_wizard_text_edit(
                app, code, &text, cursor, insert_idx,
                |t, c, e| WizardState::Pattern(t, c, e),
                true,
            ) {
                app.wizard_acc.pattern = pattern;
                let next = match app.wizard_acc.step_type.as_str() {
                    "extract" => WizardState::ExtractMode(0),
                    "rewrite" => WizardState::RewriteMode(0),
                    "standardize" => WizardState::Replacement(String::new(), 0),
                    _ => return,
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
        }

        WizardState::ExtractMode(selected) => {
            const EXTRACT_MODES: &[&str] = &["Single target", "Multiple targets"];
            match code {
                KeyCode::Down | KeyCode::Char('j') => {
                    app.input_mode = InputMode::AddStep(
                        WizardState::ExtractMode((selected + 1) % EXTRACT_MODES.len()),
                        insert_idx,
                    );
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.input_mode = InputMode::AddStep(
                        WizardState::ExtractMode(if selected == 0 { EXTRACT_MODES.len() - 1 } else { selected - 1 }),
                        insert_idx,
                    );
                }
                KeyCode::Enter => {
                    let next = if selected == 0 {
                        WizardState::PickTarget(0)
                    } else {
                        WizardState::PickTargets(0, vec![0; TARGET_FIELDS.len()])
                    };
                    app.input_mode = InputMode::AddStep(next, insert_idx);
                }
                KeyCode::Esc => { app.input_mode = InputMode::Normal; }
                _ => {}
            }
        }

        WizardState::PickTargets(selected, groups) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTargets((selected + 1) % TARGET_FIELDS.len(), groups),
                    insert_idx,
                );
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTargets(if selected == 0 { TARGET_FIELDS.len() - 1 } else { selected - 1 }, groups),
                    insert_idx,
                );
            }
            KeyCode::Char(' ') => {
                // Toggle: if already assigned, clear it; otherwise prompt for group number
                let mut groups = groups;
                if groups[selected] > 0 {
                    groups[selected] = 0;
                    app.input_mode = InputMode::AddStep(
                        WizardState::PickTargets(selected, groups),
                        insert_idx,
                    );
                } else {
                    app.input_mode = InputMode::AddStep(
                        WizardState::TargetGroup(selected, groups, String::new(), 0),
                        insert_idx,
                    );
                }
            }
            KeyCode::Enter => {
                // Finalize: build targets map from non-zero entries
                let mut targets = HashMap::new();
                for (i, &g) in groups.iter().enumerate() {
                    if g > 0 {
                        targets.insert(TARGET_FIELDS[i].0.to_string(), g);
                    }
                }
                if targets.is_empty() {
                    return; // Must select at least one target
                }
                app.wizard_acc.targets = Some(targets);
                app.input_mode = InputMode::AddStep(WizardState::SkipIfFilled, insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::TargetGroup(field_idx, groups, text, cursor) => match code {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let mut text = text;
                text.insert(cursor, c);
                app.input_mode = InputMode::AddStep(
                    WizardState::TargetGroup(field_idx, groups, text, cursor + 1),
                    insert_idx,
                );
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    let mut text = text;
                    text.remove(cursor - 1);
                    app.input_mode = InputMode::AddStep(
                        WizardState::TargetGroup(field_idx, groups, text, cursor - 1),
                        insert_idx,
                    );
                }
            }
            KeyCode::Enter => {
                if let Ok(n) = text.parse::<usize>() {
                    if n > 0 {
                        let mut groups = groups;
                        groups[field_idx] = n;
                        app.input_mode = InputMode::AddStep(
                            WizardState::PickTargets(field_idx, groups),
                            insert_idx,
                        );
                    }
                }
            }
            KeyCode::Esc => {
                // Cancel group entry, go back to PickTargets
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTargets(field_idx, groups),
                    insert_idx,
                );
            }
            _ => {}
        },

        WizardState::PickTarget(selected) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTarget((selected + 1) % TARGET_FIELDS.len()),
                    insert_idx,
                );
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTarget(if selected == 0 { TARGET_FIELDS.len() - 1 } else { selected - 1 }),
                    insert_idx,
                );
            }
            KeyCode::Enter => {
                app.wizard_acc.target = Some(TARGET_FIELDS[selected].0.to_string());
                let next = match app.wizard_acc.step_type.as_str() {
                    "extract" => WizardState::SkipIfFilled,
                    "standardize" => WizardState::StandardizeMode(0),
                    _ => return,
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::SkipIfFilled => match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.wizard_acc.skip_if_filled = Some(true);
                app.input_mode = InputMode::AddStep(
                    WizardState::Replacement(String::new(), 0), insert_idx,
                );
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                app.wizard_acc.skip_if_filled = Some(false);
                app.input_mode = InputMode::AddStep(
                    WizardState::Replacement(String::new(), 0), insert_idx,
                );
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::Replacement(text, cursor) => {
            if let Some(repl) = handle_wizard_text_edit(
                app, code, &text, cursor, insert_idx,
                |t, c, _| WizardState::Replacement(t, c),
                false,
            ) {
                app.wizard_acc.replacement = if repl.is_empty() { None } else { Some(repl) };
                let suggestion = format!("custom_{}_{}", app.wizard_acc.step_type,
                    app.wizard_acc.target.as_deref().unwrap_or("general"));
                app.input_mode = InputMode::AddStep(
                    WizardState::Label(suggestion.clone(), suggestion.len()), insert_idx,
                );
            }
        }

        WizardState::RewriteMode(selected) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(WizardState::RewriteMode((selected + 1) % 2), insert_idx);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(WizardState::RewriteMode(if selected == 0 { 1 } else { 0 }), insert_idx);
            }
            KeyCode::Enter => {
                let next = if selected == 0 {
                    WizardState::Replacement(String::new(), 0)
                } else {
                    WizardState::TableName(String::new(), 0)
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::TableName(text, cursor) => {
            if let Some(name) = handle_wizard_text_edit(
                app, code, &text, cursor, insert_idx,
                |t, c, _| WizardState::TableName(t, c),
                false,
            ) {
                app.wizard_acc.table = Some(name);
                let suggestion = format!("custom_rewrite_{}", app.wizard_acc.table.as_deref().unwrap_or("general"));
                app.input_mode = InputMode::AddStep(
                    WizardState::Label(suggestion.clone(), suggestion.len()), insert_idx,
                );
            }
        }

        WizardState::StandardizeMode(selected) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(WizardState::StandardizeMode((selected + 1) % 2), insert_idx);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(WizardState::StandardizeMode(if selected == 0 { 1 } else { 0 }), insert_idx);
            }
            KeyCode::Enter => {
                let next = if selected == 0 {
                    WizardState::Pattern(String::new(), 0, None)
                } else {
                    let names: Vec<String> = TABLE_DESCRIPTIONS.iter().map(|(n, _)| n.to_string()).collect();
                    WizardState::PickTable(0, names)
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::PickTable(selected, names) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTable((selected + 1) % names.len(), names),
                    insert_idx,
                );
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(
                    WizardState::PickTable(if selected == 0 { names.len() - 1 } else { selected - 1 }, names),
                    insert_idx,
                );
            }
            KeyCode::Enter => {
                app.wizard_acc.table = Some(names[selected].clone());
                app.input_mode = InputMode::AddStep(WizardState::WordMode(0), insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::WordMode(selected) => match code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.input_mode = InputMode::AddStep(WizardState::WordMode((selected + 1) % 2), insert_idx);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.input_mode = InputMode::AddStep(WizardState::WordMode(if selected == 0 { 1 } else { 0 }), insert_idx);
            }
            KeyCode::Enter => {
                app.wizard_acc.mode = if selected == 1 { Some("per_word".to_string()) } else { None };
                let suggestion = format!("custom_standardize_{}", app.wizard_acc.target.as_deref().unwrap_or("general"));
                app.input_mode = InputMode::AddStep(
                    WizardState::Label(suggestion.clone(), suggestion.len()), insert_idx,
                );
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::Label(text, cursor) => {
            if let Some(label) = handle_wizard_text_edit(
                app, code, &text, cursor, insert_idx,
                |t, c, _| WizardState::Label(t, c),
                false,
            ) {
                if label.is_empty() { return; }
                if app.steps.iter().any(|s| s.label == label) { return; }

                let acc = &app.wizard_acc;
                let def = crate::step::StepDef {
                    step_type: acc.step_type.clone(),
                    label: label.clone(),
                    pattern: if acc.pattern.is_empty() { None } else { Some(acc.pattern.clone()) },
                    table: acc.table.clone(),
                    target: if acc.targets.is_some() { None } else { acc.target.clone() },
                    replacement: acc.replacement.clone(),
                    skip_if_filled: acc.skip_if_filled,
                    source: None,
                    matching_table: None,
                    format_table: None,
                    mode: acc.mode.clone(),
                    targets: acc.targets.clone(),
                };

                let step_state = StepState {
                    label: label.clone(),
                    group: acc.step_type.clone(),
                    action_desc: acc.step_type.clone(),
                    pattern_template: acc.pattern.clone(),
                    enabled: true,
                    default_enabled: true,
                    is_custom: true,
                };

                app.steps.insert(insert_idx, step_state);
                app.steps_list_state.select(Some(insert_idx));
                app.custom_step_defs.insert(label, def);
                app.dirty = true;
                app.input_mode = InputMode::Normal;
            }
        }
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
        Tab::Steps => render_steps(frame, app, content_area),
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
        InputMode::EditPattern(_, _, _) => {
            // Pattern editing is rendered inline in the step detail view
        }
        InputMode::AddStep(wizard, _) => {
            render_wizard(frame, wizard, &app.wizard_acc);
        }
    }

    // Delete confirmation overlay
    if let Some(del_idx) = app.confirm_delete {
        if del_idx < app.steps.len() {
            let label = &app.steps[del_idx].label;
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
    if app.step_detail_index.is_some() {
        render_step_detail(frame, app, area);
        return;
    }

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
                format!("[+] {:27} ", r.label)
            } else {
                format!("{:30} ", r.label)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", check), check_style),
                Span::styled(label_display, style),
                Span::styled(format!("{:8} ", r.action_desc), if is_moving { style } else { Style::new().fg(Color::DarkGray) }),
                Span::styled(&r.pattern_template, pattern_style),
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

fn render_step_detail(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let step_idx = app.step_detail_index.unwrap();
    let step = &app.steps[step_idx];

    let is_editing = matches!(app.input_mode, InputMode::EditPattern(_, _, _));
    let header_height = if is_editing { 7 } else { 5 };

    let [header_area, segments_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Fill(1),
    ])
    .areas(area);

    // Header: step name, type, action, enabled status
    let header_text = format!(
        " {}  |  group: {}  |  action: {}  |  {}",
        step.label,
        step.group,
        step.action_desc,
        if step.enabled { "enabled" } else { "DISABLED" },
    );

    let mut header_lines = vec![
        Line::from(header_text),
        Line::from(""),
    ];

    if let InputMode::EditPattern(text, cursor, error) = &app.input_mode {
        let (before, after) = text.split_at(*cursor);
        header_lines.push(Line::from(vec![
            Span::styled(" Pattern: ", Style::new().fg(Color::Cyan)),
            Span::styled(before, Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(
                if after.is_empty() { "_".to_string() } else { after[..1].to_string() },
                Style::new().fg(Color::Black).bg(Color::White),
            ),
            Span::styled(
                if after.len() > 1 { &after[1..] } else { "" },
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]));
        if let Some(err_msg) = error {
            header_lines.push(Line::from(""));
            header_lines.push(Line::from(Span::styled(
                format!(" Error: {}", err_msg),
                Style::new().fg(Color::Red),
            )));
        }
    } else {
        header_lines.push(Line::from(Span::styled(
            format!(" Pattern: {}  (e to edit)", step.pattern_template),
            Style::new().fg(Color::DarkGray),
        )));
    }

    let title = if is_editing {
        format!("Step: {} (Enter: confirm, Esc: cancel)", step.label)
    } else {
        format!("Step: {} (Esc to go back)", step.label)
    };
    let header = Paragraph::new(header_lines)
        .block(Block::bordered().title(title));
    frame.render_widget(header, header_area);

    // Segments list
    let mut items: Vec<ListItem> = Vec::new();

    for (seg_idx, segment) in app.step_detail_segments.iter().enumerate() {
        match segment {
            PatternSegment::Literal(text) => {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::new()),
                    Span::styled(text.as_str(), Style::new().fg(Color::DarkGray)),
                ])));
            }
            PatternSegment::TableRef(name) => {
                let is_selected = app.step_detail_alt_selected.is_none()
                    && app.step_detail_selected == seg_idx;
                let style = if is_selected {
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(Color::Cyan)
                };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(if is_selected { "> " } else { "  " }, style),
                    Span::styled(format!("{{{}}}", name), style),
                    Span::styled(
                        "  (edit in Dictionaries tab)",
                        Style::new().fg(Color::DarkGray),
                    ),
                ])));
            }
            PatternSegment::AlternationGroup { alternatives, .. } => {
                let is_group_selected = app.step_detail_alt_selected.is_none()
                    && app.step_detail_selected == seg_idx;
                let group_style = if is_group_selected {
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(Color::Yellow)
                };
                let enabled_count = alternatives.iter().filter(|a| a.enabled).count();
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(
                        if is_group_selected { "> " } else { "  " },
                        group_style,
                    ),
                    Span::styled(
                        format!(
                            "Match group ({}/{} enabled)  (Enter to expand)",
                            enabled_count,
                            alternatives.len()
                        ),
                        group_style,
                    ),
                ])));

                // Show alternatives if this group is drilled into
                if app.step_detail_selected == seg_idx && app.step_detail_alt_selected.is_some() {
                    for (alt_idx, alt) in alternatives.iter().enumerate() {
                        let is_alt_selected = app.step_detail_alt_selected == Some(alt_idx);
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
                                if is_alt_selected {
                                    style.add_modifier(Modifier::BOLD)
                                } else {
                                    style
                                },
                            ),
                        ])));
                    }
                }
            }
        }
    }

    let list = List::new(items).block(Block::bordered().title("Pattern Segments"));
    frame.render_widget(list, segments_area);
}

fn render_wizard(frame: &mut Frame, wizard: &WizardState, acc: &WizardAccumulator) {
    let popup_area = centered_rect(60, 14, frame.area());
    frame.render_widget(ratatui::widgets::Clear, popup_area);

    match wizard {
        WizardState::PickType(selected) => {
            let items: Vec<ListItem> = STEP_TYPES
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    let prefix = if i == *selected { "> " } else { "  " };
                    ListItem::new(format!("{}{}", prefix, t)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add Step — pick type (Enter to select, Esc to cancel)"));
            frame.render_widget(list, popup_area);
        }
        WizardState::Pattern(text, cursor, error) => {
            let title = format!("Add {} — pattern (Enter to continue, Esc to cancel)", acc.step_type);
            render_wizard_text_input(frame, popup_area, &title, text, *cursor, error.as_deref());
        }
        WizardState::ExtractMode(selected) => {
            let options = ["Single target", "Multiple targets (named capture groups)"];
            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    let prefix = if i == *selected { "> " } else { "  " };
                    ListItem::new(format!("{}{}", prefix, label)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add extract — target mode"));
            frame.render_widget(list, popup_area);
        }
        WizardState::PickTarget(selected) => {
            let items: Vec<ListItem> = TARGET_FIELDS
                .iter()
                .enumerate()
                .map(|(i, (_, display))| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    let prefix = if i == *selected { "> " } else { "  " };
                    ListItem::new(format!("{}{}", prefix, display)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add step — target field (Enter to select)"));
            frame.render_widget(list, popup_area);
        }
        WizardState::PickTargets(selected, groups) => {
            let items: Vec<ListItem> = TARGET_FIELDS
                .iter()
                .enumerate()
                .map(|(i, (_, display))| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    let prefix = if i == *selected { "> " } else { "  " };
                    let check = if groups[i] > 0 {
                        format!("[group {}] ", groups[i])
                    } else {
                        "[ ] ".to_string()
                    };
                    ListItem::new(format!("{}{}{}", prefix, check, display)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add extract — targets (Space to toggle, Enter to confirm)"));
            frame.render_widget(list, popup_area);
        }
        WizardState::TargetGroup(_, _, text, cursor) => {
            render_wizard_text_input(
                frame, popup_area,
                "Capture group number for this field (Enter to confirm, Esc to cancel)",
                text, *cursor, None,
            );
        }
        WizardState::SkipIfFilled => {
            let popup = Paragraph::new("Skip if target field already has a value? (y/n)")
                .block(Block::bordered().title("Add step — skip_if_filled"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(popup, popup_area);
        }
        WizardState::Replacement(text, cursor) => {
            render_wizard_text_input(frame, popup_area, "Add step — replacement (Enter to continue, empty = none)", text, *cursor, None);
        }
        WizardState::RewriteMode(selected) => {
            let options = ["Replacement text", "Table-driven"];
            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    ListItem::new(format!("{}{}", if i == *selected { "> " } else { "  " }, t)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add rewrite — replacement mode"));
            frame.render_widget(list, popup_area);
        }
        WizardState::TableName(text, cursor) => {
            render_wizard_text_input(frame, popup_area, "Add step — table name (Enter to continue)", text, *cursor, None);
        }
        WizardState::StandardizeMode(selected) => {
            let options = ["Pattern + replacement", "Table-based"];
            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    ListItem::new(format!("{}{}", if i == *selected { "> " } else { "  " }, t)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add standardize — approach"));
            frame.render_widget(list, popup_area);
        }
        WizardState::PickTable(selected, names) => {
            let items: Vec<ListItem> = names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let desc = TABLE_DESCRIPTIONS
                        .iter()
                        .find(|(n, _)| *n == name.as_str())
                        .map(|(_, d)| *d)
                        .unwrap_or("");
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    let prefix = if i == *selected { "> " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{}{:20}", prefix, name), style),
                        Span::styled(format!("  {}", desc), Style::new().fg(Color::DarkGray)),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Pick table for standardization (Enter to select)"));
            frame.render_widget(list, popup_area);
        }
        WizardState::WordMode(selected) => {
            let options = ["Whole field", "Per word"];
            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let style = if i == *selected {
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    ListItem::new(format!("{}{}", if i == *selected { "> " } else { "  " }, t)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title("Add standardize — word mode"));
            frame.render_widget(list, popup_area);
        }
        WizardState::Label(text, cursor) => {
            render_wizard_text_input(frame, popup_area, "Add step — label (Enter to create)", text, *cursor, None);
        }
    }
}

fn render_wizard_text_input(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    title: &str,
    text: &str,
    cursor: usize,
    error: Option<&str>,
) {
    let (before, after) = text.split_at(cursor.min(text.len()));
    let mut lines = vec![Line::from(vec![
        Span::styled(before, Style::new().fg(Color::White)),
        Span::styled(
            if after.is_empty() { "_".to_string() } else { after[..1].to_string() },
            Style::new().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(
            if after.len() > 1 { &after[1..] } else { "" },
            Style::new().fg(Color::White),
        ),
    ])];
    if let Some(err) = error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Error: {}", err),
            Style::new().fg(Color::Red),
        )));
    }
    let popup = Paragraph::new(lines)
        .block(Block::bordered().title(title))
        .style(Style::new().bg(Color::Black).fg(Color::Cyan));
    frame.render_widget(popup, area);
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
fn validate_pattern_template(template: &str) -> Result<(), String> {
    let tables = build_default_tables();
    let expanded = crate::step::expand_template(template, &tables);
    fancy_regex::Regex::new(&expanded)
        .map(|_| ())
        .map_err(|e| format!("{}", e))
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
            assert!(config.steps.disabled.contains(&app.steps[0].label));
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
    fn test_to_config_pattern_override() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.steps.is_empty() {
            // Modify a step's pattern template
            let original = app.steps[0].pattern_template.clone();
            app.steps[0].pattern_template = "MODIFIED_PATTERN".to_string();
            let config = app.to_config();
            assert!(config.steps.pattern_overrides.contains_key(&app.steps[0].label));
            assert_eq!(
                config.steps.pattern_overrides.get(&app.steps[0].label).unwrap(),
                "MODIFIED_PATTERN"
            );

            // Restore to default — should NOT appear in overrides
            app.steps[0].pattern_template = original;
            let config = app.to_config();
            assert!(!config.steps.pattern_overrides.contains_key(&app.steps[0].label));
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
