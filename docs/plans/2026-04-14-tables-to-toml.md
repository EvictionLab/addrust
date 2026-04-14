# Tables to TOML Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move abbreviation table data from hardcoded Rust `build_*()` functions into TOML files, collapse `suffix_all`/`suffix_common` into a single tagged table, and replace all builder functions with one general TOML loader.

**Architecture:** Two TOML data files (`tables.toml` for 6 hand-authored tables, `suffixes.toml` for generated suffix data with tags). One general loader replaces all `build_*()` functions. A `data-raw/` script generates `suffixes.toml` from the USPS CSV. The TUI shows one suffix table with editable tags instead of two separate tables.

**Tech Stack:** Rust, serde + toml 0.8 (already in Cargo.toml), fancy-regex 0.14

**Spec:** `docs/specs/2026-04-14-tables-to-toml-design.md`

---

## File Structure

**Create:**
- `data/defaults/tables.toml` — hand-authored TOML for 6 simple tables
- `data/defaults/suffixes.toml` — generated TOML for suffix table with tags
- `data-raw/usps-street-suffix.csv` — moved from `data/`
- `src/bin/generate_suffixes.rs` — binary to generate suffixes.toml from CSV

**Modify:**
- `src/tables/abbreviations.rs` — remove `build_*()` functions, add TOML loader
- `src/tui/meta.rs` — update TABLE_DESCRIPTIONS (collapse suffix entries, add suffix description)
- `src/tui/tabs.rs` — add tags field to DictGroupState, display tags column for suffix, tag editing keys
- `src/tui/mod.rs` — update dict initialization to handle tags from suffix table

**Delete:**
- `data/usps-street-suffix.csv` — moved to `data-raw/`

---

### Task 1: Create `tables.toml` with 6 hand-authored tables

Hand-author the TOML file containing direction, unit_type, unit_location, state, na_values, and street_name_abbr. Data is copied from the existing `build_*()` functions.

**Files:**
- Create: `data/defaults/tables.toml`

- [ ] **Step 1: Write `tables.toml`**

The file contains all 6 non-suffix, non-number tables. Use inline table format with `groups = [...]` under each table name. All data comes from the existing `build_*()` functions in `src/tables/abbreviations.rs:342-517`. Every `short` and `long` value is UPPERCASE. Missing `long` defaults to `""`, missing `variants` defaults to `[]`.

