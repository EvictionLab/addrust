# Config Override Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the merge-based config override system with full-replacement semantics, eliminating silent data loss.

**Architecture:** Config entries are complete rows. `patch()` replaces defaults by short-form match (no merge). TUI computes status by diffing against the default baseline. `canonical` field is removed.

**Tech Stack:** Rust, serde, toml, ratatui

---

### Task 1: Rewrite `AbbrTable::patch()` with replace semantics

**Files:**
- Modify: `src/tables/abbreviations.rs:222-307` (the `patch` method)

- [ ] **Step 1: Write failing tests for new patch semantics**

Replace the existing patch tests (`test_patch_add_variant_to_existing_group`, `test_patch_canonical_override_demotes_old`, `test_patch_add_new_group`, `test_patch_remove_group`) with tests for the new behavior. In `src/tables/abbreviations.rs`, replace lines 639-713 with:

```rust
    #[test]
    fn test_patch_replace_existing_group() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("NE", "NORTHEAST", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NE".into(), long: "NORTHEAST".into(),
                variants: vec!["N E".into(), "NEAST".into()],
                tags: vec!["direction".into()],
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        // Replacement: config entry IS the group
        assert_eq!(patched.standardize("N E"), Some((0, "NE", "NORTHEAST")));
        assert_eq!(patched.standardize("NEAST"), Some((0, "NE", "NORTHEAST")));
        // Tags came from config entry
        assert_eq!(patched.groups[0].tags, vec!["direction"]);
    }

    #[test]
    fn test_patch_replace_drops_old_variants() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup {
                short: "NE".into(), long: "NORTHEAST".into(),
                variants: vec!["OLD_VARIANT".into()],
                tags: vec!["old_tag".into()],
            },
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NE".into(), long: "NORTHEAST".into(),
                variants: vec!["NEW_VARIANT".into()],
                tags: vec!["new_tag".into()],
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        // Full replacement: old variant gone, new variant present
        assert_eq!(patched.standardize("OLD_VARIANT"), None);
        assert_eq!(patched.standardize("NEW_VARIANT"), Some((0, "NE", "NORTHEAST")));
        assert_eq!(patched.groups[0].tags, vec!["new_tag"]);
    }

    #[test]
    fn test_patch_add_new_group() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "WH".into(), long: "WAREHOUSE".into(),
                variants: vec!["WHSE".into()],
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("WHSE"), Some((0, "WH", "WAREHOUSE")));
    }

    #[test]
    fn test_patch_same_long_form_separate_groups() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("HWY", "HIGHWAY", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "GA HWY".into(), long: "HIGHWAY".into(),
                variants: vec![],
                tags: vec!["highway".into()],
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        // Two separate groups, both matchable
        assert!(patched.standardize("HWY").is_some());
        assert!(patched.standardize("GA HWY").is_some());
        assert_eq!(patched.groups.len(), 2);
    }

    #[test]
    fn test_patch_remove_group() {
        use crate::config::{DictEntry, DictOverrides};
        let _ = DictEntry::default();
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("NE", "NORTHEAST", vec!["N E".into()]),
            AbbrGroup::new("NW", "NORTHWEST", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec!["N E".into()],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("NE"), None);
        assert_eq!(patched.standardize("NW"), Some((0, "NW", "NORTHWEST")));
    }

    #[test]
    fn test_patch_remove_then_add() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("NE", "NORTHEAST", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NE".into(), long: "NORTHEAST".into(),
                variants: vec!["NEAST".into()],
                tags: vec!["custom".into()],
                ..Default::default()
            }],
            remove: vec!["NE".into()],
        };
        let patched = table.patch(&overrides);
        // Remove runs first, then add replaces — net effect is replacement
        assert_eq!(patched.standardize("NE"), Some((0, "NE", "NORTHEAST")));
        assert_eq!(patched.groups[0].tags, vec!["custom"]);
    }

    #[test]
    fn test_patch_tags_preserved_exactly() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup {
                short: "MT".into(), long: "MOUNT".into(),
                variants: vec![], tags: vec!["start".into()],
            },
        ]);
        // Override with no tags — full replacement means tags are gone
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "MT".into(), long: "MOUNT".into(),
                variants: vec![],
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        assert!(patched.groups[0].tags.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_patch_ -- --nocapture 2>&1 | tail -20`
Expected: Multiple failures — old `patch()` uses merge logic, not replacement.

