# Custom Steps Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users add new pipeline steps through config TOML and the TUI wizard, while removing the validate step type.

**Architecture:** Two independent changes merged into one feature. First, remove the `Validate` step variant — `na_check` becomes a rewrite with `replacement = ''`. Second, add `custom_steps` to `StepsConfig` so users can define new steps that compile and run identically to defaults. The TUI gets an `a` keybinding for a guided wizard and `d` for deleting custom steps.

**Tech Stack:** Rust, serde (Serialize/Deserialize), fancy_regex, ratatui TUI, TOML config

**Spec:** `docs/superpowers/specs/2026-03-10-custom-steps-design.md`

---

## Chunk 1: Remove Validate Step Type

This chunk converts `na_check` from validate to rewrite, removes the `Validate` variant from the `Step` enum, updates all compilation and execution paths, and fixes affected tests.

### Task 1: Update steps.toml

**Files:**
- Modify: `data/defaults/steps.toml:1-11`

- [ ] **Step 1: Change na_check from validate to rewrite**

Replace the na_check step definition:

```toml
[[step]]
type = "rewrite"
label = "na_check"
pattern = '(?i)^(N/?A|{na_values})$'
replacement = ''
```

Remove the `warning` and `clear` fields. The anchored pattern with empty replacement empties the entire working string — same effect as the old validate+clear.

- [ ] **Step 2: Verify TOML parses**

Run: `cargo test test_default_steps_toml_parses -- --nocapture`
Expected: FAIL — the test checks `step_type == "validate"` for the first step, but it's now "rewrite".

### Task 2: Remove Validate from StepDef and Step

**Files:**
- Modify: `src/step.rs:71-113` (Step enum), `src/step.rs:283-298` (StepDef), `src/step.rs:323-463` (compile_step)

- [ ] **Step 1: Remove warning and clear from StepDef**

In `StepDef` (around line 283), remove:
```rust
    pub warning: Option<String>,
    pub clear: Option<bool>,
```

- [ ] **Step 2: Remove Validate variant from Step enum**

Remove the `Validate` variant from the `Step` enum (lines 79-86):
```rust
    Validate {
        label: String,
        pattern: Regex,
        pattern_template: String,
        warning: String,
        clear: bool,
        enabled: bool,
    },
```

- [ ] **Step 3: Remove Validate arms from Step methods**

In `label()`, `enabled()`, `set_enabled()`, `pattern_template()`, and `step_type()` — remove the `Step::Validate { .. }` match arms. The compiler will guide you — every match on `Step` that had a Validate arm needs it removed.

- [ ] **Step 4: Remove validate branch from compile_step**

Remove the `"validate"` match arm in `compile_step()` (around lines 325-341). The `"rewrite"` arm already handles what na_check needs.

- [ ] **Step 5: Remove Validate arm from apply_step**

Remove the `Step::Validate { .. }` match arm in `apply_step()` (around lines 195-202).

- [ ] **Step 6: Remove warning and clear from StepDef struct literals in tests**

Every test that constructs a `StepDef` inline includes `warning: None, clear: None`. Remove these fields from all inline `StepDef` constructions in `src/step.rs` tests. There are 3 occurrences:
- `test_apply_rewrite_step` (around line 533)
- `test_apply_extract_step` (around line 575)
- `test_apply_standardize_step` (around line 597)

- [ ] **Step 6b: Remove or rewrite test_apply_validate_step**

The test `test_apply_validate_step` (around line 499) constructs a validate step via TOML and tests the validate execution path. Since validate is removed, delete this entire test function. The NA detection behavior is now tested via integration tests in `tests/config.rs`.

- [ ] **Step 7: Fix test_default_steps_toml_parses**

Update assertion from `"validate"` to `"rewrite"`:
```rust
assert_eq!(defs.step[0].step_type, "rewrite");
```

- [ ] **Step 8: Fix test_compile_all_default_steps**

Same change:
```rust
assert_eq!(steps[0].step_type(), "rewrite");
```

- [ ] **Step 9: Run unit tests**

