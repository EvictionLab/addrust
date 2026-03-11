# Extract Infrastructure, Number-to-Word, and Pipeline Cleanup — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add named capture groups, source field, number-to-word conversion, and move finalize logic into declarative pipeline steps.

**Architecture:** Four layers built in dependency order. Layer 1 changes extract/rewrite infrastructure (source field, targets, generalized extract_remove). Layer 2 moves hardcoded finalize logic into steps.toml. Layer 3 adds number tables and replacement template syntax. Layer 4 adds trailing number rule.

**Tech Stack:** Rust, fancy_regex, serde/toml, ratatui (TUI updates)

**Spec:** `docs/superpowers/specs/2026-03-11-extract-numbers-design.md`

---

## Chunk 1: Extract Infrastructure (Layer 1)

### Task 1: Generalize `extract_remove` to Return Capture Groups

**Files:**
- Modify: `src/ops.rs`

- [ ] **Step 1: Write failing test for extract_remove returning groups**

Add to `src/ops.rs` tests:

```rust
#[test]
fn test_extract_remove_groups() {
    let re = Regex::new(r"(APT)\W*(\d+)\s*$").unwrap();
    let mut s = "123 MAIN ST APT 4B".to_string();
    let groups = extract_remove(&mut s, &re);
    // Group 0 = full match, group 1 = APT, group 2 = 4B
    assert!(groups.is_some());
    let groups = groups.unwrap();
    assert_eq!(groups[0].as_deref(), Some("APT 4B"));
    assert_eq!(groups[1].as_deref(), Some("APT"));
    assert_eq!(groups[2].as_deref(), Some("4B"));
    assert_eq!(s, "123 MAIN ST");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_extract_remove_groups -- --nocapture 2>&1`
Expected: FAIL — extract_remove returns `Option<String>`, not `Option<Vec<...>>`

- [ ] **Step 3: Change extract_remove return type**

In `src/ops.rs`, change `extract_remove` to:

```rust
/// Extract a pattern from `source`, remove it, trim whitespace,
/// and clean up any non-word characters left at the boundaries.
/// Returns capture groups indexed by group number (0 = full match).
pub fn extract_remove(source: &mut String, pattern: &Regex) -> Option<Vec<Option<String>>> {
    let caps = pattern.captures(source.as_str()).ok()??;
    let full_match = caps.get(0)?;
    let start = full_match.start();
    let end = full_match.end();

    // Collect all groups
    let groups: Vec<Option<String>> = (0..caps.len())
        .map(|i| caps.get(i).map(|m| m.as_str().trim().to_string()))
        .collect();

    source.replace_range(start..end, "");
    squish(source);
    trim_nonword_boundaries(source);

    // Return None if full match was empty
    if groups[0].as_ref().map_or(true, |s| s.is_empty()) {
        None
    } else {
        Some(groups)
    }
}
```

- [ ] **Step 4: Update existing callers of extract_remove**

In `src/step.rs`, `apply_step` for `Step::Extract` (around line 202), change from:

```rust
if let Some(mut val) = extract_remove(&mut state.working, pattern) {
```

to:

```rust
if let Some(groups) = extract_remove(&mut state.working, pattern) {
    let mut val = groups[0].clone().unwrap_or_default();
```

The rest of the extract logic (replacement, field assignment) stays the same for now — it uses group 0 (full match) just like before.

- [ ] **Step 5: Update existing extract_remove tests**

The old `test_extract_remove` and `test_extract_remove_no_match` tests need updating for the new return type:

```rust
#[test]
fn test_extract_remove() {
    let re = Regex::new(r"^\d+").unwrap();
    let mut s = "123 MAIN ST".to_string();
    let groups = extract_remove(&mut s, &re);
    assert!(groups.is_some());
    assert_eq!(groups.unwrap()[0].as_deref(), Some("123"));
    assert_eq!(s, "MAIN ST");
}

#[test]
fn test_extract_remove_no_match() {
    let re = Regex::new(r"^\d+").unwrap();
    let mut s = "MAIN ST".to_string();
    let groups = extract_remove(&mut s, &re);
    assert!(groups.is_none());
    assert_eq!(s, "MAIN ST");
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass (75 unit + 22 integration + 2 golden)

- [ ] **Step 7: Commit**

```bash
git add src/ops.rs src/step.rs
git commit -m "refactor: generalize extract_remove to return capture groups"
```

---

### Task 2: Add `source` Field to StepDef and Step Enums

**Files:**
- Modify: `src/step.rs` (StepDef, Step enum, compile_step, apply_step)

- [ ] **Step 1: Write failing test for source on rewrite**

Add to `src/step.rs` tests:

```rust
#[test]
fn test_rewrite_with_source_field() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;
    let abbr = build_default_tables();
    let def = StepDef {
        step_type: "rewrite".to_string(),
        label: "strip_hash".to_string(),
        pattern: Some(r"^#\s*".to_string()),
        replacement: Some("".to_string()),
        table: None, target: None, source: Some("unit".to_string()),
        skip_if_filled: None, matching_table: None, format_table: None, mode: None,
        targets: None,
    };
    let step = compile_step(&def, &abbr).unwrap();
    let mut state = AddressState::new_from_prepared("123 MAIN ST".to_string());
    state.fields.unit = Some("#4B".to_string());
    let output = OutputConfig::default();
    apply_step(&mut state, &step, &abbr, &output);
    assert_eq!(state.fields.unit.as_deref(), Some("4B"));
    assert_eq!(state.working, "123 MAIN ST"); // working string unchanged
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_rewrite_with_source_field -- --nocapture 2>&1`
Expected: FAIL — `source` field doesn't exist on StepDef

- [ ] **Step 3: Add source to StepDef**

In `src/step.rs`, add to `StepDef`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub source: Option<String>,
```