```toml
[direction]
groups = [
    { short = "NE", long = "NORTHEAST" },
    { short = "NW", long = "NORTHWEST" },
    { short = "SE", long = "SOUTHEAST" },
    { short = "SW", long = "SOUTHWEST" },
    { short = "N",  long = "NORTH" },
    { short = "S",  long = "SOUTH" },
    { short = "E",  long = "EAST" },
    { short = "W",  long = "WEST" },
]

[unit_type]
groups = [
    { short = "APT", long = "APARTMENT" },
    { short = "UNIT", long = "UNIT" },
    { short = "STE", long = "SUITE" },
    { short = "FL", long = "FLOOR" },
    { short = "FLT", long = "FLAT" },
    { short = "BLDG", long = "BUILDING" },
    { short = "RM", long = "ROOM" },
    { short = "PH", long = "PENTHOUSE" },
    { short = "TOWNHOUSE", long = "TOWNHOUSE" },
    { short = "DEPT", long = "DEPARTMENT" },
    { short = "DUPLEX", long = "DUPLEX" },
    { short = "ATTIC", long = "ATTIC" },
    { short = "BSMT", long = "BASEMENT" },
    { short = "LOT", long = "LOT" },
    { short = "LVL", long = "LEVEL" },
    { short = "OFC", long = "OFFICE" },
    { short = "NUM", long = "NUMBER", variants = ["NO"] },
    { short = "HSE", long = "HOUSE" },
    { short = "GARAGE", long = "GARAGE" },
    { short = "CONDO", long = "CONDO" },
    { short = "TRLR", long = "TRAILER" },
    { short = "#", long = "#" },
]

[unit_location]
groups = [
    { short = "UPPR", long = "UPPER", variants = ["UP"] },
    { short = "LOWR", long = "LOWER", variants = ["LWR", "LW"] },
    { short = "FRNT", long = "FRONT", variants = ["FRT"] },
    { short = "REAR", long = "REAR" },
    { short = "BACK", long = "BACK" },
    { short = "MID", long = "MIDDLE" },
    { short = "ENTIRE", long = "ENTIRE" },
    { short = "WHOLE", long = "WHOLE" },
    { short = "SINGLE", long = "SINGLE" },
    { short = "DOWN", long = "DOWN" },
    { short = "RIGHT", long = "RIGHT" },
    { short = "LEFT", long = "LEFT" },
    { short = "DOWNSTAIRS", long = "DOWNSTAIRS" },
    { short = "UPSTAIRS", long = "UPSTAIRS" },
    { short = "SIDE", long = "SIDE" },
]

[state]
groups = [
    { short = "AL", long = "ALABAMA" },
    { short = "AK", long = "ALASKA" },
    { short = "AZ", long = "ARIZONA" },
    { short = "AR", long = "ARKANSAS" },
    { short = "CA", long = "CALIFORNIA" },
    { short = "CO", long = "COLORADO" },
    { short = "CT", long = "CONNECTICUT" },
    { short = "DE", long = "DELAWARE" },
    { short = "FL", long = "FLORIDA" },
    { short = "GA", long = "GEORGIA" },
    { short = "HI", long = "HAWAII" },
    { short = "ID", long = "IDAHO" },
    { short = "IL", long = "ILLINOIS" },
    { short = "IN", long = "INDIANA" },
    { short = "IA", long = "IOWA" },
    { short = "KS", long = "KANSAS" },
    { short = "KY", long = "KENTUCKY" },
    { short = "LA", long = "LOUISIANA" },
    { short = "ME", long = "MAINE" },
    { short = "MD", long = "MARYLAND" },
    { short = "MA", long = "MASSACHUSETTS" },
    { short = "MI", long = "MICHIGAN" },
    { short = "MN", long = "MINNESOTA" },
    { short = "MS", long = "MISSISSIPPI" },
    { short = "MO", long = "MISSOURI" },
    { short = "MT", long = "MONTANA" },
    { short = "NE", long = "NEBRASKA" },
    { short = "NV", long = "NEVADA" },
    { short = "NH", long = "NEW HAMPSHIRE" },
    { short = "NJ", long = "NEW JERSEY" },
    { short = "NM", long = "NEW MEXICO" },
    { short = "NY", long = "NEW YORK" },
    { short = "NC", long = "NORTH CAROLINA" },
    { short = "ND", long = "NORTH DAKOTA" },
    { short = "OH", long = "OHIO" },
    { short = "OK", long = "OKLAHOMA" },
    { short = "OR", long = "OREGON" },
    { short = "PA", long = "PENNSYLVANIA" },
    { short = "RI", long = "RHODE ISLAND" },
    { short = "SC", long = "SOUTH CAROLINA" },
    { short = "SD", long = "SOUTH DAKOTA" },
    { short = "TN", long = "TENNESSEE" },
    { short = "TX", long = "TEXAS" },
    { short = "UT", long = "UTAH" },
    { short = "VT", long = "VERMONT" },
    { short = "VA", long = "VIRGINIA" },
    { short = "WA", long = "WASHINGTON" },
    { short = "WV", long = "WEST VIRGINIA" },
    { short = "WI", long = "WISCONSIN" },
    { short = "WY", long = "WYOMING" },
    { short = "DC", long = "DISTRICT OF COLUMBIA" },
]

[na_values]
groups = [
    { short = "NULL" },
    { short = "NAN" },
    { short = "MISSING" },
    { short = "NONE" },
    { short = "UNKNOWN" },
    { short = "NO ADDRESS" },
]

[street_name_abbr]
groups = [
    { short = "MT", long = "MOUNT" },
    { short = "FT", long = "FORT" },
]
```

- [ ] **Step 2: Commit**

```bash
git add data/defaults/tables.toml
git commit -m "data: add tables.toml with 6 hand-authored abbreviation tables"
```

---

### Task 2: Create the `data-raw/` script and generate `suffixes.toml`

Build the binary that processes the USPS CSV into `suffixes.toml`. Move the CSV to `data-raw/`. The script ports the logic from `build_all_suffixes()` (`src/tables/abbreviations.rs:423-498`) plus adds tags for common suffixes.

**Files:**
- Create: `src/bin/generate_suffixes.rs`
- Create: `data-raw/` directory (move `data/usps-street-suffix.csv` here)
- Create: `data/defaults/suffixes.toml` (generated output)

- [ ] **Step 1: Move the CSV**

```bash
mkdir -p data-raw
git mv data/usps-street-suffix.csv data-raw/usps-street-suffix.csv
```

- [ ] **Step 2: Add `[[bin]]` target to `Cargo.toml`**

Add after `[dependencies]`:

```toml
[[bin]]
name = "generate-suffixes"
path = "src/bin/generate_suffixes.rs"
```

- [ ] **Step 3: Write the generation script**

Create `src/bin/generate_suffixes.rs`. This ports the processing logic from `build_all_suffixes()` (abbreviations.rs:423-498) into a standalone binary that writes TOML.

The script must:
1. Read `data-raw/usps-street-suffix.csv`
2. Group variants by USPS abbreviation (column 3)
3. Exclude entries where USPS code is TRAILER or HIGHWAY
4. Handle plural forms: when primary is PARKS/WALKS/SPURS/LOOPS and USPS is PARK/WALK/SPUR/LOOP, use `{USPS}S` as canonical short
5. Add manual variant overrides (same list as abbreviations.rs:421-435):
   - BLVD: BVD, BV, BLV, BL
   - CIR: CI
   - CT: CRT
   - EXPY: EX, EXPWY
   - IS: ISLD
   - LN: LA
   - PKWY: PY, PARK WAY, PKW
   - TER: TE
   - TRCE: TR
   - PARK: PK
   - PL: PLC
   - AVE: AE
   - DR: DIRVE
