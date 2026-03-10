# Standardization Pipeline Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix suffix standardization to use a proper canonicalize→format flow, add per-component output format settings (short/long), and make them configurable via config file and TUI.

**Architecture:** Add `OutputConfig` to Config with per-component `OutputFormat` (Short/Long). Pipeline stores patched tables and output config. A general `standardize_value()` function handles canonicalize→format for all components. TUI gets a third "Output" tab for toggling settings.

**Tech Stack:** Rust, serde/toml (config), ratatui (TUI), fancy_regex

---

### Task 1: Add OutputFormat and OutputConfig to config

**Files:**
- Modify: `src/config.rs`

**Step 1: Write the failing tests**

Add to `mod tests` in `src/config.rs`:

```rust
#[test]
fn test_parse_output_config() {
    let toml_str = r#"
[output]
suffix = "short"
direction = "long"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.output.suffix, OutputFormat::Short);
    assert_eq!(config.output.direction, OutputFormat::Long);
    // Unset fields use defaults
    assert_eq!(config.output.unit_type, OutputFormat::Long);
    assert_eq!(config.output.state, OutputFormat::Short);
}

#[test]
fn test_output_config_defaults() {
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.output.suffix, OutputFormat::Long);
    assert_eq!(config.output.direction, OutputFormat::Short);
    assert_eq!(config.output.unit_type, OutputFormat::Long);
    assert_eq!(config.output.unit_location, OutputFormat::Long);
    assert_eq!(config.output.state, OutputFormat::Short);
}

#[test]
fn test_serialize_output_config_only_non_defaults() {
    let mut config = Config::default();
    config.output.suffix = OutputFormat::Short; // non-default
    let toml_str = config.to_toml();
    assert!(toml_str.contains("[output]"));
    assert!(toml_str.contains("suffix"));
    // Default values should not appear
    assert!(!toml_str.contains("direction"));
}

#[test]
fn test_roundtrip_output_config() {
    let mut config = Config::default();
    config.output.suffix = OutputFormat::Short;
    config.output.direction = OutputFormat::Long;
    let toml_str = config.to_toml();
    let parsed: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.output.suffix, OutputFormat::Short);
    assert_eq!(parsed.output.direction, OutputFormat::Long);
    assert_eq!(parsed.output.unit_type, OutputFormat::Long); // default preserved
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib config`
Expected: FAIL — `OutputFormat` and `output` field don't exist

**Step 3: Implement OutputFormat and OutputConfig**

Add to `src/config.rs`, before the `Config` struct:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Short,
    Long,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OutputConfig {
    #[serde(skip_serializing_if = "OutputConfig::is_suffix_default")]
    pub suffix: OutputFormat,
    #[serde(skip_serializing_if = "OutputConfig::is_direction_default")]
    pub direction: OutputFormat,
    #[serde(skip_serializing_if = "OutputConfig::is_unit_type_default")]
    pub unit_type: OutputFormat,
    #[serde(skip_serializing_if = "OutputConfig::is_unit_location_default")]
    pub unit_location: OutputFormat,
    #[serde(skip_serializing_if = "OutputConfig::is_state_default")]
    pub state: OutputFormat,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            suffix: OutputFormat::Long,
            direction: OutputFormat::Short,
            unit_type: OutputFormat::Long,
            unit_location: OutputFormat::Long,
            state: OutputFormat::Short,
        }
    }
}

impl OutputConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn is_suffix_default(&self) -> bool {
        self.suffix == OutputFormat::Long
    }
    fn is_direction_default(&self) -> bool {
        self.direction == OutputFormat::Short
    }
    fn is_unit_type_default(&self) -> bool {
        self.unit_type == OutputFormat::Long
    }
    fn is_unit_location_default(&self) -> bool {
        self.unit_location == OutputFormat::Long
    }
    fn is_state_default(&self) -> bool {
        self.state == OutputFormat::Short
    }
}

