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
use ratatui::widgets::{Block, Paragraph, Tabs};
use ratatui::{DefaultTerminal, Frame};

use crate::config::{Config, DictEntry, DictOverrides};
use crate::tables::abbreviations::load_default_tables;

use tabs::{
    DictGroupState, GroupStatus, OutputSettingState, StepState,
    handle_dict_key, handle_output_key, handle_rules_key,
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

    // -- Output tab --
    pub(crate) output_settings: Vec<OutputSettingState>,
    pub(crate) output_list_state: ratatui::widgets::TableState,

    // -- Dictionaries: cached per-table metadata --
    /// Whether each dictionary table is a value-list (no long forms).
    pub(crate) dict_is_value_list: Vec<bool>,

    // -- Step/dict editor panel --
    /// Panel overlay state (step or dict editor).
    pub(crate) panel: Option<panel::PanelKind>,
    /// If Some, we're showing delete confirmation for a custom step at this index.
    pub(crate) confirm_delete: Option<usize>,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let config = Config::load(&config_path);
        let default_tables = load_default_tables();

        // Parse default step definitions
        let toml_str = include_str!("../../data/defaults/steps.toml");
        let default_defs: crate::step::StepsDef = toml::from_str(toml_str)
            .expect("Failed to parse default steps.toml");

        // Build default StepDef map (before any overrides)
        let default_def_map: std::collections::HashMap<String, crate::step::StepDef> =
            default_defs.step.iter()
                .map(|d| (d.label.clone(), d.clone()))
                .collect();

        // Build current defs with overrides applied.
        // Track original label for each def (None = custom step).
        let mut current_defs: Vec<(Option<String>, crate::step::StepDef)> = default_defs.step.iter()
            .map(|d| {
                let original_label = d.label.clone();
                let mut def = d.clone();
                if let Some(step_override) = config.steps.step_overrides.get(&original_label) {
                    step_override.apply_to(&mut def);
                }
                (Some(original_label), def)
            })
            .collect();

        // Append custom steps
        for custom_def in &config.steps.custom_steps {
            let mut def = custom_def.clone();
            if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
                step_override.apply_to(&mut def);
            }
            current_defs.push((None, def));
        }

        // Apply step_order reordering (uses original labels for default steps)
        if !config.steps.step_order.is_empty() {
            let order = &config.steps.step_order;
            let pos_map: std::collections::HashMap<&str, usize> = order
                .iter().enumerate().map(|(i, label)| (label.as_str(), i)).collect();
            let mut ordered = Vec::new();
            let mut unordered = Vec::new();
            for (orig, def) in current_defs {
                let lookup_label = orig.as_deref().unwrap_or(&def.label);
                if let Some(&pos) = pos_map.get(lookup_label) {
                    ordered.push((pos, orig, def));
                } else {
                    unordered.push((orig, def));
                }
            }
            ordered.sort_by_key(|(pos, _, _)| *pos);
            current_defs = ordered.into_iter().map(|(_, o, d)| (o, d)).collect();
            current_defs.extend(unordered);
        }

        // Build StepState vec
        let steps: Vec<StepState> = current_defs.into_iter().map(|(orig_label, def)| {
            let is_custom = orig_label.is_none();
            let disable_label = orig_label.as_deref().unwrap_or(&def.label);
            let enabled = !config.steps.disabled.contains(&disable_label.to_string());
            StepState {
                enabled,
                is_custom,
                def,
                default_def: orig_label.and_then(|l| default_def_map.get(&l).cloned()),
            }
        }).collect();

        let mut steps_list_state = ratatui::widgets::TableState::default();
        if !steps.is_empty() {
            steps_list_state.select(Some(0));
        }

        // Build dictionary states
        // Filter out suffix_all/suffix_common, add unified "suffix" entry
        let mut table_names: Vec<String> = default_tables
            .table_names()
            .iter()
            .filter(|s| **s != "suffix_all" && **s != "suffix_common")
            .map(|s| s.to_string())
            .collect();

        // Insert "suffix" in sorted position
        let suffix_pos = table_names.partition_point(|n| n.as_str() < "suffix");
        table_names.insert(suffix_pos, "suffix".to_string());

        let dict_is_value_list: Vec<bool> = table_names.iter()
            .map(|name| {
                default_tables.get(name)
                    .map(|t| t.is_value_list())
                    .unwrap_or(false)
            })
            .collect();

        let dict_entries: Vec<Vec<DictGroupState>> = table_names
            .iter()
            .map(|name| {
                // For suffix, build from suffix_source (preserves tags)
                if name == "suffix" {
                    let source_groups = default_tables.suffix_source()
                        .expect("suffix_source should be available");
                    let overrides = config.dictionaries.get("suffix_all");

                    let mut entries: Vec<DictGroupState> = source_groups
                        .iter()
                        .map(|g| {
                            let mut status = GroupStatus::Default;
                            let mut long = g.long.clone();
                            let mut variants = g.variants.clone();

                            if let Some(ov) = overrides {
                                let is_removed = ov.remove.iter().any(|r| {
                                    let upper = r.to_uppercase();
                                    g.short == upper || g.long == upper
                                });
                                if is_removed {
                                    status = GroupStatus::Removed;
                                }
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
                                tags: g.tags.clone(),
                                status,
                                original_short: g.short.clone(),
                                original_long: g.long.clone(),
                                original_variants: g.variants.clone(),
                                original_tags: g.tags.clone(),
                            }
                        })
                        .collect();

                    if let Some(ov) = overrides {
                        for add in &ov.add {
                            let short = add.short.to_uppercase();
                            let long = add.long.to_uppercase();
                            let existing = entries.iter_mut().find(|e| e.short == short);
                            if let Some(e) = existing {
                                for v in &add.variants {
                                    if !e.variants.contains(v) {
                                        e.variants.push(v.clone());
                                    }
                                }
                                e.status = GroupStatus::Modified;
                            } else {
                                entries.push(DictGroupState {
                                    short: short.clone(),
                                    long: long.clone(),
                                    variants: add.variants.clone(),
                                    tags: vec![],
                                    status: GroupStatus::Added,
                                    original_short: short,
                                    original_long: long,
                                    original_variants: Vec::new(),
                                    original_tags: Vec::new(),
                                });
                            }
                        }
                    }

                    return entries;
                }

                // Non-suffix tables
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
                            tags: vec![],
                            status,
                            original_short: g.short.clone(),
                            original_long: g.long.clone(),
                            original_variants: g.variants.clone(),
                            original_tags: vec![],
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
                                if !e.variants.contains(v) {
                                    e.variants.push(v.clone());
                                }
                            }
                            e.status = GroupStatus::Modified;
                        } else {
                            entries.push(DictGroupState {
                                short: short.clone(),
                                long: long.clone(),
                                variants: add.variants.clone(),
                                tags: vec![],
                                status: GroupStatus::Added,
                                original_short: short,
                                original_long: long,
                                original_variants: Vec::new(),
                                original_tags: Vec::new(),
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

        // Build output settings from static metadata
        let output_settings = crate::config::OUTPUT_FIELDS.iter()
            .map(|meta| OutputSettingState {
                format: config.output.get(meta.key),
            })
            .collect::<Vec<_>>();
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
            dict_is_value_list,
            dict_list_state,
            output_settings,
            output_list_state,
            confirm_delete: None,
            panel: None,
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
            if step.is_custom {
                custom_steps.push(step.def.clone());
            } else if let Some(default) = &step.default_def {
                let original_label = &default.label;

                if !step.enabled {
                    disabled.push(original_label.clone());
                }

                // Diff against default, produce StepOverride with only changed fields
                let mut ovr = crate::config::StepOverride::default();
                let mut has_changes = false;
                if step.def.label != default.label {
                    ovr.label = Some(step.def.label.clone()); has_changes = true;
                }
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
                    step_overrides.insert(original_label.clone(), ovr);
                }
            } else {
                // Custom step that's disabled
                if !step.enabled {
                    disabled.push(step.label().to_string());
                }
            }
        }

        // step_order uses original labels for default steps
        let current_order: Vec<String> = self.steps.iter().map(|s| {
            if let Some(default) = &s.default_def {
                default.label.clone()
            } else {
                s.label().to_string()
            }
        }).collect();
        let current_order_refs: Vec<&str> = current_order.iter().map(|s| s.as_str()).collect();
        let emit_order = current_order_refs != default_order;

        config.steps = crate::config::StepsConfig {
            disabled,
            step_overrides,
            step_order: if emit_order { current_order } else { Vec::new() },
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
                // Map TUI's unified "suffix" table to the pipeline's "suffix_all" config key
                let config_key = if name == "suffix" {
                    "suffix_all".to_string()
                } else {
                    name.clone()
                };
                config.dictionaries.insert(config_key, overrides);
            }
        }

        // Output settings
        let mut output = crate::config::OutputConfig::default();
        for (i, setting) in self.output_settings.iter().enumerate() {
            output.set(crate::config::OUTPUT_FIELDS[i].key, setting.format);
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


            // Move mode: only step handler processes keys
            if app.moving_step.is_some() && app.active_tab == Tab::Steps {
                handle_rules_key(app, key.code);
                continue;
            }

            // Panel mode: panel consumes all keys
            if let Some(ref panel_kind) = app.panel {
                match panel_kind {
                    panel::PanelKind::Step(_) => panel::handle_step_panel_key(app, key.code),
                    panel::PanelKind::Dict(_) => panel::handle_dict_panel_key(app, key.code),
                }
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
            if matches!(&app.panel, Some(panel::PanelKind::Step(_))) {
                panel::render_step_panel(frame, app, content_area);
            }
        }
        Tab::Dictionaries => {
            tabs::render_dict(frame, app, content_area);
            if matches!(&app.panel, Some(panel::PanelKind::Dict(_))) {
                panel::render_dict_panel(frame, app, content_area);
            }
        }
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


    // Delete confirmation overlay
    if let Some(del_idx) = app.confirm_delete
        && del_idx < app.steps.len() {
            let label = app.steps[del_idx].label();
            let popup_area = centered_rect(50, 5, frame.area());
            let popup = Paragraph::new(format!("Delete custom step '{}'? (y/n)", label))
                .block(Block::bordered().title("Confirm Delete"))
                .style(Style::new().bg(Color::Black).fg(Color::Yellow));
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            frame.render_widget(popup, popup_area);
        }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
                tags: vec![],
                status: GroupStatus::Added,
                original_short: "TEST".to_string(),
                original_long: "TESTING".to_string(),
                original_variants: Vec::new(),
                original_tags: Vec::new(),
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
        app.active_tab = Tab::Dictionaries;

        // Press 'a' opens panel with inline edit on short form
        handle_dict_key(&mut app, KeyCode::Char('a'));
        assert!(matches!(&app.panel, Some(panel::PanelKind::Dict(_))));

        // Type "TST" then Enter to confirm short form
        panel::handle_dict_panel_key(&mut app, KeyCode::Char('T'));
        panel::handle_dict_panel_key(&mut app, KeyCode::Char('S'));
        panel::handle_dict_panel_key(&mut app, KeyCode::Char('T'));
        panel::handle_dict_panel_key(&mut app, KeyCode::Enter);

        // Move to long form, press Enter to edit
        panel::handle_dict_panel_key(&mut app, KeyCode::Down);
        panel::handle_dict_panel_key(&mut app, KeyCode::Enter);

        // Type "TESTING" then Enter
        for c in "TESTING".chars() {
            panel::handle_dict_panel_key(&mut app, KeyCode::Char(c));
        }
        panel::handle_dict_panel_key(&mut app, KeyCode::Enter);

        // Close panel with Esc
        panel::handle_dict_panel_key(&mut app, KeyCode::Esc);
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

    #[test]
    fn test_step_override_round_trip() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        // Modify a default step's pattern
        let label = app.steps[0].label().to_string();
        app.steps[0].def.pattern = Some("ROUND_TRIP_TEST".to_string());

        // Save to temp file
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        let config = app.to_config();
        config.save(&path).unwrap();

        // Read back the toml to verify override is present
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("ROUND_TRIP_TEST"), "Override not in saved toml:\n{}", saved);

        // Reload and verify the override was applied
        let app2 = App::new(path);
        let step = app2.steps.iter().find(|s| s.label() == label).unwrap();
        assert_eq!(step.def.pattern.as_deref(), Some("ROUND_TRIP_TEST"),
            "Override not applied on reload");
    }

    #[test]
    fn test_step_override_with_custom_step_round_trip() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));

        // Add a custom step
        app.steps.push(StepState {
            enabled: true,
            is_custom: true,
            def: crate::step::StepDef {
                label: "my_custom".to_string(),
                step_type: "extract".to_string(),
                pattern: Some("CUSTOM_PAT".to_string()),
                output_col: Some(crate::step::OutputCol::Single("unit".to_string())),
                ..Default::default()
            },
            default_def: None,
        });

        // Also modify a default step
        let label = app.steps[0].label().to_string();
        app.steps[0].def.pattern = Some("MODIFIED_DEFAULT".to_string());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        let config = app.to_config();
        config.save(&path).unwrap();

        let saved = std::fs::read_to_string(&path).unwrap();
        eprintln!("Saved config:\n{}", saved);

        let app2 = App::new(path);

        // Check the modified default step is in position 0 with override applied
        assert_eq!(app2.steps[0].label(), label);
        assert_eq!(app2.steps[0].def.pattern.as_deref(), Some("MODIFIED_DEFAULT"));

        // Check custom step is present
        let custom = app2.steps.iter().find(|s| s.label() == "my_custom");
        assert!(custom.is_some(), "Custom step not found");
        assert_eq!(custom.unwrap().def.pattern.as_deref(), Some("CUSTOM_PAT"));
    }

    #[test]
    fn test_step_rename_round_trip() {
        let mut app = App::new(PathBuf::from("nonexistent.toml"));
        let original_label = app.steps[0].label().to_string();

        // Rename the first default step
        app.steps[0].def.label = "renamed_step".to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        let config = app.to_config();
        config.save(&path).unwrap();

        // Reload
        let app2 = App::new(path);

        // The step should appear with the new name, in position 0
        assert_eq!(app2.steps[0].label(), "renamed_step");
        // Its default_def should still have the original label
        assert_eq!(app2.steps[0].default_def.as_ref().unwrap().label, original_label);
    }
}
