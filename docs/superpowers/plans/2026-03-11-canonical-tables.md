# Canonical Tables Redesign Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace flat `Abbr { short, long }` with group-based `AbbrGroup { short, long, variants }` model, unify standardization to single-table lookup, eliminate `suffix_usps`.

**Architecture:** `AbbrGroup` defines a canonical short/long pair plus variant match patterns. `AbbrTable` uses a two-tier lookup (hashmap for literals, regex fallback for pattern variants). Standardize steps take one table name; output config determines short vs long. Config format supports `variants` and `canonical` fields on dict entries.

**Tech Stack:** Rust, fancy_regex, serde, ratatui (TUI)

**Spec:** `docs/superpowers/specs/2026-03-11-canonical-tables-design.md`

---

## File Structure

- **Modify:** `src/tables/abbreviations.rs` — `Abbr` → `AbbrGroup`, `AbbrTable` internals, all `build_*` functions, `patch()`, tests
- **Modify:** `src/step.rs` — `standardize_value`, `Step::Standardize`, `compile_step`, `apply_step`, `StepDef` fields, tests
- **Modify:** `src/config.rs` — `DictEntry` gains `variants`/`canonical`, `DictOverrides` changes
- **Modify:** `data/defaults/steps.toml` — standardize steps: `matching_table`/`format_table` → `table`
- **Modify:** `src/tui.rs` — dict editor (group view, drill-down, variants), standardize wizard (single table pick)
- **Modify:** `src/pipeline.rs` — if any direct references to old table methods
- **Modify:** `tests/config.rs` — integration tests referencing standardize behavior

---

## Task 1: AbbrGroup struct and AbbrTable core

Replace the data model. Everything else depends on this.

**Files:**
- Modify: `src/tables/abbreviations.rs:6-170`

- [ ] **Step 1: Write tests for AbbrGroup and new AbbrTable lookup**

Add to the `tests` module in `src/tables/abbreviations.rs`:

```rust
#[test]
fn test_abbr_group_standardize_literal() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup {
            short: "AVE".into(),
            long: "AVENUE".into(),
            variants: vec!["AV".into(), "AVEN".into()],
        },
        AbbrGroup {
            short: "DR".into(),
            long: "DRIVE".into(),
            variants: vec!["DRIV".into()],
        },
    ]);
    // Canonical short
    assert_eq!(table.standardize("AVE"), Some((0, "AVE", "AVENUE")));
    // Canonical long
    assert_eq!(table.standardize("AVENUE"), Some((0, "AVE", "AVENUE")));
    // Variant
    assert_eq!(table.standardize("AV"), Some((0, "AVE", "AVENUE")));
    assert_eq!(table.standardize("AVEN"), Some((0, "AVE", "AVENUE")));
    // Different group
    assert_eq!(table.standardize("DRIV"), Some((1, "DR", "DRIVE")));
    // No match
    assert_eq!(table.standardize("BLVD"), None);
}

#[test]
fn test_abbr_group_standardize_regex_variant() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup {
            short: "CIR".into(),
            long: "CIRCLE".into(),
            variants: vec!["CIRC".into(), "C[IL]".into()],
        },
    ]);
    // Literal variant
    assert_eq!(table.standardize("CIRC"), Some((0, "CIR", "CIRCLE")));
    // Regex variant matches
    assert_eq!(table.standardize("CI"), Some((0, "CIR", "CIRCLE")));
    assert_eq!(table.standardize("CL"), Some((0, "CIR", "CIRCLE")));
}

#[test]
fn test_abbr_group_longest_match_wins() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup {
            short: "N".into(),
            long: "NORTH".into(),
            variants: vec![],
        },
        AbbrGroup {
            short: "NE".into(),
            long: "NORTHEAST".into(),
            variants: vec!["N E".into()],
        },
    ]);
    // "N E" should match NE group, not N group
    assert_eq!(table.standardize("N E"), Some((1, "NE", "NORTHEAST")));
    // "N" matches N group
    assert_eq!(table.standardize("N"), Some((0, "N", "NORTH")));
}

#[test]
fn test_all_match_values() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup {
            short: "AVE".into(),
            long: "AVENUE".into(),
            variants: vec!["AV".into()],
        },
    ]);
    let values = table.all_match_values();
    // Should contain canonical short, long, and variants — sorted longest first
    assert!(values[0] == "AVENUE"); // longest
    assert!(values.contains(&"AVE"));
    assert!(values.contains(&"AV"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_abbr_group -- --nocapture 2>&1 | head -30`
Expected: FAIL — `AbbrGroup` and `from_groups` don't exist yet

- [ ] **Step 3: Replace Abbr with AbbrGroup, rewrite AbbrTable**

In `src/tables/abbreviations.rs`, replace the `Abbr` struct (lines 6-9) and `AbbrTable` struct + impl (lines 13-166):

