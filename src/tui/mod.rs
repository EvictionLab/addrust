mod meta;
mod panel;
pub(crate) mod tabs;
mod widgets;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};
use ratatui::{DefaultTerminal, Frame};

use crate::config::{Config, DictEntry, DictOverrides};
use crate::tables::abbreviations::build_default_tables;

use tabs::{
    DictGroupState, FormState, GroupStatus, InputMode, OutputSettingState, StepState,
    handle_dict_key, handle_form_key, handle_input_mode, handle_output_key, handle_rules_key,
    render_text_with_cursor,
};

/// Which top-level tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Steps,
    Dictionaries,
    Output,
}

/// Full TUI application state.
pub(crate) struct App {
    /// Path to save config to.
    pub(crate) config_path: PathBuf,
    /// Whether there are unsaved changes.
    pub(crate) dirty: bool,
    /// Whether we're showing the quit confirmation prompt.
    pub(crate) show_quit_prompt: bool,

    // -- Top-level navigation --
    pub(crate) active_tab: Tab,

    // -- Rules tab --
    pub(crate) steps: Vec<StepState>,
    pub(crate) steps_list_state: ratatui::widgets::TableState,
    /// If Some, we're in move mode — value is the index of the step being moved.
    pub(crate) moving_step: Option<usize>,
    /// Original index before move started, for Esc cancel.
    pub(crate) moving_step_origin: Option<usize>,

    // -- Dictionaries tab --
    pub(crate) table_names: Vec<String>,
    pub(crate) dict_tab_index: usize,
    /// Dictionary entries per table, with change tracking.
    pub(crate) dict_entries: Vec<Vec<DictGroupState>>,
    pub(crate) dict_list_state: ratatui::widgets::TableState,
    pub(crate) input_mode: InputMode,

    // -- Output tab --
    pub(crate) output_settings: Vec<OutputSettingState>,
    pub(crate) output_list_state: ratatui::widgets::TableState,

    // -- Step editor form --
    /// Step editor form state (when open).
    pub(crate) form_state: Option<FormState>,
    /// If Some, we're showing delete confirmation for a custom step at this index.
    pub(crate) confirm_delete: Option<usize>,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let config = Config::load(&config_path);
        let default_tables = build_default_tables();

        // Parse default step definitions
        let toml_str = include_str!("../../data/defaults/steps.toml");
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

        let mut steps_list_state = ratatui::widgets::TableState::default();
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

        let mut dict_list_state = ratatui::widgets::TableState::default();
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
        let mut output_list_state = ratatui::widgets::TableState::default();
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
        let toml_str = include_str!("../../data/defaults/steps.toml");
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

    pub(crate) fn current_dict_entries(&self) -> &[DictGroupState] {
        &self.dict_entries[self.dict_tab_index]
    }

    pub(crate) fn current_dict_entries_mut(&mut self) -> &mut Vec<DictGroupState> {
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
    let tabs_widget = Tabs::new(tab_titles)
        .block(Block::bordered().title("addrust configure"))
        .select(selected_tab)
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ");
    frame.render_widget(tabs_widget, tabs_area);

    // Content
    match app.active_tab {
        Tab::Steps => {
            tabs::render_steps(frame, app, content_area);
            if app.form_state.is_some() {
                tabs::render_step_form(frame, app, content_area);
            }
        }
        Tab::Dictionaries => tabs::render_dict(frame, app, content_area),
        Tab::Output => tabs::render_output(frame, app, content_area),
    }

    // Status bar
    let dirty_indicator = if app.dirty { " [modified]" } else { "" };
    let status_text = if app.moving_step.is_some() {
        format!(" Up/Down: move | Enter: confirm | Esc: cancel{}", dirty_indicator)
    } else {
        format!(
            " Tab: switch | Up/Down: navigate | Space: toggle | m: move | a: add | d: delete | Enter: edit | s: save | q: quit{}",
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn centered_rect_pct(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let [_, center_v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
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

pub(crate) fn centered_rect(
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
    use crossterm::event::KeyCode;

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
