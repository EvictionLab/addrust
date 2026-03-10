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
    Rules,
    Dictionaries,
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
    /// Editing a rule's pattern template: (text, cursor position, optional validation error).
    EditPattern(String, usize, Option<String>),
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

/// A rule with its original and current enabled state.
#[derive(Debug, Clone)]
struct RuleState {
    label: String,
    group: String,
    action_desc: String,
    pattern_template: String,
    enabled: bool,
    default_enabled: bool,
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
    rules: Vec<RuleState>,
    rules_list_state: ListState,

    // -- Rule detail view --
    /// If Some, we're viewing/editing a rule's detail (index into rules vec).
    rule_detail_index: Option<usize>,
    /// Parsed pattern segments for the rule being viewed.
    rule_detail_segments: Vec<PatternSegment>,
    /// Which segment is selected (only alternation groups are selectable).
    rule_detail_selected: usize,
    /// If viewing inside an alternation group, which alternative is selected.
    rule_detail_alt_selected: Option<usize>,

    // -- Dictionaries tab --
    table_names: Vec<String>,
    dict_tab_index: usize,
    /// Dictionary entries per table, with change tracking.
    dict_entries: Vec<Vec<DictEntryState>>,
    dict_list_state: ListState,
    input_mode: InputMode,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let config = Config::load(&config_path);
        let default_tables = build_default_tables();
        let pipeline = Pipeline::from_config(&config);

        // Build rule states
        let default_pipeline = Pipeline::default();
        let default_summaries = default_pipeline.rule_summaries();
        let config_summaries = pipeline.rule_summaries();

        let rules: Vec<RuleState> = config_summaries
            .iter()
            .zip(default_summaries.iter())
            .map(|(current, default)| RuleState {
                label: current.label.clone(),
                group: current.group.clone(),
                action_desc: format!("{:?}", current.action),
                pattern_template: current.pattern_template.clone(),
                enabled: current.enabled,
                default_enabled: default.enabled,
            })
            .collect();

        let mut rules_list_state = ListState::default();
        if !rules.is_empty() {
            rules_list_state.select(Some(0));
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

        App {
            config_path,
            dirty: false,
            show_quit_prompt: false,
            active_tab: Tab::Rules,
            rules,
            rules_list_state,
            rule_detail_index: None,
            rule_detail_segments: Vec::new(),
            rule_detail_selected: 0,
            rule_detail_alt_selected: None,
            table_names,
            dict_tab_index: 0,
            dict_entries,
            dict_list_state,
            input_mode: InputMode::Normal,
        }
    }