```rust
/// A group of abbreviation variants with one canonical short/long pair.
#[derive(Debug, Clone)]
pub struct AbbrGroup {
    pub short: String,
    pub long: String,
    pub variants: Vec<String>,
}

/// A typed collection of abbreviation groups with fast lookup.
#[derive(Debug, Clone)]
pub struct AbbrTable {
    pub groups: Vec<AbbrGroup>,
    /// Maps every literal value (canonical short, long, non-regex variants) → group index.
    lookup: HashMap<String, usize>,
    /// Compiled regexes for groups with regex variants: (regex, group_index).
    regex_variants: Vec<(fancy_regex::Regex, usize)>,
}

impl AbbrTable {
    pub fn from_groups(groups: Vec<AbbrGroup>) -> Self {
        let mut lookup = HashMap::new();
        let mut regex_variants = Vec::new();

        // Collect all (value, group_index) pairs, sort longest-first so longer
        // keys take priority in the hashmap (inserted last = wins on collision).
        let mut literal_pairs: Vec<(String, usize)> = Vec::new();
        for (idx, group) in groups.iter().enumerate() {
            literal_pairs.push((group.short.to_uppercase(), idx));
            literal_pairs.push((group.long.to_uppercase(), idx));
            for v in &group.variants {
                if has_regex_chars(v) {
                    // Compile as full-match regex
                    if let Ok(re) = fancy_regex::Regex::new(&format!("^(?:{})$", v)) {
                        regex_variants.push((re, idx));
                    }
                } else {
                    literal_pairs.push((v.to_uppercase(), idx));
                }
            }
        }

        // Sort shortest-first so longest keys are inserted last and win
        literal_pairs.sort_by(|a, b| a.0.len().cmp(&b.0.len()));
        for (value, idx) in literal_pairs {
            if !value.is_empty() {
                lookup.insert(value, idx);
            }
        }

        Self { groups, lookup, regex_variants }
    }

    /// Look up a value in the table. Returns (group_index, canonical_short, canonical_long).
    pub fn standardize(&self, value: &str) -> Option<(usize, &str, &str)> {
        let upper = value.to_uppercase();
        // Tier 1: hashmap lookup
        if let Some(&idx) = self.lookup.get(&upper) {
            let g = &self.groups[idx];
            return Some((idx, &g.short, &g.long));
        }
        // Tier 2: regex fallback
        for (re, idx) in &self.regex_variants {
            if re.is_match(&upper).unwrap_or(false) {
                let g = &self.groups[*idx];
                return Some((*idx, &g.short, &g.long));
            }
        }
        None
    }

    /// All matchable values (canonical shorts + longs + variants), deduped, sorted longest-first.
    /// Regex variants included as-is (not escaped). Literals are plain strings.
    pub fn all_match_values(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let mut values = Vec::new();
        for group in &self.groups {
            if seen.insert(group.short.as_str()) {
                values.push(group.short.as_str());
            }
            if seen.insert(group.long.as_str()) {
                values.push(group.long.as_str());
            }
            for v in &group.variants {
                if seen.insert(v.as_str()) {
                    values.push(v.as_str());
                }
            }
        }
        values.sort_by(|a, b| b.len().cmp(&a.len()));
        values
    }

    /// True when all long forms are empty — a value-list table (not a short↔long mapping).
    pub fn is_value_list(&self) -> bool {
        !self.groups.is_empty() && self.groups.iter().all(|g| g.long.is_empty())
    }
}
```

Also keep the `has_regex_chars` helper (line 168-170) as-is.

- [ ] **Step 4: Run the new tests**

Run: `cargo test test_abbr_group -- --nocapture`
Expected: PASS for all 4 new tests. Other tests will fail (they use old API) — that's expected.

- [ ] **Step 5: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "feat: replace Abbr with AbbrGroup, add two-tier lookup to AbbrTable"
```

---

## Task 2: Backward-compat bridge and build functions

Update all `build_*` functions to produce `AbbrGroup`s. Add temporary bridge methods so existing callers don't break yet.

**Files:**
- Modify: `src/tables/abbreviations.rs:204-440`

- [ ] **Step 1: Add bridge methods to AbbrTable**

These let existing callers (`expand_template`, `apply_step`, pattern generation) keep working while we migrate. Add to the `impl AbbrTable` block:

```rust
    /// Bridge: construct from old-style (short, long) pairs. Each pair becomes its own group with no variants.
    pub fn from_pairs(pairs: Vec<(&str, &str)>) -> Self {
        let groups = pairs.into_iter()
            .map(|(s, l)| AbbrGroup {
                short: s.to_uppercase(),
                long: l.to_uppercase(),
                variants: vec![],
            })
            .collect();
        Self::from_groups(groups)
    }

    /// Bridge: short → long lookup (finds group, returns canonical long).
    pub fn to_long(&self, short: &str) -> Option<&str> {
        self.standardize(short).map(|(_, _, long)| long)
    }

    /// Bridge: long → short lookup (finds group, returns canonical short).
    pub fn to_short(&self, value: &str) -> Option<&str> {
        self.standardize(value).map(|(_, short, _)| short)
    }

    /// Bridge: all short→long pairs for iteration (used by PerWord standardize and pattern expansion).
    pub fn short_to_long_pairs(&self) -> Vec<(&str, &str)> {
        self.groups.iter()
            .map(|g| (g.short.as_str(), g.long.as_str()))
            .collect()
    }

    /// Bridge: bounded regex from all match values (used by pattern expansion).
    pub fn bounded_regex(&self) -> String {
        let values = self.all_match_values();
        let parts: Vec<String> = values.iter().map(|v| {
            if has_regex_chars(v) {
                v.to_string()
            } else {
                fancy_regex::escape(v)
            }
        }).collect();
        format!(r"(?:{})", parts.join("|"))
    }