6. Consolidate obvious literal variant families into regex where possible
7. Mark these canonical shorts as common (tags = ["common"]): DR, LN, AVE, RD, ST, CIR, CT, PL, WAY, BLVD, STRA, CV, LOOP (same as current `build_common_suffixes()`, abbreviations.rs:519-535)
8. Also mark the plural form suffix_common entries as common if present: none of the current 13 common suffixes have plural forms, so this is just future-proofing
9. Write `data/defaults/suffixes.toml` in the inline-table format

```rust
use std::collections::HashMap;
use std::fs;

fn main() {
    let csv_path = "data-raw/usps-street-suffix.csv";
    let output_path = "data/defaults/suffixes.toml";

    let csv_data = fs::read_to_string(csv_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", csv_path, e));

    let common_shorts: Vec<&str> = vec![
        "DR", "LN", "AVE", "RD", "ST", "CIR", "CT", "PL",
        "WAY", "BLVD", "STRA", "CV", "LOOP",
    ];

    struct Group {
        short: String,
        long: String,
        variants: Vec<String>,
    }

    let mut groups: Vec<Group> = Vec::new();
    let mut usps_to_idx: HashMap<String, usize> = HashMap::new();

    for line in csv_data.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 3 { continue; }
        let primary = cols[0].trim().to_uppercase();
        let variant = cols[1].trim().to_uppercase();
        let usps = cols[2].trim().to_uppercase();

        if usps == "TRAILER" || usps == "HIGHWAY" { continue; }

        let canonical_short = if ["PARK", "WALK", "SPUR", "LOOP"].contains(&usps.as_str())
            && ["PARKS", "WALKS", "SPURS", "LOOPS"].contains(&primary.as_str())
        {
            format!("{}S", usps)
        } else {
            usps.clone()
        };

        if let Some(&idx) = usps_to_idx.get(&canonical_short) {
            let group = &mut groups[idx];
            if variant != group.short && variant != group.long
                && !group.variants.contains(&variant)
            {
                group.variants.push(variant.clone());
            }
            if primary != group.short && primary != group.long
                && !group.variants.contains(&primary)
            {
                group.variants.push(primary);
            }
        } else {
            let idx = groups.len();
            let mut variants = vec![];
            if variant != canonical_short && variant != primary {
                variants.push(variant);
            }
            groups.push(Group {
                short: canonical_short.clone(),
                long: primary,
                variants,
            });
            usps_to_idx.insert(canonical_short, idx);
        }
    }

    // Add manual overrides
    let manual_variants: &[(&str, &[&str])] = &[
        ("BLVD", &["BVD", "BV", "BLV", "BL"]),
        ("CIR", &["CI"]),
        ("CT", &["CRT"]),
        ("EXPY", &["EX", "EXPWY"]),
        ("IS", &["ISLD"]),
        ("LN", &["LA"]),
        ("PKWY", &["PY", "PARK WAY", "PKW"]),
        ("TER", &["TE"]),
        ("TRCE", &["TR"]),
        ("PARK", &["PK"]),
        ("PL", &["PLC"]),
        ("AVE", &["AE"]),
        ("DR", &["DIRVE"]),
    ];
    for (usps_short, extras) in manual_variants {
        if let Some(&idx) = usps_to_idx.get(*usps_short) {
            for extra in *extras {
                let e = extra.to_uppercase();
                let group = &mut groups[idx];
                if e != group.short && e != group.long && !group.variants.contains(&e) {
                    group.variants.push(e);
                }
            }
        }
    }

    // Write TOML
    let mut out = String::from("[suffix]\ngroups = [\n");
    for group in &groups {
        let is_common = common_shorts.contains(&group.short.as_str());
        out.push_str("    { short = \"");
        out.push_str(&group.short);
        out.push_str("\", long = \"");
        out.push_str(&group.long);
        out.push('"');
        if !group.variants.is_empty() {
            out.push_str(", variants = [");
            for (i, v) in group.variants.iter().enumerate() {
                if i > 0 { out.push_str(", "); }
                out.push('"');
                out.push_str(v);
                out.push('"');
            }
            out.push(']');
        }
        if is_common {
            out.push_str(", tags = [\"common\"]");
        }
        out.push_str(" },\n");
    }
    out.push_str("]\n");

    fs::write(output_path, &out)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", output_path, e));

    println!("Generated {} with {} suffix groups", output_path, groups.len());
}
```

- [ ] **Step 4: Run the script and verify output**

```bash
cargo run --bin generate-suffixes
```

