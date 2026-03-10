# Pipeline Refactor: Steps as Data — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Rule/Action system with a single ordered sequence of typed Steps defined in embedded TOML, with all standardization moved to explicit steps and domain knowledge consolidated in tables.

**Architecture:** Steps are defined declaratively in `data/defaults/steps.toml`, loaded at startup, and compiled into a `Vec<Step>` enum with four variants (Validate, Rewrite, Extract, Standardize). Tables gain an optional `pattern` field for extraction patterns. The pipeline executes one loop over the step sequence. User config can disable, override, reorder, and add steps.

**Tech Stack:** Rust, serde/toml for TOML deserialization, fancy-regex for pattern compilation, existing Abbreviations/AbbrTable system.

**Spec:** `docs/superpowers/specs/2026-03-10-pipeline-refactor-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/step.rs` | **Create** | Step enum, StepDef (TOML shape), StandardizeMode enum, step compilation and application |
| `data/defaults/steps.toml` | **Create** | Default step sequence (embedded via include_str!) |
| `src/tables/abbreviations.rs` | **Modify** | Add optional `pattern` and `pattern_template` fields to AbbrTable |
| `src/pipeline.rs` | **Modify** | Replace `Vec<Rule>` with `Vec<Step>`, remove Action/Rule/RuleSummary, simplify finalize() |
| `src/tables/rules.rs` | **Delete** | Replaced by TOML loader in step.rs |
| `src/tables/mod.rs` | **Modify** | Remove rules module, export step module |
| `src/config.rs` | **Modify** | Replace RulesConfig with StepsConfig (disabled, order, overrides, additions) |
| `src/tui.rs` | **Modify** | Adapt from RuleState to StepState, update detail view |
| `src/pattern.rs` | **Modify** | Minor — adapt to work with Step pattern templates |
| `src/init.rs` | **Modify** | Update default config generation for steps |
| `src/main.rs` | **Modify** | Update `list rules` → `list steps`, update CLI output |
| `src/lib.rs` | **Modify** | Add `pub mod step`, update exports |

---

## Chunk 1: Step Enum and Table Pattern Field

Foundation work — create the new Step type and extend tables. Existing code remains untouched and all existing tests continue to pass.

### Task 1.1: Add pattern field to AbbrTable

**Files:**
- Modify: `src/tables/abbreviations.rs:13-17` (AbbrTable struct)
- Modify: `src/tables/abbreviations.rs:391-402` (build_default_tables)

- [ ] **Step 1: Write failing test for table pattern field**

In `src/tables/abbreviations.rs`, add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_table_pattern_field() {
    let abbr = build_default_tables();
    let direction = abbr.get("direction").unwrap();
    // Default tables don't have patterns yet
    assert!(direction.pattern_template.is_none());
}