- [ ] **Step 4: Add source to Step::Rewrite and Step::Extract variants**

```rust
Rewrite {
    label: String,
    pattern: Regex,
    pattern_template: String,
    replacement: Option<String>,
    rewrite_table: Option<String>,
    source: Option<Field>,  // NEW
    enabled: bool,
},
Extract {
    label: String,
    pattern: Regex,
    pattern_template: String,
    target: Field,
    skip_if_filled: bool,
    replacement: Option<(Regex, String)>,
    source: Option<Field>,  // NEW
    enabled: bool,
},
```

- [ ] **Step 5: Update compile_step to parse source**

In the "rewrite" arm of `compile_step`, add:

```rust
let source = def.source.as_ref().map(|s| parse_field(s)).transpose()?;
```

And include `source` in the `Step::Rewrite` construction. Same for the "extract" arm.

- [ ] **Step 6: Update apply_step for source on Rewrite**

In `apply_step`, the `Step::Rewrite` arm needs to check `source`. When source is Some, operate on the field value instead of `state.working`:

```rust
Step::Rewrite { pattern, replacement, rewrite_table, source, .. } => {
    let working = match source {
        Some(field) => match state.fields.field_mut(*field) {
            val @ Some(_) => val,
            None => return, // source field is empty, no-op
        },
        None => &mut Some(state.working.clone()), // placeholder, not ideal
    };
    // ... rest of logic
}
```

Actually, cleaner approach — extract the target string, do operations, write back:

```rust
Step::Rewrite { pattern, replacement, rewrite_table, source, .. } => {
    let target_str = match source {
        Some(field) => match state.fields.field(*field) {
            Some(v) => v.clone(),
            None => return,
        },
        None => state.working.clone(),
    };
    if !pattern.is_match(&target_str).unwrap_or(false) {
        return;
    }
    let mut result = target_str;
    if let Some(table_name) = rewrite_table {
        if let Some(table) = tables.get(table_name) {
            for (short, long) in table.short_to_long_pairs() {
                let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                replace_pattern(&mut result, &re, &long);
            }
        }
    } else if let Some(repl) = replacement {
        replace_pattern(&mut result, pattern, repl);
    }
    squish(&mut result);
    match source {
        Some(field) => *state.fields.field_mut(*field) = none_if_empty(result),
        None => state.working = result,
    }
}
```

- [ ] **Step 7: Update apply_step for source on Extract**

In the `Step::Extract` arm, when `source` is set, extract from the field value instead of working string:

```rust
Step::Extract { pattern, target, skip_if_filled, replacement, source, .. } => {
    if *skip_if_filled {
        if state.fields.field(*target).is_some() {
            return;
        }
    }
    let extract_result = match source {
        Some(field) => {
            let field_val = match state.fields.field(*field) {
                Some(v) => v.clone(),
                None => return,
            };
            let mut src = field_val;
            let result = extract_remove(&mut src, pattern);
            // Write back the remainder (move semantics: empty → None)
            *state.fields.field_mut(*field) = none_if_empty(src);
            result
        },
        None => extract_remove(&mut state.working, pattern),
    };
    if let Some(groups) = extract_result {
        let mut val = groups[0].clone().unwrap_or_default();
        if let Some((re, repl)) = replacement {
            replace_pattern(&mut val, re, repl);
        }
        *state.fields.field_mut(*target) = none_if_empty(val);
    }
}
```

- [ ] **Step 8: Fix all StepDef constructors in tests**

Every `StepDef { ... }` literal in tests needs `source: None` added. Search for all `StepDef {` in `src/step.rs` tests, `src/tui.rs`, and `src/config.rs`. (The `targets: None` field will be added in Task 3 when that field is introduced.)

- [ ] **Step 9: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 10: Write test for source on extract (move semantics)**

```rust
#[test]
fn test_extract_with_source_field_move() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;
    let abbr = build_default_tables();
    let def = StepDef {
        step_type: "extract".to_string(),
        label: "promote_unit".to_string(),
        pattern: Some(r"^.+$".to_string()),
        replacement: None,
        table: None, target: Some("street_number".to_string()),
        source: Some("unit".to_string()),
        skip_if_filled: Some(true),
        matching_table: None, format_table: None, mode: None,
        targets: None,
    };
    let step = compile_step(&def, &abbr).unwrap();
    let mut state = AddressState::new_from_prepared("MAIN ST".to_string());
    state.fields.unit = Some("42".to_string());
    let output = OutputConfig::default();
    apply_step(&mut state, &step, &abbr, &output);
    assert_eq!(state.fields.street_number.as_deref(), Some("42"));
    assert!(state.fields.unit.is_none()); // moved, not copied
}
```

- [ ] **Step 11: Run tests, verify pass**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 12: Commit**

```bash
git add src/step.rs
git commit -m "feat: add source field to rewrite and extract steps"
```

---

### Task 3: Add `targets` Field for Multi-Target Extract

**Files:**
- Modify: `src/step.rs` (StepDef, Step::Extract, compile_step, apply_step)

- [ ] **Step 1: Write failing test for targets**

