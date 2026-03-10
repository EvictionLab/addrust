# Rule Detail View Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a drill-down detail view for pipeline rules in the TUI, showing the pattern template with expandable alternation groups whose alternatives can be individually toggled on/off.

**Architecture:** Pressing Enter on a rule in the Rules tab opens a detail view. The view shows the pattern template at top, with auto-detected alternation groups listed below. Each group's alternatives are shown as toggleable items. Table references (`{common_suffix}`) are shown but marked as "edit in Dictionaries tab." Toggling alternatives modifies the pattern template, which is persisted as a `pattern_overrides` map in the config. At rule-build time, overridden templates are compiled instead of defaults.

**Tech Stack:** ratatui (existing), regex pattern parsing (custom, not a full parser — just alternation splitting at the top level of groups)

---

### Task 1: Add pattern_overrides to Config

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs` (inline tests)

**Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `src/config.rs`:

```rust
    #[test]
    fn test_parse_pattern_overrides() {
        let toml_str = r#"
[rules]
disabled = ["unit_pound"]

[rules.pattern_overrides]
unit_type_value = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+)\s*$'
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.disabled, vec!["unit_pound"]);
        let override_val = config.rules.pattern_overrides.get("unit_type_value").unwrap();
        assert!(override_val.contains("\\d+"));
        assert!(!override_val.contains("[A-Z])\\s*$"));
    }

    #[test]
    fn test_serialize_pattern_overrides() {
        let mut config = Config::default();
        config.rules.pattern_overrides.insert(
            "suffix_common".to_string(),
            r"(?<!^)\b({common_suffix})\s*$".to_string(),
        );
        let toml_str = config.to_toml();
        assert!(toml_str.contains("[rules.pattern_overrides]"));
        assert!(toml_str.contains("suffix_common"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert!(parsed.rules.pattern_overrides.contains_key("suffix_common"));
    }

    #[test]
    fn test_empty_pattern_overrides_not_serialized() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert!(!toml_str.contains("pattern_overrides"));
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test config::tests`
Expected: FAIL — `pattern_overrides` field doesn't exist

**Step 3: Add pattern_overrides to RulesConfig**

In `src/config.rs`, add the field to `RulesConfig`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled_groups: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub pattern_overrides: HashMap<String, String>,
}
```

Update `RulesConfig::is_empty()`:

```rust
    pub fn is_empty(&self) -> bool {
        self.disabled.is_empty() && self.disabled_groups.is_empty() && self.pattern_overrides.is_empty()
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test config::tests`
Expected: all tests PASS

**Step 5: Run all tests for regression**

Run: `cargo test`
Expected: all tests PASS

**Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add pattern_overrides to RulesConfig"
```

---

### Task 2: Wire pattern overrides into rule building

**Files:**
- Modify: `src/tables/rules.rs`
- Modify: `src/pipeline.rs`
- Test: `src/pipeline.rs` (inline tests)

**Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `src/pipeline.rs`:

```rust
    #[test]
    fn test_pipeline_from_config_with_pattern_override() {
        let toml_str = r#"
[rules.pattern_overrides]
unit_type_value = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+)\s*$'
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        // Single letter unit should NOT match (removed [A-Z] alternative)
        let addr = p.parse("123 Main St B");
        // B should end up in street name, not unit
        assert!(addr.unit.is_none() || addr.unit.as_deref() != Some("B"));
    }
```

**Step 2: Run tests to verify it fails**

Run: `cargo test pipeline::tests::test_pipeline_from_config_with_pattern_override`
Expected: FAIL — pattern overrides not applied yet

**Step 3: Change build_rules to accept pattern overrides**

In `src/tables/rules.rs`, change the signature:

```rust
pub fn build_rules(abbr: &Abbreviations, pattern_overrides: &std::collections::HashMap<String, String>) -> Vec<Rule> {
```

Update the `rule()` helper to accept an optional override lookup. Add a wrapper that checks overrides:

```rust
use std::collections::HashMap;

fn rule_with_overrides(
    label: &str,
    group: &str,
    pattern: &str,
    pattern_template: &str,
    action: Action,
    target: Option<Field>,
    standardize: Option<(&str, &str)>,
    skip_if_filled: bool,
    overrides: &HashMap<String, String>,
    table_values: &HashMap<&str, &str>,
) -> Rule {
    // Check if there's a pattern override for this rule
    let (final_pattern, final_template) = if let Some(override_template) = overrides.get(label) {
        // Expand table placeholders in the override template
        let mut expanded = override_template.clone();
        for (name, values) in table_values {
            expanded = expanded.replace(&format!("{{{}}}", name), values);
        }
        (expanded, override_template.clone())
    } else {
        (pattern.to_string(), pattern_template.to_string())
    };

    let std = standardize.map(|(m, r)| (Regex::new(m).unwrap(), r.to_string()));
    Rule {
        label: label.to_string(),
        group: group.to_string(),
        pattern: Regex::new(&final_pattern).unwrap_or_else(|e| panic!("Bad regex in rule {}: {}", label, e)),
        pattern_template: final_template,
        action,
        target,
        standardize: std,
        skip_if_filled,
        enabled: true,
    }
}
```

At the top of `build_rules`, build a table_values map from the existing variables:

```rust
    let mut table_values: HashMap<&str, &str> = HashMap::new();
    table_values.insert("direction", &nb_dir);
    table_values.insert("common_suffix", &nb_common_suffix);
    table_values.insert("all_suffix", &nb_all_suffix);
    table_values.insert("unit_type", &nb_unit_type);
    table_values.insert("unit_location", &nb_unit_loc);
    table_values.insert("state", &b_state);
```

Replace all `rule(...)` calls with `rule_with_overrides(..., &pattern_overrides, &table_values)`. Keep the existing `rule()` function but make it private and unused (or remove it).

**Step 4: Update callers of build_rules**

In `src/pipeline.rs`, update `from_config`:

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
        // ...
```

Update `Default for Pipeline`:

```rust
impl Default for Pipeline {
    fn default() -> Self {
        use crate::tables::abbreviations::ABBR;
        use crate::tables::build_rules;

        let rules = build_rules(&ABBR, &std::collections::HashMap::new());
        Self { rules }
    }
}
```

Update `src/tables/mod.rs` if the re-export signature needs updating.

**Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests PASS

**Step 6: Commit**

```bash
git add src/tables/rules.rs src/pipeline.rs src/tables/mod.rs
git commit -m "feat: wire pattern overrides into rule building"
```

---

### Task 3: Add alternation group parser

**Files:**
- Create: `src/pattern.rs`
- Modify: `src/lib.rs` (add `pub mod pattern;`)
- Test: `src/pattern.rs` (inline tests)

This module parses a pattern template string to find alternation groups — sequences of alternatives separated by `|` inside a capturing group `(...)`. It respects nesting (parentheses inside character classes `[...]` are not group boundaries).

**Step 1: Write the failing tests**

In `src/pattern.rs`:

```rust
/// A segment of a pattern template — either literal text or an alternation group.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternSegment {
    /// Literal regex text (not an alternation group).
    Literal(String),
    /// A table placeholder like {common_suffix}.
    TableRef(String),
    /// An alternation group with individually toggleable alternatives.
    AlternationGroup {
        /// The full text of the group including parens, e.g., `(\d+|[A-Z])`.
        full_text: String,
        /// Individual alternatives.
        alternatives: Vec<Alternative>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alternative {
    pub text: String,
    pub enabled: bool,
}

/// Parse a pattern template into segments.
pub fn parse_pattern(template: &str) -> Vec<PatternSegment> {
    todo!()
}

/// Rebuild a pattern template from segments (with disabled alternatives removed).
pub fn rebuild_pattern(segments: &[PatternSegment]) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_literal() {
        let segments = parse_pattern(r"^\d+\b");
        assert_eq!(segments, vec![PatternSegment::Literal(r"^\d+\b".to_string())]);
    }

    #[test]
    fn test_parse_table_ref() {
        let segments = parse_pattern(r"(?<!^)\b({common_suffix})\s*$");
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], PatternSegment::Literal(r"(?<!^)\b(".to_string()));
        assert_eq!(segments[1], PatternSegment::TableRef("common_suffix".to_string()));
        assert_eq!(segments[2], PatternSegment::Literal(r")\s*$".to_string()));
    }

    #[test]
    fn test_parse_alternation_group() {
        let segments = parse_pattern(r"(\d+|[A-Z])");
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            PatternSegment::AlternationGroup { alternatives, .. } => {
                assert_eq!(alternatives.len(), 2);
                assert_eq!(alternatives[0].text, r"\d+");
                assert_eq!(alternatives[1].text, "[A-Z]");
                assert!(alternatives.iter().all(|a| a.enabled));
            }
            _ => panic!("Expected alternation group"),
        }
    }

    #[test]
    fn test_parse_mixed() {
        let segments = parse_pattern(
            r"(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$"
        );
        // Should find: literal, table ref, literal, alternation group, literal
        let alt_groups: Vec<_> = segments.iter().filter(|s| matches!(s, PatternSegment::AlternationGroup { .. })).collect();
        assert_eq!(alt_groups.len(), 1);
        match &alt_groups[0] {
            PatternSegment::AlternationGroup { alternatives, .. } => {
                assert_eq!(alternatives.len(), 4);
                assert_eq!(alternatives[0].text, r"\d+\W?[A-Z]?");
                assert_eq!(alternatives[1].text, r"[A-Z]\W?\d+");
                assert_eq!(alternatives[2].text, r"\d+");
                assert_eq!(alternatives[3].text, "[A-Z]");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_non_capturing_group_no_alternation() {
        // (?:...) without | should be literal, not an alternation group
        let segments = parse_pattern(r"(?:abc)");
        assert_eq!(segments, vec![PatternSegment::Literal(r"(?:abc)".to_string())]);
    }

    #[test]
    fn test_rebuild_all_enabled() {
        let segments = parse_pattern(r"(\d+|[A-Z])");
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, r"(\d+|[A-Z])");
    }

    #[test]
    fn test_rebuild_with_disabled() {
        let mut segments = parse_pattern(r"(\d+|[A-Z])");
        if let PatternSegment::AlternationGroup { alternatives, .. } = &mut segments[0] {
            alternatives[1].enabled = false;
        }
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, r"(\d+)");
    }

    #[test]
    fn test_rebuild_preserves_table_refs() {
        let segments = parse_pattern(r"(?<!^)\b({common_suffix})\s*$");
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, r"(?<!^)\b({common_suffix})\s*$");
    }

    #[test]
    fn test_roundtrip_complex() {
        let template = r"(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$";
        let segments = parse_pattern(template);
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, template);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test pattern::tests`
Expected: FAIL — todo!() panics

**Step 3: Implement the parser**

The parser needs to:
1. Scan the template character by character
2. Track nesting depth (parentheses) and character class context (`[...]`)
3. Recognize `{name}` as table references
4. Recognize groups `(...)` or `(?:...)` that contain `|` as alternation groups
5. Groups without `|` or table refs inside `(...)` are literal

Key implementation details:
- When encountering `{`, scan ahead for `}` — if it matches `{word_chars}`, emit a TableRef
- When encountering `(`, find the matching `)` respecting nesting and `[...]`
- Within a matched group, check for `|` at the top level (not inside nested parens or char classes)
- If `|` found, split into alternatives; otherwise treat as literal
- `(?:`, `(?!`, `(?<!`, `(?=` prefixes are part of the group but don't affect alternation detection

```rust
pub fn parse_pattern(template: &str) -> Vec<PatternSegment> {
    let chars: Vec<char> = template.chars().collect();
    let mut segments = Vec::new();
    let mut i = 0;
    let mut literal_start = 0;

    while i < chars.len() {
        // Check for table reference {name}
        if chars[i] == '{' {
            if let Some(end) = find_table_ref(&chars, i) {
                // Flush literal before this
                if i > literal_start {
                    segments.push(PatternSegment::Literal(
                        chars[literal_start..i].iter().collect(),
                    ));
                }
                let name: String = chars[i + 1..end].iter().collect();
                segments.push(PatternSegment::TableRef(name));
                i = end + 1;
                literal_start = i;
                continue;
            }
        }

        // Check for group (...)
        if chars[i] == '(' {
            if let Some(end) = find_matching_paren(&chars, i) {
                let group_text: String = chars[i..=end].iter().collect();
                let inner = extract_inner(&chars, i, end);

                // Check if inner has top-level alternation (| not inside nested parens/brackets)
                let top_level_alts = split_alternation(&inner);
                if top_level_alts.len() > 1 {
                    // Flush literal before this
                    if i > literal_start {
                        segments.push(PatternSegment::Literal(
                            chars[literal_start..i].iter().collect(),
                        ));
                    }
                    let alternatives = top_level_alts
                        .into_iter()
                        .map(|text| Alternative { text, enabled: true })
                        .collect();
                    segments.push(PatternSegment::AlternationGroup {
                        full_text: group_text,
                        alternatives,
                    });
                    i = end + 1;
                    literal_start = i;
                    continue;
                }
            }
        }

        i += 1;
    }

    // Flush remaining literal
    if literal_start < chars.len() {
        segments.push(PatternSegment::Literal(
            chars[literal_start..].iter().collect(),
        ));
    }

    segments
}

/// Find matching } for a {name} table reference. Returns index of }.
fn find_table_ref(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '}' {
            let name: String = chars[start + 1..i].iter().collect();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(i);
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Find matching ) for a ( at `start`. Respects nesting and character classes.
fn find_matching_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_class = false;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => { i += 1; } // skip escaped char
            '[' if !in_class => { in_class = true; }
            ']' if in_class => { in_class = false; }
            '(' if !in_class => { depth += 1; }
            ')' if !in_class => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract the inner content of a group (skipping the prefix like `(?:`, `(?!`, etc.)
fn extract_inner(chars: &[char], start: usize, end: usize) -> Vec<char> {
    // Skip opening paren and any group modifier (?:, (?!, (?<!, (?=
    let mut i = start + 1;
    if i < end && chars[i] == '?' {
        i += 1;
        while i < end && chars[i] != ':' && chars[i] != ')' && !chars[i].is_alphanumeric() {
            i += 1;
        }
        if i < end && chars[i] == ':' {
            i += 1;
        }
    }
    chars[i..end].to_vec()
}

/// Split content at top-level | (not inside nested parens, char classes, or table refs).
fn split_alternation(content: &[char]) -> Vec<String> {
    let mut alternatives = Vec::new();
    let mut current_start = 0;
    let mut depth = 0;
    let mut in_class = false;
    let mut i = 0;

    while i < content.len() {
        match content[i] {
            '\\' => { i += 1; } // skip escaped char
            '[' if !in_class => { in_class = true; }
            ']' if in_class => { in_class = false; }
            '(' if !in_class => { depth += 1; }
            ')' if !in_class => { depth -= 1; }
            '{' if !in_class && depth == 0 => {
                // Skip over table ref
                while i < content.len() && content[i] != '}' {
                    i += 1;
                }
            }
            '|' if !in_class && depth == 0 => {
                alternatives.push(content[current_start..i].iter().collect());
                current_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    alternatives.push(content[current_start..].iter().collect());
    alternatives
}

pub fn rebuild_pattern(segments: &[PatternSegment]) -> String {
    let mut out = String::new();
    for segment in segments {
        match segment {
            PatternSegment::Literal(text) => out.push_str(text),
            PatternSegment::TableRef(name) => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
            PatternSegment::AlternationGroup { full_text, alternatives } => {
                let enabled: Vec<&str> = alternatives
                    .iter()
                    .filter(|a| a.enabled)
                    .map(|a| a.text.as_str())
                    .collect();
                if enabled.is_empty() {
                    // All disabled — keep the group as a never-match
                    out.push_str("(?:$^)");
                } else {
                    // Reconstruct: find the group prefix (e.g., "(", "(?:", "(?!")
                    let prefix = extract_group_prefix(full_text);
                    out.push_str(&prefix);
                    out.push_str(&enabled.join("|"));
                    out.push(')');
                }
            }
        }
    }
    out
}

/// Extract the prefix of a group: "(", "(?:", "(?!", "(?<!".
fn extract_group_prefix(full_text: &str) -> String {
    let chars: Vec<char> = full_text.chars().collect();
    let mut i = 1; // skip opening (
    if i < chars.len() && chars[i] == '?' {
        i += 1;
        while i < chars.len() && (chars[i] == '<' || chars[i] == '!' || chars[i] == '=') {
            i += 1;
        }
        if i < chars.len() && chars[i] == ':' {
            i += 1;
        }
    }
    chars[..i].iter().collect()
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test pattern::tests`
Expected: all tests PASS

**Step 5: Add `pub mod pattern;` to `src/lib.rs`**

**Step 6: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 7: Commit**

```bash
git add src/pattern.rs src/lib.rs
git commit -m "feat: add pattern template parser for alternation group detection"
```

---

### Task 4: Add rule detail view to TUI

**Files:**
- Modify: `src/tui.rs`

This is the main TUI change. When on the Rules tab and pressing Enter on a rule, the view switches to a detail view showing the pattern parsed into segments, with alternation groups expandable.

**Step 1: Add detail view state to App**

Add to the `App` struct:

```rust
    // -- Rule detail view --
    /// If Some, we're viewing/editing a rule's detail (index into rules vec).
    rule_detail_index: Option<usize>,
    /// Parsed pattern segments for the rule being viewed.
    rule_detail_segments: Vec<crate::pattern::PatternSegment>,
    /// Which segment is selected (only alternation groups are selectable).
    rule_detail_selected: usize,
    /// If viewing inside an alternation group, which alternative is selected.
    rule_detail_alt_selected: Option<usize>,
```

Initialize all to `None`/`0` in `App::new`.

**Step 2: Handle Enter on rules list to open detail view**

In `handle_rules_key`, change Enter to open detail view (keep Space for toggle):

```rust
        KeyCode::Enter => {
            if let Some(i) = app.rules_list_state.selected() {
                let segments = crate::pattern::parse_pattern(&app.rules[i].pattern_template);
                app.rule_detail_index = Some(i);
                app.rule_detail_segments = segments;
                app.rule_detail_selected = 0;
                app.rule_detail_alt_selected = None;
            }
        }
```

**Step 3: Handle navigation and toggling in detail view**

Add a new key handler `handle_rule_detail_key`:

```rust
fn handle_rule_detail_key(app: &mut App, code: KeyCode) {
    match code {
        // Back to rules list
        KeyCode::Esc | KeyCode::Left => {
            app.rule_detail_index = None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(alt_idx) = app.rule_detail_alt_selected {
                // Navigate within alternation group
                if let PatternSegment::AlternationGroup { alternatives, .. } = &app.rule_detail_segments[app.rule_detail_selected] {
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
                        PatternSegment::AlternationGroup { .. } | PatternSegment::TableRef(_) => break,
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
        // Enter/Right to drill into alternation group, or back out
        KeyCode::Enter | KeyCode::Right => {
            if app.rule_detail_alt_selected.is_none() {
                if let PatternSegment::AlternationGroup { .. } = &app.rule_detail_segments[app.rule_detail_selected] {
                    app.rule_detail_alt_selected = Some(0);
                }
            }
        }
        // Space to toggle alternative
        KeyCode::Char(' ') => {
            if let Some(alt_idx) = app.rule_detail_alt_selected {
                if let PatternSegment::AlternationGroup { alternatives, .. } = &mut app.rule_detail_segments[app.rule_detail_selected] {
                    // Don't allow disabling the last enabled alternative
                    let enabled_count = alternatives.iter().filter(|a| a.enabled).count();
                    if alternatives[alt_idx].enabled && enabled_count <= 1 {
                        return;
                    }
                    alternatives[alt_idx].enabled = !alternatives[alt_idx].enabled;
                    // Update the rule's pattern_template from the modified segments
                    let new_template = crate::pattern::rebuild_pattern(&app.rule_detail_segments);
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
```

**Step 4: Wire detail view into the event loop**

In `run_loop`, in the rules tab dispatch, check if detail view is open:

```rust
Tab::Rules => {
    if app.rule_detail_index.is_some() {
        handle_rule_detail_key(app, key.code);
    } else {
        handle_rules_key(app, key.code);
    }
}
```

**Step 5: Render the detail view**

Add `render_rule_detail` function:

```rust
fn render_rule_detail(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let rule_idx = app.rule_detail_index.unwrap();
    let rule = &app.rules[rule_idx];

    let [header_area, segments_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Fill(1),
    ]).areas(area);

    // Header: rule name, group, action, enabled status
    let header_text = format!(
        " {}  |  group: {}  |  action: {}  |  {}",
        rule.label,
        rule.group,
        rule.action_desc,
        if rule.enabled { "enabled" } else { "DISABLED" },
    );
    let header = Paragraph::new(vec![
        Line::from(header_text),
        Line::from(""),
        Line::from(Span::styled(
            format!(" Pattern: {}", rule.pattern_template),
            Style::new().fg(Color::DarkGray),
        )),
    ])
    .block(Block::bordered().title(format!("Rule: {} (Esc to go back)", rule.label)));
    frame.render_widget(header, header_area);

    // Segments list
    let mut items: Vec<ListItem> = Vec::new();
    let mut selectable_indices: Vec<usize> = Vec::new(); // maps list position to segment index

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
                    Span::styled("  (edit in Dictionaries tab)", Style::new().fg(Color::DarkGray)),
                ])));
                selectable_indices.push(seg_idx);
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
                    Span::styled(if is_group_selected { "> " } else { "  " }, group_style),
                    Span::styled(
                        format!("Match group ({}/{} enabled)  (Enter to expand)", enabled_count, alternatives.len()),
                        group_style,
                    ),
                ])));
                selectable_indices.push(seg_idx);

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
                            Span::styled(&alt.text, if is_alt_selected {
                                style.add_modifier(Modifier::BOLD)
                            } else {
                                style
                            }),
                        ])));
                    }
                }
            }
        }
    }

    let list = List::new(items)
        .block(Block::bordered().title("Pattern Segments"));
    frame.render_widget(list, segments_area);
}
```

In `render_rules`, check if detail view is active:

```rust
fn render_rules(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    if app.rule_detail_index.is_some() {
        render_rule_detail(frame, app, area);
        return;
    }
    // ... existing rules list rendering
```

**Step 6: Update to_config to include pattern overrides**

In `App::to_config()`, after collecting disabled rules, add:

```rust
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
```

**Step 7: Verify it compiles**

Run: `cargo check`
Expected: compiles

**Step 8: Manual test**

Run: `cargo run -- configure`
1. Navigate to a rule with alternation groups (e.g., `unit_type_value`)
2. Press Enter — detail view opens
3. Navigate to the match group, press Enter to expand
4. Space to toggle alternatives on/off
5. Esc back to rules list — pattern template should be updated
6. Save with `s`
7. Verify `.addrust.toml` contains `[rules.pattern_overrides]`

**Step 9: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 10: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add rule detail view with alternation group toggling"
```

---

### Task 5: Add unit test for pattern override round-trip through TUI

**Files:**
- Modify: `src/tui.rs` (add to existing tests)

**Step 1: Write the test**

Add to the existing `#[cfg(test)] mod tests` in `src/tui.rs`:

```rust
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
```

**Step 2: Run tests**

Run: `cargo test tui::tests`
Expected: all PASS

**Step 3: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "test: add pattern override round-trip test for TUI"
```