Expected: prints "Generated data/defaults/suffixes.toml with ~150 suffix groups" and creates the file. Inspect the output to verify:
- Common suffixes (DR, AVE, ST, etc.) have `tags = ["common"]`
- TRAILER and HIGHWAY are excluded
- Plural forms (PARKS, WALKS, etc.) are present
- Manual variants (BVD, BV, etc.) are included

- [ ] **Step 5: Commit**

```bash
git add data-raw/usps-street-suffix.csv src/bin/generate_suffixes.rs Cargo.toml data/defaults/suffixes.toml
git rm data/usps-street-suffix.csv
git commit -m "feat: add data-raw script to generate suffixes.toml from USPS CSV"
```

---

### Task 3: Add TOML deserialization structs and loader functions

Add serde-compatible structs and two loader functions that parse `tables.toml` and `suffixes.toml` into `HashMap<String, AbbrTable>`. This task does NOT remove the old `build_*()` functions yet — both paths will exist temporarily so we can test equivalence.

**Files:**
- Modify: `src/tables/abbreviations.rs`

- [ ] **Step 1: Write the failing test for tables.toml loading**

Add at the bottom of the `#[cfg(test)] mod tests` block in `src/tables/abbreviations.rs` (before the closing `}`):

```rust
    #[test]
    fn test_load_tables_from_toml() {
        let toml_str = r#"
[direction]
groups = [
    { short = "N", long = "NORTH" },
    { short = "S", long = "SOUTH" },
]

[na_values]
groups = [
    { short = "NULL" },
    { short = "NAN" },
]
"#;
        let tables = load_tables_from_toml(toml_str);
        assert_eq!(tables.len(), 2);

        let dir = tables.get("direction").unwrap();
        assert_eq!(dir.to_long("N"), Some("NORTH"));
        assert_eq!(dir.to_long("S"), Some("SOUTH"));

        let na = tables.get("na_values").unwrap();
        assert!(na.is_value_list());
        assert!(na.all_values().contains(&"NULL"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_load_tables_from_toml -- --nocapture
```

Expected: FAIL — `load_tables_from_toml` is not defined.

- [ ] **Step 3: Write the deserialization structs and `load_tables_from_toml`**

Add these above `build_default_tables()` in `src/tables/abbreviations.rs` (around line 535). Add `use serde::Deserialize;` at the top of the file (after the existing `use` statements).

```rust
#[derive(Deserialize)]
struct GroupDef {
    short: String,
    #[serde(default)]
    long: String,
    #[serde(default)]
    variants: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct TableDef {
    groups: Vec<GroupDef>,
}

/// Load abbreviation tables from a TOML string (tables.toml format).
/// Each top-level key becomes a table name.
pub fn load_tables_from_toml(toml_str: &str) -> HashMap<String, AbbrTable> {
    let raw: HashMap<String, TableDef> = toml::from_str(toml_str)
        .expect("Failed to parse tables TOML");
    raw.into_iter()
        .map(|(name, def)| {
            let groups = def.groups.into_iter()
                .map(|g| AbbrGroup {
                    short: g.short,
                    long: g.long,
                    variants: g.variants,
                })
                .collect();
            (name, AbbrTable::from_groups(groups))
        })
        .collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test test_load_tables_from_toml -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Write the failing test for suffixes.toml loading**

Add to the test module:

```rust
    #[test]
    fn test_load_suffixes_from_toml() {
        let toml_str = r#"
[suffix]
groups = [
    { short = "AVE", long = "AVENUE", variants = ["AV"], tags = ["common"] },
    { short = "STRA", long = "STRAVENUE" },
    { short = "BLVD", long = "BOULEVARD", tags = ["common"] },
]
"#;
        let tables = load_suffixes_from_toml(toml_str);

        // suffix_all has all 3
        let all = tables.get("suffix_all").unwrap();
        assert_eq!(all.groups.len(), 3);
        assert_eq!(all.to_long("AV"), Some("AVENUE"));
        assert_eq!(all.to_long("STRA"), Some("STRAVENUE"));

        // suffix_common has only tagged entries
        let common = tables.get("suffix_common").unwrap();
        assert_eq!(common.groups.len(), 2);
        assert_eq!(common.to_long("AVE"), Some("AVENUE"));
        assert_eq!(common.to_long("BLVD"), Some("BOULEVARD"));
        assert_eq!(common.standardize("STRA"), None);
    }
```

- [ ] **Step 6: Run test to verify it fails**

```bash
cargo test test_load_suffixes_from_toml -- --nocapture
```

Expected: FAIL — `load_suffixes_from_toml` is not defined.

- [ ] **Step 7: Write `load_suffixes_from_toml`**

Add below `load_tables_from_toml` in `src/tables/abbreviations.rs`:

```rust
#[derive(Deserialize)]
struct SuffixFileDef {
    suffix: TableDef,
}

