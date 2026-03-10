# Tables Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move hardcoded NA values and street name abbreviations into dictionary tables, add value-list table support, add `{table$short}` accessor syntax, and refactor template expansion to work directly from `&Abbreviations`.

**Architecture:** New `na_values` (value-list) and `street_name_abbr` (short/long) tables in `abbreviations.rs`. Template expansion reads from `Abbreviations` directly instead of a pre-joined `HashMap`. The `{table$short}` accessor syntax lets templates reference only the short column of a table.

**Tech Stack:** Rust, fancy_regex, ratatui (TUI), toml/serde (config)

---

### Task 1: Add `is_value_list()` and `short_values()` to AbbrTable

**Files:**
- Modify: `src/tables/abbreviations.rs:55-66`

**Step 1: Write the failing tests**

Add to the existing `mod tests` block in `src/tables/abbreviations.rs`:

```rust
#[test]
fn test_is_value_list_true() {
    let table = AbbrTable::new(vec![
        abbr("NULL", ""),
        abbr("NAN", ""),
        abbr("MISSING", ""),
    ]);
    assert!(table.is_value_list());
}

#[test]
fn test_is_value_list_false() {
    let table = AbbrTable::new(vec![abbr("ST", "STREET")]);
    assert!(!table.is_value_list());
}

#[test]
fn test_all_values_skips_empty() {
    let table = AbbrTable::new(vec![
        abbr("NULL", ""),
        abbr("NAN", ""),
    ]);
    let vals = table.all_values();
    assert_eq!(vals, vec!["NULL", "NAN"]);
    assert!(!vals.contains(&""));
}

#[test]
fn test_short_values() {
    let table = AbbrTable::new(vec![
        abbr("ST", "STREET"),
        abbr("AVE", "AVENUE"),
    ]);
    let shorts = table.short_values();
    // Sorted by length descending
    assert_eq!(shorts, vec!["AVE", "ST"]);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib tables::abbreviations::tests -- test_is_value_list test_all_values_skips_empty test_short_values`
Expected: FAIL — methods don't exist

**Step 3: Implement the methods**

Add to `impl AbbrTable` in `src/tables/abbreviations.rs`, after `all_values()`:

```rust
/// True when all long forms are empty — a value-list table (not a short↔long mapping).
pub fn is_value_list(&self) -> bool {
    !self.entries.is_empty() && self.entries.iter().all(|e| e.long.is_empty())
}

/// Only the short column values, sorted by length descending.
pub fn short_values(&self) -> Vec<&str> {
    let mut vals: Vec<&str> = self
        .entries
        .iter()
        .map(|e| e.short.as_str())
        .collect();
    vals.sort_unstable();
    vals.dedup();
    vals.sort_by(|a, b| b.len().cmp(&a.len()));
    vals
}
```

Also modify `all_values()` to skip empty strings:

```rust
pub fn all_values(&self) -> Vec<&str> {
    let mut vals: Vec<&str> = self
        .entries
        .iter()
        .flat_map(|e| [e.short.as_str(), e.long.as_str()])
        .filter(|v| !v.is_empty())
        .collect();
    vals.sort_unstable();
    vals.dedup();
    vals.sort_by(|a, b| b.len().cmp(&a.len()));
    vals
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib tables::abbreviations`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "feat: add is_value_list(), short_values(), and filter empties from all_values()"
```

---

### Task 2: Add `na_values` and `street_name_abbr` tables

**Files:**
- Modify: `src/tables/abbreviations.rs:360-383` (add builder functions and register in both `build_default_tables` and `ABBR`)

**Step 1: Write the failing tests**

Add to `mod tests` in `src/tables/abbreviations.rs`:

```rust
#[test]
fn test_na_values_table_exists() {
    let tables = build_default_tables();
    let na = tables.get("na_values").unwrap();
    assert!(na.is_value_list());
    let vals = na.all_values();
    assert!(vals.contains(&"NULL"));
    assert!(vals.contains(&"NO ADDRESS"));
}