impl PartialEq for OutputConfig {
    fn eq(&self, other: &Self) -> bool {
        self.suffix == other.suffix
            && self.direction == other.direction
            && self.unit_type == other.unit_type
            && self.unit_location == other.unit_location
            && self.state == other.state
    }
}
```

Add the `output` field to `Config`:

```rust
pub struct Config {
    #[serde(skip_serializing_if = "RulesConfig::is_empty")]
    pub rules: RulesConfig,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub dictionaries: HashMap<String, DictOverrides>,
    #[serde(skip_serializing_if = "OutputConfig::is_default")]
    pub output: OutputConfig,
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add OutputFormat and OutputConfig to config"
```

---

### Task 2: Fix suffix_usps to be a true 1:1 mapping

**Files:**
- Modify: `src/tables/abbreviations.rs`

**Step 1: Write the failing test**

Add to `mod tests` in `src/tables/abbreviations.rs`:

```rust
#[test]
fn test_suffix_usps_is_one_to_one() {
    let tables = build_default_tables();
    let usps = tables.get("suffix_usps").unwrap();
    // Every short should map to exactly one long
    let mut seen_shorts = std::collections::HashSet::new();
    for entry in &usps.entries {
        // No duplicate shorts (except plurals handled by suffix_all)
        if seen_shorts.contains(&entry.short) {
            panic!("Duplicate short in suffix_usps: {}", entry.short);
        }
        seen_shorts.insert(entry.short.clone());
        // Every entry should have a non-empty long
        assert!(!entry.long.is_empty(), "Empty long for short: {}", entry.short);
    }
}

#[test]
fn test_suffix_usps_bidirectional() {
    let tables = build_default_tables();
    let usps = tables.get("suffix_usps").unwrap();
    // Can go both ways
    assert_eq!(usps.to_long("AVE"), Some("AVENUE"));
    assert_eq!(usps.to_short("AVENUE"), Some("AVE"));
    assert_eq!(usps.to_long("DR"), Some("DRIVE"));
    assert_eq!(usps.to_short("DRIVE"), Some("DR"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib tables::abbreviations::tests::test_suffix_usps`
Expected: FAIL — duplicate shorts in suffix_usps

**Step 3: Fix build_usps_suffixes**

Replace `build_usps_suffixes()` in `src/tables/abbreviations.rs` with a version that produces one entry per USPS short code, using only the primary name (col1) as the long form:

```rust
fn build_usps_suffixes() -> AbbrTable {
    // Parse the USPS CSV into a 1:1 mapping: USPS short ↔ primary name.
    // Each short code gets exactly one long form (the primary suffix name).
    let csv = include_str!("../../data/usps-street-suffix.csv");
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    for line in csv.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() >= 3 {
            let long = cols[0].trim();   // primary name (e.g., AVENUE)
            let short = cols[2].trim();  // USPS standard (e.g., AVE)

            // Skip plurals (handled by suffix_all with distinct codes)
            if long.ends_with('S') && long.len() > 1 {
                let singular = &long[..long.len() - 1];
                if singular == short {
                    continue;
                }
            }

            // One entry per short code — first occurrence wins (primary name)
            if seen.insert(short.to_string()) {
                if short != long {
                    entries.push(abbr(short, long));
                }
            }
        }
    }

    AbbrTable::new(entries)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "fix: make suffix_usps a true 1:1 short↔long mapping"
```

---

### Task 3: Add standardize_value function and refactor finalize

**Files:**
- Modify: `src/pipeline.rs`

**Step 1: Write the failing tests**

Add to `mod tests` in `src/pipeline.rs`:

```rust
#[test]
fn test_suffix_standardize_long_output() {
    // Default output is long for suffixes
    let p = Pipeline::default();
    let addr = p.parse("123 Main Dr");
    assert_eq!(addr.suffix.as_deref(), Some("DRIVE"));
}

#[test]
fn test_suffix_standardize_short_output() {
    let mut config = crate::config::Config::default();
    config.output.suffix = crate::config::OutputFormat::Short;
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Drive");
    assert_eq!(addr.suffix.as_deref(), Some("DR"));
}

#[test]
fn test_suffix_standardize_variant_to_long() {
    // DRIV (a variant) should canonicalize to DR then expand to DRIVE
    let p = Pipeline::default();
    let addr = p.parse("123 Main Driv");
    assert_eq!(addr.suffix.as_deref(), Some("DRIVE"));
}

#[test]
fn test_direction_standardize_short_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 North Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("N"));
}

#[test]
fn test_direction_standardize_long_output() {
    let mut config = crate::config::Config::default();
    config.output.direction = crate::config::OutputFormat::Long;
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 N Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
}

#[test]
fn test_unit_type_standardize_long_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St Apt 4");
    // Unit extraction gives "APT 4" or just "4" — unit type not stored separately.
    // Actually, the unit field contains the raw extracted value.
    // This test validates that state standardization works with config.
}

#[test]
fn test_state_standardize_short_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St, Springfield IL 62701");
    // State extraction gives the state from city_state_zip —
    // but currently it's extracted as part of ExtraBack, not a standalone state field.
    // Skip this for now — state standardization applies when there's a state field.
}
```

Note: Some tests may need adjustment based on what fields actually get populated. The key tests are suffix and direction since those are the main standardization paths.

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib pipeline::tests`
Expected: FAIL — short output tests fail because current code always goes to long/short respectively

**Step 3: Implement the changes**

Add `OutputConfig` and `Abbreviations` to `Pipeline`:

```rust
pub struct Pipeline {
    rules: Vec<Rule>,
    output: crate::config::OutputConfig,
    tables: crate::tables::Abbreviations,
}
```

Add a general `standardize_value` function:

```rust
/// Standardize a value using the two-step canonicalize→format flow.
///
/// 1. Canonicalize: look up the value in `matching_table` to get the USPS short form.
/// 2. Format: based on preference, keep short or expand to long via `canonical_table`.
///
/// For most components, `matching_table` and `canonical_table` are the same.
/// For suffixes, matching uses `suffix_all` and canonical uses `suffix_usps`.
fn standardize_value(
    value: &str,
    matching_table: &crate::tables::abbreviations::AbbrTable,
    canonical_table: &crate::tables::abbreviations::AbbrTable,
    format: crate::config::OutputFormat,
) -> String {
    use crate::config::OutputFormat;

    // Step 1: Canonicalize to short form
    let short = matching_table
        .to_short(value)
        .unwrap_or(value);

    // Step 2: Format based on preference
    match format {
        OutputFormat::Short => short.to_string(),
        OutputFormat::Long => canonical_table
            .to_long(short)
            .unwrap_or(short)
            .to_string(),
    }
}
```

Update `finalize()` to use `standardize_value` and the pipeline's own tables/config instead of `ABBR`:

```rust
fn finalize(&self, state: &mut AddressState) {
    // ... (street name assignment stays the same) ...

    // Standardize directions
    if let Some(ref dir) = state.fields.pre_direction {
        if let Some(table) = self.tables.get("direction") {
            state.fields.pre_direction = Some(
                standardize_value(dir, table, table, self.output.direction)
            );
        }
    }
    if let Some(ref dir) = state.fields.post_direction {
        if let Some(table) = self.tables.get("direction") {
            state.fields.post_direction = Some(
                standardize_value(dir, table, table, self.output.direction)
            );
        }
    }

    // Standardize suffix: canonicalize via suffix_all, format via suffix_usps
    if let Some(ref sfx) = state.fields.suffix {
        let matching = self.tables.get("suffix_all");
        let canonical = self.tables.get("suffix_usps");
        if let (Some(m), Some(c)) = (matching, canonical) {
            state.fields.suffix = Some(
                standardize_value(sfx, m, c, self.output.suffix)
            );
        }
    }

    // ... (unit cleanup, street number promotion, etc. stay the same) ...
}
```

Update `Pipeline::from_config` to store tables and output config:

```rust
pub fn from_config(config: &crate::config::Config) -> Self {
    use crate::tables::abbreviations::build_default_tables;
    use crate::tables::build_rules;

    let tables = build_default_tables();
    let tables = if config.dictionaries.is_empty() {
        tables
    } else {
        tables.patch(&config.dictionaries)
    };

    let rules = build_rules(&tables, &config.rules.pattern_overrides);

    let pipeline_config = PipelineConfig {
        disabled_rules: config.rules.disabled.clone(),
        disabled_groups: config.rules.disabled_groups.clone(),
    };

    let mut pipeline = Self::new(rules, &pipeline_config);
    pipeline.output = config.output.clone();
    pipeline.tables = tables;
    pipeline
}
```

Update `Pipeline::default()` to store default tables:

```rust
impl Default for Pipeline {
    fn default() -> Self {
        use crate::tables::abbreviations::ABBR;
        use crate::tables::build_rules;

        let rules = build_rules(&ABBR, &std::collections::HashMap::new());
        Self {
            rules,
            output: crate::config::OutputConfig::default(),
            tables: ABBR.clone(),
        }
    }
}
```

Update `Pipeline::new` to initialize the new fields:

```rust
pub fn new(mut rules: Vec<Rule>, config: &PipelineConfig) -> Self {
    for rule in &mut rules {
        if config.disabled_rules.contains(&rule.label)
            || config.disabled_groups.contains(&rule.group)
        {
            rule.enabled = false;
        }
    }
    Self {
        rules,
        output: crate::config::OutputConfig::default(),
        tables: crate::tables::abbreviations::ABBR.clone(),
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat: add standardize_value and config-driven output formatting"
```

---

### Task 4: Add unit_type and unit_location standardization

**Files:**
- Modify: `src/pipeline.rs`

Currently unit type and unit location are not standardized in finalize. The unit field contains whatever was extracted (e.g., "APT 4" or just "4" — the type is stripped during extraction). Let's check what actually ends up in the unit field vs what needs standardization.

Actually, looking at the pipeline rules: `unit_type_value` extracts the whole match including the type keyword. The standardize step would need to operate on the extracted unit type specifically. But the current architecture doesn't separate unit type from unit value in the output.

**Step 1: Check what needs standardization**

The unit field stores the extracted value. For "APT 4B", the regex `(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|...)` captures group 2 (the value "4B"), not the type. So the unit type keyword is consumed and lost during extraction.

Similarly, unit_location extracts "UPPER" etc. into the unit field. These could be standardized (UPPR→UPPER or vice versa).

