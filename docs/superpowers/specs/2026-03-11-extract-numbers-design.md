# Extract Infrastructure, Number-to-Word, and Pipeline Cleanup

## Overview

Four layered changes to the address parsing pipeline, built in dependency order:

1. Extract infrastructure (named capture groups, source field)
2. Finalize cleanup → real pipeline steps
3. Number-to-word conversion
4. Trailing number rule

## Layer 1: Extract Infrastructure

### Named Capture Groups (`targets`)

Extract steps can route capture groups to different fields. New `targets` field on `StepDef`:

```toml
[[step]]
type = "extract"
label = "unit_type_value"
pattern = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$'
targets = { unit_type = 1, unit = 2 }
```

`targets` is a map of `field_name → capture_group_number`. Each group's match goes to the named field. The full match is removed from the source. Exactly one of `target` or `targets` must be set (enforced in `compile_step`).

### Source Field (`source`)

Both extract and rewrite steps can specify a `source` field to operate on an already-extracted field instead of the working string:

```toml
[[step]]
type = "rewrite"
label = "strip_unit_hash"
pattern = '^#\s*'
replacement = ''
source = "unit"
```

When `source` is set, the step reads from and writes to that field's value. For extract, the matched portion is removed from the source field. If the source field is None, the step is a no-op.

This also requires adding `source: Option<Field>` to both `Step::Extract` and `Step::Rewrite` enum variants, and `source: Option<String>` to `StepDef`.

### `extract_remove` Generalization

`extract_remove` returns `Vec<Option<String>>` indexed by group number (group 0 = full match). Callers using the old single-target behavior use group 0. Callers using `targets` route each group to its field.

### `skip_if_filled` with `targets`

When `targets` is used with `skip_if_filled = true`, the step is skipped if **any** target field is already filled. This preserves the conservative behavior — don't overwrite existing data.

### `replacement` with `targets`

Not supported. `replacement` and `targets` are mutually exclusive (enforced in `compile_step`). If post-extraction transformation is needed on multi-target extracts, use separate standardize/rewrite steps with `source` on the individual fields.

### `source` on Extract: Move Semantics

When `source` is set and the pattern matches the full field value, extraction clears the source field (sets it to None). This is intentional "move" semantics — `promote_unit_to_street_number` uses this to move the unit value to street_number, leaving unit empty.

### StepDef Changes

New optional fields on `StepDef`:

- `source: Option<String>` — field name to operate on instead of working string
- `targets: Option<HashMap<String, usize>>` — map of field_name → capture group number (TOML inline table)

Both use `#[serde(skip_serializing_if = "Option::is_none")]` like existing optional fields.

### Step Enum Changes

`Step::Extract` adds:
- `targets: Option<HashMap<Field, usize>>` — compiled from StepDef targets
- `source: Option<Field>` — compiled from StepDef source

`Step::Rewrite` adds:
- `source: Option<Field>` — compiled from StepDef source

## Layer 2: Finalize → Real Steps

Three pieces of hardcoded logic in `Pipeline::finalize()` become explicit steps in `steps.toml`:

```toml
# Strip leading # from unit
[[step]]
type = "rewrite"
label = "strip_unit_hash"
pattern = '^#\s*'
replacement = ''
source = "unit"

# Strip leading zeros from street_number (preserves "0" — pattern requires digit after)
[[step]]
type = "rewrite"
label = "strip_leading_zeros_street_number"
pattern = '^0+(?=\d)'
replacement = ''
source = "street_number"

# Strip leading zeros from unit (preserves "0")
[[step]]
type = "rewrite"
label = "strip_leading_zeros_unit"
pattern = '^0+(?=\d)'
replacement = ''
source = "unit"

# Promote unit → street_number if no street_number
[[step]]
type = "extract"
label = "promote_unit_to_street_number"
pattern = '^.+$'
source = "unit"
target = "street_number"
skip_if_filled = true
```

After this, `finalize()` only does: assign remaining working string to `street_name` and remove placeholder tags.

**Ordering note:** `strip_leading_zeros_unit` runs before `promote_unit_to_street_number`, so a unit value like "0042" becomes "42" before promotion. This means `strip_leading_zeros_street_number` only needs to handle street numbers that were directly extracted, not promoted ones. The pattern `^0+(?=\d)` preserves a bare "0" (the lookahead requires a digit after the zeros).

## Layer 3: Number-to-Word Conversion

### Tables (generated at build time)