#[test]
fn test_street_name_abbr_table_exists() {
    let tables = build_default_tables();
    let sna = tables.get("street_name_abbr").unwrap();
    assert!(!sna.is_value_list());
    assert_eq!(sna.to_long("MT"), Some("MOUNT"));
    assert_eq!(sna.to_long("FT"), Some("FORT"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib tables::abbreviations::tests -- test_na_values test_street_name_abbr`
Expected: FAIL — `unwrap()` on None

**Step 3: Implement the builder functions and register them**

Add before `build_default_tables()` in `src/tables/abbreviations.rs`:

```rust
fn build_na_values() -> AbbrTable {
    AbbrTable::new(vec![
        abbr("NULL", ""),
        abbr("NAN", ""),
        abbr("MISSING", ""),
        abbr("NONE", ""),
        abbr("UNKNOWN", ""),
        abbr("NO ADDRESS", ""),
    ])
}

fn build_street_name_abbr() -> AbbrTable {
    AbbrTable::new(vec![
        abbr("MT", "MOUNT"),
        abbr("FT", "FORT"),
    ])
}
```

Add to both `build_default_tables()` and the `ABBR` LazyLock:

```rust
tables.insert("na_values".to_string(), build_na_values());
tables.insert("street_name_abbr".to_string(), build_street_name_abbr());
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib tables::abbreviations`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "feat: add na_values and street_name_abbr dictionary tables"
```

---

### Task 3: Allow `$` in template table refs (`{table$short}` syntax)

**Files:**
- Modify: `src/pattern.rs:92-105` (`find_table_ref`)

**Step 1: Write the failing test**

Add to `mod tests` in `src/pattern.rs`:

```rust
#[test]
fn test_parse_table_ref_with_accessor() {
    let segments = parse_pattern(r"\b({street_name_abbr$short})\b");
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0], PatternSegment::Literal(r"\b(".to_string()));
    assert_eq!(segments[1], PatternSegment::TableRef("street_name_abbr$short".to_string()));
    assert_eq!(segments[2], PatternSegment::Literal(r")\b".to_string()));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib pattern::tests::test_parse_table_ref_with_accessor`
Expected: FAIL — `$` not allowed, so `{street_name_abbr$short}` parsed as literal

**Step 3: Allow `$` in table ref names**

In `src/pattern.rs`, `find_table_ref`, change the character check:

```rust
if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib pattern`
Expected: ALL PASS (including existing tests)

**Step 5: Commit**

```bash
git add src/pattern.rs
git commit -m "feat: allow \$ in template table refs for {table\$short} accessor syntax"
```

---

### Task 4: Refactor `build_rules` to expand templates from `&Abbreviations` directly

This is the core refactor: eliminate the `table_values` HashMap. The `rule` closure captures `&Abbreviations` and expands `{name}` and `{name$short}` placeholders by looking up the table and calling the appropriate method.

**Files:**
- Modify: `src/tables/rules.rs` (entire `build_rules` function)

**Step 1: Write a test for the new expansion logic**

Add a helper function and test in `src/tables/rules.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::abbreviations::build_default_tables;

    #[test]
    fn test_expand_template_all_values() {
        let abbr = build_default_tables();
        let expanded = expand_template("{direction}", &abbr);
        assert!(expanded.contains("NORTH"));
        assert!(expanded.contains("NE"));
    }

    #[test]
    fn test_expand_template_short_accessor() {
        let abbr = build_default_tables();
        let expanded = expand_template("{street_name_abbr$short}", &abbr);
        assert!(expanded.contains("MT"));
        assert!(expanded.contains("FT"));
        assert!(!expanded.contains("MOUNT"));
        assert!(!expanded.contains("FORT"));
    }

    #[test]
    fn test_expand_template_state_bounded() {
        let abbr = build_default_tables();
        let expanded = expand_template("{state}", &abbr);
        // state uses bounded_regex — should have \b wrapper
        assert!(expanded.contains(r"\b("));
    }

    #[test]
    fn test_expand_template_unit_type_excludes_hash() {
        let abbr = build_default_tables();
        let expanded = expand_template("{unit_type}", &abbr);
        assert!(!expanded.contains("#|"));
        assert!(!expanded.contains("|#"));
        assert!(expanded.contains("APARTMENT"));
    }

    #[test]
    fn test_build_rules_count() {
        let abbr = build_default_tables();
        let rules = build_rules(&abbr, &HashMap::new());
        // Should have rules (exact count may change, but sanity check)
        assert!(rules.len() > 10);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib tables::rules`
Expected: FAIL — `expand_template` doesn't exist

**Step 3: Implement `expand_template` and refactor `build_rules`**

Add the `expand_template` function in `src/tables/rules.rs`:

```rust
/// Expand all `{...}` placeholders in a template using abbreviation tables.
///
/// - `{table_name}` → `table.all_values().join("|")`
/// - `{table_name$short}` → `table.short_values().join("|")`
///
/// Special cases:
/// - `state` uses `bounded_regex()` (word-boundary-wrapped)
/// - `unit_type` excludes `#` from `all_values()`
fn expand_template(template: &str, abbr: &Abbreviations) -> String {
    let mut result = template.to_string();
    // Find all {placeholder} patterns (including $accessor)
    while let Some(start) = result.find('{') {
        if let Some(end) = result[start..].find('}') {
            let end = start + end;
            let placeholder = &result[start + 1..end];
            let (table_name, accessor) = if let Some(idx) = placeholder.find('$') {
                (&placeholder[..idx], Some(&placeholder[idx + 1..]))
            } else {
                (placeholder, None)
            };

            if let Some(table) = abbr.get(table_name) {
                let values = match (table_name, accessor) {
                    ("state", _) => table.bounded_regex(),
                    ("unit_type", None) => table
                        .all_values()
                        .into_iter()
                        .filter(|v| *v != "#")
                        .collect::<Vec<_>>()
                        .join("|"),
                    (_, Some("short")) => table.short_values().join("|"),
                    _ => table.all_values().join("|"),
                };
                result = format!("{}{}{}", &result[..start], values, &result[end + 1..]);
            } else {
                // Unknown table — leave placeholder, advance past it
                break;
            }
        } else {
            break;
        }
    }
    result
}
```

Then refactor `build_rules` to remove the `table_values` HashMap and use `expand_template`:

Replace lines 13-40 (the table lookups and `table_values` construction) and the `rule` closure to use `expand_template` for both the default pattern and overrides:

```rust
pub fn build_rules(abbr: &Abbreviations, pattern_overrides: &HashMap<String, String>) -> Vec<Rule> {
    // Closure captures shared state — each call site only passes rule-specific args.
    let rule = |label: &str, group: &str, pattern_template: &str,
                action: Action, target: Option<Field>,
                standardize: Option<(&str, &str)>, skip_if_filled: bool| -> Rule {
        let final_template = pattern_overrides
            .get(label)
            .cloned()
            .unwrap_or_else(|| pattern_template.to_string());

        let final_pattern = expand_template(&final_template, abbr);

        let std = standardize.map(|(m, r)| {
            let expanded_m = expand_template(m, abbr);
            (Regex::new(&expanded_m).unwrap(), r.to_string())
        });
        Rule {
            label: label.to_string(),
            group: group.to_string(),
            pattern: Regex::new(&final_pattern)
                .unwrap_or_else(|e| panic!("Bad regex in rule {}: {}", label, e)),
            pattern_template: final_template,
            action,
            target,
            standardize: std,
            skip_if_filled,
            enabled: true,
        }
    };
```

Now update every `rules.push(rule(...))` call — the `rule` closure no longer takes a separate `pattern` arg (the pre-expanded regex). Instead, it only takes `pattern_template` and expands it internally. This means:

- **Remove the `pattern` parameter** (was the second string arg before `pattern_template`)
- **All rule calls now pass only the template** — expansion happens inside the closure
- **Standardize regex patterns also use templates** — they may contain `{table}` placeholders too

Updated rules (all of them, showing the new call signature):

```rust
    let mut rules = Vec::new();

    // 1. NA CHECK
    rules.push(rule(
        "change_na_address",
        "na_check",
        r"(?i)^(N/?A|{na_values})$",
        Action::Warn,
        None,
        None,
        false,
    ));

    // 2. CITY / STATE / ZIP
    rules.push(rule(
        "city_state_zip",
        "city_state",
        r",\s*([A-Z][A-Z ]+)\W+{state}\W+(\d{5}(?:\W\d{4})?)(?:\s*US)?$",
        Action::Extract,
        Some(Field::ExtraBack),
        None,
        false,
    ));

    // 3. PO BOX
    rules.push(rule(
        "po_box_number",
        "po_box",
        r"\bP\W*O\W+?BOX\W*(\d+)\b",
        Action::Extract,
        Some(Field::PoBox),
        Some((r"P\W*O\W+?BOX\W*(\d+)", "PO BOX $1")),
        true,
    ));
    rules.push(rule(
        "po_box_word",
        "po_box",
        r"\bP\W*O\W+?BOX\W+(\w+)\b",
        Action::Extract,
        Some(Field::PoBox),
        Some((r"P\W*O\W+?BOX\W+(\w+)", "PO BOX $1")),
        true,
    ));

    // 4. PRE-CHECKS
    rules.push(rule(
        "change_unstick_suffix_unit",
        "pre_check",
        r"\b({common_suffix})({unit_type})\b",
        Action::Change,
        None,
        Some((r"({common_suffix})({unit_type})", "$1 $2")),
        false,
    ));

    rules.push(rule(
        "change_st_to_saint",
        "pre_check",
        r"^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{all_suffix})\b)([A-Z]{3,20})",
        Action::Change,
        None,
        Some((
            r"^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{all_suffix})\b)([A-Z]{3,20})",
            "${1}SAINT $2",
        )),
        false,
    ));

    // 5. EXTRA FRONT
    rules.push(rule(
        "extra_front",
        "extra",
        r"^(?:(?:[A-Z\W]+\s)+(?=(?:{direction})\s\d))|^(?:(?:[A-Z\W]+\s)+(?=\d))",
        Action::Extract,
        Some(Field::ExtraFront),
        None,
        true,
    ));

    // 6. STREET NUMBER
    rules.push(rule(
        "street_number_coords_two",
        "street_number",
        r"^([NSEW])\W?(\d+)\W?([NSEW])\W?(\d+)\b",
        Action::Extract,
        Some(Field::StreetNumber),
        Some((r"([NSEW])\W?(\d+)\W?([NSEW])\W?(\d+)", "${1}${2} ${3}${4}")),
        true,
    ));
    rules.push(rule(
        "street_number_simple",
        "street_number",
        r"^\d+\b",
        Action::Extract,
        Some(Field::StreetNumber),
        Some((r"^0+(\d+)", "$1")),
        true,
    ));
    rules.push(rule(
        "unit_fraction",
        "street_number",
        r"^[1-9]/\d+\b",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));

    // 7. UNIT
    rules.push(rule(
        "unit_type_value",
        "unit",
        r"(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));
    rules.push(rule(
        "unit_pound",
        "unit",
        r"#\W*(\w+)\s*$",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));
    rules.push(rule(
        "unit_location",
        "unit",
        r"\b({unit_location})\s*$",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));

    // 8. POST-DIRECTION
    rules.push(rule(
        "post_direction",
        "direction",
        r"(?<!^)\b({direction})\s*$",
        Action::Extract,
        Some(Field::PostDirection),
        None,
        true,
    ));

    // 9. SUFFIX
    rules.push(rule(
        "suffix_common",
        "suffix",
        r"(?<!^)\b({common_suffix})\s*$",
        Action::Extract,
        Some(Field::Suffix),
        None,
        true,
    ));
    rules.push(rule(
        "suffix_all",
        "suffix",
        r"(?<!^)\b({all_suffix})\s*$",
        Action::Extract,
        Some(Field::Suffix),
        None,
        true,
    ));

    // 10. PRE-DIRECTION
    rules.push(rule(
        "pre_direction",
        "direction",
        r"^\b({direction})\b(?!$)",
        Action::Extract,
        Some(Field::PreDirection),
        None,
        true,
    ));

    // 11. STREET NAME STANDARDIZATION
    rules.push(rule(
        "change_street_name_abbr",
        "street_name",
        r"\b({street_name_abbr$short})\b",
        Action::Change,
        None,
        Some((r"\b({street_name_abbr$short})\b", "STREET_NAME_ABBR_REPLACE")),
        false,
    ));
    rules.push(rule(
        "change_name_st_to_saint",
        "street_name",
        r"(?:^|\s)ST\b(?=\s[A-Z]{3,})",
        Action::Change,
        None,
        Some((r"(?:^|\b)ST\b(?=\s[A-Z]{3,})", "SAINT")),
        false,
    ));

    rules
}
```

**Important:** The `change_street_name_abbr` rule's standardize regex needs special handling — it must iterate the table's short→long pairs to replace whichever short matched. This can't use a simple regex replacement string. We need a custom approach:

The standardize tuple approach won't work for table-driven replacement (MT→MOUNT, FT→FORT dynamically). Instead, make the standardize for this rule use a different mechanism. The simplest approach: since `expand_template` already handles `{street_name_abbr$short}` to produce `MOUNT|FORT` (wait, no, `$short` produces `MT|FT`), the pattern matches correctly. For replacement, we need the rule to look up the match in the table.

**Revised approach for `change_street_name_abbr`:** Don't use the standardize tuple. Instead, add a new field `standardize_table: Option<String>` to `Rule` that names a table to use for short→long replacement. Or simpler: generate the standardize regex as individual alternations with backreferences. Actually simplest: generate two separate standardize replacements in sequence — but Rule only supports one.

**Simplest correct approach:** Since there are only 2 entries (MT→MOUNT, FT→FORT), and users might add more, the standardize regex should be a series of alternations that capture which one matched. But regex replacement can't conditionally replace based on which alternative matched.

**Actually, the cleanest approach:** Make the standardize replacement for `change_street_name_abbr` use a callback-style replacement. But the current `Rule` struct uses `(Regex, String)` for standardize.

**Practical approach:** Add a `standardize_fn` field to `Rule` — an optional closure that transforms the working string. For `change_street_name_abbr`, this closure iterates `short_to_long_pairs()` and does `\bSHORT\b` → `LONG` for each.

**Revised Task 4 scope:** This task just does the `expand_template` refactor and updates all rules EXCEPT the merged `change_street_name_abbr` rule. Keep `change_name_mt_to_mount` and `change_name_ft_to_fort` as-is for now. Task 5 will handle the merge.

Actually, re-reading the design doc: "The `change_street_name_abbr` rule standardize regex needs to iterate the table's short→long pairs to replace whichever short form matched with its long form." So the design acknowledges this needs special handling. Let me plan it properly.

**Best approach:** Add an optional `standardize_fn: Option<Box<dyn Fn(&str) -> String + Send + Sync>>` to `Rule`. For `change_street_name_abbr`, the closure captures the table's pairs and iterates them. This keeps the general architecture (domain knowledge in data) while handling the multi-replacement case.

This is getting complex. Let me split Task 4 into the refactor only (no rule merging) and put the street name abbreviation merge in Task 5 with the `standardize_fn`.

**Step 4: Run all tests**

Run: `cargo test`
Expected: ALL PASS — behavior unchanged, just refactored internals

**Step 5: Commit**

```bash
git add src/tables/rules.rs
git commit -m "refactor: expand templates from Abbreviations directly, eliminate table_values map"
```

---

### Task 5: Add `na_values` to the `change_na_address` rule pattern

This uses the `na_values` table so users can configure which values trigger NA warnings.

**Files:**
- Modify: `src/tables/rules.rs` (just the `change_na_address` rule call)

**Step 1: Write the failing test**

Add to `tests/config.rs`:

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
    assert!(addr.warnings.contains(&"change_na_address".to_string()));
}

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
    // NULL should no longer trigger NA warning
    assert!(!addr.warnings.contains(&"change_na_address".to_string()));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test config test_config_adds_custom_na_value test_config_removes_na_value`
Expected: FAIL

**Step 3: Update the rule pattern**

In `src/tables/rules.rs`, the `change_na_address` rule template becomes:

```rust
r"(?i)^(N/?A|{na_values})$"
```

This was already done in Task 4's rule listing. If Task 4 is complete, this should already work. The test just validates the config-driven behavior.

**Step 4: Run tests**

Run: `cargo test`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add tests/config.rs
git commit -m "test: verify na_values table is configurable via config"
```

---

### Task 6: Merge MT/FT rules into `change_street_name_abbr` with table-driven replacement

**Files:**
- Modify: `src/pipeline.rs:22-36` (add `standardize_fn` field to `Rule`)
- Modify: `src/tables/rules.rs` (replace two rules with one, use `standardize_fn`)

**Step 1: Write the failing tests**

Add to `tests/config.rs`:

```rust
#[test]
fn test_mt_to_mount_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 MT VERNON AVE");
    assert_eq!(addr.street_name.as_deref(), Some("MOUNT VERNON"));
}

#[test]
fn test_ft_to_fort_default() {
    let p = Pipeline::default();
    let addr = p.parse("456 FT WORTH BLVD");
    assert_eq!(addr.street_name.as_deref(), Some("FORT WORTH"));
}

#[test]
fn test_config_adds_street_name_abbr() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.street_name_abbr]
add = [{ short = "PT", long = "POINT" }]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 PT LOOKOUT RD");
    assert_eq!(addr.street_name.as_deref(), Some("POINT LOOKOUT"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test config test_mt_to_mount test_ft_to_fort test_config_adds_street_name_abbr`
Expected: First two may pass (existing rules handle them), third will FAIL

**Step 3: Add `standardize_fn` to Rule**

In `src/pipeline.rs`, add to the `Rule` struct:

```rust
/// Table-driven standardization: lookup matched value in the named table's short→long pairs.
pub standardize_table: Option<String>,
```

Update `apply_rule` in `src/pipeline.rs` — in the `Action::Change` arm, after the existing standardize block, add:

```rust
Action::Change => {
    if let Some(ref table_name) = rule.standardize_table {
        // Table-driven replacement: replace each short form with its long form
        if let Some(table) = ABBR.get(table_name) {
            for (short, long) in table.short_to_long_pairs() {
                let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                replace_pattern(&mut state.working, &re, &long);
            }
            squish(&mut state.working);
        }
    } else if let Some((ref match_re, ref replacement)) = rule.standardize {
        // ... existing code ...
    }
}
```

Wait — this uses the global `ABBR` instead of the config-patched tables. For config-driven tables, the pipeline needs to carry a reference to the patched `Abbreviations`. Let me reconsider.

**Revised approach:** Store the replacement pairs directly in the `Rule` at build time, not a table name. This way config-patched tables are already baked in.

In `src/pipeline.rs`, add to `Rule`:

```rust
/// Table-driven standardization: pairs of (match_regex, replacement_string).
pub standardize_pairs: Vec<(Regex, String)>,
```

In `apply_rule`, `Action::Change`:

```rust
Action::Change => {
    if !rule.standardize_pairs.is_empty() {
        for (ref match_re, ref replacement) in &rule.standardize_pairs {
            replace_pattern(&mut state.working, match_re, replacement);
        }
        squish(&mut state.working);
    } else if let Some((ref match_re, ref replacement)) = rule.standardize {
        #[cfg(test)]
        let before = state.working.clone();
        replace_pattern(&mut state.working, match_re, replacement);
        squish(&mut state.working);
        #[cfg(test)]
        if before != state.working {
            eprintln!("[CHANGE {}] {:?} → {:?}", rule.label, before, state.working);
        }
    }
}
```

In `src/tables/rules.rs`, update the `rule` closure to initialize `standardize_pairs: Vec::new()` on every rule. Then add a separate call for `change_street_name_abbr` that builds the pairs from the table:

```rust
    // 11. STREET NAME STANDARDIZATION — table-driven
    {
        let sna_table = abbr.get("street_name_abbr");
        let template = r"\b({street_name_abbr$short})\b";
        let final_template = pattern_overrides
            .get("change_street_name_abbr")
            .cloned()
            .unwrap_or_else(|| template.to_string());
        let final_pattern = expand_template(&final_template, abbr);

        let pairs: Vec<(Regex, String)> = sna_table
            .map(|t| {
                t.short_to_long_pairs()
                    .into_iter()
                    .map(|(short, long)| {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        (re, long)
                    })
                    .collect()
            })
            .unwrap_or_default();

        rules.push(Rule {
            label: "change_street_name_abbr".to_string(),
            group: "street_name".to_string(),
            pattern: Regex::new(&final_pattern)
                .unwrap_or_else(|e| panic!("Bad regex in rule change_street_name_abbr: {}", e)),
            pattern_template: final_template,
            action: Action::Change,
            target: None,
            standardize: None,
            standardize_pairs: pairs,
            skip_if_filled: false,
            enabled: true,
        });
    }
```

Remove the old `change_name_mt_to_mount` and `change_name_ft_to_fort` rules.

**Step 4: Run all tests**

Run: `cargo test`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/pipeline.rs src/tables/rules.rs tests/config.rs
git commit -m "feat: merge MT/FT rules into table-driven change_street_name_abbr"
```

---

### Task 7: Update TUI `validate_pattern_template` and dict rendering for new tables

**Files:**
- Modify: `src/tui.rs:1088-1122` (`validate_pattern_template`)
- Modify: `src/tui.rs:1042-1070` (dict entry rendering for value-list tables)

**Step 1: Update `validate_pattern_template` to use `expand_template`**

Replace the hardcoded table list and manual expansion in `validate_pattern_template` with a call to `expand_template`:

```rust
fn validate_pattern_template(template: &str) -> Result<(), String> {
    let tables = build_default_tables();
    let expanded = crate::tables::rules::expand_template(template, &tables);
    fancy_regex::Regex::new(&expanded)
        .map(|_| ())
        .map_err(|e| format!("{}", e))
}
```

This requires making `expand_template` public in `src/tables/rules.rs`:

```rust
pub fn expand_template(template: &str, abbr: &Abbreviations) -> String {
```

**Step 2: Update dict rendering for value-list tables**

In `render_dict`, when building `ListItem`s, check if the current table is a value-list and hide the long column:

```rust
let is_value_list = {
    let tables = build_default_tables();
    tables
        .get(&app.table_names[app.dict_tab_index])
        .map(|t| t.is_value_list())
        .unwrap_or(false)
};

// Then in the map closure:
if is_value_list {
    ListItem::new(Line::from(vec![
        Span::styled(marker, style),
        Span::styled(e.short.clone(), style),
        Span::styled(detail, Style::new().fg(Color::DarkGray)),
    ]))
} else {
    ListItem::new(Line::from(vec![
        Span::styled(marker, style),
        Span::styled(format!("{:20}", e.short), style),
        Span::styled(" -> ", Style::new().fg(Color::DarkGray)),
        Span::styled(e.long.clone(), style),
        Span::styled(detail, Style::new().fg(Color::DarkGray)),
    ]))
}
```

**Step 3: Update dict add flow for value-list tables**

In `handle_input_mode`, for `AddShort` → when the current table is a value-list, skip the `AddLong` step and go straight to adding the entry with an empty long:

In `handle_input_mode`, `InputMode::AddShort` arm:

```rust
InputMode::AddShort(short) => match code {
    KeyCode::Enter => {
        if !short.is_empty() {
            let s = short.to_uppercase();
            let is_vl = {
                let tables = build_default_tables();
                tables
                    .get(&app.table_names[app.dict_tab_index])
                    .map(|t| t.is_value_list())
                    .unwrap_or(false)
            };
            if is_vl {
                // Value-list: no long form needed
                let new_entry = DictEntryState {
                    short: s,
                    long: String::new(),
                    status: EntryStatus::Added,
                    original_long: None,
                };
                app.current_dict_entries_mut().push(new_entry);
                let len = app.current_dict_entries().len();
                app.dict_list_state.select(Some(len - 1));
                app.dirty = true;
                app.input_mode = InputMode::Normal;
            } else {
                app.input_mode = InputMode::AddLong(s, String::new());
            }
        }
    }
    // ... rest unchanged
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add src/tui.rs src/tables/rules.rs
git commit -m "feat: update TUI for new tables, value-list display, and expand_template validation"
```

---

### Task 8: Final integration test and cleanup

**Files:**
- Modify: `tests/config.rs` (add integration test)
- Run: `cargo test` (full suite)

**Step 1: Write integration test for full pipeline with new tables**

Add to `tests/config.rs`:

```rust
#[test]
fn test_full_pipeline_with_tables_cleanup() {
    // Default pipeline: NA values from table
    let p = Pipeline::default();
    let addr = p.parse("NULL");
    assert!(addr.warnings.contains(&"change_na_address".to_string()));

    let addr = p.parse("UNKNOWN");
    assert!(addr.warnings.contains(&"change_na_address".to_string()));

    // Street name abbreviations from table
    let addr = p.parse("123 MT PLEASANT AVE");
    assert_eq!(addr.street_name.as_deref(), Some("MOUNT PLEASANT"));

    let addr = p.parse("456 FT HAMILTON PKWY");
    assert_eq!(addr.street_name.as_deref(), Some("FORT HAMILTON"));

    // ST → SAINT still works (hardcoded rule)
    let addr = p.parse("789 ST MARKS PL");
    assert_eq!(addr.street_name.as_deref(), Some("SAINT MARKS"));
}
```

**Step 2: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

**Step 3: Commit**

```bash
git add tests/config.rs
git commit -m "test: add integration tests for tables cleanup"
```
