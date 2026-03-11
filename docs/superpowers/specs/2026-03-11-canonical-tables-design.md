# Canonical Table Standardization Redesign

## Overview

Replace the flat `Abbr { short, long }` pair model with a group-based `AbbrGroup { short, long, variants }` model. Each group has one canonical short/long pair and a list of variant match patterns. Standardization uses a single table (not matching + format), and output config determines short vs long form. Eliminates `suffix_usps` as a separate table.

## Problem

1. `standardize_value` always calls `to_short(input)` first, which only searches the long column. Custom entries like `short = "N E", long = "NORTHEAST"` don't get found because "N E" is in the short column.
2. The `matching_table` / `format_table` split is confusing — users don't know which table to pick or what the difference is.
3. `suffix_usps` exists only to define which short forms are canonical. This should be a property of entries in `suffix_all`, not a separate table.

## Data Model

### AbbrGroup

Replaces `Abbr`:

```rust
pub struct AbbrGroup {
    pub short: String,          // canonical short form, e.g. "CIR"
    pub long: String,           // canonical long form, e.g. "CIRCLE"
    pub variants: Vec<String>,  // additional match patterns, can include regex: ["CIRC", "CIRCL", "C[IL]"]
}
```

A group is identified by its canonical pair. All values (short, long, variants) resolve to the same canonical output during standardization.

### AbbrTable

Contains:
- `groups: Vec<AbbrGroup>`
- `lookup: HashMap<String, usize>` — maps every **literal** value (canonical short, canonical long, non-regex variants) to a group index. Built with longest keys first so "N E" is found before "N".
- `regex_variants: Vec<(Regex, usize)>` — compiled regexes for groups that have regex variants, paired with group index. Only consulted when hashmap lookup misses.

Key methods:
- `standardize(&self, value: &str) -> Option<(usize, &str, &str)>` — hashmap lookup first, regex fallback second. Returns `(group_index, canonical_short, canonical_long)`.
- `all_match_values(&self) -> Vec<&str>` — collects all canonical shorts + canonical longs + all variants, deduped, sorted longest-first. Used for `{table_name}` pattern expansion in extraction regexes. Regex variants go in as-is (not escaped), literals are escaped.

### Standardize Step

`standardize_value` becomes:
1. Call `table.standardize(value)`
2. Output config says short → return canonical_short, long → return canonical_long
3. No match → return value unchanged

`Step::Standardize` changes:
- `matching_table: Option<String>` and `format_table: Option<String>` → replaced by `table: String`
- `StepDef` loses `matching_table` and `format_table` fields, uses existing `table` field

Steps in `steps.toml` change from:
```toml
[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
matching_table = "suffix_all"
format_table = "suffix_usps"
```
to:
```toml
[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
table = "suffix_all"
```

### Lookup: Two-Tier

1. **Hashmap** — all literal values from canonical shorts, canonical longs, and non-regex variants. O(1) lookup. Handles the vast majority of cases.
2. **Regex fallback** — only for groups that have regex variants (e.g., `C[IL]`). If hashmap misses, iterate through compiled regexes. List is small (only groups with regex variants), so scan is fast.

## suffix_usps Elimination

`suffix_usps` is deleted as a table. Its canonical short forms (col 3 of the USPS CSV) become the canonical `short` values on groups in `suffix_all`. The CSV parsing logic:

- Column 3 (postal service standard abbreviation) = canonical short
- Column 1 (primary suffix name) = canonical long
- Column 2 values (commonly used abbreviations) that aren't the canonical short or long = variants

## Config Format

### Adding a new group

```toml
[[dictionaries.direction.add]]
short = "NE"
long = "NORTHEAST"
variants = ["N E", "NEAST", "NO EAST"]
```

If a group with canonical long "NORTHEAST" already exists, merges variants into it (and updates canonical short if different). If no group matches, creates a new one.

### Overriding canonical

```toml
[[dictionaries.direction.add]]
short = "NEAST"
long = "NORTHEAST"
canonical = true
```

Finds the existing NORTHEAST group. Changes its canonical short to "NEAST". The old canonical short ("NE") is automatically demoted to a variant — nothing is lost.

### Removing

```toml
[dictionaries.direction]
remove = ["APARTMENT"]
```

Searches all values (canonical short, canonical long, variants) across all groups. Removes the matching group entirely.

## TUI Changes

### Dict editor

Main list shows groups as rows:
```
★ N    NORTH
★ S    SOUTH        SO
★ NE   NORTHEAST    N E, NEAST
★ NW   NORTHWEST
```

Drill into a group (Enter) to see variants with toggles:
```
NE → NORTHEAST
  [x] N E
  [x] NEAST
  [ ] NO EAST
```

Space to toggle variants on/off. Add new variants. Star on main list indicates canonical (every group has one).

### Standardize wizard

Simplified flow:
1. Pick target field
2. Pick table (single pick-list with descriptions)
3. Done — output format controlled by output config

Replaces the confusing matching_table / format_table two-step.

### Dict add entry

When adding to a table:
- Add variant to existing group (pick group, type variant)
- Add new group (enter canonical short, canonical long, optional variants)
- Mark entry as canonical (star moves, old canonical short demoted to variant)

## Built-in Table Construction

Tables built with `AbbrGroup` directly:

```rust
AbbrGroup {
    short: "AVE".into(),
    long: "AVENUE".into(),
    variants: vec!["AV", "AVEN", "AVENU", "AVN", "AVNUE"],
}
```

**Suffix tables:** `suffix_all` absorbs `suffix_usps`'s canonical data. `suffix_usps` deleted. `suffix_common` remains as a separate table (serves a different extraction-ordering role, to be addressed in a future redesign).

**Other tables** (direction, unit_type, unit_location, state, street_name_abbr, na_values): most groups have few or no variants. Convert straightforwardly.

**Number tables** (number_cardinal, number_ordinal): no variants, just `{ short: "42", long: "FORTYTWO", variants: [] }`. Standardize works the same way.

## Pattern Generation

`{suffix_all}` in extraction regexes expands by collecting `all_match_values()`: all canonical shorts + canonical longs + all variants, deduped, sorted longest-first, joined with `|`. Regex variants (containing regex metacharacters) are included as-is; literal values are escaped.

## Migration

- `matching_table` and `format_table` on StepDef: removed. `table` field used instead.
- `suffix_usps` table: deleted. References in steps.toml updated.
- `Abbr` struct: replaced by `AbbrGroup`.
- All table construction functions: updated to return `Vec<AbbrGroup>`.
- `DictEntry` config struct: gains `variants: Option<Vec<String>>` and `canonical: Option<bool>`.
- User configs referencing `matching_table`/`format_table`: will fail to compile step with a clear error message pointing to the new `table` field.
