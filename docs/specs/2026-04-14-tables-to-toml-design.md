# Tables to TOML — Design Spec

**Issue:** EvictionLab/addrust#4
**Branch:** `refactor/tables-to-toml`
**Date:** 2026-04-14

## Problem

The 7 hand-coded `build_*()` functions in `abbreviations.rs` are repetitive ceremony
for what is fundamentally tabular data. Each function does the same thing — pass groups
to `AbbrTable::from_groups()`. Domain knowledge belongs in data files, not code.

Additionally, `suffix_common` duplicates a subset of `suffix_all` with no metadata
explaining why those entries are special. Two separate tables for one domain concept
is a data shape problem.

## Solution

Move all abbreviation table data from Rust `build_*()` functions into TOML files.
Collapse `suffix_all` and `suffix_common` into a single `suffix` table with tags.
Replace all builder functions with one general TOML loader.

## File Layout

```
data/
  defaults/
    steps.toml          # (existing, unchanged)
    tables.toml         # hand-authored: 6 simple tables
    suffixes.toml       # generated from data-raw, then hand-edited for tags/regex
  usps-street-suffix.csv  # (removed — moves to data-raw/)

data-raw/
  usps-street-suffix.csv  # raw USPS data (source of truth)
```

The `data-raw/` script is a Rust binary (`src/bin/generate_suffixes.rs`) run via
`cargo run --bin generate-suffixes`. It runs manually, not on every build. The output
`suffixes.toml` is committed to git.

## TOML Format

Both files use the same inline-table format. All fields except `short` are optional,
with defaults: `long = ""`, `variants = []`, `tags = []`.

### tables.toml

```toml
[direction]
groups = [
    { short = "NE", long = "NORTHEAST" },
    { short = "NW", long = "NORTHWEST" },
    { short = "N",  long = "NORTH" },
    { short = "S",  long = "SOUTH" },
    { short = "E",  long = "EAST" },
    { short = "W",  long = "WEST" },
    { short = "SE", long = "SOUTHEAST" },
    { short = "SW", long = "SOUTHWEST" },
]

[unit_type]
groups = [
    { short = "APT", long = "APARTMENT" },
    { short = "NUM", long = "NUMBER", variants = ["NO"] },
    # ...
]

[na_values]
groups = [
    { short = "NULL" },
    { short = "NAN" },
    { short = "NO ADDRESS" },
    # ...
]

[street_name_abbr]
groups = [
    { short = "MT", long = "MOUNT" },
    { short = "FT", long = "FORT" },
]
```

Tables included: `direction`, `unit_type`, `unit_location`, `state`, `na_values`,
`street_name_abbr`.

### suffixes.toml

```toml
[suffix]
groups = [
    { short = "AVE", long = "AVENUE", variants = ["AV(?:EN?U?E?)?", "AE"], tags = ["common"] },
    { short = "BLVD", long = "BOULEVARD", variants = ["BOUL?V?", "BV?D?", "BL"], tags = ["common"] },
    { short = "STRA", long = "STRAVENUE" },
    # ...
]
```

The `tags` field is an array of strings. The loader uses tags to derive filtered views:
- `suffix_all` = all groups
- `suffix_common` = groups where tags contains `"common"`

Variants use regex patterns (consolidated, not one literal per variant) since the
existing `AbbrTable` machinery already supports fancy_regex in variants.

## Loader Design

### Deserialization structs

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
```

### Loader functions

- `load_tables_from_toml(toml_str: &str) -> HashMap<String, AbbrTable>` — parses
  `tables.toml`. Top-level keys become table names. Each `GroupDef` converts to an
  `AbbrGroup` and feeds into `AbbrTable::from_groups()`.

- `load_suffixes_from_toml(toml_str: &str) -> HashMap<String, AbbrTable>` — parses
  `suffixes.toml`. Returns two entries: `suffix_all` (all groups) and `suffix_common`
  (groups filtered by `tags` containing `"common"`).

- `load_default_tables() -> Abbreviations` — replaces both `build_default_tables()` and
  the `ABBR` static:

```rust
pub fn load_default_tables() -> Abbreviations {
    let mut tables = load_tables_from_toml(
        include_str!("../../data/defaults/tables.toml")
    );
    tables.extend(load_suffixes_from_toml(
        include_str!("../../data/defaults/suffixes.toml")
    ));
    let (cardinal, ordinal) = build_number_tables();
    tables.insert("number_cardinal".into(), cardinal);
    tables.insert("number_ordinal".into(), ordinal);
    Abbreviations { tables }
}
```

TOML files are embedded at compile time via `include_str!`. No runtime file I/O.

### What stays as code

- `AbbrTable`, `AbbrGroup`, `from_groups()`, `standardize()`, `patch()`,
  `bounded_regex()` — all table machinery
- `build_number_tables()` in `numbers.rs` — genuinely computed (1-999 combinatorial)

### What gets removed

- All 7 `build_*()` functions (build_directions, build_unit_types, build_unit_locations,
  build_states, build_all_suffixes, build_common_suffixes, build_na_values,
  build_street_name_abbr)
- The duplicated `ABBR` static initialization (replaced by one call to
  `load_default_tables()`)
- `include_str!` of the CSV in `build_all_suffixes()`

## data-raw Script

`src/bin/generate_suffixes.rs` — a Rust binary target run via
`cargo run --bin generate-suffixes`.

Processing steps (same logic as current `build_all_suffixes()`):
1. Read `data-raw/usps-street-suffix.csv`
2. Group variants by USPS abbreviation
3. Exclude TRAILER, HIGHWAY
4. Merge plural forms (PARKS, WALKS, SPURS, LOOPS)
5. Add manual variant overrides (from R package's `abbr_more_suffix`)
6. Consolidate literal variants into regex patterns where possible
7. Mark common suffixes with `tags = ["common"]`
8. Write `data/defaults/suffixes.toml`

Run manually when the CSV or script changes (the CSV is stable USPS data — rarely
changes). Output is committed to git. CI does not run this script.

## TUI Changes

- The dictionary panel shows one **suffix** table instead of separate `suffix_all` and
  `suffix_common` entries
- Each group row displays its tags
- Users can edit tags per group (toggle "common" on/off)
- Other tables display as before (no tags column unless they have tagged groups)
- Step patterns (`{suffix_all}`, `{suffix_common}`) still work — the loader derives
  both views from the tagged source table. The TUI edits source data; the pipeline
  sees derived views.

## Testing

All 137 existing tests should pass unchanged. The parsed output is identical — only the
data source changes, not the resulting `AbbrTable` contents.

Additional tests:
- TOML deserialization round-trip
- Tag filtering produces correct suffix_common subset
- Loader produces same tables as current build_*() functions (migration correctness)