- [ ] **Step 3: Rewrite `patch()` with replace semantics**

In `src/tables/abbreviations.rs`, replace the `patch` method (lines 222-307) with:

```rust
    /// Apply dictionary overrides: remove matching groups, then replace or add.
    ///
    /// Config entries are complete rows — a matching short form fully replaces
    /// the default group (no merge). Non-matching entries are appended as new groups.
    pub fn patch(&self, overrides: &crate::config::DictOverrides) -> Self {
        let mut groups = self.groups.clone();

        // Remove phase: remove groups where any value matches (case-insensitive)
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

        // Add/replace phase
        for entry in &overrides.add {
            let short = entry.short.to_uppercase();
            let long = entry.long.to_uppercase();

            // Replace existing group with matching short form
            if let Some(idx) = groups.iter().position(|g| g.short == short) {
                groups[idx] = AbbrGroup {
                    short,
                    long,
                    variants: entry.variants.clone(),
                    tags: entry.tags.clone(),
                };
            } else {
                // New group
                groups.push(AbbrGroup {
                    short,
                    long,
                    variants: entry.variants.clone(),
                    tags: entry.tags.clone(),
                });
            }
        }

        Self::from_groups(groups)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_patch_ -- --nocapture 2>&1 | tail -20`
Expected: All patch tests pass.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED"`
Expected: All pass. Some tests in other modules may fail if they depend on old `canonical` field — we'll fix those in the next task.

- [ ] **Step 6: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "refactor: rewrite patch() with full-replacement semantics

Config entries fully replace default groups by short-form match.
No merge logic, no long-form matching, no canonical branching."
```

---

### Task 2: Remove `canonical` field from `DictEntry`

**Files:**
- Modify: `src/config.rs:178-188` (DictEntry struct)
- Modify: `src/config.rs:224-344` (tests)

- [ ] **Step 1: Remove `canonical` from `DictEntry`**

In `src/config.rs`, change the `DictEntry` struct to:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct DictEntry {
    pub short: String,
    pub long: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}
```

- [ ] **Step 2: Fix compilation errors**

Search for all references to `canonical` in the codebase and remove them:

- `src/tables/abbreviations.rs` — the `add_iter` line that reads `e.canonical`: already removed in Task 1.
- `src/config.rs` test `test_roundtrip_full_config` line 256: remove `canonical: Some(true)` from the DictEntry construction.
- `src/tui/mod.rs` `to_config()` lines 379 and 391: remove `canonical: None` and `canonical: Some(true)` from DictEntry construction.

Run: `cargo build 2>&1 | tail -10`
Expected: Clean build, no errors.

- [ ] **Step 3: Verify backward compatibility with existing configs**

Add a test to `src/config.rs`:

```rust
    #[test]
    fn test_canonical_field_ignored_on_load() {
        let toml_str = r#"
[[dictionaries.suffix.add]]
short = "AVE"
long = "AVENUE"
canonical = true
tags = ["common"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let entry = &config.dictionaries.get("suffix").unwrap().add[0];
        assert_eq!(entry.short, "AVE");
        assert_eq!(entry.tags, vec!["common"]);
    }
```

Run: `cargo test test_canonical_field 2>&1 | tail -5`
Expected: PASS. The `canonical` field in existing TOML is silently ignored by serde (no `deny_unknown_fields`).

- [ ] **Step 4: Run full test suite**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED"`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/tui/mod.rs
git commit -m "refactor: remove canonical field from DictEntry

