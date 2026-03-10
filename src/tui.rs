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
    /// Adding a new entry: typing the short form.
    AddShort(String),
    /// Adding a new entry: short form done, typing the long form.
    AddLong(String, String),
    /// Editing an existing entry's long form: (index, current_text).
    EditLong(usize, String),
    /// Editing a step's pattern template: (text, cursor position, optional validation error).
    EditPattern(String, usize, Option<String>),
}

/// A per-component output format setting.
#[derive(Debug, Clone)]
struct OutputSettingState {
    component: String,
    format: crate::config::OutputFormat,
    default_format: crate::config::OutputFormat,
    example_short: String,
    example_long: String,
}

/// A dictionary entry with its change status.
#[derive(Debug, Clone)]
struct DictEntryState {
    short: String,
    long: String,
    status: EntryStatus,
    /// Original long form (for overrides).
    original_long: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryStatus {
    Default,
    Added,
    Removed,
    Overridden,
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
}

impl StepState {
    fn from_step_summaries(
        current: &crate::pipeline::StepSummary,
        default: &crate::pipeline::StepSummary,
    ) -> Self {
        Self {
            label: current.label.clone(),
            group: current.step_type.clone(),
            action_desc: current.step_type.clone(),
            pattern_template: current.pattern_template.clone().unwrap_or_default(),
            enabled: current.enabled,
            default_enabled: default.enabled,
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
    dict_entries: Vec<Vec<DictEntryState>>,
    dict_list_state: ListState,
    input_mode: InputMode,

    // -- Output tab --
    output_settings: Vec<OutputSettingState>,
    output_list_state: ListState,
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
                }
            })
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

