use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs};
use ratatui::{DefaultTerminal, Frame};

use crate::config::{Config, DictEntry, DictOverrides};
use crate::pipeline::Pipeline;
use crate::tables::abbreviations::build_default_tables;

/// Which top-level tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Rules,
    Dictionaries,
}

/// Input mode for dictionary editing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    /// Adding a new entry: typing the short form.
    AddShort(String),
    /// Adding a new entry: short form done, typing the long form.
    AddLong(String, String),
    /// Editing an existing entry's long form: (index, current_text).
    EditLong(usize, String),
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
                    Tab::Rules => handle_rules_key(app, key.code),
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
        KeyCode::Char(' ') | KeyCode::Enter => {
            if let Some(i) = app.rules_list_state.selected() {
                app.rules[i].enabled = !app.rules[i].enabled;
                app.dirty = true;
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
                    app.input_mode = InputMode::AddLong(s, String::new());
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

fn render_dict(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let [subtab_area, entries_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(area);

    // Sub-tabs for table names
    let subtab_titles: Vec<String> = app
        .table_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let count = app.dict_entries[i].len();
            format!("{} ({})", name, count)
        })
        .collect();
    let subtabs = Tabs::new(subtab_titles)
        .block(Block::bordered().title("Tables (left/right to switch)"))
        .select(app.dict_tab_index)
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ");
    frame.render_widget(subtabs, subtab_area);

    // Entries — build items from immutable borrow, then drop it before mutable borrow
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
                ListItem::new(Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(format!("{:20}", e.short), style),
                    Span::styled(" -> ", Style::new().fg(Color::DarkGray)),
                    Span::styled(e.long.clone(), style),
                    Span::styled(detail, Style::new().fg(Color::DarkGray)),
                ]))
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