System infers replacement vs addition by short-form matching
against defaults. Existing configs with canonical = true still
parse — the field is silently ignored."
```

---

### Task 3: Rewrite TUI dict loading

**Files:**
- Modify: `src/tui/mod.rs:165-250` (dict loading in `App::new()`)

- [ ] **Step 1: Rewrite dict loading to use `patch()` then compute status**

Replace lines 165-250 in `src/tui/mod.rs` (the `dict_entries` construction) with:

```rust
        let dict_entries: Vec<Vec<DictGroupState>> = table_names
            .iter()
            .map(|name| {
                let default_table = default_tables.get(name).unwrap();
                let overrides = config.dictionaries.get(name);

                // Build default lookup: short -> (long, variants, tags)
                let default_map: std::collections::HashMap<&str, &crate::tables::abbreviations::AbbrGroup> =
                    default_table.groups.iter()
                        .map(|g| (g.short.as_str(), g))
                        .collect();

                // Patch defaults with config overrides
                let patched = match overrides {
                    Some(ov) => default_table.patch(ov),
                    None => default_table.clone(),
                };

                // Build entries with status from patched table
                let mut entries: Vec<DictGroupState> = patched.groups.iter()
                    .map(|g| {
                        let (status, orig_short, orig_long, orig_variants, orig_tags) =
                            if let Some(default_group) = default_map.get(g.short.as_str()) {
                                // Exists in defaults — check if identical
                                let is_same = g.long == default_group.long
                                    && g.variants == default_group.variants
                                    && g.tags == default_group.tags;
                                if is_same {
                                    (GroupStatus::Default,
                                     default_group.short.clone(), default_group.long.clone(),
                                     default_group.variants.clone(), default_group.tags.clone())
                                } else {
                                    (GroupStatus::Modified,
                                     default_group.short.clone(), default_group.long.clone(),
                                     default_group.variants.clone(), default_group.tags.clone())
                                }
                            } else {
                                // Not in defaults — added by config
                                (GroupStatus::Added,
                                 g.short.clone(), g.long.clone(),
                                 g.variants.clone(), g.tags.clone())
                            };

                        DictGroupState {
                            short: g.short.clone(),
                            long: g.long.clone(),
                            variants: g.variants.clone(),
                            tags: g.tags.clone(),
                            status,
                            original_short: orig_short,
                            original_long: orig_long,
                            original_variants: orig_variants,
                            original_tags: orig_tags,
                        }
                    })
                    .collect();

                // Add removed entries for display (from config remove list vs defaults)
                if let Some(ov) = overrides {
                    for remove_val in &ov.remove {
                        let upper = remove_val.to_uppercase();
                        // Find the default group that was removed
                        if let Some(default_group) = default_table.groups.iter().find(|g| {
                            g.short == upper || g.long == upper
                                || g.variants.iter().any(|v| v.to_uppercase() == upper)
                        }) {
                            entries.push(DictGroupState {
                                short: default_group.short.clone(),
                                long: default_group.long.clone(),
                                variants: default_group.variants.clone(),
                                tags: default_group.tags.clone(),
                                status: GroupStatus::Removed,
                                original_short: default_group.short.clone(),
                                original_long: default_group.long.clone(),
                                original_variants: default_group.variants.clone(),
                                original_tags: default_group.tags.clone(),
                            });
                        }
                    }
                }

                entries
            })
            .collect();
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED"`
Expected: All pass. The TUI loads the same data through a simpler path.

- [ ] **Step 3: Commit**

```bash
git add src/tui/mod.rs
git commit -m "refactor: rewrite TUI dict loading via patch()

Single code path: call patch() then compute status by comparing
each group against the default baseline. No separate merge logic."
```

---

### Task 4: Rewrite TUI `to_config()` dict serialization

**Files:**
- Modify: `src/tui/mod.rs:367-402` (`to_config()` dict section)

- [ ] **Step 1: Simplify `to_config()` dict serialization**

Replace lines 367-402 in `src/tui/mod.rs` with:

```rust
        // Dictionaries: collect changes per table
        for (i, name) in self.table_names.iter().enumerate() {
            let entries = &self.dict_entries[i];
            let mut overrides = DictOverrides::default();

            for entry in entries {
                match entry.status {
                    GroupStatus::Added | GroupStatus::Modified => {
                        overrides.add.push(DictEntry {
                            short: entry.short.clone(),
                            long: entry.long.clone(),
                            variants: entry.variants.clone(),
                            tags: entry.tags.clone(),
                        });
                    }
                    GroupStatus::Removed => {
                        overrides.remove.push(entry.short.clone());
                    }
                    GroupStatus::Default => {}
                }
            }

            if !overrides.add.is_empty() || !overrides.remove.is_empty() {
                config.dictionaries.insert(name.clone(), overrides);
            }
        }
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED"`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/tui/mod.rs
git commit -m "refactor: simplify to_config() dict serialization