For now, add standardization for the unit field when it contains a unit location value:

```rust
// Standardize unit if it's a unit location value
if let Some(ref unit) = state.fields.unit {
    if let Some(table) = self.tables.get("unit_location") {
        if table.to_short(unit).is_some() || table.to_long(unit).is_some() {
            state.fields.unit = Some(
                standardize_value(unit, table, table, self.output.unit_location)
            );
        }
    }
}
```

Unit type standardization is more complex because the type keyword is lost during extraction. This is a future concern — skip for this plan.

**Step 2: Write test**

```rust
#[test]
fn test_unit_location_standardize_long_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St Rear");
    assert_eq!(addr.unit.as_deref(), Some("REAR"));
}

#[test]
fn test_unit_location_standardize_short() {
    let mut config = crate::config::Config::default();
    config.output.unit_location = crate::config::OutputFormat::Short;
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main St Upper");
    assert_eq!(addr.unit.as_deref(), Some("UPPR"));
}
```

**Step 3: Implement, test, commit**

Run: `cargo test`
Expected: ALL PASS

```bash
git add src/pipeline.rs
git commit -m "feat: add unit_location standardization in finalize"
```

---

### Task 5: Add Output tab to TUI

**Files:**
- Modify: `src/tui.rs`

**Step 1: Add Output variant to Tab enum**

```rust
enum Tab {
    Rules,
    Dictionaries,
    Output,
}
```

**Step 2: Add output settings to App state**

Add to the `App` struct:

```rust
// -- Output tab --
output_settings: Vec<OutputSettingState>,
output_list_state: ListState,
```

Add the setting state struct:

```rust
#[derive(Debug, Clone)]
struct OutputSettingState {
    component: String,
    format: OutputFormat,
    default_format: OutputFormat,
    example_short: String,
    example_long: String,
}
```

**Step 3: Initialize output settings in App::new**

In `App::new()`, after the dictionary initialization, add:

```rust
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
```

**Step 4: Update tab switching**

Update the Tab key handler to cycle through three tabs:

```rust
app.active_tab = match app.active_tab {
    Tab::Rules => Tab::Dictionaries,
    Tab::Dictionaries => Tab::Output,
    Tab::Output => Tab::Rules,
};
```

**Step 5: Add key handler for Output tab**

```rust
Tab::Output => handle_output_key(app, key.code),
```

```rust
fn handle_output_key(app: &mut App, code: KeyCode) {
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
```

**Step 6: Add render function for Output tab**

```rust
fn render_output(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
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
```

**Step 7: Wire into rendering**

Update the tab titles and rendering:

```rust
let tab_titles = vec!["Rules", "Dictionaries", "Output"];
let selected_tab = match app.active_tab {
    Tab::Rules => 0,
    Tab::Dictionaries => 1,
    Tab::Output => 2,
};
```

```rust
match app.active_tab {
    Tab::Rules => render_rules(frame, app, content_area),
    Tab::Dictionaries => render_dict(frame, app, content_area),
    Tab::Output => render_output(frame, app, content_area),
}
```

**Step 8: Update to_config to include output settings**

In `App::to_config()`, add after the dictionaries section:

```rust
// Output settings
use crate::config::OutputConfig;
let mut output = OutputConfig::default();
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
```

**Step 9: Run tests, commit**

Run: `cargo test`
Expected: ALL PASS

```bash
git add src/tui.rs
git commit -m "feat: add Output tab to TUI for per-component format settings"
```

---

### Task 6: Integration tests for output format

**Files:**
- Modify: `tests/config.rs`

**Step 1: Write integration tests**

```rust
#[test]
fn test_output_suffix_short() {
    let config: Config = toml::from_str(
        r#"
[output]
suffix = "short"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Street");
    assert_eq!(addr.suffix.as_deref(), Some("ST"));
}

#[test]
fn test_output_suffix_long_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St");
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}

#[test]
fn test_output_direction_long() {
    let config: Config = toml::from_str(
        r#"
[output]
direction = "long"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 N Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
}

#[test]
fn test_output_direction_short_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 North Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("N"));
}

#[test]
fn test_output_suffix_variant_canonicalizes() {
    // DRIV is a variant of DR — should canonicalize and format
    let config: Config = toml::from_str(
        r#"
[output]
suffix = "short"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Driv");
    assert_eq!(addr.suffix.as_deref(), Some("DR"));
}

#[test]
fn test_output_combined_settings() {
    let config: Config = toml::from_str(
        r#"
[output]
suffix = "short"
direction = "long"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 N Main Drive");
    assert_eq!(addr.suffix.as_deref(), Some("DR"));
    assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
}
```

**Step 2: Run tests**

Run: `cargo test`
Expected: ALL PASS

**Step 3: Commit**

```bash
git add tests/config.rs
git commit -m "test: add integration tests for output format settings"
```