```

- [ ] **Step 2: Rewrite build_directions()**

Replace lines 211-222 with:

```rust
fn build_directions() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec![] },
        AbbrGroup { short: "NW".into(), long: "NORTHWEST".into(), variants: vec![] },
        AbbrGroup { short: "SE".into(), long: "SOUTHEAST".into(), variants: vec![] },
        AbbrGroup { short: "SW".into(), long: "SOUTHWEST".into(), variants: vec![] },
        AbbrGroup { short: "N".into(), long: "NORTH".into(), variants: vec![] },
        AbbrGroup { short: "S".into(), long: "SOUTH".into(), variants: vec![] },
        AbbrGroup { short: "E".into(), long: "EAST".into(), variants: vec![] },
        AbbrGroup { short: "W".into(), long: "WEST".into(), variants: vec![] },
    ])
}
```

Note: multi-char directions (NE, NW, SE, SW) must come before single-char (N, S, E, W) in the group list so that `all_match_values()` sorts them correctly by length.

- [ ] **Step 3: Rewrite build_unit_types()**

Replace lines 224-250. Each entry becomes a group. No variants on most:

```rust
fn build_unit_types() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "APT".into(), long: "APARTMENT".into(), variants: vec![] },
        AbbrGroup { short: "UNIT".into(), long: "UNIT".into(), variants: vec![] },
        AbbrGroup { short: "STE".into(), long: "SUITE".into(), variants: vec![] },
        AbbrGroup { short: "FL".into(), long: "FLOOR".into(), variants: vec![] },
        AbbrGroup { short: "FLT".into(), long: "FLAT".into(), variants: vec![] },
        AbbrGroup { short: "BLDG".into(), long: "BUILDING".into(), variants: vec![] },
        AbbrGroup { short: "RM".into(), long: "ROOM".into(), variants: vec![] },
        AbbrGroup { short: "PH".into(), long: "PENTHOUSE".into(), variants: vec![] },
        AbbrGroup { short: "TOWNHOUSE".into(), long: "TOWNHOUSE".into(), variants: vec![] },
        AbbrGroup { short: "DEPT".into(), long: "DEPARTMENT".into(), variants: vec![] },
        AbbrGroup { short: "DUPLEX".into(), long: "DUPLEX".into(), variants: vec![] },
        AbbrGroup { short: "ATTIC".into(), long: "ATTIC".into(), variants: vec![] },
        AbbrGroup { short: "BSMT".into(), long: "BASEMENT".into(), variants: vec![] },
        AbbrGroup { short: "LOT".into(), long: "LOT".into(), variants: vec![] },
        AbbrGroup { short: "LVL".into(), long: "LEVEL".into(), variants: vec![] },
        AbbrGroup { short: "OFC".into(), long: "OFFICE".into(), variants: vec![] },
        AbbrGroup { short: "NUM".into(), long: "NUMBER".into(), variants: vec!["NO".into()] },
        AbbrGroup { short: "HSE".into(), long: "HOUSE".into(), variants: vec![] },
        AbbrGroup { short: "GARAGE".into(), long: "GARAGE".into(), variants: vec![] },
        AbbrGroup { short: "CONDO".into(), long: "CONDO".into(), variants: vec![] },
        AbbrGroup { short: "TRLR".into(), long: "TRAILER".into(), variants: vec![] },
        AbbrGroup { short: "#".into(), long: "#".into(), variants: vec![] },
    ])
}
```

Note: "NO" was a separate entry mapping to "NUMBER" — now it's a variant of the NUM group.

- [ ] **Step 4: Rewrite build_unit_locations()**

Replace lines 252-274:

```rust
fn build_unit_locations() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "UPPR".into(), long: "UPPER".into(), variants: vec!["UP".into()] },
        AbbrGroup { short: "LOWR".into(), long: "LOWER".into(), variants: vec!["LWR".into(), "LW".into()] },
        AbbrGroup { short: "FRNT".into(), long: "FRONT".into(), variants: vec!["FRT".into()] },
        AbbrGroup { short: "REAR".into(), long: "REAR".into(), variants: vec![] },
        AbbrGroup { short: "BACK".into(), long: "BACK".into(), variants: vec![] },
        AbbrGroup { short: "MID".into(), long: "MIDDLE".into(), variants: vec![] },
        AbbrGroup { short: "ENTIRE".into(), long: "ENTIRE".into(), variants: vec![] },
        AbbrGroup { short: "WHOLE".into(), long: "WHOLE".into(), variants: vec![] },
        AbbrGroup { short: "SINGLE".into(), long: "SINGLE".into(), variants: vec![] },
        AbbrGroup { short: "DOWN".into(), long: "DOWN".into(), variants: vec![] },
        AbbrGroup { short: "RIGHT".into(), long: "RIGHT".into(), variants: vec![] },
        AbbrGroup { short: "LEFT".into(), long: "LEFT".into(), variants: vec![] },
        AbbrGroup { short: "DOWNSTAIRS".into(), long: "DOWNSTAIRS".into(), variants: vec![] },
        AbbrGroup { short: "UPSTAIRS".into(), long: "UPSTAIRS".into(), variants: vec![] },
    ])
}
```

- [ ] **Step 5: Rewrite build_states() and build_na_values()**

`build_states()` (lines 276-296) — each state is a group with no variants. Use `from_pairs` bridge since there are many entries and no variants:

```rust
fn build_states() -> AbbrTable {
    AbbrTable::from_pairs(vec![
        ("AL", "ALABAMA"), ("AK", "ALASKA"), /* ... keep all existing pairs ... */
    ])
}
```

`build_na_values()` (lines 366-375) — value-list table where short = value, long = "". Use `from_pairs`:

```rust
fn build_na_values() -> AbbrTable {
    AbbrTable::from_pairs(vec![
        ("NA", ""), ("N/A", ""), ("N A", ""), ("NONE", ""), ("NULL", ""),
        ("-", ""), ("--", ""), ("---", ""), ("UNKNOWN", ""),
    ])
}
```

`build_street_name_abbr()` (lines 377-382) — keep using `from_pairs`:

```rust
fn build_street_name_abbr() -> AbbrTable {
    AbbrTable::from_pairs(vec![
        ("MT", "MOUNT"), ("FT", "FORT"),
    ])
}
```

- [ ] **Step 6: Rewrite build_all_suffixes() — absorb suffix_usps canonical data**

Replace both `build_usps_suffixes()` (lines 298-319) and `build_all_suffixes()` (lines 321-364). Delete `build_usps_suffixes()` entirely. The new `build_all_suffixes()` reads the CSV and groups entries:

```rust
fn build_all_suffixes() -> AbbrTable {
    let csv_data = include_str!("../../data/usps-street-suffix.csv");
    let mut groups: Vec<AbbrGroup> = Vec::new();
    // Map from USPS abbreviation (col3) → group index
    let mut usps_to_idx: HashMap<String, usize> = HashMap::new();

    for line in csv_data.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 3 { continue; }
        let primary = cols[0].trim().to_uppercase();    // col1: canonical long
        let variant = cols[1].trim().to_uppercase();    // col2: commonly used form
        let usps = cols[2].trim().to_uppercase();       // col3: canonical short

        // Skip excluded suffixes
        if usps == "TRAILER" || usps == "HIGHWAY" { continue; }

        if let Some(&idx) = usps_to_idx.get(&usps) {
            // Add variant to existing group (if not already canonical short or long)
            let group = &mut groups[idx];
            if variant != group.short && variant != group.long
                && !group.variants.contains(&variant)
            {
                group.variants.push(variant);
            }
        } else {
            // New group: col3 = canonical short, col1 = canonical long
            let idx = groups.len();
            let mut variants = vec![];
            // If the variant (col2) differs from both canonical forms, add it
            if variant != usps && variant != primary {
                variants.push(variant);
            }
            groups.push(AbbrGroup {
                short: usps.clone(),
                long: primary,
                variants,
            });
            usps_to_idx.insert(usps, idx);
        }
    }

    // Add manual overrides from R package's abbr_more_suffix
    let manual_variants: &[(&str, &[&str])] = &[
        ("BLVD", &["BVD", "BV", "BLV", "BL"]),
        ("CIR", &["CI"]),
        ("EXPY", &["EX", "EXPWY"]),
        ("HTS", &["HEIGHT", "HEIGHTS"]),
        ("PLZ", &["PLZA"]),
        ("XING", &["CROSSING"]),
    ];
    for (usps_short, extras) in manual_variants {
        if let Some(&idx) = usps_to_idx.get(*usps_short) {
            for extra in *extras {
                let e = extra.to_string();
                if !groups[idx].variants.contains(&e) {
                    groups[idx].variants.push(e);
                }
            }
        }
    }

    AbbrTable::from_groups(groups)
}
```

- [ ] **Step 7: Rewrite build_common_suffixes()**

Replace lines 384-404. Same structure, just simpler groups:

```rust
fn build_common_suffixes() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "DR".into(), long: "DRIVE".into(), variants: vec![] },
        AbbrGroup { short: "LN".into(), long: "LANE".into(), variants: vec![] },
        AbbrGroup { short: "AVE".into(), long: "AVENUE".into(), variants: vec![] },
        AbbrGroup { short: "RD".into(), long: "ROAD".into(), variants: vec![] },
        AbbrGroup { short: "ST".into(), long: "STREET".into(), variants: vec![] },
        AbbrGroup { short: "CIR".into(), long: "CIRCLE".into(), variants: vec![] },
        AbbrGroup { short: "CT".into(), long: "COURT".into(), variants: vec![] },
        AbbrGroup { short: "PL".into(), long: "PLACE".into(), variants: vec![] },
        AbbrGroup { short: "WAY".into(), long: "WAY".into(), variants: vec![] },
        AbbrGroup { short: "BLVD".into(), long: "BOULEVARD".into(), variants: vec![] },
        AbbrGroup { short: "STRA".into(), long: "STRAVENUE".into(), variants: vec![] },
        AbbrGroup { short: "CV".into(), long: "COVE".into(), variants: vec![] },
        AbbrGroup { short: "LOOP".into(), long: "LOOP".into(), variants: vec![] },
    ])
}
```

- [ ] **Step 8: Update build_default_tables() — remove suffix_usps**

In `build_default_tables()` (lines 406-422) and the `ABBR` static (lines 424-440):
- Remove `tables.insert("suffix_usps", build_usps_suffixes())` line
- Everything else stays the same

- [ ] **Step 9: Update number table registration**

`build_number_tables()` in `src/tables/numbers.rs` currently returns `(AbbrTable, AbbrTable)` built from `Abbr` entries. Update to use `AbbrGroup`:

In `src/tables/numbers.rs`, change `build_number_tables()`:

```rust
pub fn build_number_tables() -> (AbbrTable, AbbrTable) {
    let mut cardinal_groups = Vec::with_capacity(999);
    let mut ordinal_groups = Vec::with_capacity(999);

    for n in 1..=999u16 {
        cardinal_groups.push(AbbrGroup {
            short: n.to_string(),
            long: cardinal(n),
            variants: vec![],
        });
        ordinal_groups.push(AbbrGroup {
            short: n.to_string(),
            long: ordinal(n),
            variants: vec![],
        });
    }

    (AbbrTable::from_groups(cardinal_groups), AbbrTable::from_groups(ordinal_groups))
}
```

Update imports at the top of `numbers.rs`:
```rust
use super::abbreviations::{AbbrGroup, AbbrTable};
```

- [ ] **Step 10: Run all tests**

Run: `cargo test 2>&1`
Expected: New tests pass. Some old tests may fail due to removed `Abbr` references — fix in next step.

- [ ] **Step 11: Fix remaining test compilation errors**

Update any tests in `src/tables/abbreviations.rs` that reference the old `Abbr` struct or old `AbbrTable` methods like `new()`, `from_pairs_with_pattern()`. The bridge methods (`to_long`, `to_short`, `from_pairs`) should cover most callers.

Tests to check:
- `test_all_values_skips_empty` — uses `all_values()`, update to `all_match_values()`
- `test_is_value_list_true/false` — should work via bridge
- `test_number_tables_registered` — should work via bridge `to_long()`
- `test_suffix_usps_is_one_to_one` — DELETE this test (suffix_usps removed)
- `test_suffix_usps_bidirectional` — DELETE or rewrite for suffix_all
- `test_patch_*` tests — need rewriting (see Task 5)

- [ ] **Step 12: Run full test suite, fix any remaining failures**

Run: `cargo test 2>&1`
Expected: All tests pass

- [ ] **Step 13: Commit**

```bash
git add src/tables/abbreviations.rs src/tables/numbers.rs
git commit -m "feat: rewrite all table build functions to use AbbrGroup, remove suffix_usps"
```

---

## Task 3: Standardize step — single table model

Update `standardize_value`, `Step::Standardize`, `compile_step`, and `apply_step` to use one table.

**Files:**
- Modify: `src/step.rs:154-610`
- Modify: `data/defaults/steps.toml:195-230`

- [ ] **Step 1: Write test for new standardize behavior**

Update `test_apply_standardize_step` in `src/step.rs` (line 721):

```rust
#[test]
fn test_apply_standardize_step() {
    use crate::address::AddressState;
    use crate::tables::abbreviations::build_default_tables;
    use crate::config::OutputConfig;
    let abbr = build_default_tables();
    let def = StepDef {
        step_type: "standardize".to_string(),
        label: "std_suffix".to_string(),
        pattern: None, replacement: None,
        table: Some("suffix_all".to_string()),
        source: None,
        target: Some("suffix".to_string()),
        skip_if_filled: None,
        matching_table: None,
        format_table: None,
        mode: None,
        targets: None,
    };
    let step = compile_step(&def, &abbr).unwrap();
    let output = OutputConfig::default();
    let mut state = AddressState::new("test");
    *state.fields.field_mut(crate::address::Field::Suffix) = Some("AV".to_string());
    apply_step(&mut state, &step, &abbr, &output);
    // AV → finds AVENUE group → canonical long (default output) = AVENUE
    assert_eq!(state.fields.field(crate::address::Field::Suffix), Some(&"AVENUE".to_string()));
}
```

- [ ] **Step 2: Update Step::Standardize variant**

Replace `matching_table: Option<String>` and `format_table: Option<String>` with `table: String`:

```rust
Standardize {
    label: String,
    target: Field,
    table: String,
    pattern: Option<(Regex, String)>,
    mode: StandardizeMode,
    enabled: bool,
},
```

- [ ] **Step 3: Rewrite standardize_value()**

Replace lines 239-253:

```rust
fn standardize_value(
    value: &str,
    table: &AbbrTable,
    format: OutputFormat,
) -> String {
    match table.standardize(value) {
        Some((_, short, long)) => match format {
            OutputFormat::Short => short.to_string(),
            OutputFormat::Long => long.to_string(),
        },
        None => value.to_string(),
    }
}
```

- [ ] **Step 4: Update apply_step() for Standardize**

Replace the Standardize arm in `apply_step()` (lines 367-414):

```rust
Step::Standardize { target, table, pattern, mode, .. } => {
    // Handle regex-based standardize (like po_box)
    if let Some((re, repl)) = pattern {
        if let Some(val) = state.fields.field(*target) {
            let mut result = val.clone();
            if let Ok(replaced) = re.replace(&result, repl.as_str()) {
                result = replaced.to_string();
            }
            *state.fields.field_mut(*target) = Some(result);
        }
        return;
    }

    // Table-based standardize
    let val = match state.fields.field(*target) {
        Some(v) => v.to_string(),
        None => return,
    };

    let t = match tables.get(table) {
        Some(t) => t,
        None => return,
    };

    let fmt = output.format_for_field(*target);

    match mode {
        StandardizeMode::WholeField => {
            *state.fields.field_mut(*target) = Some(standardize_value(&val, t, fmt));
        }
        StandardizeMode::PerWord => {
            let words: Vec<&str> = val.split_whitespace().collect();
            let result: Vec<String> = words.iter()
                .map(|w| standardize_value(w, t, fmt))
                .collect();
            *state.fields.field_mut(*target) = Some(result.join(" "));
        }
    }
}
```

- [ ] **Step 5: Update compile_step() for "standardize"**

Replace the standardize arm in `compile_step()` (lines 568-610):

```rust
"standardize" => {
    let target = def
        .target
        .as_ref()
        .ok_or_else(|| format!("standardize step '{}' missing target", def.label))?;
    let mode = match def.mode.as_deref() {
        Some("per_word") => StandardizeMode::PerWord,
        _ => StandardizeMode::WholeField,
    };

    // Regex-based standardize: has pattern+replacement instead of table
    let pattern = if let Some(ref p) = def.pattern {
        let expanded = expand_template(p, abbr);
        let re = Regex::new(&expanded)
            .map_err(|e| format!("standardize step '{}' bad pattern: {}", def.label, e))?;
        let repl = def.replacement.clone().unwrap_or_default();
        Some((re, repl))
    } else {
        None
    };

    // Table-based standardize requires table name
    let table = if pattern.is_none() {
        def.table.clone()
            .or(def.matching_table.clone()) // backward compat: accept old field name
            .ok_or_else(|| format!(
                "standardize step '{}' needs either pattern+replacement or table",
                def.label
            ))?
    } else {
        def.table.clone().unwrap_or_default()
    };

    Ok(Step::Standardize {
        label: def.label.clone(),
        target: parse_field(target)?,
        table,
        pattern,
        mode,
        enabled: true,
    })
}
```

Note: `matching_table` accepted as fallback for backward compat with user configs during transition. Can be removed later.

- [ ] **Step 6: Update steps.toml — standardize steps**

Replace lines 195-215 in `data/defaults/steps.toml`:

```toml
# --- Standardization ---
[[step]]
type = "standardize"
label = "standardize_pre_direction"
target = "pre_direction"
table = "direction"