Added and Modified entries both write full rows. No canonical field."
```

---

### Task 5: Add duplicate validation in TUI panel

**Files:**
- Modify: `src/tui/panel.rs:1096-1155` (`close_dict_panel`)

- [ ] **Step 1: Add duplicate check before saving**

In `src/tui/panel.rs`, add a duplicate validation function above `close_dict_panel`:

```rust
/// Check if short or long form collides with another entry in the table.
/// Returns an error message if there's a collision, None if OK.
/// `exclude_index` is the index of the entry being edited (excluded from check).
fn check_dict_duplicates(
    entries: &[super::tabs::DictGroupState],
    short: &str,
    long: &str,
    exclude_index: Option<usize>,
) -> Option<String> {
    for (i, e) in entries.iter().enumerate() {
        if Some(i) == exclude_index {
            continue;
        }
        if e.status == super::tabs::GroupStatus::Removed {
            continue;
        }
        if !short.is_empty() && e.short == short {
            return Some(format!("Short form '{}' already exists in this table", short));
        }
        if !long.is_empty() && e.long == long {
            return Some(format!("Long form '{}' already exists in this table", long));
        }
    }
    None
}
```

- [ ] **Step 2: Wire duplicate check into `close_dict_panel`**

In `close_dict_panel`, after extracting fields from the panel (after line 1116 `_ => return`), add the duplicate check before proceeding:

```rust
    // Check for duplicate short/long forms
    let exclude = if is_new { None } else { Some(entry_index) };
    let short_upper = short.to_uppercase();
    let long_upper = long.to_uppercase();
    if let Some(_err_msg) = check_dict_duplicates(
        app.current_dict_entries(),
        &short_upper,
        &long_upper,
        exclude,
    ) {
        // TODO: display error via status bar (issue #2)
        // For now, just don't save
        app.panel = None;
        return;
    }
```

- [ ] **Step 3: Run full test suite and clippy**

Run: `cargo clippy 2>&1 | tail -5 && cargo test 2>&1 | grep -E "^test result|FAILED"`
Expected: Zero warnings, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: reject duplicate short/long forms in dict panel

Validates on panel close that the short and long forms don't
collide with other entries in the same table."
```

---

### Task 6: Final verification

**Files:**
- No changes — verification only.

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass across all test targets.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: Zero warnings.

- [ ] **Step 3: Build release and test with real config**

```bash
cargo build --release
cd /Users/sj2690/Projects/data-requests
/Users/sj2690/Projects/addrust/target/release/addrust parse \
  --duckdb ets.duckdb \
  --input-table fulton_2018 \
  --output-table fulton_2018_parsed_2 \
  --column address \
  --overwrite \
  --config atlanta-2018-2.toml
```

Expected: Parses 43,667 addresses without hanging. Check key addresses:

```sql
-- HWY should resolve
SELECT address, street_name FROM fulton_2018_parsed_2
WHERE address LIKE '5252 HWY 138%';
-- Expected: street_name = 'HIGHWAY ONEHUNDREDTHIRTYEIGHT'

-- MT should resolve (if street_name step matches)
SELECT address, street_name FROM fulton_2018_parsed_2
WHERE address ILIKE '%MT ZION%' LIMIT 3;

-- ABERNATHY variant should resolve
SELECT address, street_name FROM fulton_2018_parsed_2
WHERE address ILIKE '%ABERNETHY%';
-- Expected: street_name contains 'RALPH DAVID ABERNATHY'
```

- [ ] **Step 4: Commit all remaining changes**

```bash
git add -A
git commit -m "refactor: complete config override redesign

Full-replacement semantics for dictionary overrides. Config entries
are complete rows — no merge logic, no canonical field, no silent
data loss. Single patch() function used by both pipeline and TUI."
```
