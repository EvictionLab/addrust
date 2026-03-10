# Design: Move Hardcoded Values into Dictionary Tables

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create an implementation plan from this design.

**Goal:** Move hardcoded NA values and street name abbreviations out of rule code and into dictionary tables. Refactor template expansion to work directly from `Abbreviations` instead of pre-joined strings.

## New Tables

### `na_values`
Value-list table (all longs empty). Default entries:
- NULL, NAN, MISSING, NONE, UNKNOWN, NO ADDRESS

The `change_na_address` rule pattern becomes `(?i)^({na_values})$` instead of a hardcoded alternation. Users can add/remove NA indicators via the TUI Dictionaries tab.

### `street_name_abbr`
Standard short/long pair table. Default entries:
- MT → MOUNT
- FT → FORT

Two existing rules (`change_name_mt_to_mount`, `change_name_ft_to_fort`) merge into one `change_street_name_abbr` rule with pattern `\b({street_name_abbr$short})\b`. Users can add entries like `ATL` → `ATLANTA`.

The `change_name_st_to_saint` rule stays hardcoded — its positional constraint (only before 3+ letter words) is genuinely different logic that doesn't belong in a lookup table.

## Value-List Table Support

`AbbrTable` gains awareness of its shape when all `long` fields are empty:

- `is_value_list()` — returns true when all longs are empty
- `all_values()` — skips empty strings so value-list tables return only shorts
- `short_values()` — new method, returns only short column values sorted by length descending (useful for any table, not just value-lists)
- TUI dictionary tab hides the long column and `->` arrow for value-list tables, shows single-column list
- Config serialization unchanged: `{ short = "VACANT", long = "" }` round-trips through existing `DictEntry`

## Template Placeholder Syntax

- `{table_name}` — expands via `all_values().join("|")` (existing behavior)
- `{table_name$short}` — expands via `short_values().join("|")` (new)

Mirrors R's `str_glue` accessor pattern (`{table$column}`). Extensible to `{table_name$long}` if needed later.

## Refactored Template Expansion

The `rule` closure in `build_rules` captures `&Abbreviations` directly instead of a pre-joined `HashMap<&str, &str>`. Template expansion parses each `{...}` placeholder at rule-build time:

1. Find `{...}` in the template
2. Split contents on `$` — left side is table name, right side (if present) is accessor
3. Look up the table from `Abbreviations`
4. Call `all_values()` or `short_values()` based on accessor
5. Join with `|`

This eliminates the `table_values` map. One source of truth — the `Abbreviations` struct.

Exception: `state` table uses `bounded_regex()` instead of `all_values()`. The expansion logic needs to handle this (either as a special case or by making the default expansion bounded for that table).

## Rule Changes

| Before | After |
|--------|-------|
| `change_na_address` with hardcoded alternation | `change_na_address` with `{na_values}` placeholder |
| `change_name_mt_to_mount` (separate rule) | Deleted — merged into `change_street_name_abbr` |
| `change_name_ft_to_fort` (separate rule) | Deleted — merged into `change_street_name_abbr` |
| (new) | `change_street_name_abbr` with `{street_name_abbr$short}` pattern |
| `change_name_st_to_saint` | Unchanged |

The `change_street_name_abbr` rule standardize regex needs to iterate the table's short→long pairs to replace whichever short form matched with its long form.

## Future Work (Separate Design)

Captured for a separate design doc:
- **Standardization pipeline cleanup** — clarify the match-then-standardize flow for suffixes (many forms → canonical short → output format)
- **Output format preferences** — let users choose short vs long output, case format (upper/lower/title) per component
- **Suffix table clarification** — `all_suffix` entries like `DR → DIRVE` should be one-directional (typo correction), not bidirectional