#[test]
fn test_table_with_pattern() {
    let table = AbbrTable::from_pairs_with_pattern(
        vec![("N", "NORTH"), ("S", "SOUTH")],
        Some(r"\b({direction})\b".to_string()),
    );
    assert_eq!(table.pattern_template.as_deref(), Some(r"\b({direction})\b"));
    assert_eq!(table.to_long("N"), Some("NORTH"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_table_pattern_field test_table_with_pattern -- --nocapture 2>&1 | tail -20`
Expected: FAIL — `pattern_template` field doesn't exist

- [ ] **Step 3: Add pattern_template field to AbbrTable**

In `src/tables/abbreviations.rs`, modify the `AbbrTable` struct:

```rust
pub struct AbbrTable {
    pub entries: Vec<Abbr>,
    short_to_long: HashMap<String, String>,
    long_to_short: HashMap<String, String>,
    /// Optional extraction pattern template for this table (e.g., `\b({suffix_common})\s*$`).
    /// Used by Extract steps that reference this table.
    pub pattern_template: Option<String>,
}
```

Update `AbbrTable::new()` (or wherever it's constructed) to set `pattern_template: None`. Add the constructor:

Note: `AbbrTable` does not have a `from_pairs` constructor — it has `AbbrTable::new(entries: Vec<Abbr>)`. Add a `from_pairs` helper and `from_pairs_with_pattern`:

```rust
pub fn from_pairs(pairs: Vec<(&str, &str)>) -> Self {
    let entries = pairs.into_iter()
        .map(|(s, l)| Abbr { short: s.to_string(), long: l.to_string() })
        .collect();
    Self::new(entries)
}

pub fn from_pairs_with_pattern(pairs: Vec<(&str, &str)>, pattern_template: Option<String>) -> Self {
    let mut table = Self::from_pairs(pairs);
    table.pattern_template = pattern_template;
    table
}
```

Also update the existing `AbbrTable::new()` to initialize `pattern_template: None`.

Make sure `Clone` derive still works (it should — `Option<String>` is Clone).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_table_pattern_field test_table_with_pattern -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All 86 tests pass (no existing behavior changed)

- [ ] **Step 6: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "feat: add optional pattern_template field to AbbrTable"
```

### Task 1.2: Create Step enum and StepDef

**Files:**
- Create: `src/step.rs`
- Modify: `src/lib.rs` (add module)

- [ ] **Step 1: Write the Step enum and StepDef types**

Create `src/step.rs`:

```rust
use std::collections::HashMap;

use fancy_regex::Regex;
use serde::Deserialize;

use crate::address::Field;
use crate::config::OutputFormat;
use crate::ops::{extract_remove, none_if_empty, replace_pattern, squish};
use crate::tables::abbreviations::Abbreviations;

/// How a Standardize step matches values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardizeMode {
    /// Whole-field lookup (suffix, direction).
    WholeField,
    /// Per-word lookup within the field value (street_name_abbr).
    PerWord,
}

/// A single pipeline step — compiled and ready to execute.
#[derive(Debug)]
pub enum Step {
    Validate {
        label: String,
        pattern: Regex,
        pattern_template: String,
        warning: String,
        clear: bool,
        enabled: bool,
    },
    Rewrite {
        label: String,
        pattern: Regex,
        pattern_template: String,
        /// Simple replacement string (for most rewrites).
        replacement: Option<String>,
        /// Table name for per-value replacement (e.g., street_name_abbr: MT→MOUNT).
        rewrite_table: Option<String>,
        enabled: bool,
    },
    Extract {
        label: String,
        pattern: Regex,
        pattern_template: String,
        target: Field,
        skip_if_filled: bool,
        /// Optional regex replacement on extracted value (structural reformatting).
        replacement: Option<(Regex, String)>,
        enabled: bool,
    },
    Standardize {
        label: String,
        target: Field,
        matching_table: String,
        format_table: String,
        mode: StandardizeMode,
        enabled: bool,
    },
}

impl Step {
    pub fn label(&self) -> &str {
        match self {
            Step::Validate { label, .. }
            | Step::Rewrite { label, .. }
            | Step::Extract { label, .. }
            | Step::Standardize { label, .. } => label,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Step::Validate { enabled, .. }
            | Step::Rewrite { enabled, .. }
            | Step::Extract { enabled, .. }
            | Step::Standardize { enabled, .. } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, val: bool) {
        match self {
            Step::Validate { enabled, .. }
            | Step::Rewrite { enabled, .. }
            | Step::Extract { enabled, .. }
            | Step::Standardize { enabled, .. } => *enabled = val,
        }
    }

    pub fn pattern_template(&self) -> Option<&str> {
        match self {
            Step::Validate { pattern_template, .. }
            | Step::Rewrite { pattern_template, .. }
            | Step::Extract { pattern_template, .. } => Some(pattern_template),
            Step::Standardize { .. } => None,
        }
    }

    pub fn step_type(&self) -> &'static str {
        match self {
            Step::Validate { .. } => "validate",
            Step::Rewrite { .. } => "rewrite",
            Step::Extract { .. } => "extract",
            Step::Standardize { .. } => "standardize",
        }
    }
}

/// TOML-deserializable step definition (before regex compilation).
#[derive(Debug, Deserialize, Clone)]
pub struct StepDef {
    #[serde(rename = "type")]
    pub step_type: String,
    pub label: String,
    /// Regex pattern (for validate, rewrite, extract without table).
    pub pattern: Option<String>,
    /// Table name (for extract-from-table, standardize).
    pub table: Option<String>,
    /// Target field name (for extract, standardize).
    pub target: Option<String>,
    /// Replacement string (for rewrite, extract structural reformatting).
    pub replacement: Option<String>,
    /// Warning label (for validate).
    pub warning: Option<String>,
    /// Whether to clear working string on match (for validate).
    pub clear: Option<bool>,
    /// Skip if target field already filled (for extract).
    pub skip_if_filled: Option<bool>,
    /// Matching table name (for standardize).
    pub matching_table: Option<String>,
    /// Format table name (for standardize).
    pub format_table: Option<String>,
    /// Standardize mode: "whole_field" or "per_word".
    pub mode: Option<String>,
}

/// Top-level TOML wrapper for step definitions.
#[derive(Debug, Deserialize)]
pub struct StepsDef {
    pub step: Vec<StepDef>,
}

/// Parse a field name string to the Field enum.
fn parse_field(name: &str) -> Field {
    match name {
        "street_number" => Field::StreetNumber,
        "pre_direction" => Field::PreDirection,
        "street_name" => Field::StreetName,
        "suffix" => Field::Suffix,
        "post_direction" => Field::PostDirection,
        "unit" => Field::Unit,
        "unit_type" => Field::UnitType,
        "po_box" => Field::PoBox,
        "building" => Field::Building,
        "extra_front" => Field::ExtraFront,
        "extra_back" => Field::ExtraBack,
        _ => panic!("Unknown field name: {}", name),
    }
}
```

- [ ] **Step 2: Add the module to lib.rs**

In `src/lib.rs`, add:

```rust
pub mod step;
```

- [ ] **Step 3: Write tests for StepDef deserialization**

Add to `src/step.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_def_deserialize_extract() {
        let toml_str = r#"
[[step]]
type = "extract"
label = "po_box"
table = "po_box"
target = "po_box"
skip_if_filled = true
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(defs.step.len(), 1);
        assert_eq!(defs.step[0].step_type, "extract");
        assert_eq!(defs.step[0].label, "po_box");
        assert_eq!(defs.step[0].table.as_deref(), Some("po_box"));
        assert_eq!(defs.step[0].target.as_deref(), Some("po_box"));
        assert_eq!(defs.step[0].skip_if_filled, Some(true));
    }

    #[test]
    fn test_step_def_deserialize_rewrite() {
        let toml_str = r#"
[[step]]
type = "rewrite"
label = "unstick"
pattern = '\b(ST)(APT)\b'
replacement = '$1 $2'
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(defs.step[0].step_type, "rewrite");
        assert_eq!(defs.step[0].replacement.as_deref(), Some("$1 $2"));
    }

    #[test]
    fn test_step_def_deserialize_validate() {
        let toml_str = r#"
[[step]]
type = "validate"
label = "na_check"
pattern = '(?i)^(N/?A)$'
warning = "na_address"
clear = true
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(defs.step[0].warning.as_deref(), Some("na_address"));
        assert_eq!(defs.step[0].clear, Some(true));
    }

    #[test]
    fn test_step_def_deserialize_standardize() {
        let toml_str = r#"
[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
matching_table = "suffix_all"
format_table = "suffix_usps"
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(defs.step[0].matching_table.as_deref(), Some("suffix_all"));
        assert_eq!(defs.step[0].format_table.as_deref(), Some("suffix_usps"));
    }

    #[test]
    fn test_parse_field() {
        assert_eq!(parse_field("suffix"), Field::Suffix);
        assert_eq!(parse_field("po_box"), Field::PoBox);
        assert_eq!(parse_field("street_number"), Field::StreetNumber);
    }

    #[test]
    fn test_step_accessors() {
        let step = Step::Rewrite {
            label: "test".to_string(),
            pattern: Regex::new("x").unwrap(),
            pattern_template: "x".to_string(),
            replacement: "y".to_string(),
            enabled: true,
        };
        assert_eq!(step.label(), "test");
        assert_eq!(step.step_type(), "rewrite");
        assert!(step.enabled());
        assert_eq!(step.pattern_template(), Some("x"));
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test step::tests -- --nocapture 2>&1 | tail -20`
Expected: All pass

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All 86+ tests pass

- [ ] **Step 6: Commit**

```bash
git add src/step.rs src/lib.rs
git commit -m "feat: add Step enum and StepDef TOML types"
```

### Task 1.3: Write default steps.toml

**Files:**
- Create: `data/defaults/steps.toml`

- [ ] **Step 1: Create the default steps TOML file**

Create `data/defaults/steps.toml` with all current rules translated to step definitions. This should produce identical parsing results to the current `build_rules()`. Reference the spec for the full step sequence.

```toml
# addrust default pipeline steps
# Order matters — steps execute top to bottom.

# --- Validation ---
[[step]]
type = "validate"
label = "na_check"
pattern = '(?i)^(N/?A|{na_values})$'
warning = "na_address"
clear = true

# --- City / State / Zip ---
[[step]]
type = "extract"
label = "city_state_zip"
pattern = ',\s*([A-Z][A-Z ]+)\W+{state}\W+(\d{5}(?:\W\d{4})?)(?:\s*US)?$'
target = "extra_back"

# --- PO Box ---
# Note: These carry inline replacement to standardize "P O BOX" → "PO BOX" during
# extraction, matching the current behavior. Chunk 6 consolidates this into a single
# extract + standardize step. Until then, inline replacement keeps golden tests passing.
[[step]]
type = "extract"
label = "po_box"
pattern = '\bP\W*O\W*BOX\W*(\d+)\b'
target = "po_box"
skip_if_filled = true
replacement = 'PO BOX $1'

[[step]]
type = "extract"
label = "po_box_word"
pattern = '\bP\W*O\W*BOX\W+(\w+)\b'
target = "po_box"
skip_if_filled = true
replacement = 'PO BOX $1'

# --- Pre-processing rewrites ---
[[step]]
type = "rewrite"
label = "unstick_suffix_unit"
pattern = '\b({suffix_common})({unit_type})\b'
replacement = '$1 $2'

[[step]]
type = "rewrite"
label = "st_to_saint"
pattern = '^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{suffix_all})\b)([A-Z]{3,20})'
replacement = '${1}SAINT $2'

# --- Extra front ---
[[step]]
type = "extract"
label = "extra_front"
pattern = '^(?:(?:[A-Z\W]+\s)+(?=(?:{direction})\s\d))|^(?:(?:[A-Z\W]+\s)+(?=\d))'
target = "extra_front"
skip_if_filled = true

# --- Street number ---
[[step]]
type = "extract"
label = "street_number_coords"
pattern = '^([NSEW])\W?(\d+)\W?([NSEW])\W?(\d+)\b'
target = "street_number"
skip_if_filled = true
replacement = '${1}${2} ${3}${4}'

[[step]]
type = "extract"
label = "street_number"
pattern = '^\d+\b'
target = "street_number"
skip_if_filled = true

[[step]]
type = "extract"
label = "unit_fraction"
pattern = '^[1-9]/\d+\b'
target = "unit"
skip_if_filled = true

# --- Unit ---
[[step]]
type = "extract"
label = "unit_type_value"
pattern = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$'
target = "unit"
skip_if_filled = true

[[step]]
type = "extract"
label = "unit_pound"
pattern = '#\W*(\w+)\s*$'
target = "unit"
skip_if_filled = true

[[step]]
type = "extract"
label = "unit_location"
pattern = '\b({unit_location})\s*$'
target = "unit"
skip_if_filled = true

# --- Direction ---
[[step]]
type = "extract"
label = "post_direction"
pattern = '(?<!^)\b({direction})\s*$'
target = "post_direction"
skip_if_filled = true

# --- Suffix ---
[[step]]
type = "extract"
label = "suffix_common"
pattern = '(?<!^)\b({suffix_common})\s*$'
target = "suffix"
skip_if_filled = true

[[step]]
type = "extract"
label = "suffix_all"
pattern = '(?<!^)\b({suffix_all})\s*$'
target = "suffix"
skip_if_filled = true

# --- Pre-direction ---
[[step]]
type = "extract"
label = "pre_direction"
pattern = '^\b({direction})\b(?!$)'
target = "pre_direction"
skip_if_filled = true

# --- Street name cleanup ---
[[step]]
type = "rewrite"
label = "street_name_abbr"
pattern = '\b({street_name_abbr$short})\b'
table = "street_name_abbr"

[[step]]
type = "rewrite"
label = "name_st_to_saint"
pattern = '(?:^|\s)ST\b(?=\s[A-Z]{3,})'
replacement = 'SAINT'

# --- Standardization ---
[[step]]
type = "standardize"
label = "standardize_pre_direction"
target = "pre_direction"
matching_table = "direction"
format_table = "direction"

[[step]]
type = "standardize"
label = "standardize_post_direction"
target = "post_direction"
matching_table = "direction"
format_table = "direction"

[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
matching_table = "suffix_all"
format_table = "suffix_usps"

[[step]]
type = "standardize"
label = "standardize_unit_location"
target = "unit"
matching_table = "unit_location"
format_table = "unit_location"

[[step]]
type = "standardize"
label = "standardize_po_box"
target = "po_box"
pattern = 'P\W*O\W*BOX\W*(\w+)'
replacement = 'PO BOX $1'
```

Note: The `street_name_abbr` rewrite step uses a `table` field instead of `replacement`. When a Rewrite step has `table`, it does per-value lookup replacement from the table (MT→MOUNT, FT→FORT). This is handled in `compile_step` from the start — see Task 1.4.

Also note: `standardize_po_box` uses pattern/replacement instead of table lookup because PO box standardization is regex reformatting, not a short↔long lookup. The Standardize step type will support both modes.

- [ ] **Step 2: Verify TOML parses**

Add a test in `src/step.rs`:

```rust
#[test]
fn test_default_steps_toml_parses() {
    let toml_str = include_str!("../data/defaults/steps.toml");
    let defs: StepsDef = toml::from_str(toml_str).unwrap();
    assert!(defs.step.len() > 20, "Expected 20+ steps, got {}", defs.step.len());

    // Check first step is validate
    assert_eq!(defs.step[0].step_type, "validate");
    assert_eq!(defs.step[0].label, "na_check");

    // Check last step is standardize
    let last = defs.step.last().unwrap();
    assert_eq!(last.step_type, "standardize");
}
```

- [ ] **Step 3: Run test**

Run: `cargo test test_default_steps_toml_parses -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add data/defaults/steps.toml src/step.rs
git commit -m "feat: add default steps.toml definition file"
```

### Task 1.4: Step compilation (StepDef → Step)

**Files:**
- Modify: `src/step.rs`

This is the key function that takes TOML definitions + abbreviation tables and produces compiled Steps with expanded regex patterns.

- [ ] **Step 1: Write failing test for step compilation**

Add to `src/step.rs` tests:

```rust
#[test]
fn test_compile_rewrite_step() {
    use crate::tables::abbreviations::build_default_tables;

    let def = StepDef {
        step_type: "rewrite".to_string(),
        label: "test_rewrite".to_string(),
        pattern: Some(r"\b({direction})\b".to_string()),
        replacement: Some("$1".to_string()),
        table: None,
        target: None,
        warning: None,
        clear: None,
        skip_if_filled: None,
        matching_table: None,
        format_table: None,
        mode: None,
    };

    let abbr = build_default_tables();
    let step = compile_step(&def, &abbr).unwrap();

    assert_eq!(step.label(), "test_rewrite");
    assert_eq!(step.step_type(), "rewrite");
    // Pattern should be expanded (no {direction} placeholder)
    if let Step::Rewrite { pattern_template, .. } = &step {
        assert!(pattern_template.contains("{direction}"));
    }
}

#[test]
fn test_compile_extract_step() {
    use crate::tables::abbreviations::build_default_tables;

    let def = StepDef {
        step_type: "extract".to_string(),
        label: "test_suffix".to_string(),
        pattern: Some(r"(?<!^)\b({suffix_common})\s*$".to_string()),
        replacement: None,
        table: None,
        target: Some("suffix".to_string()),
        warning: None,
        clear: None,
        skip_if_filled: Some(true),
        matching_table: None,
        format_table: None,
        mode: None,
    };

    let abbr = build_default_tables();
    let step = compile_step(&def, &abbr).unwrap();

    if let Step::Extract { target, skip_if_filled, .. } = &step {
        assert_eq!(*target, Field::Suffix);
        assert!(*skip_if_filled);
    } else {
        panic!("Expected Extract step");
    }
}

#[test]
fn test_compile_standardize_step() {
    use crate::tables::abbreviations::build_default_tables;

    let def = StepDef {
        step_type: "standardize".to_string(),
        label: "std_suffix".to_string(),
        pattern: None,
        replacement: None,
        table: None,
        target: Some("suffix".to_string()),
        warning: None,
        clear: None,
        skip_if_filled: None,
        matching_table: Some("suffix_all".to_string()),
        format_table: Some("suffix_usps".to_string()),
        mode: None,
    };

    let abbr = build_default_tables();
    let step = compile_step(&def, &abbr).unwrap();

    if let Step::Standardize { target, matching_table, format_table, mode, .. } = &step {
        assert_eq!(*target, Field::Suffix);
        assert_eq!(matching_table, "suffix_all");
        assert_eq!(format_table, "suffix_usps");
        assert_eq!(*mode, StandardizeMode::WholeField);
    } else {
        panic!("Expected Standardize step");
    }
}

#[test]
fn test_compile_all_default_steps() {
    use crate::tables::abbreviations::build_default_tables;

    let toml_str = include_str!("../data/defaults/steps.toml");
    let defs: StepsDef = toml::from_str(toml_str).unwrap();
    let abbr = build_default_tables();

    let steps = compile_steps(&defs.step, &abbr);
    assert!(steps.len() > 20);

    // Verify each step compiled with correct type
    assert_eq!(steps[0].step_type(), "validate");
    assert_eq!(steps[0].label(), "na_check");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_compile -- --nocapture 2>&1 | tail -20`
Expected: FAIL — `compile_step` doesn't exist

- [ ] **Step 3: Implement compile_step and compile_steps**

Add to `src/step.rs` (before the tests module):

```rust
use crate::tables::rules::expand_template;

/// Compile a single StepDef into a Step, expanding table references in patterns.
pub fn compile_step(def: &StepDef, abbr: &Abbreviations) -> Result<Step, String> {
    match def.step_type.as_str() {
        "validate" => {
            let template = def.pattern.as_ref()
                .ok_or_else(|| format!("validate step '{}' missing pattern", def.label))?;
            let expanded = expand_template(template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;
            Ok(Step::Validate {
                label: def.label.clone(),
                pattern,
                pattern_template: template.clone(),
                warning: def.warning.clone().unwrap_or_else(|| def.label.clone()),
                clear: def.clear.unwrap_or(false),
                enabled: true,
            })
        }
        "rewrite" => {
            let template = def.pattern.as_ref()
                .ok_or_else(|| format!("rewrite step '{}' missing pattern", def.label))?;
            let expanded = expand_template(template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;
            Ok(Step::Rewrite {
                label: def.label.clone(),
                pattern,
                pattern_template: template.clone(),
                replacement: def.replacement.clone(),
                rewrite_table: def.table.clone(),
                enabled: true,
            })
        }
        "extract" => {
            let template = if let Some(ref p) = def.pattern {
                p.clone()
            } else if let Some(ref table_name) = def.table {
                // Get pattern from the table
                let table = abbr.get(table_name)
                    .ok_or_else(|| format!("extract step '{}' references unknown table '{}'", def.label, table_name))?;
                table.pattern_template.as_ref()
                    .ok_or_else(|| format!("table '{}' has no pattern_template", table_name))?
                    .clone()
            } else {
                return Err(format!("extract step '{}' needs either pattern or table", def.label));
            };

            let expanded = expand_template(&template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;

            let target = def.target.as_ref()
                .ok_or_else(|| format!("extract step '{}' missing target", def.label))?;

            let replacement = if let Some(ref r) = def.replacement {
                let expanded_r = expand_template(r, abbr);
                // For extract replacement, the pattern is used to match the extracted value
                // and the replacement restructures it
                Some((
                    Regex::new(&expanded).map_err(|e| format!("Bad replacement regex in step '{}': {}", def.label, e))?,
                    expanded_r,
                ))
            } else {
                None
            };

            Ok(Step::Extract {
                label: def.label.clone(),
                pattern,
                pattern_template: template,
                target: parse_field(target),
                skip_if_filled: def.skip_if_filled.unwrap_or(false),
                replacement,
                enabled: true,
            })
        }
        "standardize" => {
            let target = def.target.as_ref()
                .ok_or_else(|| format!("standardize step '{}' missing target", def.label))?;
            let matching = def.matching_table.as_ref()
                .ok_or_else(|| format!("standardize step '{}' missing matching_table", def.label))?;
            let format = def.format_table.as_ref()
                .ok_or_else(|| format!("standardize step '{}' missing format_table", def.label))?;
            let mode = match def.mode.as_deref() {
                Some("per_word") => StandardizeMode::PerWord,
                _ => StandardizeMode::WholeField,
            };

            Ok(Step::Standardize {
                label: def.label.clone(),
                target: parse_field(target),
                matching_table: matching.clone(),
                format_table: format.clone(),
                mode,
                enabled: true,
            })
        }
        other => Err(format!("Unknown step type '{}' in step '{}'", other, def.label)),
    }
}

/// Compile all step definitions into executable Steps.
pub fn compile_steps(defs: &[StepDef], abbr: &Abbreviations) -> Vec<Step> {
    defs.iter()
        .map(|d| compile_step(d, abbr).unwrap_or_else(|e| panic!("{}", e)))
        .collect()
}
```

Note: This imports `expand_template` from `tables::rules`. First, add `pub use rules::expand_template;` to `src/tables/mod.rs` so it's accessible as `crate::tables::expand_template`. The function will be moved to `step.rs` later (Task 3.1) when `tables::rules` is deleted.

Also: create the `data/defaults/` directory before writing `steps.toml` (it doesn't exist yet).

- [ ] **Step 4: Run compilation tests**

Run: `cargo test test_compile -- --nocapture 2>&1 | tail -30`
Expected: All compile tests pass

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/step.rs
git commit -m "feat: add step compilation from TOML definitions"
```

### Task 1.5: Step application

**Files:**
- Modify: `src/step.rs`

- [ ] **Step 1: Write failing test for step application**

Add to `src/step.rs` tests:

```rust
#[test]
fn test_apply_validate_step() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;

    let abbr = build_default_tables();
    let toml_str = r#"
[[step]]
type = "validate"
label = "na_check"
pattern = '(?i)^(N/?A)$'
warning = "na_address"
clear = true
"#;
    let defs: StepsDef = toml::from_str(toml_str).unwrap();
    let steps = compile_steps(&defs.step, &abbr);

    let mut state = AddressState::new_from_prepared("N/A".to_string());
    let output = OutputConfig::default();
    apply_step(&mut state, &steps[0], &abbr, &output);

    assert!(state.fields.warnings.contains(&"na_address".to_string()));
    assert!(state.working.is_empty());
}

#[test]
fn test_apply_rewrite_step() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;

    let abbr = build_default_tables();
    let def = StepDef {
        step_type: "rewrite".to_string(),
        label: "test_rewrite".to_string(),
        pattern: Some(r"STAPT".to_string()),
        replacement: Some("ST APT".to_string()),
        table: None, target: None, warning: None, clear: None,
        skip_if_filled: None, matching_table: None, format_table: None, mode: None,
    };
    let step = compile_step(&def, &abbr).unwrap();

    let mut state = AddressState::new_from_prepared("123 N STAPT 4B".to_string());
    let output = OutputConfig::default();
    apply_step(&mut state, &step, &abbr, &output);

    assert_eq!(state.working, "123 N ST APT 4B");
}

#[test]
fn test_apply_rewrite_from_table() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;

    let abbr = build_default_tables();
    let toml_str = r#"
[[step]]
type = "rewrite"
label = "street_name_abbr"
pattern = '\b({street_name_abbr$short})\b'
table = "street_name_abbr"
"#;
    let defs: StepsDef = toml::from_str(toml_str).unwrap();
    let steps = compile_steps(&defs.step, &abbr);

    let mut state = AddressState::new_from_prepared("MT VERNON".to_string());
    let output = OutputConfig::default();
    apply_step(&mut state, &steps[0], &abbr, &output);

    assert_eq!(state.working, "MOUNT VERNON");
}

#[test]
fn test_apply_extract_step() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;

    let abbr = build_default_tables();
    let def = StepDef {
        step_type: "extract".to_string(),
        label: "test_number".to_string(),
        pattern: Some(r"^\d+\b".to_string()),
        replacement: None,
        table: None, target: Some("street_number".to_string()),
        warning: None, clear: None,
        skip_if_filled: Some(true),
        matching_table: None, format_table: None, mode: None,
    };
    let step = compile_step(&def, &abbr).unwrap();

    let mut state = AddressState::new_from_prepared("123 MAIN ST".to_string());
    let output = OutputConfig::default();
    apply_step(&mut state, &step, &abbr, &output);

    assert_eq!(state.fields.street_number.as_deref(), Some("123"));
    assert_eq!(state.working, "MAIN ST");
}