/// Load suffix table from TOML, producing both suffix_all and suffix_common.
/// suffix_common is derived by filtering groups with tags containing "common".
pub fn load_suffixes_from_toml(toml_str: &str) -> HashMap<String, AbbrTable> {
    let raw: SuffixFileDef = toml::from_str(toml_str)
        .expect("Failed to parse suffixes TOML");

    let all_groups: Vec<(AbbrGroup, Vec<String>)> = raw.suffix.groups.into_iter()
        .map(|g| {
            let tags = g.tags;
            let group = AbbrGroup {
                short: g.short,
                long: g.long,
                variants: g.variants,
            };
            (group, tags)
        })
        .collect();

    let common_groups: Vec<AbbrGroup> = all_groups.iter()
        .filter(|(_, tags)| tags.contains(&"common".to_string()))
        .map(|(g, _)| g.clone())
        .collect();

    let all: Vec<AbbrGroup> = all_groups.into_iter()
        .map(|(g, _)| g)
        .collect();

    let mut tables = HashMap::new();
    tables.insert("suffix_all".to_string(), AbbrTable::from_groups(all));
    tables.insert("suffix_common".to_string(), AbbrTable::from_groups(common_groups));
    tables
}
```

- [ ] **Step 8: Run test to verify it passes**

```bash
cargo test test_load_suffixes_from_toml -- --nocapture
```

Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "feat: add TOML deserialization structs and loader functions"
```

---

### Task 4: Write migration equivalence test

Before switching over, verify that the TOML-loaded tables produce identical results to the current `build_*()` functions. This is the safety net.

**Files:**
- Modify: `src/tables/abbreviations.rs` (test module)

- [ ] **Step 1: Write the equivalence test**

Add to the test module in `src/tables/abbreviations.rs`:

```rust
    #[test]
    fn test_toml_tables_match_build_functions() {
        let old_tables = build_default_tables();

        let mut new_tables_map = load_tables_from_toml(
            include_str!("../../data/defaults/tables.toml")
        );
        new_tables_map.extend(load_suffixes_from_toml(
            include_str!("../../data/defaults/suffixes.toml")
        ));

        // Check each non-number table
        for name in &[
            "direction", "unit_type", "unit_location", "state",
            "na_values", "street_name_abbr", "suffix_all", "suffix_common",
        ] {
            let old = old_tables.get(name)
                .unwrap_or_else(|| panic!("Old tables missing: {}", name));
            let new = new_tables_map.get(*name)
                .unwrap_or_else(|| panic!("New tables missing: {}", name));

            // Same number of groups
            assert_eq!(
                old.groups.len(), new.groups.len(),
                "Group count mismatch for {}: old={}, new={}",
                name, old.groups.len(), new.groups.len()
            );

            // Same standardize results for every value in old table
            for val in old.all_match_values() {
                let old_result = old.standardize(val);
                let new_result = new.standardize(val);
                assert_eq!(
                    old_result.map(|(_, s, l)| (s, l)),
                    new_result.map(|(_, s, l)| (s, l)),
                    "Standardize mismatch for '{}' in table '{}': old={:?}, new={:?}",
                    val, name, old_result, new_result
                );
            }
        }
    }
```

- [ ] **Step 2: Run the test**

```bash
cargo test test_toml_tables_match_build_functions -- --nocapture
```

Expected: PASS — the TOML data produces identical lookup results. If it fails, fix the TOML data to match (likely a missing variant or capitalization issue).

- [ ] **Step 3: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "test: add migration equivalence test for TOML-loaded tables"
```

---

### Task 5: Switch `build_default_tables` to use TOML loader

Replace the body of `build_default_tables()` to load from TOML files instead of calling `build_*()` functions. Remove the `ABBR` static (unused). Remove all `build_*()` functions. Remove the `include_str!` of the CSV.

**Files:**
- Modify: `src/tables/abbreviations.rs`

- [ ] **Step 1: Replace `build_default_tables` body**

In `src/tables/abbreviations.rs`, replace the `build_default_tables()` function (line 538) and the `ABBR` static (line 554-555) with:

```rust
/// Build the default abbreviation tables from TOML data files.
pub fn build_default_tables() -> Abbreviations {
    let mut tables = load_tables_from_toml(
        include_str!("../../data/defaults/tables.toml")
    );
    tables.extend(load_suffixes_from_toml(
        include_str!("../../data/defaults/suffixes.toml")
    ));
    let (number_cardinal, number_ordinal) = crate::tables::numbers::build_number_tables();
    tables.insert("number_cardinal".to_string(), number_cardinal);
    tables.insert("number_ordinal".to_string(), number_ordinal);
    Abbreviations { tables }
}
```

- [ ] **Step 2: Delete all `build_*()` functions**

Remove these functions from `src/tables/abbreviations.rs`:
- `build_directions()` (starts at line 342)
- `build_unit_types()` (starts at line 355)
- `build_unit_locations()` (starts at line 381)
- `build_states()` (starts at line 401)
- `build_all_suffixes()` (starts at line 423)
- `build_na_values()` (starts at line 502)
- `build_street_name_abbr()` (starts at line 509)
- `build_common_suffixes()` (starts at line 519)
- `pub static ABBR` (line 554-555)

- [ ] **Step 3: Run all tests**

```bash
cargo test
```

Expected: all 137 tests pass. The equivalence test from Task 4 can now be removed or kept as ongoing validation.

- [ ] **Step 4: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "refactor: replace build_*() functions with TOML loader"
```