    /// Build a Config from current TUI state (diff from defaults only).
    fn to_config(&self) -> Config {
        let mut config = Config::default();

        // Rules: collect individually disabled labels
        let disabled: Vec<String> = self
            .rules
            .iter()
            .filter(|r| !r.enabled && r.default_enabled)
            .map(|r| r.label.clone())
            .collect();
        config.rules.disabled = disabled;
        // Per-rule disabled for simplicity; no group-level collapsing
        config.rules.disabled_groups = vec![];

        // Pattern overrides: collect modified templates
        let default_pipeline = Pipeline::default();
        let default_summaries = default_pipeline.rule_summaries();
        for (rule, default) in self.rules.iter().zip(default_summaries.iter()) {
            if rule.pattern_template != default.pattern_template {
                config.rules.pattern_overrides.insert(
                    rule.label.clone(),
                    rule.pattern_template.clone(),
                );
            }
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
                        Tab::Rules => Tab::Dictionaries,
                        Tab::Dictionaries => Tab::Rules,
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
                    Tab::Rules => {
                        if app.rule_detail_index.is_some() {
                            handle_rule_detail_key(app, key.code);
                        } else {
                            handle_rules_key(app, key.code);
                        }
                    }
                    Tab::Dictionaries => handle_dict_key(app, key.code),
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Key handlers
// ---------------------------------------------------------------------------

fn handle_rules_key(app: &mut App, code: KeyCode) {
    let len = app.rules.len();
    if len == 0 {
        return;
    }
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            let i = app.rules_list_state.selected().unwrap_or(0);
            app.rules_list_state.select(Some((i + 1) % len));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let i = app.rules_list_state.selected().unwrap_or(0);
            app.rules_list_state
                .select(Some(if i == 0 { len - 1 } else { i - 1 }));
        }
        KeyCode::Char(' ') => {
            if let Some(i) = app.rules_list_state.selected() {
                app.rules[i].enabled = !app.rules[i].enabled;
                app.dirty = true;
            }
        }
        KeyCode::Enter => {
            if let Some(i) = app.rules_list_state.selected() {
                let segments = crate::pattern::parse_pattern(&app.rules[i].pattern_template);
                app.rule_detail_index = Some(i);
                app.rule_detail_segments = segments;
                app.rule_detail_selected = 0;
                app.rule_detail_alt_selected = None;
            }
        }
        _ => {}
    }
}

fn handle_rule_detail_key(app: &mut App, code: KeyCode) {
    match code {
        // Back to rules list
        KeyCode::Esc | KeyCode::Left => {
            if app.rule_detail_alt_selected.is_some() {
                app.rule_detail_alt_selected = None;
            } else {
                app.rule_detail_index = None;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(alt_idx) = app.rule_detail_alt_selected {
                // Navigate within alternation group
                if let PatternSegment::AlternationGroup { alternatives, .. } =
                    &app.rule_detail_segments[app.rule_detail_selected]
                {
                    if alt_idx + 1 < alternatives.len() {
                        app.rule_detail_alt_selected = Some(alt_idx + 1);
                    }
                }
            } else {
                // Navigate between segments (skip non-actionable ones)
                let len = app.rule_detail_segments.len();
                let mut next = app.rule_detail_selected + 1;
                while next < len {
                    match &app.rule_detail_segments[next] {
                        PatternSegment::AlternationGroup { .. } | PatternSegment::TableRef(_) => {
                            break
                        }
                        _ => next += 1,
                    }
                }
                if next < len {
                    app.rule_detail_selected = next;
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(alt_idx) = app.rule_detail_alt_selected {
                if alt_idx > 0 {
                    app.rule_detail_alt_selected = Some(alt_idx - 1);
                }
            } else {
                // Navigate between segments (skip non-actionable ones)
                let mut prev = app.rule_detail_selected;
                while prev > 0 {
                    prev -= 1;
                    match &app.rule_detail_segments[prev] {
                        PatternSegment::AlternationGroup { .. } | PatternSegment::TableRef(_) => {
                            app.rule_detail_selected = prev;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        // Enter/Right to drill into alternation group
        KeyCode::Enter | KeyCode::Right => {
            if app.rule_detail_alt_selected.is_none() {
                if let PatternSegment::AlternationGroup { .. } =
                    &app.rule_detail_segments[app.rule_detail_selected]
                {
                    app.rule_detail_alt_selected = Some(0);
                }
            }
        }
        // Edit the full pattern template
        KeyCode::Char('e') => {
            if let Some(rule_idx) = app.rule_detail_index {
                let template = app.rules[rule_idx].pattern_template.clone();
                let len = template.len();
                app.input_mode = InputMode::EditPattern(template, len, None);
            }
        }
        // Space to toggle alternative
        KeyCode::Char(' ') => {
            if let Some(alt_idx) = app.rule_detail_alt_selected {
                if let PatternSegment::AlternationGroup { alternatives, .. } =
                    &mut app.rule_detail_segments[app.rule_detail_selected]
                {
                    // Don't allow disabling the last enabled alternative
                    let enabled_count = alternatives.iter().filter(|a| a.enabled).count();
                    if alternatives[alt_idx].enabled && enabled_count <= 1 {
                        return;
                    }
                    alternatives[alt_idx].enabled = !alternatives[alt_idx].enabled;
                    // Update the rule's pattern_template from the modified segments
                    let new_template =
                        crate::pattern::rebuild_pattern(&app.rule_detail_segments);
                    if let Some(rule_idx) = app.rule_detail_index {
                        app.rules[rule_idx].pattern_template = new_template;
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
                        if let Some(rule_idx) = app.rule_detail_index {
                            app.rules[rule_idx].pattern_template = new_template;
                            // Re-parse segments for the detail view
                            app.rule_detail_segments = crate::pattern::parse_pattern(
                                &app.rules[rule_idx].pattern_template,
                            );
                            app.rule_detail_selected = 0;
                            app.rule_detail_alt_selected = None;
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
    let tab_titles = vec!["Rules", "Dictionaries"];
    let selected_tab = match app.active_tab {
        Tab::Rules => 0,
        Tab::Dictionaries => 1,
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
        Tab::Rules => render_rules(frame, app, content_area),
        Tab::Dictionaries => render_dict(frame, app, content_area),
    }

    // Status bar
    let dirty_indicator = if app.dirty { " [modified]" } else { "" };
    let mode_text = match &app.input_mode {
        InputMode::Normal => String::new(),
        InputMode::AddShort(s) => format!(" | Add short form: {}_", s),
        InputMode::AddLong(short, l) => format!(" | Add {} -> {}_", short, l),
        InputMode::EditLong(_, t) => format!(" | Edit long form: {}_", t),
        InputMode::EditPattern(_, _, _) => " | Editing pattern (Enter: confirm, Esc: cancel)".to_string(),
    };
    let status_text = format!(
        " Tab: switch | j/k: navigate | Space: toggle | a: add | d: delete | Enter: edit | s: save | q: quit{}{}",
        dirty_indicator, mode_text
    );
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
}

fn render_rules(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    if app.rule_detail_index.is_some() {
        render_rule_detail(frame, app, area);
        return;
    }

    let items: Vec<ListItem> = app
        .rules
        .iter()
        .map(|r| {
            let check = if r.enabled { " " } else { "x" };
            let style = if !r.enabled {
                Style::new().fg(Color::DarkGray)
            } else if r.enabled != r.default_enabled {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{}] ", check),
                    if r.enabled {
                        Style::new().fg(Color::Green)
                    } else {
                        Style::new().fg(Color::Red)
                    },
                ),
                Span::styled(format!("{:30} ", r.label), style),
                Span::styled(format!("{:8} ", r.action_desc), Style::new().fg(Color::DarkGray)),
                Span::styled(&r.pattern_template, Style::new().fg(Color::DarkGray)),
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

    frame.render_stateful_widget(list, area, &mut app.rules_list_state);
}

fn render_rule_detail(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let rule_idx = app.rule_detail_index.unwrap();
    let rule = &app.rules[rule_idx];

    let is_editing = matches!(app.input_mode, InputMode::EditPattern(_, _, _));
    let header_height = if is_editing { 7 } else { 5 };

    let [header_area, segments_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Fill(1),
    ])
    .areas(area);

    // Header: rule name, group, action, enabled status
    let header_text = format!(
        " {}  |  group: {}  |  action: {}  |  {}",
        rule.label,
        rule.group,
        rule.action_desc,
        if rule.enabled { "enabled" } else { "DISABLED" },
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
            format!(" Pattern: {}  (e to edit)", rule.pattern_template),
            Style::new().fg(Color::DarkGray),
        )));
    }

    let title = if is_editing {
        format!("Rule: {} (Enter: confirm, Esc: cancel)", rule.label)
    } else {
        format!("Rule: {} (Esc to go back)", rule.label)
    };
    let header = Paragraph::new(header_lines)
        .block(Block::bordered().title(title));
    frame.render_widget(header, header_area);

    // Segments list
    let mut items: Vec<ListItem> = Vec::new();

    for (seg_idx, segment) in app.rule_detail_segments.iter().enumerate() {
        match segment {
            PatternSegment::Literal(text) => {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::new()),
                    Span::styled(text.as_str(), Style::new().fg(Color::DarkGray)),
                ])));
            }
            PatternSegment::TableRef(name) => {
                let is_selected = app.rule_detail_alt_selected.is_none()
                    && app.rule_detail_selected == seg_idx;
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
                let is_group_selected = app.rule_detail_alt_selected.is_none()
                    && app.rule_detail_selected == seg_idx;
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
                if app.rule_detail_selected == seg_idx && app.rule_detail_alt_selected.is_some() {
                    for (alt_idx, alt) in alternatives.iter().enumerate() {
                        let is_alt_selected = app.rule_detail_alt_selected == Some(alt_idx);
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
    let expanded = crate::tables::rules::expand_template(template, &tables);
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_config_no_changes() {
        let app = App::new(PathBuf::from("nonexistent.toml"));
        let config = app.to_config();
        assert!(config.rules.disabled.is_empty());
        assert!(config.dictionaries.is_empty());
    }

    #[test]
    fn test_to_config_disabled_rule() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        // Disable first rule
        if !app.rules.is_empty() {
            app.rules[0].enabled = false;
            let config = app.to_config();
            assert!(config.rules.disabled.contains(&app.rules[0].label));
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
        if !app.rules.is_empty() {
            // Modify a rule's pattern template
            let original = app.rules[0].pattern_template.clone();
            app.rules[0].pattern_template = "MODIFIED_PATTERN".to_string();
            let config = app.to_config();
            assert!(config.rules.pattern_overrides.contains_key(&app.rules[0].label));
            assert_eq!(
                config.rules.pattern_overrides.get(&app.rules[0].label).unwrap(),
                "MODIFIED_PATTERN"
            );

            // Restore to default — should NOT appear in overrides
            app.rules[0].pattern_template = original;
            let config = app.to_config();
            assert!(!config.rules.pattern_overrides.contains_key(&app.rules[0].label));
        }
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