#[test]
fn test_apply_standardize_step() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;

    let abbr = build_default_tables();
    let def = StepDef {
        step_type: "standardize".to_string(),
        label: "std_suffix".to_string(),
        pattern: None, replacement: None, table: None,
        target: Some("suffix".to_string()),
        warning: None, clear: None, skip_if_filled: None,
        matching_table: Some("suffix_all".to_string()),
        format_table: Some("suffix_usps".to_string()),
        mode: None,
    };
    let step = compile_step(&def, &abbr).unwrap();

    let mut state = AddressState::new_from_prepared(String::new());
    state.fields.suffix = Some("ST".to_string());
    let output = OutputConfig::default(); // suffix default is Long
    apply_step(&mut state, &step, &abbr, &output);

    assert_eq!(state.fields.suffix.as_deref(), Some("STREET"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_apply_ -- --nocapture 2>&1 | tail -20`
Expected: FAIL — `apply_step` doesn't exist

- [ ] **Step 3: Implement apply_step**

Add to `src/step.rs`:

```rust
/// Standardize a value using two-step canonicalize→format flow.
fn standardize_value(
    value: &str,
    matching_table: &crate::tables::abbreviations::AbbrTable,
    canonical_table: &crate::tables::abbreviations::AbbrTable,
    format: OutputFormat,
) -> String {
    let short = matching_table.to_short(value).unwrap_or(value);
    match format {
        OutputFormat::Short => short.to_string(),
        OutputFormat::Long => canonical_table
            .to_long(short)
            .unwrap_or(short)
            .to_string(),
    }
}

/// Apply a single step to an address state.
pub fn apply_step(
    state: &mut crate::address::AddressState,
    step: &Step,
    tables: &Abbreviations,
    output: &crate::config::OutputConfig,
) {
    if !step.enabled() {
        return;
    }

    match step {
        Step::Validate { pattern, warning, clear, .. } => {
            if pattern.is_match(&state.working).unwrap_or(false) {
                state.fields.warnings.push(warning.clone());
                if *clear {
                    state.working.clear();
                }
            }
        }
        Step::Rewrite { pattern, replacement, rewrite_table, .. } => {
            if !pattern.is_match(&state.working).unwrap_or(false) {
                return;
            }
            if let Some(ref table_name) = rewrite_table {
                // Table-driven rewrite: replace each matched value with its long form
                if let Some(table) = tables.get(table_name) {
                    for (short, long) in table.short_to_long_pairs() {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        replace_pattern(&mut state.working, &re, &long);
                    }
                }
            } else if let Some(ref repl) = replacement {
                replace_pattern(&mut state.working, pattern, repl);
            }
            squish(&mut state.working);
        }
        Step::Extract { pattern, target, skip_if_filled, replacement, .. } => {
            if *skip_if_filled {
                if state.fields.field(*target).is_some() {
                    return;
                }
            }
            if let Some(mut val) = extract_remove(&mut state.working, pattern) {
                if let Some((ref re, ref repl)) = replacement {
                    replace_pattern(&mut val, re, repl);
                }
                *state.fields.field_mut(*target) = none_if_empty(val);
            }
        }
        Step::Standardize { target, matching_table, format_table, mode, .. } => {
            let val = match state.fields.field(*target) {
                Some(v) => v.to_string(),
                None => return,
            };
            let m = match tables.get(matching_table) {
                Some(t) => t,
                None => return,
            };
            let c = match tables.get(format_table) {
                Some(t) => t,
                None => return,
            };
            let fmt = output.format_for_field(*target);

            match mode {
                StandardizeMode::WholeField => {
                    *state.fields.field_mut(*target) = Some(standardize_value(&val, m, c, fmt));
                }
                StandardizeMode::PerWord => {
                    let mut result = val.clone();
                    for (short, long) in m.short_to_long_pairs() {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        replace_pattern(&mut result, &re, &long);
                    }
                    *state.fields.field_mut(*target) = none_if_empty(result);
                }
            }
        }
    }
}
```

Note: This requires a `format_for_field(Field) -> OutputFormat` method on `OutputConfig`. Add it:

In `src/config.rs`, add to `impl OutputConfig`:

```rust
/// Get the output format for a given field.
pub fn format_for_field(&self, field: crate::address::Field) -> OutputFormat {
    use crate::address::Field;
    match field {
        Field::Suffix => self.suffix,
        Field::PreDirection | Field::PostDirection => self.direction,
        Field::Unit => self.unit_location,  // only applies when unit is a location value
        _ => OutputFormat::Long, // default for fields without format config
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_apply_ -- --nocapture 2>&1 | tail -30`
Expected: All pass

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass (old code untouched)

- [ ] **Step 6: Commit**

```bash
git add src/step.rs src/config.rs
git commit -m "feat: implement step application logic"
```

---

## Chunk 2: Pipeline Switchover

Replace the Rule-based pipeline with the Step-based pipeline. This is the critical chunk — the golden tests are the gate.

### Task 2.1: Add step-based parse path to Pipeline

**Files:**
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Write a test that uses the new step-based parsing**

Add to `src/pipeline.rs` tests:

```rust
#[test]
fn test_step_pipeline_basic() {
    let p = Pipeline::from_steps_default();
    let addr = p.parse("123 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_step_pipeline_basic -- --nocapture 2>&1 | tail -20`
Expected: FAIL — `from_steps_default` doesn't exist

- [ ] **Step 3: Add from_steps_default constructor**

Add to `Pipeline` impl in `src/pipeline.rs`:

```rust
/// Build pipeline from embedded default steps.toml.
pub fn from_steps_default() -> Self {
    use crate::step::{compile_steps, StepsDef};
    use crate::tables::abbreviations::build_default_tables;

    let tables = build_default_tables();
    let toml_str = include_str!("../data/defaults/steps.toml");
    let defs: StepsDef = toml::from_str(toml_str)
        .expect("Failed to parse default steps.toml");
    let steps = compile_steps(&defs.step, &tables);

    Self {
        rules: Vec::new(),  // empty — not used in step mode
        steps,
        output: crate::config::OutputConfig::default(),
        tables,
        use_steps: true,
    }
}
```

Add `steps: Vec<Step>` and `use_steps: bool` fields to the `Pipeline` struct. Update the `parse()` method to branch:

```rust
pub fn parse(&self, input: &str) -> Address {
    let prepared = match prepare::prepare(input) {
        Some(s) => s,
        None => {
            let mut addr = Address::default();
            addr.warnings.push("na_address".to_string());
            return addr;
        }
    };

    let mut state = AddressState::new_from_prepared(prepared);

    if self.use_steps {
        for step in &self.steps {
            crate::step::apply_step(&mut state, step, &self.tables, &self.output);
        }
    } else {
        for rule in &self.rules {
            apply_rule(&mut state, rule);
        }
    }

    self.finalize(&mut state);
    state.fields
}
```

Update `Default`, `new()`, and `from_config()` to set `use_steps: false` and `steps: Vec::new()` so existing code still works.

- [ ] **Step 4: Run the test**

Run: `cargo test test_step_pipeline_basic -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Add more step-based tests matching existing rule tests**

```rust
#[test]
fn test_step_pipeline_with_direction() {
    let p = Pipeline::from_steps_default();
    let addr = p.parse("123 N Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("N"));
    assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
}

#[test]
fn test_step_pipeline_with_unit() {
    let p = Pipeline::from_steps_default();
    let addr = p.parse("123 Main St Apt 4B");
    assert_eq!(addr.unit.as_deref(), Some("4B"));
}

#[test]
fn test_step_pipeline_po_box() {
    let p = Pipeline::from_steps_default();
    let addr = p.parse("PO BOX 123");
    assert_eq!(addr.po_box.as_deref(), Some("PO BOX 123"));
}
```

- [ ] **Step 6: Run all new tests**

Run: `cargo test test_step_pipeline -- --nocapture 2>&1 | tail -30`
Expected: All pass. If any fail, debug and fix the steps.toml or apply_step logic until they match the old behavior.

- [ ] **Step 7: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat: add step-based parse path alongside rule-based"
```

### Task 2.2: Run golden tests against step pipeline

**Files:**
- Modify: `tests/golden.rs`

- [ ] **Step 1: Add a golden test that uses the step pipeline**

Add to `tests/golden.rs`:

```rust
#[test]
fn test_golden_dataset_steps() {
    let pipeline = addrust::pipeline::Pipeline::from_steps_default();
    // ... same golden test logic as test_golden_dataset but using step pipeline
    // Copy the existing test body, just change Pipeline::default() to Pipeline::from_steps_default()
}
```

- [ ] **Step 2: Run the golden test**

Run: `cargo test test_golden_dataset_steps -- --nocapture 2>&1 | tail -40`
Expected: PASS. If there are failures, they indicate behavioral differences between the rule and step pipelines. Fix steps.toml and apply_step until all golden tests pass.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass (both old rule-based and new step-based)

- [ ] **Step 4: Commit**

```bash
git add tests/golden.rs
git commit -m "test: add golden dataset test for step-based pipeline"
```

### Task 2.3: Add from_steps_config constructor

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `src/config.rs`

- [ ] **Step 1: Extend Config for step overrides**

Add to `src/config.rs`:

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StepsConfig {
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub pattern_overrides: HashMap<String, String>,
}
```

Add `steps: StepsConfig` field to `Config` struct (with `#[serde(default)]`).

- [ ] **Step 2: Add from_steps_config to Pipeline**

```rust
pub fn from_steps_config(config: &crate::config::Config) -> Self {
    use crate::step::{compile_steps, StepsDef};
    use crate::tables::abbreviations::build_default_tables;

    let tables = build_default_tables();
    let tables = if config.dictionaries.is_empty() {
        tables
    } else {
        tables.patch(&config.dictionaries)
    };

    let toml_str = include_str!("../data/defaults/steps.toml");
    let mut defs: StepsDef = toml::from_str(toml_str)
        .expect("Failed to parse default steps.toml");

    // Apply pattern overrides
    for def in &mut defs.step {
        if let Some(override_pattern) = config.steps.pattern_overrides.get(&def.label) {
            def.pattern = Some(override_pattern.clone());
        }
    }

    let mut steps = compile_steps(&defs.step, &tables);

    // Apply disabled list
    for step in &mut steps {
        if config.steps.disabled.contains(&step.label().to_string()) {
            step.set_enabled(false);
        }
    }

    Self {
        rules: Vec::new(),
        steps,
        output: config.output.clone(),
        tables,
        use_steps: true,
    }
}
```

- [ ] **Step 3: Write test for config-based step pipeline**

```rust
#[test]
fn test_step_pipeline_from_config_disabled() {
    let toml_str = r#"
[steps]
disabled = ["suffix_common", "suffix_all"]
"#;
    let config: crate::config::Config = toml::from_str(toml_str).unwrap();
    let p = Pipeline::from_steps_config(&config);
    let addr = p.parse("123 Main St");
    assert!(addr.suffix.is_none());
    assert_eq!(addr.street_name.as_deref(), Some("MAIN ST"));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_step_pipeline_from_config -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/pipeline.rs src/config.rs
git commit -m "feat: add config-based step pipeline with disable/override support"
```

### Task 2.4: Switch default pipeline to steps

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Switch Pipeline::default() to use steps**

Change `Default` impl to use `from_steps_default()`:

```rust
impl Default for Pipeline {
    fn default() -> Self {
        Self::from_steps_default()
    }
}
```

Change `from_config()` to call `from_steps_config()`:

```rust
pub fn from_config(config: &crate::config::Config) -> Self {
    Self::from_steps_config(config)
}
```

- [ ] **Step 2: Run the FULL test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: ALL tests pass. This is the critical gate — every existing test (unit, integration, golden) must pass with the step-based pipeline as the default.

If any tests fail, do NOT proceed. Debug and fix until all pass.

- [ ] **Step 3: Commit**

```bash
git add src/pipeline.rs src/lib.rs
git commit -m "feat: switch default pipeline to step-based execution"
```

---

## Chunk 3: Cleanup

Remove old Rule-based code now that everything runs on Steps.

### Task 3.1: Move expand_template to step.rs

**Files:**
- Modify: `src/step.rs`
- Modify: `src/tables/rules.rs`

- [ ] **Step 1: Copy expand_template into step.rs**

Copy the `expand_template()` function and its tests from `src/tables/rules.rs` into `src/step.rs`. Make it `pub`.

- [ ] **Step 2: Update step.rs to use local expand_template**

Change the import in `compile_step` from `crate::tables::rules::expand_template` to the local function.

- [ ] **Step 3: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add src/step.rs
git commit -m "refactor: move expand_template to step module"
```

### Task 3.2: Remove Rule, Action, build_rules, and old pipeline code

**Files:**
- Delete: `src/tables/rules.rs`
- Modify: `src/tables/mod.rs`
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Remove the rules module**

Delete `src/tables/rules.rs`. In `src/tables/mod.rs`, remove `pub mod rules` and the `pub use rules::build_rules` export.

- [ ] **Step 2: Remove Rule/Action/RuleSummary from pipeline.rs**

Remove:
- `Action` enum
- `Rule` struct
- `RuleSummary` struct
- `apply_rule()` function
- `PipelineConfig` struct
- `Pipeline::new()` (the old rule-based constructor)
- `Pipeline::rule_summaries()`
- The `rules: Vec<Rule>` and `use_steps: bool` fields from Pipeline
- The `if self.use_steps` branch in `parse()` — just keep the step loop

- [ ] **Step 3: Simplify finalize()**

Remove standardization logic from `finalize()` — it's now handled by Standardize steps. Keep only:
- Assign remaining working string to street_name
- Remove leftover placeholder tags
- Strip leading zeros from street_number
- Clean `#` from unit
- Promote unit to street_number if street_number is empty

- [ ] **Step 4: Update any remaining references**

Search for `Rule`, `Action`, `build_rules`, `rule_summaries`, `PipelineConfig` across the codebase. Update or remove each reference. Key files: `main.rs`, `init.rs`, `tui.rs`, `tests/golden.rs`, `tests/config.rs`.

For `main.rs`: update `list rules` subcommand to `list steps`, use step metadata.
For `init.rs`: update to generate step-aware config.
For `tests/golden.rs`: update imports — remove `PipelineConfig`, `build_rules`, `ABBR` references. Use `Pipeline::default()` or `Pipeline::from_steps_default()`.
For `tests/config.rs`: update any tests referencing `config.rules` to use `config.steps`.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass. Some tests that directly tested Rule/build_rules will need to be removed or adapted.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: remove Rule/Action/build_rules, pipeline runs on Steps only"
```

### Task 3.3: Clean up config for steps

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Remove RulesConfig, replace with StepsConfig**

If not already done, remove `RulesConfig` from config.rs. The `Config` struct should have `steps: StepsConfig` instead of `rules: RulesConfig`.

Handle backward compatibility: if old configs have `[rules]`, the `steps` field can be aliased or a migration note added.

- [ ] **Step 2: Update integration tests in tests/config.rs**

Update tests that reference `config.rules.disabled` to use `config.steps.disabled`. Update pattern override tests similarly.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add src/config.rs tests/config.rs
git commit -m "refactor: replace RulesConfig with StepsConfig in config"
```

---

## Chunk 4: TUI Update

Adapt the TUI to work with Steps instead of Rules.

### Task 4.1: Replace RuleState with StepState in TUI

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Define StepState**

Replace `RuleState` with:

```rust
struct StepState {
    pub label: String,
    pub step_type: String, // "validate", "rewrite", "extract", "standardize"
    pub pattern_template: Option<String>,
    pub enabled: bool,
    pub default_enabled: bool,
}
```

- [ ] **Step 2: Update App struct**

Replace `rules: Vec<RuleState>` with `steps: Vec<StepState>`. Update the App constructor to build StepState from the pipeline's step metadata.

Add a method to Pipeline that returns step summaries:

```rust
pub fn step_summaries(&self) -> Vec<StepSummary> {
    self.steps.iter().map(|s| StepSummary {
        label: s.label().to_string(),
        step_type: s.step_type().to_string(),
        pattern_template: s.pattern_template().map(|s| s.to_string()),
        enabled: s.enabled(),
    }).collect()
}
```

- [ ] **Step 3: Update render functions**

Update `render_rules()` to `render_steps()` — same display logic but using StepState fields. Update tab label from "Rules" to "Steps". Update detail view.

- [ ] **Step 4: Update save-to-config**

When saving, write `[steps]` section instead of `[rules]` section. Disabled steps go to `steps.disabled`.

- [ ] **Step 5: Run TUI tests**

Run: `cargo test tui::tests -- --nocapture 2>&1 | tail -20`
Expected: All TUI tests pass (may need updating)

- [ ] **Step 6: Manual TUI test**

Run: `cargo run -- configure`
Verify: Steps tab shows all steps with types, enable/disable works, detail view works.

- [ ] **Step 7: Commit**

```bash
git add src/tui.rs src/pipeline.rs
git commit -m "refactor: update TUI from Rules to Steps"
```

### Task 4.2: Update main.rs and init.rs

**Files:**
- Modify: `src/main.rs`
- Modify: `src/init.rs`

- [ ] **Step 1: Update CLI list command**

Change `list rules` to `list steps`. Update the output to show step type, label, pattern template, enabled status.

- [ ] **Step 2: Update init.rs**

Update `generate_default_config()` to show step labels (not rule labels) in the generated config comments.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/init.rs
git commit -m "refactor: update CLI and init for step-based pipeline"
```

---

## Chunk 5: PO Box Standardization Consolidation

Move PO box canonical formatting from inline extraction to a proper Standardize step, proving the architecture works for the original pain point.

### Task 6.1: Consolidate PO box rules

**Files:**
- Modify: `data/defaults/steps.toml`
- Modify: `src/step.rs` (if needed for regex-based standardization)

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_po_box_variants_all_standardize() {
    let p = Pipeline::from_steps_default();

    let cases = vec![
        ("PO BOX 123", "PO BOX 123"),
        ("P O BOX 123", "PO BOX 123"),
        ("P.O. BOX 123", "PO BOX 123"),
        ("POBOX 123", "PO BOX 123"),
        ("PO BOX A", "PO BOX A"),
    ];

    for (input, expected) in cases {
        let addr = p.parse(input);
        assert_eq!(
            addr.po_box.as_deref(), Some(expected),
            "Failed for input: {}", input
        );
    }
}
```

- [ ] **Step 2: Update steps.toml — single PO box extraction + standardization**

Merge `po_box` and `po_box_word` into one extraction step with a broader pattern. Add a regex-based standardize step for PO box formatting:

```toml
[[step]]
type = "extract"
label = "po_box"
pattern = '\bP\W*O\W*BOX\W+(\w+)\b'
target = "po_box"
skip_if_filled = true

# Later in the standardize section:
[[step]]
type = "standardize"
label = "standardize_po_box"
target = "po_box"
pattern = '.*'
replacement = 'PO BOX $0'
```

Wait — the extraction captures just the box number/word (`\w+`). The standardize step just needs to prepend "PO BOX ". This might need a `RegexReplace` standardize mode. Or the extraction step's pattern captures the full match and the standardize step reformats it.

The cleanest approach: extraction captures the full "P O BOX 123" text. The standardize step uses a regex to reformat: `P\W*O\W*BOX\W+(\w+)` → `PO BOX $1`. This is a regex-based Standardize mode.

Add `StandardizeMode::Regex` variant:

```rust
enum StandardizeMode {
    WholeField,
    PerWord,
    Regex { pattern: Regex, replacement: String },
}
```

Update the TOML and compiler accordingly. The standardize step in TOML:

```toml
[[step]]
type = "standardize"
label = "standardize_po_box"
target = "po_box"
pattern = 'P\W*O\W*BOX\W*(\w+)'
replacement = 'PO BOX $1'
```

- [ ] **Step 3: Implement and test**

Update `compile_step` for standardize to handle pattern/replacement fields → `StandardizeMode::Regex`.
Update `apply_step` for the Regex mode.

- [ ] **Step 4: Run tests**

Run: `cargo test test_po_box -- --nocapture 2>&1 | tail -20`
Expected: All PO box tests pass

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 5: Verify the original pain point is solved**

PO BOX canonical spacing now lives in exactly ONE place: the `standardize_po_box` step's replacement string. Adding "POB" as a variant means updating the extraction pattern — one line.

- [ ] **Step 6: Commit**

```bash
git add src/step.rs data/defaults/steps.toml
git commit -m "feat: consolidate PO box to single extract + standardize step"
```

---

## Summary

| Chunk | Tasks | What it does |
|-------|-------|-------------|
| 1 | 1.1–1.5 | Foundation: Step enum, StepDef, table pattern field, compilation (incl. table-driven rewrite), application |
| 2 | 2.1–2.4 | Pipeline switchover: step-based parsing alongside then replacing rule-based |
| 3 | 3.1–3.3 | Cleanup: remove Rule/Action/build_rules, simplify finalize, update config |
| 4 | 4.1–4.2 | TUI: adapt to Steps, update CLI commands |
| 5 | 5.1 | PO box consolidation: prove the architecture solves the original pain point |
| 6 | 6.1 | PO box consolidation: prove the architecture solves the original pain point |

Golden tests are the gate at every stage. Parsing results must not change.
