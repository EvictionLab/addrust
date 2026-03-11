# Step Editor Form Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the linear step wizard and limited detail view with a unified two-panel form for adding and editing all pipeline steps, migrate prepare rules into the step pipeline, and introduce `step_overrides` config persistence.

**Architecture:** The form is a new rendering/key-handling mode in the TUI that operates on `StepState` (which now carries the full `StepDef`). The left panel lists fields with cursor navigation; the right panel shows either a rich editor (pattern drill-down, target picker) or contextual help. Prepare rules move from hardcoded `prepare.rs` into `data/defaults/steps.toml` as regular rewrite steps.

**Tech Stack:** Rust, ratatui (TUI framework), fancy_regex, toml (serde), existing step/config infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-11-step-editor-form-design.md`

---

## Chunk 1: Data Model & Config Foundation

### Task 1: Add `step_overrides` to StepsConfig

**Files:**
- Modify: `src/config.rs:61-81` (StepsConfig struct)
- Test: `tests/config.rs`

- [ ] **Step 1: Write test for step_overrides deserialization**

```rust
// tests/config.rs
#[test]
fn test_step_overrides_deserialize() {
    let config: Config = toml::from_str(
        r#"
[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
skip_if_filled = false

[steps.step_overrides.unit_type_value]
target = "unit"
"#,
    )
    .unwrap();
    assert_eq!(config.steps.step_overrides.len(), 2);
    let po_box = &config.steps.step_overrides["po_box"];
    assert_eq!(po_box.pattern.as_deref(), Some(r"\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b"));
    assert_eq!(po_box.skip_if_filled, Some(false));
    let utv = &config.steps.step_overrides["unit_type_value"];
    assert_eq!(utv.target.as_deref(), Some("unit"));
    assert!(utv.pattern.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_step_overrides_deserialize -- --nocapture`
Expected: FAIL — `step_overrides` field doesn't exist on StepsConfig

- [ ] **Step 3: Add step_overrides field to StepsConfig**

In `src/config.rs`, add to `StepsConfig`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct StepsConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub pattern_overrides: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub step_overrides: HashMap<String, StepOverride>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub step_order: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_steps: Vec<StepDef>,
}
```

Add the `StepOverride` struct (partial StepDef — all fields optional):

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct StepOverride {
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
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<std::collections::HashMap<String, usize>>,
}
```

Update `StepsConfig::is_empty()` to include `&& self.step_overrides.is_empty()`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_step_overrides_deserialize -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs tests/config.rs
git commit -m "feat: add step_overrides to StepsConfig for per-field default step overrides"
```

### Task 2: Apply step_overrides in pipeline

**Files:**
- Modify: `src/pipeline.rs:31-97` (from_steps_config)
- Modify: `src/config.rs` (add StepOverride::apply_to method)
- Test: `tests/config.rs`

- [ ] **Step 1: Write test for step_overrides applied in pipeline**

```rust
// tests/config.rs
#[test]
fn test_step_overrides_applied_in_pipeline() {
    let config: Config = toml::from_str(
        r#"
[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    // The dash-digit variant should now be captured
    let addr = p.parse("PO BOX 123-4");
    assert_eq!(addr.po_box.as_deref(), Some("PO BOX 123-4"));
}

#[test]
fn test_step_overrides_backward_compat_with_pattern_overrides() {
    // pattern_overrides still works
    let config: Config = toml::from_str(
        r#"
[steps.pattern_overrides]
po_box = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("PO BOX 123-4");
    assert_eq!(addr.po_box.as_deref(), Some("PO BOX 123-4"));
}

#[test]
fn test_step_overrides_override_pattern_overrides() {
    // step_overrides takes precedence over pattern_overrides
    let config: Config = toml::from_str(
        r#"
[steps.pattern_overrides]
po_box = 'OLD_PATTERN'

[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("PO BOX 123-4");
    assert_eq!(addr.po_box.as_deref(), Some("PO BOX 123-4"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_step_overrides_applied test_step_overrides_backward test_step_overrides_override -- --nocapture`
Expected: First test FAILS (step_overrides not applied), others may pass

- [ ] **Step 3: Add StepOverride::apply_to method**

In `src/config.rs`:

```rust
impl StepOverride {
    /// Apply this override to a StepDef, replacing only the fields that are Some.
    pub fn apply_to(&self, def: &mut crate::step::StepDef) {
        if let Some(ref p) = self.pattern { def.pattern = Some(p.clone()); }
        if let Some(ref t) = self.table { def.table = Some(t.clone()); }
        if let Some(ref t) = self.target { def.target = Some(t.clone()); }
        if let Some(ref r) = self.replacement { def.replacement = Some(r.clone()); }
        if let Some(s) = self.skip_if_filled { def.skip_if_filled = Some(s); }
        if let Some(ref m) = self.mode { def.mode = Some(m.clone()); }
        if let Some(ref s) = self.source { def.source = Some(s.clone()); }
        if let Some(ref t) = self.targets { def.targets = Some(t.clone()); }
    }
}
```

- [ ] **Step 4: Apply step_overrides in pipeline**

In `src/pipeline.rs`, in `from_steps_config`, after the existing `pattern_overrides` loop (line 47-51) and before `compile_steps`, add:

```rust
// Apply step_overrides (takes precedence over pattern_overrides)
for def in &mut defs.step {
    if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
        step_override.apply_to(def);
    }
}
```

Also apply step_overrides to custom_steps in the same way (lines 56-65):

```rust
for custom_def in &config.steps.custom_steps {
    let mut def = custom_def.clone();
    if let Some(override_pattern) = config.steps.pattern_overrides.get(&def.label) {
        def.pattern = Some(override_pattern.clone());
    }
    if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
        step_override.apply_to(&mut def);
    }
    // ... rest unchanged
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_step_overrides -- --nocapture`
Expected: All 3 PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass (existing behavior preserved)

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/pipeline.rs tests/config.rs
git commit -m "feat: apply step_overrides in pipeline, backward compat with pattern_overrides"
```

### Task 3: Migrate prepare rules to steps.toml

**Files:**
- Modify: `data/defaults/steps.toml` (add prepare steps at top)
- Modify: `src/pipeline.rs:136-160` (simplify prepare to uppercase+squish only)
- Modify: `src/prepare.rs` (reduce to uppercase+squish)
- Test: `src/prepare.rs` (existing tests), `tests/config.rs`

- [ ] **Step 1: Write test to verify prepare steps are in the pipeline**

```rust
// tests/config.rs
#[test]
fn test_prepare_steps_in_pipeline() {
    let config = Config::default();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    // First steps should be prepare rules
    assert_eq!(summaries[0].label, "prep_fix_ampersand");
    assert_eq!(summaries[0].step_type, "rewrite");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_prepare_steps_in_pipeline -- --nocapture`
Expected: FAIL — first step is still `na_check`

- [ ] **Step 3: Add prepare steps to steps.toml**

Prepend to `data/defaults/steps.toml` (before `na_check`):

```toml
# --- Pre-cleaning (migrated from prepare.rs) ---
[[step]]
type = "rewrite"
label = "prep_fix_ampersand"
pattern = '&AMP;'
replacement = '&'

[[step]]
type = "rewrite"
label = "prep_dedup_nonword"
pattern = '(\W)\1+'
replacement = '$1'

[[step]]
type = "rewrite"
label = "prep_period_between"
pattern = '([^\s])\.([^\s])'
replacement = '$1 $2'

[[step]]
type = "rewrite"
label = "prep_hyphen_spaces"
pattern = '([A-Z0-9])\s*-\s*([A-Z0-9])'
replacement = '$1-$2'

[[step]]
type = "rewrite"
label = "prep_unstick_number_letters"
pattern = '^(\d+)([A-Z]{2,})'
replacement = '$1 $2'

[[step]]
type = "rewrite"
label = "prep_stick_dir_number"
pattern = '^([NSEW]) (\d+)\b'
replacement = '${1}${2}'

[[step]]
type = "rewrite"
label = "prep_pound_ordinal"
pattern = '#(\d+[RNTS][DHT])'
replacement = '$1'

[[step]]
type = "rewrite"
label = "prep_ampersand_to_and"
pattern = '([A-Z])\s*&\s*([A-Z])'
replacement = '$1 AND $2'

[[step]]
type = "rewrite"
label = "prep_slash_to_space"
pattern = '(?<!\d)/(?!\d)'
replacement = ' '

# NOTE: na_check must come AFTER prep steps but its pattern needs updating
# because prep_slash_to_space converts N/A to N A. Update na_check pattern
# from '(?i)^(N/?A|{na_values})$' to '(?i)^(N\W?A|{na_values})$'
# to match both N/A and N A (and NA).

[[step]]
type = "rewrite"
label = "prep_trailing_nonword"
pattern = '\W+$'
replacement = ''

[[step]]
type = "rewrite"
label = "prep_remove_periods_apostrophes"
pattern = "[.']+"
replacement = ''

[[step]]
type = "rewrite"
label = "prep_remove_punctuation"
pattern = '[;<>$()"]+|`'
replacement = ''

[[step]]
type = "rewrite"
label = "prep_mlk"
pattern = '(?:(?:DR|DOCTOR)\W*)?M(?:ARTIN)?\W*L(?:UTHER)?\W*K(?:ING)?(?:\W+(?:JR|JUNIOR))?'
replacement = 'MARTIN LUTHER KING'
```

- [ ] **Step 3b: Update na_check pattern for N/A regression**

The `prep_slash_to_space` step converts `N/A` to `N A` before `na_check` runs. Update the `na_check` pattern in `data/defaults/steps.toml`:

```toml
# Change from:
pattern = '(?i)^(N/?A|{na_values})$'
# To:
pattern = '(?i)^(N\W?A|{na_values})$'
```

This matches `NA`, `N/A`, `N A`, and `N.A` — all valid NA representations.

- [ ] **Step 4: Simplify prepare.rs to uppercase+squish only**

Replace `src/prepare.rs` with:

```rust
use crate::ops::squish;

/// Prepare an address string for parsing: uppercase and normalize whitespace.
/// Domain-specific cleaning rules are now pipeline steps in steps.toml.
pub fn prepare(input: &str) -> Option<String> {
    let mut s = input.to_uppercase();
    squish(&mut s);
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_basic() {
        assert_eq!(prepare("  hello   world  "), Some("HELLO WORLD".into()));
    }

    #[test]
    fn test_prepare_empty() {
        assert_eq!(prepare(""), None);
    }
}
```

- [ ] **Step 5: Run the new test to verify it passes**

Run: `cargo test test_prepare_steps_in_pipeline -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite to check for regressions**

Run: `cargo test`
Expected: All tests pass. If existing prepare tests or integration tests fail, the prepare steps in steps.toml need adjustment (check regex syntax differences between fancy_regex patterns — the pipe steps use `is_match` guard before `replace_all`, while prepare.rs used `replace_all` directly without guard). The rewrite step's `is_match` check (line 271 in step.rs) will short-circuit if the pattern doesn't match, which is functionally identical.

- [ ] **Step 7: Fix any test failures**

If integration/golden tests fail, compare outputs. Likely issues:
- The rewrite step squishes after each replacement; prepare.rs only squished once at the end. Multiple squishes should be idempotent, so this should be fine.
- Order-dependent interactions between prepare rules and pipeline steps: since prepare steps run first (before na_check), same order is preserved.

Adjust as needed and ensure all tests pass.

- [ ] **Step 8: Commit**

```bash
git add data/defaults/steps.toml src/prepare.rs tests/config.rs
git commit -m "feat: migrate prepare rules to steps.toml, simplify prepare to uppercase+squish"
```

### Task 4: Expand StepState to carry full StepDef

**Files:**
- Modify: `src/tui.rs:142-152` (StepState struct)
- Modify: `src/tui.rs:206-250` (App::new — build step states with full defs)
- Modify: `src/tui.rs:415-470` (to_config — use def instead of custom_step_defs)
- Modify: `src/tui.rs:196-203` (remove custom_step_defs, wizard_acc)

- [ ] **Step 1: Update StepState struct**

Replace the existing StepState (lines 142-152) with:

```rust
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
            None => false, // custom steps aren't "modified" vs default
            Some(default) => self.def != *default || self.enabled != self.default_enabled,
        }
    }
    fn is_field_modified(&self, field: &str) -> bool {
        let Some(default) = &self.default_def else { return false };
        match field {
            "pattern" => self.def.pattern != default.pattern,
            "target" => self.def.target != default.target,
            "targets" => self.def.targets != default.targets,
            "replacement" => self.def.replacement != default.replacement,
            "skip_if_filled" => self.def.skip_if_filled != default.skip_if_filled,
            "table" => self.def.table != default.table,
            "source" => self.def.source != default.source,
            "mode" => self.def.mode != default.mode,
            "label" => false, // label can't be modified on defaults
            _ => false,
        }
    }
}
```

Note: `StepDef` needs to derive `PartialEq`. Add `#[derive(PartialEq)]` to `StepDef` in `src/step.rs:404`.

- [ ] **Step 2: Update App::new() to populate full StepDefs**

In `App::new()`, the step state initialization needs to store the original `StepDef` from the parsed TOML. Currently it only gets `StepSummary` (label, type, pattern, enabled). Change the approach:

1. Parse `steps.toml` into `StepsDef` directly in App::new (same as pipeline does).
2. Apply pattern_overrides and step_overrides to get the current defs.
3. Store both current def and original def in each StepState.

```rust
// In App::new(), replace the step state initialization block:
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
    if let Some(override_pattern) = config.steps.pattern_overrides.get(&def.label) {
        def.pattern = Some(override_pattern.clone());
    }
    if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
        step_override.apply_to(def);
    }
}

// Append custom steps
for custom_def in &config.steps.custom_steps {
    let mut def = custom_def.clone();
    if let Some(override_pattern) = config.steps.pattern_overrides.get(&def.label) {
        def.pattern = Some(override_pattern.clone());
    }
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
    let default_enabled = true; // all steps default to enabled (custom and default)
    let enabled = !config.steps.disabled.contains(&def.label);
    StepState {
        enabled,
        default_enabled,
        is_custom,
        def: def.clone(),
        default_def: default_def_map.get(&def.label).cloned(),
    }
}).collect();
```

- [ ] **Step 3: Update all references to old StepState fields**

Search for `step.label`, `step.group`, `step.action_desc`, `step.pattern_template` in tui.rs and replace:
- `step.label` → `step.label()` or `step.def.label`
- `step.group` → `step.step_type()`
- `step.action_desc` → `step.step_type()`
- `step.pattern_template` → `step.pattern_template()`

Also update `custom_step_defs` references — replace lookups with `step.def` access.

Remove `custom_step_defs` field from App. Remove `wizard_acc` field (the wizard accumulator will be replaced in a later task by the form state). Keep `wizard_acc` temporarily if needed for compilation; it can be removed when the wizard is replaced.

- [ ] **Step 4: Update to_config() to use StepState.def**

Rewrite `to_config()` to diff `def` vs `default_def`:

```rust
fn to_config(&self) -> Config {
    let mut disabled = Vec::new();
    let mut step_overrides = HashMap::new();
    let mut custom_steps = Vec::new();
    let mut step_order: Vec<String> = Vec::new();

    // Parse default step order for comparison
    let toml_str = include_str!("../data/defaults/steps.toml");
    let default_defs: crate::step::StepsDef = toml::from_str(toml_str).unwrap();
    let default_order: Vec<&str> = default_defs.step.iter().map(|d| d.label.as_str()).collect();

    for step in &self.steps {
        step_order.push(step.label().to_string());

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
            if step.def.target != default.target {
                ovr.target = step.def.target.clone(); has_changes = true;
            }
            if step.def.targets != default.targets {
                ovr.targets = step.def.targets.clone(); has_changes = true;
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
            if step.def.source != default.source {
                ovr.source = step.def.source.clone(); has_changes = true;
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

    Config {
        steps: StepsConfig {
            disabled,
            pattern_overrides: HashMap::new(), // no longer written; read-only for backward compat
            step_overrides,
            step_order: if emit_order { step_order } else { Vec::new() },
            custom_steps,
        },
        // Extract the existing dict/output serialization from the current to_config()
        // (lines 472-523 in tui.rs) into these helper methods:
        // fn dict_to_config(&self) -> HashMap<String, DictOverrides> { ... }
        // fn output_to_config(&self) -> OutputConfig { ... }
        dictionaries: self.dict_to_config(),
        output: self.output_to_config(),
    }
}
```

Note: the existing dict and output serialization logic from `to_config()` should be extracted into helper methods `dict_to_config()` and `output_to_config()` to keep things clean.

- [ ] **Step 5: Verify compilation and all tests pass**

Run: `cargo test`
Expected: All tests pass. The TUI behavior is functionally identical — this is a data model change, not a UI change.

- [ ] **Step 6: Commit**

```bash
git add src/tui.rs src/step.rs src/config.rs
git commit -m "refactor: StepState carries full StepDef, to_config uses step_overrides"
```

---

## Chunk 2: Two-Panel Form — Rendering

### Task 5: Define form state types and visible fields logic

**Files:**
- Modify: `src/tui.rs` (add FormField enum, FormState, visible_fields function)

- [ ] **Step 1: Add FormField enum and FormState**

Add near the top of `src/tui.rs`, after the existing enums:

```rust
/// Fields in the step editor form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormField {
    Pattern,
    TargetMode,   // Extract only: single vs multi
    Target,       // Single target
    Targets,      // Multi-target picker
    SkipIfFilled, // Extract only
    Replacement,
    Table,
    Source,
    Mode,         // Standardize only: whole_field / per_word
    Label,
}

/// Which panel has focus in the form.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FormFocus {
    Left,          // navigating field list
    RightPattern,  // in pattern drill-down
    RightTargets,  // in target picker
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
```

- [ ] **Step 2: Add visible_fields function**

```rust
fn visible_fields_for_type(step_type: &str, def: &crate::step::StepDef) -> Vec<FormField> {
    match step_type {
        "extract" => {
            let mut fields = vec![FormField::Pattern, FormField::TargetMode];
            if def.targets.is_some() {
                fields.push(FormField::Targets);
            } else {
                fields.push(FormField::Target);
            }
            fields.push(FormField::SkipIfFilled);
            fields.push(FormField::Replacement);
            fields.push(FormField::Source);
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
            fields.push(FormField::Source);
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
            fields.push(FormField::Target);
            fields.push(FormField::Mode);
            fields.push(FormField::Label);
            fields
        }
        _ => vec![FormField::Label],
    }
}
```

- [ ] **Step 3: Add FormState to App and InputMode**

Add to App struct:
```rust
/// Step editor form state (when open).
form_state: Option<FormState>,
```

Initialize as `None` in `App::new()`.

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add FormField, FormState, and visible_fields_for_type for step editor form"
```

### Task 6: Render the left panel

**Files:**
- Modify: `src/tui.rs` (add render_step_form, render_form_left_panel)

- [ ] **Step 1: Add render_step_form function**

```rust
fn render_step_form(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let form = match &app.form_state {
        Some(f) => f,
        None => return,
    };

    let step_state = form.step_index.map(|i| &app.steps[i]);

    // Header
    let header_height = 3;
    let [header_area, body_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Fill(1),
    ]).areas(area);

    // Header content
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
```

- [ ] **Step 2: Add render_form_left_panel**

```rust
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
        FormField::TargetMode => "target",
        FormField::Target => "target",
        FormField::Targets => "targets",
        FormField::SkipIfFilled => "skip_if_filled",
        FormField::Replacement => "replacement",
        FormField::Table => "table",
        FormField::Source => "source",
        FormField::Mode => "mode",
        FormField::Label => "label",
    }
}

fn form_field_display(field: FormField, def: &crate::step::StepDef) -> (&'static str, String) {
    match field {
        FormField::Pattern => ("Pattern", def.pattern.as_deref().unwrap_or("(none)").to_string()),
        FormField::TargetMode => ("Target mode", if def.targets.is_some() { "Multiple targets" } else { "Single target" }.to_string()),
        FormField::Target => ("Target", def.target.as_deref().unwrap_or("(none)").to_string()),
        FormField::Targets => {
            let t = def.targets.as_ref().map(|m| {
                let mut pairs: Vec<_> = m.iter().collect();
                pairs.sort_by_key(|(_, v)| *v);
                pairs.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join(", ")
            }).unwrap_or_default();
            ("Targets", if t.is_empty() { "(none)".to_string() } else { t })
        }
        FormField::SkipIfFilled => ("Skip if filled", if def.skip_if_filled == Some(true) { "yes" } else { "no" }.to_string()),
        FormField::Replacement => ("Replacement", def.replacement.as_deref().unwrap_or("(none)").to_string()),
        FormField::Table => ("Table", def.table.as_deref().unwrap_or("(none)").to_string()),
        FormField::Source => ("Source", def.source.as_deref().unwrap_or("working string").to_string()),
        FormField::Mode => ("Mode", def.mode.as_deref().unwrap_or("whole field").to_string()),
        FormField::Label => ("Label", def.label.clone()),
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles (render_form_right_panel can be a stub for now — `fn render_form_right_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {}`)

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat: render step form left panel with field list, modified markers"
```

### Task 7: Render the right panel — contextual help

**Files:**
- Modify: `src/tui.rs` (implement render_form_right_panel)

- [ ] **Step 1: Implement render_form_right_panel**

```rust
fn render_form_right_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let form = app.form_state.as_ref().unwrap();
    let step_state = form.step_index.map(|i| &app.steps[i]);
    let current_field = form.visible_fields.get(form.field_cursor).copied();

    // Check focus state first — Target/Source fields use the targets panel when focused
    match &form.focus {
        FormFocus::RightPattern => { render_form_pattern_panel(frame, app, area); return; }
        FormFocus::RightTargets => { render_form_targets_panel(frame, app, area); return; }
        FormFocus::RightTable => { render_form_table_panel(frame, app, area); return; }
        FormFocus::EditingText(field_name, cursor, text) => {
            render_form_text_edit_panel(frame, field_name, text, *cursor, area);
            return;
        }
        FormFocus::Left => {}
    }

    // Left panel focused — show contextual help or rich preview
    match current_field {
        Some(FormField::Pattern) => render_form_pattern_panel(frame, app, area),
        Some(FormField::Targets) => render_form_targets_panel(frame, app, area),
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
    let (title, help_text, current_value, edit_hint) = match field {
        FormField::SkipIfFilled => (
            "Skip If Filled",
            "When yes, this step is skipped if the target field(s) already have a value from a previous step.\n\nUse this for extraction steps that should only fire once — for example, extracting a street number should not overwrite one that was already found.",
            if def.skip_if_filled == Some(true) { "yes" } else { "no" },
            "Space to toggle",
        ),
        FormField::Replacement => (
            "Replacement",
            "Text that replaces the matched pattern. Supports backreferences to capture groups:\n\n  $1        — capture group 1\n  ${N:table} — look up group N in a table\n  ${N/M:fraction} — fraction (group N / group M)",
            def.replacement.as_deref().unwrap_or("(none)"),
            "Enter to edit",
        ),
        FormField::Source => (
            "Source",
            "Which text this step operates on.\n\n'working string' is the main address being parsed. Selecting a field (e.g., 'unit') makes the step operate on that extracted field instead.",
            def.source.as_deref().unwrap_or("working string"),
            "Enter to pick",
        ),
        FormField::Mode => (
            "Mode",
            "How standardization is applied.\n\n'Whole field' standardizes the entire field value as one lookup.\n\n'Per word' splits on spaces and standardizes each word independently.",
            def.mode.as_deref().unwrap_or("whole field"),
            "Space to toggle",
        ),
        FormField::Label => (
            "Label",
            "Unique identifier for this step. Used in config files for overrides, ordering, and disable lists.",
            def.label.as_str(),
            "Enter to edit",
        ),
        FormField::Target => (
            "Target",
            "The address field where the extracted value is stored.",
            def.target.as_deref().unwrap_or("(none)"),
            "Enter to pick",
        ),
        FormField::TargetMode => (
            "Target Mode",
            "Choose whether this step extracts to a single field or routes capture groups to multiple fields.",
            if def.targets.is_some() { "Multiple targets" } else { "Single target" },
            "Space to toggle",
        ),
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
                let default_val = match field {
                    FormField::Replacement => default_def.replacement.as_deref().unwrap_or("(none)"),
                    FormField::Source => default_def.source.as_deref().unwrap_or("working string"),
                    FormField::Target => default_def.target.as_deref().unwrap_or("(none)"),
                    FormField::SkipIfFilled => if default_def.skip_if_filled == Some(true) { "yes" } else { "no" },
                    FormField::Mode => default_def.mode.as_deref().unwrap_or("whole field"),
                    _ => "",
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
```

- [ ] **Step 2: Add stub functions for pattern, targets, and table panels**

```rust
fn render_form_pattern_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    // Will be implemented in Task 9
    let form = app.form_state.as_ref().unwrap();
    let text = form.def.pattern.as_deref().unwrap_or("(no pattern)");
    let panel = Paragraph::new(vec![
        Line::from(Span::styled("Pattern", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("e: edit raw regex   Enter: drill into group", Style::new().fg(Color::DarkGray))),
        Line::from(""),
        Line::from(Span::styled(text, Style::new().fg(Color::White))),
    ])
    .wrap(Wrap { trim: false })
    .block(Block::bordered().border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(panel, area);
}

fn render_form_targets_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    // Will be implemented in Task 10
    let panel = Paragraph::new("Targets (TODO)")
        .block(Block::bordered().border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(panel, area);
}

fn render_form_table_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    // Will be implemented in Task 11
    let panel = Paragraph::new("Table picker (TODO)")
        .block(Block::bordered().border_style(Style::new().fg(Color::DarkGray)));
    frame.render_widget(panel, area);
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat: render step form right panel with contextual help for all fields"
```

---

## Chunk 3: Two-Panel Form — Key Handling & Integration

### Task 8: Wire form into TUI — open/close, left panel navigation

**Files:**
- Modify: `src/tui.rs` (handle_rules_key, render dispatch, form key handling)

- [ ] **Step 1: Open form on Enter in step list**

In `handle_rules_key` (line ~735 area where Enter currently opens step detail), replace the step detail drill-down with opening the form:

```rust
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
```

- [ ] **Step 2: Open form on 'a' for new step (after type pick)**

Modify the 'a' key handler. Keep the PickType popup (3-item type selector). When type is selected, instead of entering the wizard, open the form:

```rust
// After type is selected (in existing PickType Enter handler):
let step_type = STEP_TYPES[selected].to_string();
let mut def = crate::step::StepDef {
    step_type: step_type.clone(),
    label: format!("custom_{}", step_type),
    pattern: None,
    table: None,
    target: None,
    replacement: None,
    skip_if_filled: None,
    matching_table: None,
    format_table: None,
    mode: None,
    source: None,
    targets: None,
};
let visible = visible_fields_for_type(&step_type, &def);
app.form_state = Some(FormState {
    step_index: None,
    def,
    visible_fields: visible,
    field_cursor: 0,
    focus: FormFocus::Left,
    right_cursor: 0,
    right_alt_selected: None,
    pattern_segments: Vec::new(),
    is_new: true,
    show_discard_prompt: false,
});
app.input_mode = InputMode::Normal;
```

- [ ] **Step 3: Add form key handler for left panel**

```rust
fn handle_form_key(app: &mut App, code: KeyCode) {
    let form = match &mut app.form_state {
        Some(f) => f,
        None => return,
    };

    if form.show_discard_prompt {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.form_state = None; // discard
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                form.show_discard_prompt = false;
            }
            _ => {}
        }
        return;
    }

    match form.focus {
        FormFocus::Left => handle_form_left_key(app, code),
        FormFocus::RightPattern => handle_form_pattern_key(app, code),
        FormFocus::RightTargets => handle_form_targets_key(app, code),
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
                FormField::Targets => {
                    form.focus = FormFocus::RightTargets;
                    form.right_cursor = 0;
                }
                FormField::Table => {
                    form.focus = FormFocus::RightTable;
                    form.right_cursor = 0;
                }
                FormField::Target | FormField::Source => {
                    // Open a field picker (reuse target picker with different options for source)
                    form.focus = FormFocus::RightTargets;
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
                FormField::TargetMode => {
                    // Toggle between single and multi target
                    if form.def.targets.is_some() {
                        // Switch to single: take first target from map
                        let first = form.def.targets.as_ref()
                            .and_then(|m| m.keys().next().cloned());
                        form.def.targets = None;
                        form.def.target = first;
                    } else {
                        // Switch to multi: move target into map
                        let mut map = std::collections::HashMap::new();
                        if let Some(t) = form.def.target.take() {
                            map.insert(t, 1);
                        }
                        form.def.targets = Some(map);
                    }
                    form.visible_fields = visible_fields_for_type(&form.def.step_type, &form.def);
                    app.dirty = true;
                }
                _ => {}
            }
        }
        KeyCode::Char('r') => {
            // Reset field to default
            if let Some(step_idx) = form.step_index {
                let step = &app.steps[step_idx];
                if let Some(default) = &step.default_def {
                    let field = form.visible_fields[form.field_cursor];
                    match field {
                        FormField::Pattern => form.def.pattern = default.pattern.clone(),
                        FormField::Target => form.def.target = default.target.clone(),
                        FormField::Targets => form.def.targets = default.targets.clone(),
                        FormField::Replacement => form.def.replacement = default.replacement.clone(),
                        FormField::SkipIfFilled => form.def.skip_if_filled = default.skip_if_filled,
                        FormField::Table => form.def.table = default.table.clone(),
                        FormField::Source => form.def.source = default.source.clone(),
                        FormField::Mode => form.def.mode = default.mode.clone(),
                        _ => {}
                    }
                    app.dirty = true;
                }
            }
        }
        KeyCode::Esc => {
            close_form(app);
        }
        _ => {}
    }
}

fn close_form(app: &mut App) {
    let form = app.form_state.as_mut().unwrap();

    if form.is_new {
        // Validate required fields
        let valid = validate_step_def(&form.def);
        if valid {
            // Create the new step
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
        // Apply changes to existing step
        if let Some(idx) = form.step_index {
            app.steps[idx].def = form.def.clone();
            app.dirty = true;
        }
        app.form_state = None;
    }
}

fn validate_step_def(def: &crate::step::StepDef) -> bool {
    match def.step_type.as_str() {
        "extract" => {
            def.pattern.is_some()
                && (def.target.is_some() || def.targets.as_ref().map(|t| !t.is_empty()).unwrap_or(false))
        }
        "rewrite" => {
            def.pattern.is_some()
                && (def.replacement.is_some() || def.table.is_some())
        }
        "standardize" => {
            def.target.is_some()
                && (def.table.is_some() || (def.pattern.is_some() && def.replacement.is_some()))
        }
        _ => false,
    }
}
```

- [ ] **Step 4: Add stub handlers for right panel focus states**

```rust
fn handle_form_pattern_key(app: &mut App, code: KeyCode) {
    // Will be implemented in Task 9
    let form = app.form_state.as_mut().unwrap();
    if code == KeyCode::Esc {
        form.focus = FormFocus::Left;
    }
}

fn handle_form_targets_key(app: &mut App, code: KeyCode) {
    // Will be implemented in Task 10
    let form = app.form_state.as_mut().unwrap();
    if code == KeyCode::Esc {
        form.focus = FormFocus::Left;
    }
}

fn handle_form_table_key(app: &mut App, code: KeyCode) {
    // Will be implemented in Task 11
    let form = app.form_state.as_mut().unwrap();
    if code == KeyCode::Esc {
        form.focus = FormFocus::Left;
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
```

- [ ] **Step 5: Wire form rendering into main render()**

In the main `render()` function, when `form_state.is_some()`, render the form instead of the step list/detail:

```rust
// In render(), replace the step detail rendering section:
if app.form_state.is_some() {
    render_step_form(frame, app, content_area);
} else if app.step_detail_index.is_some() {
    render_step_detail(frame, app, content_area);
} else {
    render_steps(frame, app, content_area);
}
```

- [ ] **Step 6: Wire form key handling into run_loop**

In `run_loop`, add form key dispatch before the existing step detail handling:

```rust
// In run_loop, after input_mode handlers and before tab-specific handlers:
if app.form_state.is_some() {
    handle_form_key(&mut app, code);
    continue; // form consumes all keys while open
}
```

- [ ] **Step 7: Verify compilation and manual test**

Run: `cargo build && cargo run -- configure --config /tmp/test-addrust.toml`
Expected: Compiles. In the TUI, pressing Enter on a step opens the two-panel form. j/k navigates fields. Esc closes. Space toggles booleans. The right panel shows contextual help.

- [ ] **Step 8: Commit**

```bash
git add src/tui.rs
git commit -m "feat: wire two-panel step form into TUI, left panel navigation and field editing"
```

---

## Chunk 4: Right Panel Editors

### Task 9: Pattern drill-down in right panel

**Files:**
- Modify: `src/tui.rs` (render_form_pattern_panel, handle_form_pattern_key)

Reuse the existing pattern segment parsing (`PatternSegment`) and the existing rendering logic from `render_step_detail` (lines 1918-1990), adapted to work within the form's right panel. The key addition is `a` to add and `d` to delete alternatives, matching dict variant keybindings.

- [ ] **Step 1: Implement render_form_pattern_panel**

Replace the stub with the full implementation. Reuse the existing `PatternSegment` parsing and segment rendering logic. The form stores parsed segments in `FormState` (add a `pattern_segments: Vec<PatternSegment>` field to FormState, populated when pattern changes or form opens).

Add to FormState:
```rust
pattern_segments: Vec<crate::pattern::PatternSegment>,
```

Populate when form opens and when pattern is edited:
```rust
// In form open logic:
let segments = crate::pattern::parse_pattern(def.pattern.as_deref().unwrap_or(""));
// Store in form_state.pattern_segments
```

Render pattern segments in the right panel using the same visual style as the existing step detail view (lines 1918-1990), but adapted for the right panel area.

- [ ] **Step 2: Implement handle_form_pattern_key**

```rust
fn handle_form_pattern_key(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            // Navigate segments/alternatives
            if let Some(alt_idx) = form.right_alt_selected {
                // Inside an alternation group
                if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                    form.pattern_segments.get(form.right_cursor)
                {
                    if alt_idx + 1 < alternatives.len() {
                        form.right_alt_selected = Some(alt_idx + 1);
                    }
                }
            } else {
                // Navigate segments
                let selectable_count = form.pattern_segments.iter()
                    .filter(|s| matches!(s, crate::pattern::PatternSegment::AlternationGroup { .. } | crate::pattern::PatternSegment::TableRef(_)))
                    .count();
                if selectable_count > 0 {
                    // Move to next selectable segment
                    let mut next = form.right_cursor + 1;
                    while next < form.pattern_segments.len() {
                        if matches!(form.pattern_segments[next],
                            crate::pattern::PatternSegment::AlternationGroup { .. } |
                            crate::pattern::PatternSegment::TableRef(_))
                        {
                            form.right_cursor = next;
                            break;
                        }
                        next += 1;
                    }
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // Similar to Down but in reverse
            if let Some(alt_idx) = form.right_alt_selected {
                if alt_idx > 0 {
                    form.right_alt_selected = Some(alt_idx - 1);
                }
            } else {
                let mut prev = form.right_cursor.wrapping_sub(1);
                while prev < form.pattern_segments.len() {
                    if matches!(form.pattern_segments[prev],
                        crate::pattern::PatternSegment::AlternationGroup { .. } |
                        crate::pattern::PatternSegment::TableRef(_))
                    {
                        form.right_cursor = prev;
                        break;
                    }
                    if prev == 0 { break; }
                    prev -= 1;
                }
            }
        }
        KeyCode::Enter => {
            // Drill into alternation group
            if form.right_alt_selected.is_none() {
                if matches!(form.pattern_segments.get(form.right_cursor),
                    Some(crate::pattern::PatternSegment::AlternationGroup { .. }))
                {
                    form.right_alt_selected = Some(0);
                }
            }
        }
        KeyCode::Char(' ') => {
            // Toggle alternative
            if let Some(alt_idx) = form.right_alt_selected {
                if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                    form.pattern_segments.get_mut(form.right_cursor)
                {
                    alternatives[alt_idx].enabled = !alternatives[alt_idx].enabled;
                    // Rebuild pattern from segments
                    form.def.pattern = Some(crate::pattern::rebuild_pattern(&form.pattern_segments));
                    app.dirty = true;
                }
            }
        }
        KeyCode::Char('a') => {
            // Add alternative — enter text input mode
            if form.right_alt_selected.is_some() {
                form.focus = FormFocus::EditingText("add_alternative".to_string(), 0, String::new());
            }
        }
        KeyCode::Char('d') => {
            // Delete selected alternative
            if let Some(alt_idx) = form.right_alt_selected {
                if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
                    form.pattern_segments.get_mut(form.right_cursor)
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
        KeyCode::Char('e') => {
            // Edit raw pattern
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
```

- [ ] **Step 3: Handle "add_alternative" text input completion**

In `handle_form_text_edit`, add handling for when the field name is "add_alternative":

```rust
"add_alternative" => {
    if !value.is_empty() {
        if let Some(crate::pattern::PatternSegment::AlternationGroup { alternatives, .. }) =
            form.pattern_segments.get_mut(form.right_cursor)
        {
            alternatives.push(crate::pattern::Alternative { text: value, enabled: true });
            form.def.pattern = Some(crate::pattern::rebuild_pattern(&form.pattern_segments));
        }
    }
    form.focus = FormFocus::RightPattern;
}
"pattern" => {
    // Validate and update pattern
    form.def.pattern = if value.is_empty() { None } else { Some(value) };
    form.pattern_segments = crate::pattern::parse_pattern(
        form.def.pattern.as_deref().unwrap_or("")
    );
    form.focus = FormFocus::RightPattern;
}
```

- [ ] **Step 4: Verify compilation and manual test**

Run: `cargo build && cargo run -- configure --config /tmp/test-addrust.toml`
Test: Open a step with alternation groups (e.g., suffix_common). Navigate to Pattern field, press Enter. Drill into alternation group. Press `a` to add, type a value, Enter. Press `d` to delete. Press `e` to edit raw pattern.

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs
git commit -m "feat: pattern drill-down in form right panel with add/delete alternatives"
```

### Task 10: Target picker in right panel

**Files:**
- Modify: `src/tui.rs` (render_form_targets_panel, handle_form_targets_key)

- [ ] **Step 1: Implement render_form_targets_panel**

Show all 11 fields with `[N]` for assigned, `[ ]` for unassigned. Also handle single-target mode (simple field picker for Target and Source fields).

```rust
fn render_form_targets_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let form = app.form_state.as_ref().unwrap();
    let current_form_field = form.visible_fields[form.field_cursor];

    match current_form_field {
        FormField::Targets => {
            // Multi-target picker
            let targets = form.def.targets.as_ref();
            let mut items = Vec::new();
            for (i, (key, label)) in TARGET_FIELDS.iter().enumerate() {
                let is_selected = form.focus == FormFocus::RightTargets && form.right_cursor == i;
                let group_num = targets.and_then(|t| t.get(*key)).copied();
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
                let prefix = if is_selected { "▸ " } else { "  " };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(format!("{} {:16}", marker, label), style),
                    Span::styled(detail, Style::new().fg(Color::DarkGray)),
                ])));
            }
            let list = List::new(items).block(
                Block::bordered()
                    .title("Targets")
                    .title_bottom("Space: toggle  1-9: set group  d: remove")
                    .border_style(if form.focus == FormFocus::RightTargets {
                        Style::new().fg(Color::Cyan)
                    } else {
                        Style::new().fg(Color::DarkGray)
                    })
            );
            frame.render_widget(list, area);
        }
        FormField::Target | FormField::Source => {
            // Single field picker
            let is_source = current_form_field == FormField::Source;
            let current = if is_source {
                form.def.source.as_deref()
            } else {
                form.def.target.as_deref()
            };
            let mut items = Vec::new();
            // For source, add "working string" option
            if is_source {
                let is_selected = form.focus == FormFocus::RightTargets && form.right_cursor == 0;
                let is_current = current.is_none();
                let style = if is_selected {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(if is_selected { "▸ " } else { "  " }, style),
                    Span::styled(if is_current { "[x] " } else { "[ ] " }, style),
                    Span::styled("working string", style),
                ])));
            }
            let offset = if is_source { 1 } else { 0 };
            for (i, (key, label)) in TARGET_FIELDS.iter().enumerate() {
                let list_idx = i + offset;
                let is_selected = form.focus == FormFocus::RightTargets && form.right_cursor == list_idx;
                let is_current = current == Some(*key);
                let style = if is_selected {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(if is_selected { "▸ " } else { "  " }, style),
                    Span::styled(if is_current { "[x] " } else { "[ ] " }, style),
                    Span::styled(label.to_string(), style),
                ])));
            }
            let title = if is_source { "Source" } else { "Target" };
            let list = List::new(items).block(
                Block::bordered()
                    .title(title)
                    .title_bottom("Enter: select  Esc: cancel")
                    .border_style(if form.focus == FormFocus::RightTargets {
                        Style::new().fg(Color::Cyan)
                    } else {
                        Style::new().fg(Color::DarkGray)
                    })
            );
            frame.render_widget(list, area);
        }
        _ => {}
    }
}
```

- [ ] **Step 2: Implement handle_form_targets_key**

```rust
fn handle_form_targets_key(app: &mut App, code: KeyCode) {
    let form = app.form_state.as_mut().unwrap();
    let current_field = form.visible_fields[form.field_cursor];
    let is_source = current_field == FormField::Source;
    let is_multi = current_field == FormField::Targets;
    let item_count = if is_source { TARGET_FIELDS.len() + 1 } else { TARGET_FIELDS.len() };

    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            form.right_cursor = (form.right_cursor + 1) % item_count;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            form.right_cursor = if form.right_cursor == 0 { item_count - 1 } else { form.right_cursor - 1 };
        }
        KeyCode::Enter => {
            if !is_multi {
                // Single target/source picker — select the item
                let offset = if is_source { 1 } else { 0 };
                if is_source && form.right_cursor == 0 {
                    form.def.source = None; // working string
                } else {
                    let field_key = TARGET_FIELDS[form.right_cursor - offset].0;
                    if is_source {
                        form.def.source = Some(field_key.to_string());
                    } else {
                        form.def.target = Some(field_key.to_string());
                        // Update auto-generated label if still default
                        if form.def.label.starts_with("custom_") {
                            form.def.label = format!("custom_{}_{}", form.def.step_type, field_key);
                        }
                    }
                }
                form.focus = FormFocus::Left;
                app.dirty = true;
            }
        }
        KeyCode::Char(' ') if is_multi => {
            // Toggle field assignment
            let field_key = TARGET_FIELDS[form.right_cursor].0.to_string();
            let targets = form.def.targets.get_or_insert_with(std::collections::HashMap::new);
            if targets.contains_key(&field_key) {
                targets.remove(&field_key);
            } else {
                // Default to next available group number
                let max = targets.values().max().copied().unwrap_or(0);
                targets.insert(field_key, max + 1);
            }
            app.dirty = true;
        }
        KeyCode::Char(c) if is_multi && c.is_ascii_digit() && c != '0' => {
            let group = (c as u8 - b'0') as usize;
            let field_key = TARGET_FIELDS[form.right_cursor].0.to_string();
            let targets = form.def.targets.get_or_insert_with(std::collections::HashMap::new);
            targets.insert(field_key, group);
            app.dirty = true;
        }
        KeyCode::Char('d') if is_multi => {
            let field_key = TARGET_FIELDS[form.right_cursor].0;
            if let Some(targets) = &mut form.def.targets {
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
```

- [ ] **Step 3: Verify compilation and manual test**

Run: `cargo build && cargo run -- configure --config /tmp/test-addrust.toml`
Test: Open `unit_type_value`, navigate to Targets, press Enter. See the picker. Use Space/1-9/d to modify targets. Esc back. Open a single-target step, navigate to Target, Enter, pick a new target.

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat: target picker and source picker in form right panel"
```

### Task 11: Table picker in right panel

**Files:**
- Modify: `src/tui.rs` (render_form_table_panel, handle_form_table_key)

- [ ] **Step 1: Implement render_form_table_panel and handle_form_table_key**

Reuse the `TABLE_DESCRIPTIONS` constant. Show a selectable list of tables with descriptions. Enter selects, Esc cancels.

```rust
fn render_form_table_panel(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let form = app.form_state.as_ref().unwrap();
    let current_table = form.def.table.as_deref();
    let mut items = Vec::new();

    for (i, (name, desc)) in TABLE_DESCRIPTIONS.iter().enumerate() {
        let is_selected = form.focus == FormFocus::RightTable && form.right_cursor == i;
        let is_current = current_table == Some(*name);
        let style = if is_selected {
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::new().fg(Color::Green)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(if is_selected { "▸ " } else { "  " }, style),
            Span::styled(if is_current { "[x] " } else { "[ ] " }, style),
            Span::styled(format!("{:20}", name), style),
            Span::styled(*desc, Style::new().fg(Color::DarkGray)),
        ])));
    }

    let list = List::new(items).block(
        Block::bordered()
            .title("Table")
            .title_bottom("Enter: select  Esc: cancel")
            .border_style(if form.focus == FormFocus::RightTable {
                Style::new().fg(Color::Cyan)
            } else {
                Style::new().fg(Color::DarkGray)
            })
    );
    frame.render_widget(list, area);
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
```

- [ ] **Step 2: Verify compilation and manual test**

Run: `cargo build && cargo run -- configure --config /tmp/test-addrust.toml`
Test: Open a standardize step, navigate to Table, Enter. See table list with descriptions. Enter to select. Esc to cancel.

- [ ] **Step 3: Commit**

```bash
git add src/tui.rs
git commit -m "feat: table picker in form right panel"
```

---

## Chunk 5: Cleanup & Remove Old Wizard

### Task 12: Remove old wizard and step detail view

**Files:**
- Modify: `src/tui.rs` (remove WizardState, WizardAccumulator, handle_wizard_key, render_wizard, render_step_detail, handle_step_detail_key, related InputMode variants)

- [ ] **Step 1: Remove old wizard code**

Also update remaining references to removed state:
- Delete handler (~line 597): remove `app.custom_step_defs.remove(&label)` — custom steps are now tracked in `StepState.def`
- `confirm_delete` rendering: update `app.steps[del_idx].label` → `app.steps[del_idx].label()`
- Any remaining `app.steps[i].group` / `.action_desc` / `.pattern_template` → use the StepState accessor methods

Remove or comment out:
- `WizardState` enum (lines 58-80)
- `WizardAccumulator` struct (lines 45-56)
- `InputMode::AddStep` variant (line 42)
- `InputMode::EditPattern` variant (line 40)
- `handle_wizard_key()` function (lines 1274-1638)
- `handle_wizard_text_edit()` function (lines 1221-1273)
- `render_wizard()` function (lines 1999-2190)
- `render_wizard_text_input()` function (lines 2191-2223)
- `render_step_detail()` function (lines 1851-1998)
- `handle_step_detail_key()` function (lines 769-869)
- Related App fields: `step_detail_index`, `step_detail_segments`, `step_detail_selected`, `step_detail_alt_selected`, `wizard_acc`, `custom_step_defs`

Remove the corresponding dispatch branches in `run_loop` and `render()`.

- [ ] **Step 2: Remove InputMode variants that are now handled by FormState**

`EditPattern` is now handled by `FormFocus::EditingText`. Remove it and update any remaining references.

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles with no dead code warnings from removed items. There may be warnings about unused functions — clean those up.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass. The TUI is not directly tested, but the pipeline/config tests should still pass.

- [ ] **Step 5: Manual smoke test**

Run: `cargo run -- configure --config /tmp/test-addrust.toml`

Verify:
- Enter on a step opens the form (not old detail view)
- `a` opens type picker then form (not old wizard)
- All field editing works (pattern, targets, table, toggles, text fields)
- Esc closes form
- Ctrl+S saves
- Modified markers show on changed fields
- `r` resets fields on default steps

- [ ] **Step 6: Commit**

```bash
git add src/tui.rs
git commit -m "refactor: remove old wizard and step detail view, replaced by two-panel form"
```

### Task 13: Update step_order tracking for prepare steps

**Files:**
- Modify: `src/tui.rs` (to_config step_order comparison)
- Test: `tests/config.rs`

- [ ] **Step 1: Write test for step_order with prepare steps**

```rust
#[test]
fn test_prepare_steps_dont_force_step_order() {
    // Default config should not emit step_order
    let config = Config::default();
    let toml = config.to_toml();
    assert!(!toml.contains("step_order"), "Default config should not emit step_order");
}
```

- [ ] **Step 2: Verify the test passes**

Run: `cargo test test_prepare_steps_dont_force_step_order -- --nocapture`
Expected: PASS (if to_config correctly handles prepare steps in default order comparison)

If it fails, update the default order comparison in `to_config()` to include prepare step labels.

- [ ] **Step 3: Commit**

```bash
git add tests/config.rs src/tui.rs
git commit -m "test: verify prepare steps don't force step_order emission"
```

### Task 14: Final integration test

**Files:**
- Test: `tests/config.rs`

- [ ] **Step 1: Write round-trip config test with step_overrides**

```rust
#[test]
fn test_step_overrides_round_trip() {
    let toml_str = r#"
[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
skip_if_filled = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let output = config.to_toml();
    let reparsed: Config = toml::from_str(&output).unwrap();
    assert_eq!(reparsed.steps.step_overrides.len(), 1);
    let po_box = &reparsed.steps.step_overrides["po_box"];
    assert_eq!(po_box.skip_if_filled, Some(false));
}
```

- [ ] **Step 2: Run test**

Run: `cargo test test_step_overrides_round_trip -- --nocapture`
Expected: PASS

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add tests/config.rs
git commit -m "test: step_overrides config round-trip"
```

- [ ] **Step 5: Run golden tests and fix if needed**

Run: `cargo test golden`
Expected: PASS. If golden tests fail due to prepare rule ordering changes, update golden test expected outputs.

- [ ] **Step 6: Final commit if golden tests needed updates**

```bash
git add tests/
git commit -m "test: update golden tests for prepare rule migration"
```