[[step]]
type = "standardize"
label = "standardize_post_direction"
target = "post_direction"
table = "direction"

[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
table = "suffix_all"

[[step]]
type = "standardize"
label = "standardize_unit_location"
target = "unit"
table = "unit_location"
```

The `standardize_po_box` step (pattern-based) stays the same — it doesn't use tables.

- [ ] **Step 7: Remove matching_table/format_table from StepDef if no longer needed**

Check if any code still references `def.matching_table` or `def.format_table` outside of `compile_step`. If not, consider removing them from `StepDef` or marking them deprecated. For now, keep them on `StepDef` (serde skip_serializing_if) so old user configs don't hard-fail on deserialization, but `compile_step` prefers `table`.

- [ ] **Step 8: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add src/step.rs data/defaults/steps.toml
git commit -m "feat: standardize steps use single table, unified lookup"
```

---

## Task 4: Config — DictEntry gains variants and canonical

Update config structs so users can add groups with variants and override canonical markers.

**Files:**
- Modify: `src/config.rs:94-109`
- Modify: `src/tables/abbreviations.rs` — `patch()` method

- [ ] **Step 1: Update DictEntry**

In `src/config.rs`, update `DictEntry` (lines 105-109):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DictEntry {
    pub short: String,
    pub long: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<bool>,
}
```

- [ ] **Step 2: Rewrite AbbrTable::patch()**

Replace lines 103-133 in `src/tables/abbreviations.rs`. The new patch handles:
- **Remove:** search all values across all groups, remove entire matching group
- **Add:** if a group with matching canonical short or long exists, merge variants. If `canonical = true`, demote old canonical short to variant and replace. If no group matches, create new group.
- **Override:** removed as a concept (canonical flag replaces it)

```rust
impl AbbrTable {
    pub fn patch(&self, overrides: &DictOverrides) -> Self {
        let mut groups = self.groups.clone();

        // Remove phase: remove groups where any value matches
        if !overrides.remove.is_empty() {
            let remove_set: std::collections::HashSet<String> = overrides.remove.iter()
                .map(|v| v.to_uppercase())
                .collect();
            groups.retain(|g| {
                !remove_set.contains(&g.short)
                    && !remove_set.contains(&g.long)
                    && !g.variants.iter().any(|v| remove_set.contains(&v.to_uppercase()))
            });
        }

        // Add phase
        for entry in &overrides.add {
            let short = entry.short.to_uppercase();
            let long = entry.long.to_uppercase();
            let new_variants: Vec<String> = entry.variants.iter()
                .map(|v| v.to_uppercase())
                .collect();
            let is_canonical = entry.canonical.unwrap_or(false);

            // Find existing group by canonical short or long
            let existing = groups.iter().position(|g| {
                g.short == short || g.short == long
                    || g.long == short || g.long == long
            });

            if let Some(idx) = existing {
                let group = &mut groups[idx];
                // Merge variants
                for v in &new_variants {
                    if *v != group.short && *v != group.long && !group.variants.contains(v) {
                        group.variants.push(v.clone());
                    }
                }
                if is_canonical {
                    // Demote old canonical short to variant (if different from new)
                    if group.short != short {
                        let old_short = group.short.clone();
                        if !group.variants.contains(&old_short) {
                            group.variants.push(old_short);
                        }
                        group.short = short;
                    }
                    if group.long != long {
                        let old_long = group.long.clone();
                        if !group.variants.contains(&old_long) {
                            group.variants.push(old_long);
                        }
                        group.long = long;
                    }
                }
            } else {
                // New group
                groups.push(AbbrGroup {
                    short,
                    long,
                    variants: new_variants,
                });
            }
        }

        Self::from_groups(groups)
    }
}
```

- [ ] **Step 3: Write tests for new patch behavior**

```rust
#[test]
fn test_patch_add_variant_to_existing_group() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec![] },
    ]);
    let overrides = DictOverrides {
        add: vec![DictEntry {
            short: "NE".into(), long: "NORTHEAST".into(),
            variants: vec!["N E".into(), "NEAST".into()],
            canonical: None,
        }],
        remove: vec![],
        override_entries: vec![],
    };
    let patched = table.patch(&overrides);
    assert_eq!(patched.standardize("N E"), Some((0, "NE", "NORTHEAST")));
    assert_eq!(patched.standardize("NEAST"), Some((0, "NE", "NORTHEAST")));
}