---

### Task 6: Delete the old CSV include and clean up dead code

The `data/usps-street-suffix.csv` was moved to `data-raw/` in Task 2, but `build_all_suffixes()` had an `include_str!` reference to it. That function is now deleted (Task 5), so the old path is gone. Verify no stale references remain.

**Files:**
- Modify: `src/tables/abbreviations.rs` (if any cleanup needed)

- [ ] **Step 1: Check for stale references**

```bash
grep -r "usps-street-suffix" src/
grep -r "build_directions\|build_unit_types\|build_unit_locations\|build_states\|build_all_suffixes\|build_na_values\|build_street_name_abbr\|build_common_suffixes" src/
grep -r "ABBR" src/tables/abbreviations.rs
```

Expected: no matches (all references removed in Task 5). If anything remains, remove it.

- [ ] **Step 2: Run all tests**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Commit (if changes were needed)**

```bash
git add -A
git commit -m "chore: clean up stale references to old build functions and CSV path"
```

---

### Task 7: Update TUI — add tags to `DictGroupState` and suffix display

Update the TUI to support the new single suffix table with tags. Add a `tags` field to `DictGroupState`, update dict initialization to populate tags from the suffix TOML, and add a tags column to the dict display for suffix entries.

**Files:**
- Modify: `src/tui/tabs.rs:25-34` — add `tags` field to `DictGroupState`
- Modify: `src/tui/mod.rs:153-230` — populate tags during dict init
- Modify: `src/tui/meta.rs:21-32` — update TABLE_DESCRIPTIONS

- [ ] **Step 1: Update TABLE_DESCRIPTIONS**

In `src/tui/meta.rs`, replace the two suffix entries (lines 25-26):

```rust
    ("suffix_all", "All suffix variants (AVE/AV/AVEN -> AVENUE)"),
    ("suffix_common", "Common suffixes only"),
```

with one entry:

```rust
    ("suffix", "Street suffixes (AVE/AVENUE, tags: common)"),
```

- [ ] **Step 2: Add `tags` field to `DictGroupState`**

In `src/tui/tabs.rs`, add `tags` fields to the `DictGroupState` struct (after line 29):

```rust
pub(crate) struct DictGroupState {
    pub(crate) short: String,
    pub(crate) long: String,
    pub(crate) variants: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) status: GroupStatus,
    pub(crate) original_short: String,
    pub(crate) original_long: String,
    pub(crate) original_variants: Vec<String>,
    pub(crate) original_tags: Vec<String>,
}
```

- [ ] **Step 3: Fix all `DictGroupState` construction sites**

Every place that constructs a `DictGroupState` needs the new fields. Search with:

```bash
grep -n "DictGroupState {" src/tui/
```

For each construction site, add `tags: vec![]` and `original_tags: vec![]` (or the appropriate values — see Step 4 for the dict init site which needs actual tag data).

Key locations:
- `src/tui/mod.rs:187` — default group construction (tags from table)
- `src/tui/mod.rs:215` — added entries from config overrides (tags: vec![])
- `src/tui/mod.rs:670` — test helper (tags: vec![])
- `src/tui/panel.rs:1092` — add-new-entry in dict panel (tags: vec![])

- [ ] **Step 4: Populate tags in dict initialization**

The dict initialization in `src/tui/mod.rs:153-230` currently gets groups from `AbbrTable.groups`, which are `AbbrGroup` structs (no tags). For the suffix table, we need to get tags from the TOML source.

The simplest approach: add a `tags` field to `AbbrGroup` so it flows through naturally.

In `src/tables/abbreviations.rs`, add `tags` to `AbbrGroup`:

```rust
pub struct AbbrGroup {
    pub short: String,
    pub long: String,
    pub variants: Vec<String>,
    pub tags: Vec<String>,
}
```