        let dict_entries: Vec<Vec<DictEntryState>> = table_names
            .iter()
            .map(|name| {
                let table = default_tables.get(name).unwrap();
                let overrides = config.dictionaries.get(name);

                let mut entries: Vec<DictEntryState> = table
                    .entries
                    .iter()
                    .map(|e| {
                        let mut status = EntryStatus::Default;
                        let mut long = e.long.clone();
                        let mut original_long = None;

                        if let Some(ov) = overrides {
                            // Check if removed
                            let is_removed = ov.remove.iter().any(|r| {
                                let upper = r.to_uppercase();
                                e.short == upper || e.long == upper
                            });
                            if is_removed {
                                status = EntryStatus::Removed;
                            }

                            // Check if overridden
                            for o in &ov.override_entries {
                                if o.short.to_uppercase() == e.short {
                                    original_long = Some(e.long.clone());
                                    long = o.long.to_uppercase();
                                    status = EntryStatus::Overridden;
                                }
                            }
                        }

                        DictEntryState {
                            short: e.short.clone(),
                            long,
                            status,
                            original_long,
                        }
                    })
                    .collect();

                // Append added entries from config
                if let Some(ov) = overrides {
                    for add in &ov.add {
                        entries.push(DictEntryState {
                            short: add.short.to_uppercase(),
                            long: add.long.to_uppercase(),
                            status: EntryStatus::Added,
                            original_long: None,
                        });
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
                    EntryStatus::Added => {
                        overrides.add.push(DictEntry {
                            short: entry.short.clone(),
                            long: entry.long.clone(),
                        });
                    }
                    EntryStatus::Removed => {
                        overrides.remove.push(entry.long.clone());
                    }
                    EntryStatus::Overridden => {
                        overrides.override_entries.push(DictEntry {
                            short: entry.short.clone(),
                            long: entry.long.clone(),
                        });
                    }
                    EntryStatus::Default => {}
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

    fn current_dict_entries(&self) -> &[DictEntryState] {
        &self.dict_entries[self.dict_tab_index]
    }

    fn current_dict_entries_mut(&mut self) -> &mut Vec<DictEntryState> {
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
                    EntryStatus::Default => {
                        entry.status = EntryStatus::Removed;
                        app.dirty = true;
                    }
                    EntryStatus::Removed => {
                        entry.status = EntryStatus::Default;
                        app.dirty = true;
                    }
                    EntryStatus::Added => {
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
                    EntryStatus::Overridden => {
                        // Revert override to default
                        if let Some(ref orig) = entry.original_long.clone() {
                            entry.long = orig.clone();
                        }
                        entry.original_long = None;
                        entry.status = EntryStatus::Default;
                        app.dirty = true;
                    }
                }
            }
        }
        // Add new entry
        KeyCode::Char('a') => {
            app.input_mode = InputMode::AddShort(String::new());
        }
        // Edit long form
        KeyCode::Enter => {
            if let Some(i) = app.dict_list_state.selected() {
                let entry = &app.current_dict_entries()[i];
                if entry.status != EntryStatus::Removed {
                    app.input_mode = InputMode::EditLong(i, entry.long.clone());
                }
            }
        }
        _ => {}
    }
}

fn handle_input_mode(app: &mut App, code: KeyCode) {
    match &mut app.input_mode {
        InputMode::AddShort(short) => match code {
            KeyCode::Enter => {
                if !short.is_empty() {
                    let s = short.to_uppercase();
                    let is_vl = {
                        let tables = build_default_tables();
                        tables
                            .get(&app.table_names[app.dict_tab_index])
                            .map(|t| t.is_value_list())
                            .unwrap_or(false)
                    };
                    if is_vl {
                        let new_entry = DictEntryState {
                            short: s,
                            long: String::new(),
                            status: EntryStatus::Added,
                            original_long: None,
                        };
                        app.current_dict_entries_mut().push(new_entry);
                        let len = app.current_dict_entries().len();
                        app.dict_list_state.select(Some(len - 1));
                        app.dirty = true;
                        app.input_mode = InputMode::Normal;
                    } else {
                        app.input_mode = InputMode::AddLong(s, String::new());
                    }
                }
            }
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                short.push(c);
            }
            KeyCode::Backspace => {
                short.pop();
            }
            _ => {}
        },
        InputMode::AddLong(short, long) => match code {
            KeyCode::Enter => {
                if !long.is_empty() {
                    let new_entry = DictEntryState {
                        short: short.to_uppercase(),
                        long: long.to_uppercase(),
                        status: EntryStatus::Added,
                        original_long: None,
                    };
                    app.current_dict_entries_mut().push(new_entry);
                    let len = app.current_dict_entries().len();
                    app.dict_list_state.select(Some(len - 1));
                    app.dirty = true;
                }
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                long.push(c);
            }
            KeyCode::Backspace => {
                long.pop();
            }
            _ => {}
        },
        InputMode::EditLong(idx, text) => match code {
            KeyCode::Enter => {
                let idx = *idx;
                let new_long = text.to_uppercase();
                let entry = &mut app.current_dict_entries_mut()[idx];
                if entry.status == EntryStatus::Default {
                    entry.original_long = Some(entry.long.clone());
                    entry.status = EntryStatus::Overridden;
                }
                entry.long = new_long;
                app.dirty = true;
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                text.push(c);
            }
            KeyCode::Backspace => {
                text.pop();
            }
            _ => {}
        },
        InputMode::EditPattern(text, cursor, error) => match code {
            KeyCode::Enter => {
                // Validate: expand table placeholders and try to compile
                match validate_pattern_template(text) {
                    Ok(()) => {
                        let new_template = text.clone();
                        if let Some(step_idx) = app.step_detail_index {
                            app.steps[step_idx].pattern_template = new_template;
                            // Re-parse segments for the detail view
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
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Left => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
            }
            KeyCode::Right => {
                if *cursor < text.len() {
                    *cursor += 1;
                }
            }
            KeyCode::Home => {
                *cursor = 0;
            }
            KeyCode::End => {
                *cursor = text.len();
            }
            KeyCode::Char(c) => {
                *error = None;
                text.insert(*cursor, c);
                *cursor += 1;
            }
            KeyCode::Backspace => {
                *error = None;
                if *cursor > 0 {
                    *cursor -= 1;
                    text.remove(*cursor);
                }
            }
            KeyCode::Delete => {
                *error = None;
                if *cursor < text.len() {
                    text.remove(*cursor);
                }
            }
            _ => {}
        },
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
            " Tab: switch | j/k: navigate | Space: toggle | m: move | Enter: edit | s: save | q: quit{}",
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
        InputMode::AddShort(s) => {
            let popup_area = centered_rect(50, 5, frame.area());
            let display = format!("{}_", s);
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Add entry — short form (Enter to continue, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::AddLong(short, long) => {
            let popup_area = centered_rect(50, 5, frame.area());
            let display = format!("{} -> {}_", short, long);
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Add entry — long form (Enter to confirm, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::EditLong(_, text) => {
            let popup_area = centered_rect(50, 5, frame.area());
            let display = format!("{}_", text);
            let popup = Paragraph::new(display)
                .block(Block::bordered().title("Edit long form (Enter to confirm, Esc to cancel)"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
        InputMode::EditPattern(_, _, _) => {
            // Pattern editing is rendered inline in the step detail view
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
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", check), check_style),
                Span::styled(format!("{:30} ", r.label), style),
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
                    EntryStatus::Default => ("  ", Style::new()),
                    EntryStatus::Added => ("+ ", Style::new().fg(Color::Green)),
                    EntryStatus::Removed => ("- ", Style::new().fg(Color::Red)),
                    EntryStatus::Overridden => ("~ ", Style::new().fg(Color::Yellow)),
                };
                let detail = if let Some(ref orig) = e.original_long {
                    format!(" (was {})", orig)
                } else {
                    String::new()
                };
                if is_value_list {
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(e.short.clone(), style),
                        Span::styled(detail, Style::new().fg(Color::DarkGray)),
                    ]))
                } else {
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(format!("{:20}", e.short), style),
                        Span::styled(" -> ", Style::new().fg(Color::DarkGray)),
                        Span::styled(e.long.clone(), style),
                        Span::styled(detail, Style::new().fg(Color::DarkGray)),
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
            app.dict_entries[0].push(DictEntryState {
                short: "TEST".to_string(),
                long: "TESTING".to_string(),
                status: EntryStatus::Added,
                original_long: None,
            });
            let config = app.to_config();
            let name = &app.table_names[0];
            let overrides = config.dictionaries.get(name).unwrap();
            assert_eq!(overrides.add.len(), 1);
            assert_eq!(overrides.add[0].short, "TEST");
        }
    }

    #[test]
    fn test_to_config_dict_remove() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.dict_entries[0].is_empty() {
            app.dict_entries[0][0].status = EntryStatus::Removed;
            let config = app.to_config();
            let name = &app.table_names[0];
            let overrides = config.dictionaries.get(name).unwrap();
            assert_eq!(overrides.remove.len(), 1);
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
        assert!(matches!(app.input_mode, InputMode::AddShort(_)));

        // Type "TST"
        handle_input_mode(&mut app, KeyCode::Char('T'));
        handle_input_mode(&mut app, KeyCode::Char('S'));
        handle_input_mode(&mut app, KeyCode::Char('T'));
        if let InputMode::AddShort(ref s) = app.input_mode {
            assert_eq!(s, "TST");
        } else {
            panic!("Expected AddShort mode");
        }

        // Press Enter to move to long form
        handle_input_mode(&mut app, KeyCode::Enter);
        assert!(matches!(app.input_mode, InputMode::AddLong(_, _)));

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
        assert_eq!(last.status, EntryStatus::Added);
    }

    #[test]
    fn test_to_config_dict_override() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        if !app.dict_entries[0].is_empty() {
            app.dict_entries[0][0].original_long = Some(app.dict_entries[0][0].long.clone());
            app.dict_entries[0][0].long = "CHANGED".to_string();
            app.dict_entries[0][0].status = EntryStatus::Overridden;
            let config = app.to_config();
            let name = &app.table_names[0];
            let overrides = config.dictionaries.get(name).unwrap();
            assert_eq!(overrides.override_entries.len(), 1);
            assert_eq!(overrides.override_entries[0].long, "CHANGED");
        }
    }
}
