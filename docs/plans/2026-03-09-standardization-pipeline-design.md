# Design: Standardization Pipeline & Output Format Settings

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create an implementation plan from this design.

**Goal:** Fix the suffix standardization flow, add per-component output format settings (short/long), and make them configurable via config file and TUI.

## Output Format Settings

New `[output]` section in `.addrust.toml`:

```toml
[output]
suffix = "long"           # "short" (DR) or "long" (DRIVE) — default: long
direction = "short"       # "short" (N) or "long" (NORTH) — default: short
unit_type = "long"        # "short" (APT) or "long" (APARTMENT) — default: long
unit_location = "long"    # "short" (UPPR) or "long" (UPPER) — default: long
state = "short"           # "short" (NY) or "long" (NEW YORK) — default: short
```

Each component gets a `"short"` or `"long"` preference. Defaults match current behavior (directions and state → short, everything else → long).

## Standardization Flow

Every component follows the same two-step process:

1. **Canonicalize** — map extracted value to its USPS short form via `to_short()`. If already short or unrecognized, keep as-is.
2. **Format** — based on user preference, either keep the short form or expand to long form via the canonical mapping table's `to_long()`.

### Suffix-Specific Flow

Suffixes have two separate concerns: matching (many variant forms) and output (one canonical form).

- **Step 1 (canonicalize):** Use `suffix_all` — it maps every variant (DRIV, DRV, DIRVE, DRIVE) to the USPS short (DR) via `to_short()`.
- **Step 2 (format):** Use `suffix_usps` — a 1:1 short↔long mapping (DR ↔ DRIVE). If preference is "long", look up `to_long()`. If "short", keep as-is.

### Other Components

Directions, unit types, unit locations, and state all have clean short↔long tables already. The same table handles both canonicalize and format steps.

## Suffix Table Cleanup

### `suffix_usps` → True 1:1 Mapping

Currently `build_usps_suffixes()` creates entries for every CSV row, producing a many-to-one table (multiple longs per short). Fix: only keep the primary mapping — rows where the primary name (col1) maps to the USPS short (col3). This gives exactly one long per short.

The CSV structure is:
```
primary_street_suffix_name, commonly_used_suffix_or_abbreviation, postal_service_standard_suffix_abbreviation
AVENUE,                     AV,                                   AVE
AVENUE,                     AVEN,                                 AVE
```

For `suffix_usps`, only keep: `abbr("AVE", "AVENUE")` — one entry per unique (short, primary_name) pair.

Plural edge cases (PARK/PARKS sharing short code PARK) are already handled by `suffix_all` with distinct codes (PARKS → PARKS).

### `suffix_all` — No Changes

Stays as-is. It's the matching table with all variant forms. Not used for output.

### `suffix_common` — No Changes

Stays as-is. It's the confident-match extraction table. Not used for output.

## Pipeline Changes

### Store Output Settings and Patched Tables

Currently `finalize()` uses the global `ABBR` static. This means config overrides to dictionaries don't affect standardization. Fix:

- Add `OutputConfig` to `Config` (the `[output]` section)
- `Pipeline` stores the `OutputConfig` and a clone of the patched `Abbreviations`
- `finalize()` uses the pipeline's tables and output config instead of the global `ABBR`

### Standardize Function

A general function that standardizes any extracted value:

```
standardize(value, matching_table, canonical_table, preference) -> String
```

1. Canonicalize: `matching_table.to_short(value)` (or identity if already short/unrecognized)
2. Format: if preference is "long", `canonical_table.to_long(short)` (or identity if no mapping)
3. Return the result

For most components, `matching_table` == `canonical_table`. For suffixes, matching uses `suffix_all` and canonical uses `suffix_usps`.

## TUI: Output Tab

Third tab alongside Rules and Dictionaries. Displays:

```
> suffix         long    (DRIVE)
  direction      short   (N)
  unit_type      long    (APARTMENT)
  unit_location  long    (UPPER)
  state          short   (NY)
```

- Navigate with up/down
- Toggle with Space (flips between short/long)
- Preview column shows an example of the current setting
- Save writes to `[output]` section of `.addrust.toml`

## Config Serialization

`OutputConfig` uses `#[serde(default)]` so missing fields use defaults. The `[output]` section is only written when values differ from defaults (diff-only, matching existing config behavior).

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct OutputConfig {
    pub suffix: OutputFormat,
    pub direction: OutputFormat,
    pub unit_type: OutputFormat,
    pub unit_location: OutputFormat,
    pub state: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Short,
    Long,
}
```

Defaults: suffix → Long, direction → Short, unit_type → Long, unit_location → Long, state → Short.

## Future Work (Not This Design)

- **Case formatting** — upper/lower/title per component. Layers on top of short/long without affecting this architecture.
- **Street name case** — title case for street names (currently always UPPER).