Update `AbbrTable::from_groups` — no changes needed (it doesn't touch tags, just stores groups).

Update `AbbrTable::from_pairs` — add `tags: vec![]` to the AbbrGroup construction.

Update `load_tables_from_toml` — preserve tags from `GroupDef`:

```rust
let group = AbbrGroup {
    short: g.short,
    long: g.long,
    variants: g.variants,
    tags: g.tags,
};
```

Update `load_suffixes_from_toml` — same, preserve tags.

Update `build_number_tables()` in `src/tables/numbers.rs` — add `tags: vec![]` to the AbbrGroup constructions (lines ~148 and ~161).

Then in the TUI dict init (`src/tui/mod.rs:162`), populate tags:

```rust
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
```

- [ ] **Step 5: Update `AbbrTable::patch` to preserve tags**

In the `patch` method (`src/tables/abbreviations.rs`), the `AbbrGroup` constructions in the add/merge phase need `tags: vec![]` for newly created groups. Existing groups retain their tags through the clone.

- [ ] **Step 6: Run all tests**

```bash
cargo test
```

Expected: all tests pass. Some tests construct `AbbrGroup` directly and will need `tags: vec![]` added.

Fix any compilation errors from missing `tags` field in test AbbrGroup constructions.

- [ ] **Step 7: Commit**

```bash
git add src/tables/abbreviations.rs src/tables/numbers.rs src/tui/meta.rs src/tui/tabs.rs src/tui/mod.rs src/tui/panel.rs
git commit -m "feat: add tags to AbbrGroup, update TUI DictGroupState with tags field"
```

---

### Task 8: Display tags column in suffix dict panel

Add the tags column to the dictionary table display, but only when viewing the suffix table (other tables don't have tags yet).

**Files:**
- Modify: `src/tui/tabs.rs` — `render_dict` function (line 487)

- [ ] **Step 1: Update render_dict to show tags column**

In `src/tui/tabs.rs`, the `render_dict` function (starting at line 487) builds the table display. Modify it to include a tags column when the current table has any tagged entries.

Add a `has_tags` check after the `is_value_list` check (around line 563):

```rust
let has_tags = {
    let entries = app.current_dict_entries();
    entries.iter().any(|e| !e.tags.is_empty())
};
```

Update the row construction (around line 578) to include tags:

```rust
let tags_str = if has_tags {
    e.tags.join(", ")
} else {
    String::new()
};
```

Add the tags cell to each Row (after the variants cell):

```rust
if has_tags {
    Row::new(vec![
        check,
        Cell::from(e.short.clone()).style(style),
        Cell::from(e.long.clone()).style(style),
        Cell::from(variants_str).style(Style::new().fg(Color::DarkGray)),
        Cell::from(tags_str).style(Style::new().fg(Color::Cyan)),
    ])
} else {
    // existing row construction (no change)
}
```

Update the column widths and header to include tags when `has_tags`:

```rust
if has_tags {
    let widths = [
        Constraint::Length(1),    // check
        Constraint::Length(12),   // short
        Constraint::Length(20),   // long
        Constraint::Fill(1),      // variants
        Constraint::Length(12),   // tags
    ];
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Short").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Long").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Variants").style(Style::new().add_modifier(Modifier::BOLD)),
        Cell::from("Tags").style(Style::new().add_modifier(Modifier::BOLD)),
    ]).style(Style::new().fg(Color::Cyan));
    // ... build table_widget with these widths and header
}
```

- [ ] **Step 2: Run all tests**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/tui/tabs.rs
git commit -m "feat: display tags column in dict panel for suffix table"
```

---

### Task 9: Add tag editing in the TUI

Allow users to toggle the "common" tag on suffix entries using a key binding in the dictionary panel.

**Files:**
- Modify: `src/tui/tabs.rs` — dict key handling (around line 190)

- [ ] **Step 1: Add tag toggle key handler**

In the dict key handling section of `src/tui/tabs.rs` (the `handle_dict_keys` function or equivalent), add a handler for the `t` key that toggles the "common" tag on the selected entry:

```rust
KeyCode::Char('t') => {
    if let Some(i) = app.dict_list_state.selected() {
        let entries = app.current_dict_entries_mut();
        if i < entries.len() {
            let entry = &mut entries[i];
            let tag = "common".to_string();
            if entry.tags.contains(&tag) {
                entry.tags.retain(|t| t != &tag);
            } else {
                entry.tags.push(tag);
            }
            if entry.tags != entry.original_tags {
                entry.status = GroupStatus::Modified;
            } else if entry.status == GroupStatus::Modified {
                // Check if everything else is also unchanged
                if entry.short == entry.original_short
                    && entry.long == entry.original_long
                    && entry.variants == entry.original_variants
                {
                    entry.status = GroupStatus::Default;
                }
            }
            app.dirty = true;
        }
    }
}
```

- [ ] **Step 2: Run all tests**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/tui/tabs.rs
git commit -m "feat: add 't' key to toggle common tag on dict entries"
```

---

### Task 10: Update Abbreviations to expose suffix as a single table for TUI

The TUI currently shows whatever tables `Abbreviations::table_names()` returns. After the TOML switch, the loader still produces `suffix_all` and `suffix_common` as separate entries because the pipeline needs both for pattern expansion. But the TUI should show one `suffix` table.

**Files:**
- Modify: `src/tables/abbreviations.rs` — add method to get the source suffix groups with tags
- Modify: `src/tui/mod.rs` — dict initialization uses suffix source data instead of derived views

- [ ] **Step 1: Store suffix source data in Abbreviations**

Add a field to `Abbreviations` to hold the pre-filtered suffix groups (with tags):

```rust
pub struct Abbreviations {
    tables: HashMap<String, AbbrTable>,
    /// Source suffix groups with tags, for TUI display. Not present in older code paths.
    suffix_source: Option<Vec<AbbrGroup>>,
}
```

Update `load_suffixes_from_toml` to return the source groups alongside the derived tables. Change its return type to `(HashMap<String, AbbrTable>, Vec<AbbrGroup>)`:

```rust
pub fn load_suffixes_from_toml(toml_str: &str) -> (HashMap<String, AbbrTable>, Vec<AbbrGroup>) {
    // ... existing parsing ...
    let source_groups: Vec<AbbrGroup> = all_groups.into_iter()
        .map(|(g, tags)| AbbrGroup {
            short: g.short,
            long: g.long,
            variants: g.variants,
            tags,
        })
        .collect();

    let all = source_groups.iter().cloned().collect();
    let common_groups: Vec<AbbrGroup> = source_groups.iter()
        .filter(|g| g.tags.contains(&"common".to_string()))
        .cloned()
        .collect();

    let mut tables = HashMap::new();
    tables.insert("suffix_all".to_string(), AbbrTable::from_groups(all));
    tables.insert("suffix_common".to_string(), AbbrTable::from_groups(common_groups));
    (tables, source_groups)
}
```

Update `build_default_tables()`:

```rust
pub fn build_default_tables() -> Abbreviations {
    let mut tables = load_tables_from_toml(
        include_str!("../../data/defaults/tables.toml")
    );
    let (suffix_tables, suffix_source) = load_suffixes_from_toml(
        include_str!("../../data/defaults/suffixes.toml")
    );
    tables.extend(suffix_tables);
    let (number_cardinal, number_ordinal) = crate::tables::numbers::build_number_tables();
    tables.insert("number_cardinal".to_string(), number_cardinal);
    tables.insert("number_ordinal".to_string(), number_ordinal);
    Abbreviations { tables, suffix_source: Some(suffix_source) }
}
```

Add accessor:

```rust
impl Abbreviations {
    pub fn suffix_source(&self) -> Option<&[AbbrGroup]> {
        self.suffix_source.as_deref()
    }
}
```

Update `Abbreviations::patch` to pass through `suffix_source: self.suffix_source.clone()`.

- [ ] **Step 2: Update TUI dict initialization**

In `src/tui/mod.rs`, the dict init (line 147) currently iterates `table_names()` and builds entries from each table's groups. Modify this to:

1. Use `table_names()` but filter out `suffix_all` and `suffix_common`
2. Add a single `suffix` entry using `suffix_source()` groups (which have tags)
3. The table names list for TUI becomes: direction, na_values, state, street_name_abbr, **suffix**, unit_location, unit_type, number_cardinal, number_ordinal

- [ ] **Step 3: Run all tests**

```bash
cargo test
```

Expected: all tests pass. The pipeline still uses `suffix_all`/`suffix_common` internally. The TUI shows `suffix` as one table.

- [ ] **Step 4: Commit**

```bash
git add src/tables/abbreviations.rs src/tui/mod.rs
git commit -m "feat: expose suffix source data for TUI, display as single table"
```

---

### Task 11: Update TUI config export to handle suffix tags

When the user saves config changes from the TUI, the export needs to handle the merged suffix table correctly — translating tag changes into the right config format.

**Files:**
- Modify: `src/tui/mod.rs` — config export (look for `to_config` or dict override generation)

- [ ] **Step 1: Find and understand the config export code**

Search for where dict changes are converted back to `DictOverrides`:

```bash
grep -n "DictOverrides\|to_config\|dict_overrides\|dictionaries" src/tui/mod.rs
```

The export needs to map the TUI's single `suffix` table back to `suffix_all` dict overrides in the config. Tag changes on the suffix table should also be exported (though the config override system may need to be extended to support tag overrides — this is a scope decision to evaluate during implementation).

- [ ] **Step 2: Update the export logic**

If the dict override export iterates `table_names`, update it to map `suffix` back to `suffix_all` for the config file. Tag-only changes (toggling common on/off) can be tracked as modified entries.

- [ ] **Step 3: Run all tests**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat: update TUI config export for single suffix table"
```

---

### Task 12: Final cleanup and full test run

Remove any dead code, update the equivalence test (no longer needed since old functions are gone), and do a final verification.

**Files:**
- Modify: `src/tables/abbreviations.rs` — remove equivalence test if desired
- Verify: all files

- [ ] **Step 1: Remove the migration equivalence test**

Delete `test_toml_tables_match_build_functions` from the test module in `src/tables/abbreviations.rs` — the old `build_*()` functions no longer exist, so this test can't compile as-is. (It already served its purpose.)

- [ ] **Step 2: Run full test suite**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy -- -D warnings
```

Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: final cleanup for tables-to-TOML migration"
```