#[test]
fn test_patch_canonical_override_demotes_old() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec![] },
    ]);
    let overrides = DictOverrides {
        add: vec![DictEntry {
            short: "NEAST".into(), long: "NORTHEAST".into(),
            variants: vec![],
            canonical: Some(true),
        }],
        remove: vec![],
        override_entries: vec![],
    };
    let patched = table.patch(&overrides);
    // New canonical
    let result = patched.standardize("NORTHEAST").unwrap();
    assert_eq!(result.1, "NEAST");
    // Old short demoted to variant, still findable
    assert_eq!(patched.standardize("NE").unwrap().1, "NEAST");
}

#[test]
fn test_patch_add_new_group() {
    let table = AbbrTable::from_groups(vec![]);
    let overrides = DictOverrides {
        add: vec![DictEntry {
            short: "WH".into(), long: "WAREHOUSE".into(),
            variants: vec!["WHSE".into()],
            canonical: None,
        }],
        remove: vec![],
        override_entries: vec![],
    };
    let patched = table.patch(&overrides);
    assert_eq!(patched.standardize("WHSE"), Some((0, "WH", "WAREHOUSE")));
}

#[test]
fn test_patch_remove_group() {
    let table = AbbrTable::from_groups(vec![
        AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec!["N E".into()] },
        AbbrGroup { short: "NW".into(), long: "NORTHWEST".into(), variants: vec![] },
    ]);
    let overrides = DictOverrides {
        add: vec![],
        remove: vec!["N E".into()], // matches a variant → removes the whole NE group
        override_entries: vec![],
    };
    let patched = table.patch(&overrides);
    assert_eq!(patched.standardize("NE"), None);
    assert_eq!(patched.standardize("NW"), Some((0, "NW", "NORTHWEST")));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_patch -- --nocapture`
Expected: All pass

- [ ] **Step 5: Update config serialization tests**

Update `test_dictionaries_roundtrip` in `src/config.rs` if it exists, to include `variants` and `canonical` fields.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/tables/abbreviations.rs
git commit -m "feat: DictEntry supports variants and canonical, patch rewritten for AbbrGroup"
```