Cardinal and ordinal tables for 1–999, generated in Rust code (not a TOML file). Generated as `AbbrTable` instances via a function in the tables module (e.g., `build_number_tables() -> (AbbrTable, AbbrTable)`), registered into `Abbreviations` as `number_cardinal` and `number_ordinal`. Keys are the digit strings ("1", "2", ... "999") — no leading zeros.

- `number_cardinal`: `"1" → "ONE"`, `"2" → "TWO"`, ... `"999" → "NINE HUNDRED NINETY NINE"`
- `number_ordinal`: `"1" → "FIRST"`, `"2" → "SECOND"`, ... `"999" → "NINE HUNDRED NINETY NINTH"`

The generation function builds the English words algorithmically (ones, teens, tens, hundreds composition). No external crate dependency — the word lists are small enough to hardcode as arrays.

### Replacement Template Syntax

New `${N:table_name}` syntax in replacement strings. When the rewrite engine encounters this token, it takes capture group N's value, looks it up in the named table (via `to_long()`), and substitutes the result. If the lookup fails, the original captured value is kept as-is (no silent data loss).

This is processed in `apply_step`, not in `expand_template`. The existing `expand_template` only expands `{table_name}` in *patterns* at compile time. The `${N:table}` syntax is resolved at *apply time* against capture group values. The `expand_template` function will not touch `${N:table}` tokens — the `$` prefix distinguishes them from pattern-time `{table}` references. (Currently, `expand_template` would encounter `2:number_cardinal` inside braces, fail to find a table by that name, and skip it — which is correct but accidental. The `$` prefix makes the distinction explicit.)

Implementation: a new function `expand_replacement(template: &str, captures: &Captures, tables: &Abbreviations) -> String` that handles `$N` backrefs, `${N:table}` lookups, and `${N/M:fraction}` expansions. Called from `apply_step` for rewrite steps that have replacement templates containing these tokens.

```toml
replacement = '$1 ${2:number_cardinal}'  # "HIGHWAY ${2}" → "HIGHWAY ONE"
replacement = '${1:number_ordinal}'       # "1ST" → "FIRST"
```

### Fraction Syntax

New `${N/M:fraction}` token for fraction expansion. N = numerator group, M = denominator group. The code:

1. Converts numerator via `number_cardinal` table
2. If denominator is 2: always "HALF" (consistent output, regardless of numerator)
3. Otherwise: converts denominator via `number_ordinal` table, appends "S" if numerator > 1
4. Result: `"{cardinal} {fraction_word}"`

Examples:
- 1/2 → "ONE HALF"
- 5/2 → "FIVE HALF"
- 1/8 → "ONE EIGHTH"
- 5/8 → "FIVE EIGHTHS"
- 3/4 → "THREE FOURTHS"

### Pipeline Steps

Placed after all extractions, before trailing-number movement. Operate on the working string (which is the proto-street-name at this point):

```toml
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

## Layer 4: Trailing Number Rule

After number-to-word conversion, a trailing number in the working string is moved to `street_number` if none exists:

```toml
[[step]]
type = "extract"
label = "trailing_number_to_street_number"
pattern = '\b(\d{1,3})\s*$'
target = "street_number"
skip_if_filled = true
```

This runs after number-to-word so "HIGHWAY 1" becomes "HIGHWAY ONE" first and the "1" isn't mistakenly treated as a street number.

## Also Included: Boundary Cleanup

`extract_remove` now trims non-word characters (punctuation, symbols) from the start and end of the source string after extraction. This prevents dangling commas, dashes, etc. at the boundaries. (Already implemented.)

## Step Ordering (full pipeline after all changes)

1. na_check (rewrite)
2. city_state_zip (extract)
3. po_box (extract)
4. Pre-processing rewrites (unstick_suffix_unit, st_to_saint)
5. extra_front (extract)
6. street_number (extract)
7. unit_fraction (extract)
8. unit_type_value (extract, now with `targets = { unit_type = 1, unit = 2 }`)
9. unit_pound, unit_location (extract)
10. post_direction, suffix, pre_direction (extract)
11. Street name rewrites (street_name_abbr, name_st_to_saint)
12. **strip_unit_hash** (rewrite, source = unit) — NEW
13. **strip_leading_zeros_street_number** (rewrite, source = street_number) — NEW
14. **strip_leading_zeros_unit** (rewrite, source = unit) — NEW
15. **fractional_road** (rewrite) — NEW
16. **highway_number_to_word** (rewrite) — NEW
17. **ordinal_to_word** (rewrite) — NEW
18. **trailing_number_to_street_number** (extract) — NEW
19. **promote_unit_to_street_number** (extract, source = unit) — NEW
20. Standardization steps
21. Finalize (assign working → street_name, remove tags only)