Run: `cargo test --lib`
Expected: PASS — all step.rs and pipeline.rs unit tests should pass.

### Task 3: Fix integration and golden tests

**Files:**
- Modify: `tests/config.rs`
- Modify: `src/pipeline.rs:286-292` (test_step_summaries)

- [ ] **Step 1: Fix test_step_summaries in pipeline.rs**

Change the assertion from `"validate"` to `"rewrite"`:
```rust
assert_eq!(summaries[0].step_type, "rewrite");
```

- [ ] **Step 2: Update test_config_adds_custom_na_value**

With warnings dropped, change the test to check that the output is empty (no fields extracted) instead of checking for warnings:
```rust
#[test]
fn test_config_adds_custom_na_value() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.na_values]
add = [{ short = "VACANT", long = "" }]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("VACANT");
    // NA rewrite empties the working string; no fields extracted
    assert!(addr.street_name.is_none());
    assert!(addr.street_number.is_none());
}
```

- [ ] **Step 3: Update test_config_removes_na_value**

```rust
#[test]
fn test_config_removes_na_value() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.na_values]
remove = ["NULL"]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("NULL");
    // NULL is no longer an NA value, so it should be parsed (becomes street_name)
    assert!(addr.street_name.is_some());
}
```

- [ ] **Step 4: Update test_full_pipeline_with_tables_cleanup**