---

## Task 5: Pattern expansion — use all_match_values()

Update `expand_template` in `src/step.rs` to use the new `all_match_values()` method for pattern generation.

**Files:**
- Modify: `src/step.rs` — `expand_template()` function
- Modify: `src/tables/abbreviations.rs` — ensure `all_values()` is replaced

- [ ] **Step 1: Check expand_template current implementation**

Read `expand_template` in `src/step.rs`. It currently calls `table.all_values()` or `table.short_values()` for pattern generation. Update these calls to use `all_match_values()`.

The `$short` accessor (e.g., `{suffix_common$short}`) should use a new method that returns only canonical short values. Add to AbbrTable:

```rust
    /// All canonical short values, sorted longest-first.
    pub fn short_values(&self) -> Vec<&str> {
        let mut vals: Vec<&str> = self.groups.iter()
            .map(|g| g.short.as_str())
            .collect();
        vals.sort_unstable();
        vals.dedup();
        vals.sort_by(|a, b| b.len().cmp(&a.len()));
        vals
    }
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1`
Expected: All pass — pattern expansion should work with new method names.

- [ ] **Step 3: Commit**

```bash
git add src/step.rs src/tables/abbreviations.rs
git commit -m "feat: pattern expansion uses all_match_values from AbbrGroup tables"
```