```rust
#[test]
fn test_extract_with_targets() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;
    let abbr = build_default_tables();
    let toml_str = r#"
[[step]]
type = "extract"
label = "unit_split"
pattern = '(APT)\W*(\d+)\s*$'
targets = { unit_type = 1, unit = 2 }
"#;
    let defs: crate::step::StepsDef = toml::from_str(toml_str).unwrap();
    let steps = crate::step::compile_steps(&defs.step, &abbr);
    let mut state = AddressState::new_from_prepared("123 MAIN ST APT 4B".to_string());
    let output = OutputConfig::default();
    crate::step::apply_step(&mut state, &steps[0], &abbr, &output);
    assert_eq!(state.fields.unit_type.as_deref(), Some("APT"));
    assert_eq!(state.fields.unit.as_deref(), Some("4B"));
    assert_eq!(state.working, "123 MAIN ST");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_extract_with_targets -- --nocapture 2>&1`
Expected: FAIL — `targets` field doesn't exist on StepDef

- [ ] **Step 3: Add targets to StepDef**

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub targets: Option<HashMap<String, usize>>,
```

Add `use std::collections::HashMap;` at top of `src/step.rs` if not present.

- [ ] **Step 4: Add targets to Step::Extract**

```rust
Extract {
    label: String,
    pattern: Regex,
    pattern_template: String,
    target: Option<Field>,       // Changed from Field to Option<Field>
    targets: Option<HashMap<Field, usize>>,  // NEW
    skip_if_filled: bool,
    replacement: Option<(Regex, String)>,
    source: Option<Field>,
    enabled: bool,
},
```

Note: `target` becomes `Option<Field>` since now exactly one of `target` or `targets` is required.

- [ ] **Step 5: Update compile_step for targets**

In the "extract" arm:

```rust
// Parse targets (multi-field) or target (single field)
let targets = if let Some(ref t) = def.targets {
    let mut map = HashMap::new();
    for (field_name, group_num) in t {
        map.insert(parse_field(field_name)?, *group_num);
    }
    Some(map)
} else {
    None
};

let target = if targets.is_some() {
    // targets mode — no single target
    if def.target.is_some() {
        return Err(format!(
            "extract step '{}' has both target and targets — use one",
            def.label
        ));
    }
    if def.replacement.is_some() {
        return Err(format!(
            "extract step '{}' has both targets and replacement — not supported",
            def.label
        ));
    }
    None
} else {
    Some(parse_field(
        def.target.as_ref()
            .ok_or_else(|| format!("extract step '{}' missing target or targets", def.label))?
    )?)
};
```

- [ ] **Step 6: Update apply_step for targets**

In the `Step::Extract` arm, after extracting groups, check targets:

```rust
if let Some(groups) = extract_result {
    if let Some(ref targets_map) = targets {
        // Multi-target mode: route each group to its field
        for (field, group_num) in targets_map {
            if let Some(Some(val)) = groups.get(*group_num) {
                if !val.is_empty() {
                    *state.fields.field_mut(*field) = Some(val.clone());
                }
            }
        }
    } else if let Some(target_field) = target {
        // Single-target mode (existing behavior)
        let mut val = groups[0].clone().unwrap_or_default();
        if let Some((re, repl)) = replacement {
            replace_pattern(&mut val, re, repl);
        }
        *state.fields.field_mut(*target_field) = none_if_empty(val);
    }
}
```

- [ ] **Step 7: Update skip_if_filled for targets**

When using targets + skip_if_filled, skip if ANY target is filled:

```rust
if *skip_if_filled {
    if let Some(ref targets_map) = targets {
        if targets_map.keys().any(|f| state.fields.field(*f).is_some()) {
            return;
        }
    } else if let Some(target_field) = target {
        if state.fields.field(*target_field).is_some() {
            return;
        }
    }
}
```

- [ ] **Step 8: Add targets: None to all existing StepDef constructors and update Step::Extract references**

Add `targets: None` to every `StepDef { ... }` literal in `src/step.rs` tests, `src/tui.rs`, and `src/config.rs`. Also search codebase for `Step::Extract { ... target, ...` patterns. The `step_type()`, `label()`, `enabled()`, `set_enabled()`, `pattern_template()` methods on Step don't reference target directly, so they're fine. But any destructured matches need updating.

Also update `Step.label()` etc. methods — they use `..` so they should be fine.

- [ ] **Step 9: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass including new `test_extract_with_targets`

- [ ] **Step 10: Write test for targets + skip_if_filled**

```rust
#[test]
fn test_extract_targets_skip_if_filled() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;
    let abbr = build_default_tables();
    let toml_str = r#"
[[step]]
type = "extract"
label = "unit_split"
pattern = '(APT)\W*(\d+)\s*$'
targets = { unit_type = 1, unit = 2 }
skip_if_filled = true
"#;
    let defs: crate::step::StepsDef = toml::from_str(toml_str).unwrap();
    let steps = crate::step::compile_steps(&defs.step, &abbr);
    let mut state = AddressState::new_from_prepared("123 MAIN ST APT 4B".to_string());
    state.fields.unit = Some("EXISTING".to_string()); // pre-filled
    let output = OutputConfig::default();
    crate::step::apply_step(&mut state, &steps[0], &abbr, &output);
    // Should skip because unit is already filled
    assert_eq!(state.fields.unit.as_deref(), Some("EXISTING"));
    assert!(state.fields.unit_type.is_none());
}
```

- [ ] **Step 11: Run tests, verify pass**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 12: Commit**

```bash
git add src/step.rs
git commit -m "feat: add targets field for multi-target extract steps"
```

---

### Task 4: Update TUI Wizard for New Fields

**Files:**
- Modify: `src/tui.rs` (WizardAccumulator, to_stepdef, any StepDef constructors)

- [ ] **Step 1: Add source and targets to WizardAccumulator**

Add to `WizardAccumulator`:

```rust
source: Option<String>,
```

(Targets is complex enough that the wizard should probably not support it initially — custom TOML config is the path for multi-target. Just ensure it doesn't break.)

- [ ] **Step 2: Update all StepDef constructors in tui.rs**

Search for `StepDef {` in `src/tui.rs` and add `source: None, targets: None` to each.

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "fix: update TUI wizard for new StepDef fields"
```

---

### Task 5: Update StepDef Constructors in Config Tests

**Files:**
- Modify: `src/config.rs` (any StepDef literals in tests)

- [ ] **Step 1: Add source and targets to StepDef in config tests**

Search `src/config.rs` for `StepDef {` and add `source: None, targets: None`.

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "fix: update StepDef constructors in config tests"
```

---

## Chunk 2: Finalize Cleanup (Layer 2)

### Task 6: Add New Steps to steps.toml and Strip Finalize Logic

**Files:**
- Modify: `data/defaults/steps.toml`
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Write failing integration test for strip_unit_hash as a pipeline step**

Add to `tests/config.rs`:

```rust
#[test]
fn test_strip_unit_hash_step() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St #4B");
    assert_eq!(addr.unit.as_deref(), Some("4B")); // not "#4B"
}
```

(This test may already pass due to current finalize logic — that's fine, it serves as a regression test when we remove finalize.)

- [ ] **Step 2: Add new steps to steps.toml**

Append before the standardization section in `data/defaults/steps.toml`:

```toml
# --- Post-extraction cleanup ---
[[step]]
type = "rewrite"
label = "strip_unit_hash"
pattern = '^#\s*'
replacement = ''
source = "unit"