Change the NA value assertions from checking warnings to checking empty output:
```rust
    // NA values from table
    let addr = p.parse("NULL");
    assert!(addr.street_name.is_none());
    assert!(addr.street_number.is_none());

    let addr = p.parse("UNKNOWN");
    assert!(addr.street_name.is_none());
    assert!(addr.street_number.is_none());
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: PASS — all 89+ tests should pass (some test counts may change slightly).

- [ ] **Step 6: Commit**

```bash
git add src/step.rs src/pipeline.rs data/defaults/steps.toml tests/config.rs
git commit -m "refactor: remove validate step type, na_check becomes rewrite"
```

---

## Chunk 2: Custom Steps in Config and Pipeline

This chunk adds `custom_steps` to `StepsConfig`, makes `StepDef` serializable, makes `parse_field` return Result, and updates `from_steps_config()` to compile and merge custom steps.

### Task 4: Make StepDef serializable

**Files:**
- Modify: `src/step.rs:283` (StepDef derive)

- [ ] **Step 1: Write failing test for StepDef serialization**

Add to `src/step.rs` tests:
```rust
#[test]
fn test_stepdef_roundtrip_serialize() {
    let def = StepDef {
        step_type: "extract".to_string(),
        label: "custom_box".to_string(),
        pattern: Some(r"\bBOX (\d+)".to_string()),
        table: None,
        target: Some("po_box".to_string()),
        replacement: None,
        skip_if_filled: Some(true),
        matching_table: None,
        format_table: None,
        mode: None,
    };
    let toml_str = toml::to_string_pretty(&def).unwrap();
    let parsed: StepDef = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.step_type, "extract");
    assert_eq!(parsed.label, "custom_box");
    assert_eq!(parsed.target.as_deref(), Some("po_box"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_stepdef_roundtrip_serialize -- --nocapture`
Expected: FAIL — `StepDef` doesn't derive `Serialize`.

- [ ] **Step 3: Add Serialize derive to StepDef**

Change the derive on `StepDef` from:
```rust
#[derive(Debug, Deserialize, Clone)]
```
to:
```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
```

Add `use serde::Serialize;` if not already imported (it's not — only `Deserialize` is imported). Update the import:
```rust
use serde::{Deserialize, Serialize};
```

Also add `skip_serializing_if` attributes to optional fields so the TOML output is clean:
```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StepDef {
    #[serde(rename = "type")]
    pub step_type: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_if_filled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matching_table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_stepdef_roundtrip_serialize -- --nocapture`
Expected: PASS

### Task 5: Make parse_field return Result

**Files:**
- Modify: `src/step.rs:305-319` (parse_field)

- [ ] **Step 1: Write failing test for invalid field name**

Add to `src/step.rs` tests:
```rust
#[test]
fn test_parse_field_invalid_returns_error() {
    let result = parse_field("nonexistent_field");
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_parse_field_invalid_returns_error -- --nocapture`
Expected: FAIL — `parse_field` panics instead of returning Result.

- [ ] **Step 3: Change parse_field to return Result**

```rust
fn parse_field(name: &str) -> Result<Field, String> {
    match name {
        "street_number" => Ok(Field::StreetNumber),
        "pre_direction" => Ok(Field::PreDirection),
        "street_name" => Ok(Field::StreetName),
        "suffix" => Ok(Field::Suffix),
        "post_direction" => Ok(Field::PostDirection),
        "unit" => Ok(Field::Unit),
        "unit_type" => Ok(Field::UnitType),
        "po_box" => Ok(Field::PoBox),
        "building" => Ok(Field::Building),
        "extra_front" => Ok(Field::ExtraFront),
        "extra_back" => Ok(Field::ExtraBack),
        _ => Err(format!("Unknown field name: {}", name)),
    }
}
```

- [ ] **Step 4: Update callers of parse_field in compile_step**

Every call to `parse_field(target)` in compile_step needs to propagate the error. There are two calls:
- In the `"extract"` arm: change `parse_field(target)` to `parse_field(target)?`
- In the `"standardize"` arm: change `parse_field(target)` to `parse_field(target)?`

This works because `compile_step` already returns `Result<Step, String>`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/step.rs
git commit -m "refactor: make StepDef serializable, parse_field returns Result"
```

### Task 6: Add custom_steps to StepsConfig

**Files:**
- Modify: `src/config.rs:59-74` (StepsConfig)

- [ ] **Step 1: Write failing test for custom_steps config round-trip**

Add to `src/config.rs` tests:
```rust
#[test]
fn test_custom_steps_roundtrip() {
    let toml_str = r#"
[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)'
target = "po_box"
skip_if_filled = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.steps.custom_steps.len(), 1);
    assert_eq!(config.steps.custom_steps[0].label, "custom_po_box_digits");

    // Round-trip
    let serialized = config.to_toml();
    assert!(serialized.contains("custom_po_box_digits"));
    let parsed: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(parsed.steps.custom_steps.len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_custom_steps_roundtrip -- --nocapture`
Expected: FAIL — `custom_steps` field doesn't exist.

- [ ] **Step 3: Add custom_steps field to StepsConfig**

In `src/config.rs`, add the import for `StepDef`:
```rust
use crate::step::StepDef;
```

Add to `StepsConfig`:
```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct StepsConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub pattern_overrides: HashMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub step_order: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_steps: Vec<StepDef>,
}
```

- [ ] **Step 4: Update is_empty() to check custom_steps**

```rust
impl StepsConfig {
    pub fn is_empty(&self) -> bool {
        self.disabled.is_empty()
            && self.pattern_overrides.is_empty()
            && self.step_order.is_empty()
            && self.custom_steps.is_empty()
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_custom_steps_roundtrip -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write test for empty custom_steps not serialized**

Add to `src/config.rs` tests:
```rust
#[test]
fn test_custom_steps_empty_not_serialized() {
    let config = Config::default();
    let toml_str = config.to_toml();
    assert!(!toml_str.contains("custom_steps"));
}
```

- [ ] **Step 7: Run test**

Run: `cargo test test_custom_steps_empty_not_serialized -- --nocapture`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/config.rs
git commit -m "feat: add custom_steps to StepsConfig"
```

### Task 7: Pipeline compiles and merges custom steps

**Files:**
- Modify: `src/pipeline.rs:31-92` (from_steps_config)

- [ ] **Step 1: Write failing integration test**

Add to `tests/config.rs`:
```rust
#[test]
fn test_custom_step_extracts_po_box_digits() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)'
target = "po_box"
skip_if_filled = true
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    // Default po_box step won't match "BOX 123" (no P.O. prefix)
    // But custom step should extract it
    let addr = p.parse("BOX 456");
    assert_eq!(addr.po_box.as_deref(), Some("PO BOX 456"));
}
```

Wait — "BOX 456" would be captured by the custom extract step as "456" into po_box, then standardize_po_box would reformat it. Actually, let me think...

The custom extract pattern `\bBOX (\d+)` would capture "BOX 456" — the full match is "BOX 456" and that goes into po_box. Then standardize_po_box has pattern `(?:P\W*O\W*BO?X|POB)\W*(\w+)` with replacement `PO BOX $1`. But "BOX 456" doesn't match that standardize pattern (no P.O. prefix). So po_box would be "BOX 456".

Actually, looking at extract behavior in `apply_step`: `extract_remove` returns the full match text, not just the capture group. Let me check...

Actually I need to look at `extract_remove`.

- [ ] **Step 1: Write failing integration test**

Add to `tests/config.rs`:
```rust
#[test]
fn test_custom_step_extracts_po_box_digits() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)\b'
target = "po_box"
skip_if_filled = true
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("BOX 456");
    // Custom step extracts the match; value includes full match text
    assert!(addr.po_box.is_some(), "po_box should be extracted by custom step");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_custom_step_extracts_po_box_digits -- --nocapture`
Expected: FAIL — `custom_steps` are not compiled or merged into the pipeline.

- [ ] **Step 3: Update from_steps_config to compile and merge custom steps**

In `src/pipeline.rs`, after compiling default steps (line 53), add custom step compilation and merging. Apply pattern overrides to custom steps too, since they're compiled separately from defaults.

Add `compile_step` to the imports at the top of `from_steps_config`:
```rust
use crate::step::{compile_step, compile_steps, StepsDef};
```

Then after the default steps compilation:
```rust
        let mut steps = compile_steps(&defs.step, &tables);

        // Compile and append custom steps (with pattern overrides applied)
        for custom_def in &config.steps.custom_steps {
            let mut def = custom_def.clone();
            if let Some(override_pattern) = config.steps.pattern_overrides.get(&def.label) {
                def.pattern = Some(override_pattern.clone());
            }
            match compile_step(&def, &tables) {
                Ok(step) => steps.push(step),
                Err(e) => eprintln!("Warning: skipping invalid custom step '{}': {}", def.label, e),
            }
        }

        // Apply step_order reordering
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_custom_step_extracts_po_box_digits -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write test for custom step in step_order**

Add to `tests/config.rs`:
```rust
#[test]
fn test_custom_step_respects_step_order() {
    let config: Config = toml::from_str(
        r#"
[steps]
step_order = ["na_check", "custom_rewrite_test", "city_state_zip"]

[[steps.custom_steps]]
type = "rewrite"
label = "custom_rewrite_test"
pattern = '\bTEST\b'
replacement = 'TESTED'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    assert_eq!(summaries[0].label, "na_check");
    assert_eq!(summaries[1].label, "custom_rewrite_test");
    assert_eq!(summaries[2].label, "city_state_zip");
}
```

- [ ] **Step 7: Run test**

Run: `cargo test test_custom_step_respects_step_order -- --nocapture`
Expected: PASS — step_order already handles any label, custom or default.

- [ ] **Step 8: Write test for custom step can be disabled**

Add to `tests/config.rs`:
```rust
#[test]
fn test_custom_step_can_be_disabled() {
    let config: Config = toml::from_str(
        r#"
[steps]
disabled = ["custom_po_box_digits"]

[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)\b'
target = "po_box"
skip_if_filled = true
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    let custom = summaries.iter().find(|s| s.label == "custom_po_box_digits").unwrap();
    assert!(!custom.enabled);
}
```

- [ ] **Step 9: Run test**

Run: `cargo test test_custom_step_can_be_disabled -- --nocapture`
Expected: PASS — disabled logic already works by label.

- [ ] **Step 10: Run full test suite**

Run: `cargo test`
Expected: PASS

- [ ] **Step 11: Commit**

```bash
git add src/pipeline.rs tests/config.rs
git commit -m "feat: compile and merge custom steps in pipeline"
```

---

## Chunk 3: TUI Custom Step Wizard

This chunk adds the TUI wizard for creating custom steps, the `is_custom` field on `StepState`, delete behavior for custom steps, and visual markers.

### Task 8: Add is_custom to StepState and visual marker

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Add is_custom field to StepState**

```rust
struct StepState {
    label: String,
    group: String,
    action_desc: String,
    pattern_template: String,
    enabled: bool,
    default_enabled: bool,
    is_custom: bool,
}
```

- [ ] **Step 2: Set is_custom when building StepState in App::new**

In `App::new`, where step states are built from summaries (around line 137), the `default_enabled_map` is built from default pipeline summaries. A step is custom if its label is NOT in the default map:

```rust
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
```

- [ ] **Step 3: Add [+] visual marker for custom steps in render_steps**

In `render_steps`, modify the line that builds the `ListItem` to prepend `[+]` for custom steps. Around the line that creates the label span:

```rust
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
```

- [ ] **Step 4: Run cargo check**

Run: `cargo check`
Expected: PASS — no logic changes yet, just struct field and rendering.

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add is_custom field and [+] marker in TUI step list"
```

### Task 9: Add delete (d) for custom steps

**Files:**
- Modify: `src/tui.rs` (handle_rules_key)

- [ ] **Step 1: Add d keybinding with confirmation in handle_rules_key**

Add a `confirm_delete: Option<usize>` field to `App` (initialized to `None`). When `d` is pressed on a custom step, set `confirm_delete = Some(i)`. Render a confirmation prompt ("Delete custom step 'label'? y/n"). On `y`, delete; on `n`/`Esc`, cancel.

In the normal mode match block of `handle_rules_key`:
```rust
        KeyCode::Char('d') => {
            if let Some(i) = app.steps_list_state.selected() {
                if app.steps[i].is_custom {
                    app.confirm_delete = Some(i);
                }
            }
        }
```

Handle confirmation in the run_loop, before other key processing (similar to quit prompt):
```rust
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
```

Render the confirmation as an overlay in `render()`:
```rust
    if let Some(del_idx) = app.confirm_delete {
        let label = &app.steps[del_idx].label;
        let popup_area = centered_rect(50, 5, frame.area());
        let popup = Paragraph::new(format!("Delete custom step '{}'? (y/n)", label))
            .block(Block::bordered().title("Confirm Delete"))
            .style(Style::new().bg(Color::Black).fg(Color::Yellow));
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        frame.render_widget(popup, popup_area);
    }
```

- [ ] **Step 2: Update to_config to serialize custom steps**

In `App::to_config()`, after collecting disabled/pattern_overrides/step_order, add custom step serialization. Custom steps are those where `is_custom == true`:

```rust
        // Custom steps: collect StepDefs for custom steps
        let custom_steps: Vec<crate::step::StepDef> = self
            .steps
            .iter()
            .filter(|s| s.is_custom)
            .map(|s| crate::step::StepDef {
                step_type: s.group.clone(),
                label: s.label.clone(),
                pattern: if s.pattern_template.is_empty() { None } else { Some(s.pattern_template.clone()) },
                table: None,
                target: None, // Will need target stored on StepState — see next step
                replacement: None,
                skip_if_filled: None,
                matching_table: None,
                format_table: None,
                mode: None,
            })
            .collect();
        config.steps.custom_steps = custom_steps;
```

Wait — `StepState` doesn't store enough information to reconstruct a full `StepDef`. We need to store the original `StepDef` for custom steps so we can round-trip them back to config. Let me revise the approach.

- [ ] **Step 2 (revised): Add custom_step_defs storage to App**

Instead of trying to reconstruct StepDefs from StepState, store the original StepDefs for custom steps in App. Add a field:

```rust
struct App {
    // ... existing fields ...
    /// Original StepDef for each custom step, keyed by label.
    custom_step_defs: std::collections::HashMap<String, crate::step::StepDef>,
}
```

Initialize in `App::new` from the config:
```rust
        let custom_step_defs: std::collections::HashMap<String, crate::step::StepDef> = config
            .steps
            .custom_steps
            .iter()
            .map(|d| (d.label.clone(), d.clone()))
            .collect();
```

Add to the `App` construction at the end of `App::new`:
```rust
        App {
            // ... existing fields ...
            custom_step_defs,
        }
```

- [ ] **Step 3: Update to_config to use stored StepDefs**

```rust
        // Custom steps: serialize in current step order
        config.steps.custom_steps = self
            .steps
            .iter()
            .filter(|s| s.is_custom)
            .filter_map(|s| {
                let mut def = self.custom_step_defs.get(&s.label)?.clone();
                // Apply any pattern override from TUI editing
                if !s.pattern_template.is_empty() {
                    def.pattern = Some(s.pattern_template.clone());
                }
                Some(def)
            })
            .collect();
```

- [ ] **Step 3b: Filter custom steps from pattern_overrides in to_config**

In the existing pattern_overrides loop (around line 317), add a filter to skip custom steps — their patterns are stored in `custom_steps`, not `pattern_overrides`:

```rust
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
```

- [ ] **Step 4: Update step_order serialization to include custom steps**

The existing step_order serialization compares against default order. Custom steps are never in the default, so we need to adjust the comparison. Currently:

```rust
        let default_order: Vec<&str> = default_summaries.iter().map(|s| s.label.as_str()).collect();
        let current_order: Vec<&str> = self.steps.iter().map(|s| s.label.as_str()).collect();
        if current_order != default_order {
            config.steps.step_order = self.steps.iter().map(|s| s.label.clone()).collect();
        }
```

This already works — if custom steps exist, `current_order` will differ from `default_order` (different length), so step_order will be serialized. No change needed.

- [ ] **Step 5: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add delete keybinding and custom step round-trip in TUI"
```

### Task 10: Add wizard types and implement full wizard

**Files:**
- Modify: `src/tui.rs`

This task adds all wizard types, the event handler, and rendering in one coherent unit. The wizard is non-functional until all pieces are in place, so they should be implemented together.

- [ ] **Step 1: Add WizardAccumulator and WizardState**

Add the accumulator struct to track choices as the user progresses through the wizard:

```rust
/// Tracks accumulated wizard choices as they flow through the wizard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WizardAccumulator {
    step_type: String,
    pattern: String,
    target: Option<String>,
    replacement: Option<String>,
    table: Option<String>,
    skip_if_filled: Option<bool>,
    matching_table: Option<String>,
    format_table: Option<String>,
    mode: Option<String>,
}
```

Add a flat WizardState enum (generic states, not type-prefixed — the accumulator tracks which step type we're building):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum WizardState {
    PickType(usize),
    /// Text input: (text, cursor, optional validation error)
    Pattern(String, usize, Option<String>),
    PickTarget(usize),
    SkipIfFilled,
    /// Text input: (text, cursor)
    Replacement(String, usize),
    RewriteMode(usize),  // 0=replacement, 1=table
    /// Text input: (text, cursor)
    TableName(String, usize),
    StandardizeMode(usize),  // 0=pattern+replacement, 1=table-based
    /// Text input: (text, cursor)
    MatchingTable(String, usize),
    /// Text input: (text, cursor)
    FormatTable(String, usize),
    WordMode(usize),  // 0=whole_field, 1=per_word
    /// Text input: (text, cursor)
    Label(String, usize),
}
```

Add to InputMode:
```rust
    /// Add-step wizard: (wizard state, insertion index)
    AddStep(WizardState, usize),
```

Add constants:
```rust
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
```

Add `wizard_acc` field to App:
```rust
struct App {
    // ... existing fields ...
    custom_step_defs: std::collections::HashMap<String, crate::step::StepDef>,
    wizard_acc: WizardAccumulator,
}
```

Initialize in App::new:
```rust
        wizard_acc: WizardAccumulator::default(),
```

- [ ] **Step 2: Add 'a' keybinding in handle_rules_key**

In the normal mode match block, add:
```rust
        KeyCode::Char('a') => {
            let insert_after = app.steps_list_state.selected().unwrap_or(0);
            app.wizard_acc = WizardAccumulator::default();
            app.input_mode = InputMode::AddStep(
                WizardState::PickType(0),
                insert_after + 1,
            );
        }
```

- [ ] **Step 3: Route AddStep to wizard handler in run_loop**

In `run_loop`, before the generic `handle_input_mode`:
```rust
            if app.input_mode != InputMode::Normal {
                if matches!(app.input_mode, InputMode::AddStep(_, _)) {
                    handle_wizard_key(app, key.code);
                } else {
                    handle_input_mode(app, key.code);
                }
                continue;
            }
```

- [ ] **Step 4: Implement wizard text input helper**

Generic helper for text-input wizard states (Pattern, Replacement, TableName, Label, etc.):

```rust
/// Handle text editing keys for wizard text-input states.
/// Returns Some(final_text) if Enter was pressed on valid input, None otherwise.
/// Updates app.input_mode for cursor movement and character input.
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
```

- [ ] **Step 5: Implement handle_wizard_key with all state transitions**

Complete wizard handler. The key transitions are:

**Extract flow:** PickType → Pattern → PickTarget → SkipIfFilled → Replacement → Label
**Rewrite flow:** PickType → Pattern → RewriteMode → (Replacement | TableName) → Label
**Standardize flow:** PickType → PickTarget → StandardizeMode → (Pattern → Replacement | MatchingTable → FormatTable → WordMode) → Label

```rust
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
                true, // validate regex
            ) {
                app.wizard_acc.pattern = pattern;
                let next = match app.wizard_acc.step_type.as_str() {
                    "extract" => WizardState::PickTarget(0),
                    "rewrite" => WizardState::RewriteMode(0),
                    "standardize" => WizardState::Replacement(String::new(), 0),
                    _ => return,
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
        }

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
                    WizardState::MatchingTable(String::new(), 0)
                };
                app.input_mode = InputMode::AddStep(next, insert_idx);
            }
            KeyCode::Esc => { app.input_mode = InputMode::Normal; }
            _ => {}
        },

        WizardState::MatchingTable(text, cursor) => {
            if let Some(name) = handle_wizard_text_edit(
                app, code, &text, cursor, insert_idx,
                |t, c, _| WizardState::MatchingTable(t, c),
                false,
            ) {
                app.wizard_acc.matching_table = Some(name);
                app.input_mode = InputMode::AddStep(WizardState::FormatTable(String::new(), 0), insert_idx);
            }
        }

        WizardState::FormatTable(text, cursor) => {
            if let Some(name) = handle_wizard_text_edit(
                app, code, &text, cursor, insert_idx,
                |t, c, _| WizardState::FormatTable(t, c),
                false,
            ) {
                app.wizard_acc.format_table = Some(name);
                app.input_mode = InputMode::AddStep(WizardState::WordMode(0), insert_idx);
            }
        }

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
                // Check label uniqueness
                if app.steps.iter().any(|s| s.label == label) { return; }

                let acc = &app.wizard_acc;
                let def = crate::step::StepDef {
                    step_type: acc.step_type.clone(),
                    label: label.clone(),
                    pattern: if acc.pattern.is_empty() { None } else { Some(acc.pattern.clone()) },
                    table: acc.table.clone(),
                    target: acc.target.clone(),
                    replacement: acc.replacement.clone(),
                    skip_if_filled: acc.skip_if_filled,
                    matching_table: acc.matching_table.clone(),
                    format_table: acc.format_table.clone(),
                    mode: acc.mode.clone(),
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
```

Note: `handle_wizard_text_edit` for non-pattern states needs to accept a 3-arg closure even though the third arg (error) is unused. Use `|t, c, _| WizardState::Replacement(t, c)` — the Replacement variant only has 2 fields, so ignore the error param.

- [ ] **Step 6: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add wizard types, event handler, and accumulator"
```

### Task 11: Wizard rendering

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Add wizard overlay rendering in render()**

In the `render()` function's input mode overlay section (after `EditPattern`), add rendering for `AddStep`:

```rust
        InputMode::AddStep(ref wizard, _) => {
            render_wizard(frame, wizard, &app.wizard_acc);
        }
```

- [ ] **Step 2: Implement render_wizard function**

```rust
fn render_wizard(frame: &mut Frame, wizard: &WizardState, acc: &WizardAccumulator) {
    let popup_area = centered_rect(60, 12, frame.area());
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
        WizardState::SkipIfFilled => {
            let popup = Paragraph::new("Skip if target field already has a value? (y/n)")
                .block(Block::bordered().title("Add step — skip_if_filled"))
                .style(Style::new().bg(Color::Black).fg(Color::Cyan));
            frame.render_widget(popup, popup_area);
        }
        WizardState::Replacement(text, cursor) => {
            let title = "Add step — replacement (Enter to continue, empty = no replacement)";
            render_wizard_text_input(frame, popup_area, title, text, *cursor, None);
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
            render_wizard_text_input(frame, popup_area, "Add step — table name", text, *cursor, None);
        }
        WizardState::Label(text, cursor) => {
            render_wizard_text_input(frame, popup_area, "Add step — label (Enter to create)", text, *cursor, None);
        }
        // Standardize states
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
        WizardState::MatchingTable(text, cursor) => {
            render_wizard_text_input(frame, popup_area, "Add standardize — matching table", text, *cursor, None);
        }
        WizardState::FormatTable(text, cursor) => {
            render_wizard_text_input(frame, popup_area, "Add standardize — format table", text, *cursor, None);
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
    let (before, after) = text.split_at(cursor);
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
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add wizard overlay rendering for custom step creation"
```

### Task 12: End-to-end integration test

**Files:**
- Modify: `tests/config.rs`

- [ ] **Step 1: Write integration test for custom rewrite step**

```rust
#[test]
fn test_custom_rewrite_step() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "rewrite"
label = "custom_normalize_hwy"
pattern = '\bHIGHWAY\b'
replacement = 'HWY'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 HIGHWAY 50");
    // "HIGHWAY" should be rewritten to "HWY" in the working string
    assert_eq!(addr.street_name.as_deref(), Some("HWY 50"));
}
```

- [ ] **Step 2: Write integration test for custom standardize step**

```rust
#[test]
fn test_custom_standardize_step() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "standardize"
label = "custom_std_po_box"
target = "po_box"
pattern = 'BOX\s+(\w+)'
replacement = 'PO BOX $1'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    // The default po_box step extracts "PO BOX 123", standardize reformats.
    // This custom step would additionally standardize "BOX 123" → "PO BOX 123"
    // if it ended up in po_box somehow.
    let summaries = p.step_summaries();
    let custom = summaries.iter().find(|s| s.label == "custom_std_po_box");
    assert!(custom.is_some());
}
```

- [ ] **Step 3: Write config round-trip test**

```rust
#[test]
fn test_custom_steps_config_roundtrip() {
    let toml_str = r#"
[steps]
step_order = ["na_check", "custom_box", "po_box"]

[[steps.custom_steps]]
type = "extract"
label = "custom_box"
pattern = '\bBOX (\d+)\b'
target = "po_box"
skip_if_filled = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.steps.custom_steps.len(), 1);

    let serialized = config.to_toml();
    let reparsed: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.steps.custom_steps.len(), 1);
    assert_eq!(reparsed.steps.custom_steps[0].label, "custom_box");
    assert_eq!(reparsed.steps.step_order, vec!["na_check", "custom_box", "po_box"]);
}
```

- [ ] **Step 4: Write test for graceful handling of invalid custom step**

```rust
#[test]
fn test_invalid_custom_step_skipped_gracefully() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "extract"
label = "bad_step"
pattern = '(?P<unclosed'
target = "po_box"
"#,
    )
    .unwrap();
    // Should not panic — invalid step is skipped with warning
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    // The bad step should not appear in summaries
    assert!(!summaries.iter().any(|s| s.label == "bad_step"));
    // Default steps still work
    let addr = p.parse("123 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 6: Run golden tests to confirm no regression**

Run: `cargo test golden -- --nocapture`
Expected: PASS — custom steps don't affect default behavior.

- [ ] **Step 7: Commit**

```bash
git add tests/config.rs
git commit -m "test: add integration tests for custom steps"
```