---

## Task 6: TUI — dict editor with group view

Rewrite the dictionary editor to show groups with canonical short, long, and variants.

**Files:**
- Modify: `src/tui.rs` — `DictEntryState`, dict rendering, dict input handling

- [ ] **Step 1: Replace DictEntryState with DictGroupState**

Replace `DictEntryState` (lines 123-129) and `EntryStatus` (lines 131-137):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct DictGroupState {
    short: String,
    long: String,
    variants: Vec<String>,
    status: GroupStatus,
    /// Original values for tracking overrides
    original_short: String,
    original_long: String,
    original_variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupStatus {
    Default,
    Added,
    Removed,
    Modified,
}
```

- [ ] **Step 2: Update App struct and initialization**

Change `dict_entries: Vec<Vec<DictEntryState>>` to `dict_entries: Vec<Vec<DictGroupState>>`. Update the initialization code (lines 250-320) to build `DictGroupState` from `AbbrGroup`s and apply config overrides.

- [ ] **Step 3: Update dict rendering — main list**

Update `render_dict()` (lines 2091-2223) to show groups:

```
★ NE    NORTHEAST    N E, NEAST
★ NW    NORTHWEST
★ N     NORTH
```

Format: `★ {short}  {long}  {variants joined by ", "}` with status coloring (green = added, red = removed, yellow = modified).

- [ ] **Step 4: Add drill-down view for variant editing**

Add a new `InputMode::DictGroupDetail(usize)` (or similar) that shows when you press Enter on a group:

```
NE → NORTHEAST
  [x] N E
  [x] NEAST
  [+] Add variant...
```

Space toggles variants, 'a' to add new variant, 'd' to delete. Esc returns to main list.

- [ ] **Step 5: Update to_config() — collect dict overrides**

Update `to_config()` (lines 448-480) to produce `DictEntry` with `variants` and `canonical` fields from `DictGroupState`.

- [ ] **Step 6: Update dict-related tests**

Update `test_to_config_dict_add`, `test_to_config_dict_remove`, `test_to_config_dict_override`, `test_dict_entry_add` to work with the new group model.

- [ ] **Step 7: Run tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 8: Commit**

```bash
git add src/tui.rs
git commit -m "feat: TUI dict editor shows groups with canonical markers and variant drill-down"
```

---

## Task 7: TUI — standardize wizard simplification

Replace the confusing matching_table + format_table flow with a single table pick.

**Files:**
- Modify: `src/tui.rs` — wizard states, input handling, rendering

- [ ] **Step 1: Remove PickMatchingTable and PickFormatTable states**

Delete `PickMatchingTable` and `PickFormatTable` wizard states (and the `PickTable` state that was partially added). Replace with a single `PickTable(usize, Vec<String>)` state.

- [ ] **Step 2: Update StandardizeMode flow**

When user picks "Table-based" in `StandardizeMode`:
- Go to `PickTable` (single table pick with descriptions)
- Then go to `WordMode`
- Then `Label`

The table picked is stored in `app.wizard_acc.table` (which already exists on the accumulator).

- [ ] **Step 3: Update wizard rendering**

Render `PickTable` as a pick-list using `TABLE_DESCRIPTIONS` constant (already defined). Title: "Pick table for standardization".

- [ ] **Step 4: Update step finalization**

In the Label handler where the `StepDef` is constructed, set `table` from the accumulator:

```rust
table: acc.table.clone(),
matching_table: None,
format_table: None,
```

- [ ] **Step 5: Clean up unused wizard state rendering**

Remove render code for `PickMatchingTable` and `PickFormatTable` if they still exist.

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
git add src/tui.rs
git commit -m "feat: standardize wizard uses single table pick instead of matching+format"
```

---

## Task 8: Cleanup and integration test updates

Fix any remaining test failures, remove dead code, update integration tests.

**Files:**
- Modify: `tests/config.rs`
- Modify: `src/tables/abbreviations.rs` — remove old methods
- Modify: `src/step.rs` — remove old StepDef fields if safe

- [ ] **Step 1: Remove bridge methods no longer called**

Check if `to_long()`, `to_short()`, `short_to_long_pairs()` are still called anywhere. If not, delete them. Keep `from_pairs()` if number tables or simple tables still use it.

Run: `cargo test 2>&1` after each removal.

- [ ] **Step 2: Update integration tests**

Check `tests/config.rs` for any tests that reference `suffix_usps`, `matching_table`, `format_table`, or old standardize behavior. Update to use new `table` field.

- [ ] **Step 3: Verify user config compatibility**

Test with the user's config at `/tmp/test-addrust.toml`. Ensure:
- `short = "N E", long = "NORTHEAST"` direction entries work (standardization finds "N E")
- Custom steps with old `matching_table` field still compile (backward compat in compile_step)
- Suffix standardization works without suffix_usps

Run: `cargo run -- parse "123 N E Main Ave" --config /tmp/test-addrust.toml`

- [ ] **Step 4: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass, zero warnings about unused code

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: clean up dead code, update integration tests for canonical tables"
```

---

## Task Order and Dependencies

```
Task 1 (AbbrGroup + AbbrTable core)
  └→ Task 2 (build functions + bridge methods)
       ├→ Task 3 (standardize step)
       │    └→ Task 5 (pattern expansion)
       ├→ Task 4 (config + patch)
       │    └→ Task 6 (TUI dict editor)
       └→ Task 7 (TUI standardize wizard)
            └→ Task 8 (cleanup)
```

Tasks 3, 4 can run in parallel after Task 2.
Tasks 5, 6, 7 can run in parallel after their respective dependencies.
Task 8 runs last.