[[step]]
type = "rewrite"
label = "strip_leading_zeros_street_number"
pattern = '^0+(?=\d)'
replacement = ''
source = "street_number"

[[step]]
type = "rewrite"
label = "strip_leading_zeros_unit"
pattern = '^0+(?=\d)'
replacement = ''
source = "unit"

[[step]]
type = "extract"
label = "promote_unit_to_street_number"
pattern = '^.+$'
source = "unit"
target = "street_number"
skip_if_filled = true
```

- [ ] **Step 3: Strip finalize() down to just street_name assignment**

In `src/pipeline.rs`, replace `finalize()` with:

```rust
fn finalize(&self, state: &mut AddressState) {
    // Remove any leftover placeholder tags
    let re_tags = Regex::new(r"<[a-z0-9_]+>").unwrap();
    let remaining = re_tags.replace_all(&state.working, "").to_string();
    let mut remaining = remaining.trim().to_string();
    squish(&mut remaining);

    if state.fields.street_name.is_none() && !remaining.is_empty() {
        state.fields.street_name = Some(remaining);
    }
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass. The golden tests are the key regression check here.

- [ ] **Step 5: Write test for leading zeros strip**

Add to `tests/config.rs`:

```rust
#[test]
fn test_strip_leading_zeros() {
    let p = Pipeline::default();
    let addr = p.parse("0123 Main St Apt 007");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    // Unit 007 gets leading zeros stripped, then promoted if no unit_type
    // The exact result depends on whether unit retains or is promoted
}
```

- [ ] **Step 6: Run tests, verify pass**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 7: Update step count assertions in existing tests**

The `test_default_steps_toml_parses` test asserts `defs.step.len() > 20`. With 4 new steps (26 → 30), this still holds. But `test_compile_all_default_steps` also asserts `steps.len() > 20`. Both are fine. Just verify.

- [ ] **Step 8: Commit**

```bash
git add data/defaults/steps.toml src/pipeline.rs tests/config.rs
git commit -m "refactor: move finalize logic into declarative pipeline steps"
```

---

## Chunk 3: Number-to-Word Conversion (Layer 3)

### Task 7: Build Number Generation Module

**Files:**
- Create: `src/tables/numbers.rs`
- Modify: `src/tables/mod.rs`

- [ ] **Step 1: Write tests for cardinal generation**

Create `src/tables/numbers.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cardinal_ones() {
        assert_eq!(cardinal(1), "ONE");
        assert_eq!(cardinal(9), "NINE");
    }

    #[test]
    fn test_cardinal_teens() {
        assert_eq!(cardinal(11), "ELEVEN");
        assert_eq!(cardinal(19), "NINETEEN");
    }

    #[test]
    fn test_cardinal_tens() {
        assert_eq!(cardinal(20), "TWENTY");
        assert_eq!(cardinal(42), "FORTY TWO");
        assert_eq!(cardinal(99), "NINETY NINE");
    }

    #[test]
    fn test_cardinal_hundreds() {
        assert_eq!(cardinal(100), "ONE HUNDRED");
        assert_eq!(cardinal(101), "ONE HUNDRED ONE");
        assert_eq!(cardinal(999), "NINE HUNDRED NINETY NINE");
        assert_eq!(cardinal(250), "TWO HUNDRED FIFTY");
    }

    #[test]
    fn test_ordinal_basic() {
        assert_eq!(ordinal(1), "FIRST");
        assert_eq!(ordinal(2), "SECOND");
        assert_eq!(ordinal(3), "THIRD");
        assert_eq!(ordinal(12), "TWELFTH");
    }

    #[test]
    fn test_ordinal_regular() {
        assert_eq!(ordinal(4), "FOURTH");
        assert_eq!(ordinal(21), "TWENTY FIRST");
        assert_eq!(ordinal(100), "ONE HUNDREDTH");
        assert_eq!(ordinal(101), "ONE HUNDRED FIRST");
        assert_eq!(ordinal(999), "NINE HUNDRED NINETY NINTH");
    }

    #[test]
    fn test_fraction_half() {
        assert_eq!(fraction(1, 2), "ONE HALF");
        assert_eq!(fraction(5, 2), "FIVE HALF");
    }

    #[test]
    fn test_fraction_regular() {
        assert_eq!(fraction(1, 4), "ONE FOURTH");
        assert_eq!(fraction(3, 4), "THREE FOURTHS");
        assert_eq!(fraction(1, 8), "ONE EIGHTH");
        assert_eq!(fraction(5, 8), "FIVE EIGHTHS");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test numbers::tests -- --nocapture 2>&1`
Expected: FAIL — module doesn't exist yet

- [ ] **Step 3: Implement cardinal()**

```rust
const ONES: [&str; 20] = [
    "", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE",
    "TEN", "ELEVEN", "TWELVE", "THIRTEEN", "FOURTEEN", "FIFTEEN",
    "SIXTEEN", "SEVENTEEN", "EIGHTEEN", "NINETEEN",
];

const TENS: [&str; 10] = [
    "", "", "TWENTY", "THIRTY", "FORTY", "FIFTY",
    "SIXTY", "SEVENTY", "EIGHTY", "NINETY",
];

/// Convert a number 1-999 to its English cardinal word.
pub fn cardinal(n: u16) -> String {
    assert!(n >= 1 && n <= 999, "cardinal: n must be 1-999, got {}", n);

    let h = n / 100;
    let rem = n % 100;

    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{} HUNDRED", ONES[h as usize]));
    }
    if rem > 0 {
        if rem < 20 {
            parts.push(ONES[rem as usize].to_string());
        } else {
            let t = rem / 10;
            let o = rem % 10;
            if o > 0 {
                parts.push(format!("{} {}", TENS[t as usize], ONES[o as usize]));
            } else {
                parts.push(TENS[t as usize].to_string());
            }
        }
    }

    parts.join(" ")
}
```

- [ ] **Step 4: Implement ordinal()**

```rust
const ORDINAL_ONES: [&str; 20] = [
    "", "FIRST", "SECOND", "THIRD", "FOURTH", "FIFTH", "SIXTH", "SEVENTH",
    "EIGHTH", "NINTH", "TENTH", "ELEVENTH", "TWELFTH", "THIRTEENTH",
    "FOURTEENTH", "FIFTEENTH", "SIXTEENTH", "SEVENTEENTH", "EIGHTEENTH", "NINETEENTH",
];

const ORDINAL_TENS: [&str; 10] = [
    "", "", "TWENTIETH", "THIRTIETH", "FORTIETH", "FIFTIETH",
    "SIXTIETH", "SEVENTIETH", "EIGHTIETH", "NINETIETH",
];

/// Convert a number 1-999 to its English ordinal word.
pub fn ordinal(n: u16) -> String {
    assert!(n >= 1 && n <= 999, "ordinal: n must be 1-999, got {}", n);

    let h = n / 100;
    let rem = n % 100;

    if h > 0 && rem == 0 {
        return format!("{} HUNDREDTH", ONES[h as usize]);
    }

    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{} HUNDRED", ONES[h as usize]));
    }

    if rem > 0 {
        if rem < 20 {
            parts.push(ORDINAL_ONES[rem as usize].to_string());
        } else {
            let t = rem / 10;
            let o = rem % 10;
            if o > 0 {
                parts.push(format!("{} {}", TENS[t as usize], ORDINAL_ONES[o as usize]));
            } else {
                parts.push(ORDINAL_TENS[t as usize].to_string());
            }
        }
    }

    parts.join(" ")
}
```

- [ ] **Step 5: Implement fraction()**

```rust
/// Convert a fraction (numerator/denominator) to English words.
/// Denominator 2 always produces "HALF" (not "HALVES").
/// Other denominators use ordinal + "S" for plural.
pub fn fraction(num: u16, den: u16) -> String {
    let num_word = cardinal(num);
    if den == 2 {
        format!("{} HALF", num_word)
    } else {
        let den_word = ordinal(den);
        if num > 1 {
            format!("{} {}S", num_word, den_word)
        } else {
            format!("{} {}", num_word, den_word)
        }
    }
}
```

- [ ] **Step 6: Add module to mod.rs**

In `src/tables/mod.rs`, add:

```rust
pub mod numbers;
```

- [ ] **Step 7: Run tests**

Run: `cargo test numbers::tests -- --nocapture 2>&1`
Expected: All pass

- [ ] **Step 8: Commit**

```bash
git add src/tables/numbers.rs src/tables/mod.rs
git commit -m "feat: add number-to-word generation (cardinal, ordinal, fraction)"
```

---

### Task 8: Register Number Tables in Abbreviations

**Files:**
- Modify: `src/tables/numbers.rs` (add build function)
- Modify: `src/tables/abbreviations.rs` (register in build_default_tables)

- [ ] **Step 1: Write test for number table registration**

Add to `src/tables/abbreviations.rs` tests:

```rust
#[test]
fn test_number_tables_registered() {
    let tables = build_default_tables();
    let cardinal = tables.get("number_cardinal").unwrap();
    assert_eq!(cardinal.to_long("1"), Some("ONE"));
    assert_eq!(cardinal.to_long("42"), Some("FORTY TWO"));
    assert_eq!(cardinal.to_long("999"), Some("NINE HUNDRED NINETY NINE"));

    let ordinal = tables.get("number_ordinal").unwrap();
    assert_eq!(ordinal.to_long("1"), Some("FIRST"));
    assert_eq!(ordinal.to_long("21"), Some("TWENTY FIRST"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_number_tables_registered -- --nocapture 2>&1`
Expected: FAIL — tables not registered yet

- [ ] **Step 3: Add build_number_tables() to numbers.rs**

```rust
use super::abbreviations::{Abbr, AbbrTable};

/// Build cardinal and ordinal lookup tables for 1-999.
pub fn build_number_tables() -> (AbbrTable, AbbrTable) {
    let mut cardinal_entries = Vec::with_capacity(999);
    let mut ordinal_entries = Vec::with_capacity(999);

    for n in 1..=999u16 {
        cardinal_entries.push(Abbr {
            short: n.to_string(),
            long: cardinal(n),
        });
        ordinal_entries.push(Abbr {
            short: n.to_string(),
            long: ordinal(n),
        });
    }

    (AbbrTable::new(cardinal_entries), AbbrTable::new(ordinal_entries))
}
```

- [ ] **Step 4: Register in build_default_tables()**

In `src/tables/abbreviations.rs`, in `build_default_tables()`:

```rust
let (number_cardinal, number_ordinal) = crate::tables::numbers::build_number_tables();
tables.insert("number_cardinal".to_string(), number_cardinal);
tables.insert("number_ordinal".to_string(), number_ordinal);
```

Also update the `ABBR` static in the same way.

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add src/tables/numbers.rs src/tables/abbreviations.rs
git commit -m "feat: register number_cardinal and number_ordinal tables"
```

---

### Task 9: Implement expand_replacement() for Table Lookup Syntax

**Files:**
- Modify: `src/step.rs` (add expand_replacement, update apply_step for rewrite)

- [ ] **Step 1: Write tests for expand_replacement**

Add to `src/step.rs` tests:

```rust
#[test]
fn test_expand_replacement_simple_backref() {
    use crate::tables::abbreviations::build_default_tables;
    let abbr = build_default_tables();
    let re = Regex::new(r"(HIGHWAY)\s+(\d{1,3})").unwrap();
    let caps = re.captures("HIGHWAY 42").unwrap().unwrap();
    let result = expand_replacement("$1 ${2:number_cardinal}", &caps, &abbr);
    assert_eq!(result, "HIGHWAY FORTY TWO");
}

#[test]
fn test_expand_replacement_ordinal() {
    use crate::tables::abbreviations::build_default_tables;
    let abbr = build_default_tables();
    let re = Regex::new(r"(\d{1,3})(ST|ND|RD|TH)").unwrap();
    let caps = re.captures("21ST").unwrap().unwrap();
    let result = expand_replacement("${1:number_ordinal}", &caps, &abbr);
    assert_eq!(result, "TWENTY FIRST");
}

#[test]
fn test_expand_replacement_fraction() {
    use crate::tables::abbreviations::build_default_tables;
    let abbr = build_default_tables();
    let re = Regex::new(r"(\d{1,3})\s+(\d+)/(\d+)").unwrap();
    let caps = re.captures("8 5/8").unwrap().unwrap();
    let result = expand_replacement("${1:number_cardinal} AND ${2/3:fraction}", &caps, &abbr);
    assert_eq!(result, "EIGHT AND FIVE EIGHTHS");
}

#[test]
fn test_expand_replacement_fraction_half() {
    use crate::tables::abbreviations::build_default_tables;
    let abbr = build_default_tables();
    let re = Regex::new(r"(\d{1,3})\s+(\d+)/(\d+)").unwrap();
    let caps = re.captures("8 1/2").unwrap().unwrap();
    let result = expand_replacement("${1:number_cardinal} AND ${2/3:fraction}", &caps, &abbr);
    assert_eq!(result, "EIGHT AND ONE HALF");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_expand_replacement -- --nocapture 2>&1`
Expected: FAIL — function doesn't exist

- [ ] **Step 3: Implement expand_replacement()**

```rust
use fancy_regex::Captures;

/// Expand a replacement template with capture group backrefs and table lookups.
///
/// Syntax:
/// - `$N` or `${N}` — capture group N (standard backref)
/// - `${N:table_name}` — capture group N, looked up in table (via to_long)
/// - `${N/M:fraction}` — fraction expansion (N=numerator group, M=denominator group)
pub fn expand_replacement(template: &str, caps: &Captures, tables: &Abbreviations) -> String {
    let mut result = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            if chars[i + 1] == '{' {
                // ${...} syntax
                if let Some(close) = chars[i + 2..].iter().position(|&c| c == '}') {
                    let inner: String = chars[i + 2..i + 2 + close].iter().collect();
                    result.push_str(&resolve_template_token(&inner, caps, tables));
                    i = i + 2 + close + 1;
                    continue;
                }
            } else if chars[i + 1].is_ascii_digit() {
                // $N syntax (single digit)
                let n = (chars[i + 1] as u8 - b'0') as usize;
                if let Some(m) = caps.get(n) {
                    result.push_str(m.as_str());
                }
                i += 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Resolve a single template token (the content inside ${...}).
fn resolve_template_token(token: &str, caps: &Captures, tables: &Abbreviations) -> String {
    // Check for fraction: N/M:fraction
    if let Some(frac_idx) = token.find(":fraction") {
        let nums = &token[..frac_idx];
        if let Some(slash) = nums.find('/') {
            let num_group: usize = nums[..slash].parse().unwrap_or(0);
            let den_group: usize = nums[slash + 1..].parse().unwrap_or(0);
            let num_val: u16 = caps.get(num_group)
                .map(|m| m.as_str().parse().unwrap_or(0))
                .unwrap_or(0);
            let den_val: u16 = caps.get(den_group)
                .map(|m| m.as_str().parse().unwrap_or(0))
                .unwrap_or(0);
            if num_val > 0 && den_val > 0 && num_val <= 999 && den_val <= 999 {
                return crate::tables::numbers::fraction(num_val, den_val);
            }
        }
        return String::new();
    }

    // Check for table lookup: N:table_name
    if let Some(colon) = token.find(':') {
        let group_num: usize = token[..colon].parse().unwrap_or(0);
        let table_name = &token[colon + 1..];
        if let Some(m) = caps.get(group_num) {
            let captured = m.as_str().trim();
            if let Some(table) = tables.get(table_name) {
                return table.to_long(captured).unwrap_or(captured).to_string();
            }
        }
        return String::new();
    }

    // Plain group number
    let group_num: usize = token.parse().unwrap_or(0);
    caps.get(group_num).map(|m| m.as_str().to_string()).unwrap_or_default()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_expand_replacement -- --nocapture 2>&1`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add src/step.rs
git commit -m "feat: implement expand_replacement for table lookup syntax in rewrites"
```

---

### Task 10: Wire expand_replacement into apply_step for Rewrite

**Files:**
- Modify: `src/step.rs` (apply_step rewrite arm)

- [ ] **Step 1: Write integration test for highway number conversion**

Add to `tests/config.rs`:

```rust
#[test]
fn test_highway_number_to_word() {
    let p = Pipeline::default();
    let addr = p.parse("HIGHWAY 1");
    assert_eq!(addr.street_name.as_deref(), Some("HIGHWAY ONE"));
}

#[test]
fn test_ordinal_to_word() {
    let p = Pipeline::default();
    let addr = p.parse("123 42ND ST");
    assert_eq!(addr.street_name.as_deref(), Some("FORTY SECOND"));
}

#[test]
fn test_fractional_road() {
    let p = Pipeline::default();
    let addr = p.parse("123 8 1/2 MILE RD");
    assert_eq!(addr.street_name.as_deref(), Some("EIGHT AND ONE HALF MILE"));
}
```

- [ ] **Step 2: Add number-to-word steps to steps.toml**

Insert after the street name rewrites section, before standardization, in `data/defaults/steps.toml`:

```toml
# --- Number-to-word conversion ---
[[step]]
type = "rewrite"
label = "fractional_road"
pattern = '\b(\d{1,3})\s+(\d+)/(\d+)\b'
replacement = '${1:number_cardinal} AND ${2/3:fraction}'

[[step]]
type = "rewrite"
label = "highway_number_to_word"
pattern = '\b(HIGHWAY|FARM ROAD|COUNTY ROAD|STATE ROAD|ROUTE)\s+(\d{1,3})\b'
replacement = '$1 ${2:number_cardinal}'

[[step]]
type = "rewrite"
label = "ordinal_to_word"
pattern = '\b(\d{1,3})(ST|ND|RD|TH)\b'
replacement = '${1:number_ordinal}'
```

- [ ] **Step 3: Detect table lookup syntax in apply_step**

In the `Step::Rewrite` arm of `apply_step`, when the replacement contains `${`, use `expand_replacement` instead of plain `replace_pattern`. Update the rewrite logic:

```rust
} else if let Some(repl) = replacement {
    if repl.contains("${") {
        // Table lookup replacement — use expand_replacement
        if let Ok(Some(caps)) = pattern.captures(&target_str) {
            result = expand_replacement(repl, &caps, tables);
        }
    } else {
        replace_pattern(&mut result, pattern, repl);
    }
}
```

Note: when using `expand_replacement`, the whole match is replaced by the expanded template (not `replace_all` — `replace_all` wouldn't work with the custom expansion). Use `pattern.replace` with a closure or manually replace the match range.

For table-lookup replacements, loop to replace all matches (not just the first). Process from right to left to avoid invalidating offsets:

```rust
if repl.contains("${") {
    // Table lookup replacement — replace all matches
    // Collect matches first, then replace right-to-left
    let mut matches = Vec::new();
    let mut search_start = 0;
    while search_start < result.len() {
        if let Ok(Some(caps)) = pattern.captures(&result[search_start..]) {
            if let Some(full_match) = caps.get(0) {
                let abs_start = search_start + full_match.start();
                let abs_end = search_start + full_match.end();
                let expanded = expand_replacement(repl, &caps, tables);
                matches.push((abs_start, abs_end, expanded));
                search_start = abs_end;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    // Replace right-to-left to preserve offsets
    for (start, end, expanded) in matches.into_iter().rev() {
        result.replace_range(start..end, &expanded);
    }
} else {
    replace_pattern(&mut result, pattern, repl);
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass including new integration tests

- [ ] **Step 5: Run golden tests specifically**

Run: `cargo test golden -- --nocapture 2>&1`
Expected: Pass — the number conversion shouldn't break existing golden data (existing addresses don't have highway numbers or ordinals in the golden set, or if they do, the expected output will need updating).

- [ ] **Step 6: Commit**

```bash
git add src/step.rs data/defaults/steps.toml tests/config.rs
git commit -m "feat: wire table lookup replacement into rewrite steps, add number-to-word steps"
```

---

## Chunk 4: Trailing Number Rule and unit_type_value Update (Layer 4)

### Task 11: Add Trailing Number to Street Number Step

**Files:**
- Modify: `data/defaults/steps.toml`

- [ ] **Step 1: Write test**

Add to `tests/config.rs`:

```rust
#[test]
fn test_trailing_number_to_street_number() {
    let p = Pipeline::default();
    // "MAIN 123" — trailing 123 should become street_number
    let addr = p.parse("MAIN 123");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
}

#[test]
fn test_trailing_number_skipped_when_street_number_exists() {
    let p = Pipeline::default();
    let addr = p.parse("123 MAIN ST 456");
    // 123 is already street_number, 456 should stay as unit or in working
    assert_eq!(addr.street_number.as_deref(), Some("123"));
}
```

- [ ] **Step 2: Add step to steps.toml**

Insert after `ordinal_to_word`, before `promote_unit_to_street_number`:

```toml
[[step]]
type = "extract"
label = "trailing_number_to_street_number"
pattern = '\b(\d{1,3})\s*$'
target = "street_number"
skip_if_filled = true
```

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add data/defaults/steps.toml tests/config.rs
git commit -m "feat: add trailing number to street_number step"
```

---

### Task 12: Update unit_type_value to Use targets

**Files:**
- Modify: `data/defaults/steps.toml`

- [ ] **Step 1: Write test for unit_type extraction**

Add to `tests/config.rs`:

```rust
#[test]
fn test_unit_type_extracted() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St Apt 4B");
    assert_eq!(addr.unit_type.as_deref(), Some("APT"));
    assert_eq!(addr.unit.as_deref(), Some("4B"));
}
```

- [ ] **Step 2: Update unit_type_value step in steps.toml**

Change from:

```toml
[[step]]
type = "extract"
label = "unit_type_value"
pattern = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$'
target = "unit"
skip_if_filled = true
```

To:

```toml
[[step]]
type = "extract"
label = "unit_type_value"
pattern = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$'
targets = { unit_type = 1, unit = 2 }
skip_if_filled = true
```

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass. The golden tests will tell us if anything broke.

- [ ] **Step 4: Check golden test output carefully**

Run: `cargo test golden -- --nocapture 2>&1`

If golden tests fail, inspect the output — some addresses may now have unit_type populated where before both went to unit. The golden CSV may need the unit column values updated to reflect the split. If unit values change (e.g., "APT 4B" → "4B"), update `data/golden.csv` accordingly.

- [ ] **Step 5: Commit**

```bash
git add data/defaults/steps.toml tests/config.rs
git commit -m "feat: split unit_type_value into separate unit_type and unit fields"
```

---

### Task 13: Final Integration Tests and Golden Data Update

**Files:**
- Modify: `tests/config.rs` (comprehensive tests)
- Possibly modify: `data/golden.csv` (if unit values changed)

- [ ] **Step 1: Write comprehensive end-to-end tests**

Add to `tests/config.rs`:

```rust
#[test]
fn test_multiple_ordinals() {
    let p = Pipeline::default();
    let addr = p.parse("123 1ST AND 2ND ST");
    assert!(addr.street_name.as_deref().unwrap().contains("FIRST"));
    assert!(addr.street_name.as_deref().unwrap().contains("SECOND"));
}

#[test]
fn test_bare_zero_street_number_preserved() {
    let p = Pipeline::default();
    let addr = p.parse("0 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("0"));
}

#[test]
fn test_highway_one_not_extracted_as_number() {
    let p = Pipeline::default();
    let addr = p.parse("HIGHWAY 1");
    // "1" should become "ONE", not be extracted as street_number
    assert!(addr.street_number.is_none());
    assert_eq!(addr.street_name.as_deref(), Some("HIGHWAY ONE"));
}

#[test]
fn test_county_road_number() {
    let p = Pipeline::default();
    let addr = p.parse("123 COUNTY ROAD 5");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("COUNTY ROAD FIVE"));
}

#[test]
fn test_wisconsin_fraction() {
    let p = Pipeline::default();
    let addr = p.parse("123 8 1/2 MILE RD");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("EIGHT AND ONE HALF MILE"));
    assert_eq!(addr.suffix.as_deref(), Some("ROAD"));
}

#[test]
fn test_ordinal_street() {
    let p = Pipeline::default();
    let addr = p.parse("123 W 42ND ST");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.pre_direction.as_deref(), Some("W"));
    assert_eq!(addr.street_name.as_deref(), Some("FORTY SECOND"));
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}

#[test]
fn test_source_rewrite_no_side_effects() {
    // Rewrite with source shouldn't touch working string
    let p = Pipeline::default();
    let addr = p.parse("123 Main St #007");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.unit.as_deref(), Some("7")); // leading zeros stripped
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 3: Update golden.csv if needed**

If unit values changed due to unit_type_value split, update the golden CSV. Run golden tests, inspect failures, and fix expected values.

- [ ] **Step 4: Final full test run**

Run: `cargo test 2>&1`
Expected: All 99+ tests pass (count will be higher with new tests)

- [ ] **Step 5: Commit**

```bash
git add tests/config.rs data/golden.csv
git commit -m "test: add comprehensive integration tests for extract infrastructure and number-to-word"
```
