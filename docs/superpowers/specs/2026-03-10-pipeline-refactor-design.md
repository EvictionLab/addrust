# Pipeline Refactor: Steps as Data

## Problem

The current pipeline has one `Rule` struct doing four jobs (extract, rewrite, standardize, validate) through an `Action` enum. This causes:

- Domain knowledge scattered across rules and `finalize()` (PO BOX spacing in 4 places)
- Two standardization systems (inline regex on rules vs. table lookup in finalize)
- Unused fields on every rule (target=None for rewrites, standardize=None for extractions)
- Rules defined as imperative Rust code — can't reorder without editing source
- Adding features (POB, ordinals, highways) requires touching multiple spots

## Design

### Core idea

Replace `Rule` + `Action` enum with a single ordered sequence of **steps** defined in TOML. Each step type carries only the data it needs. Tables expand to include regex patterns alongside short/long forms, so one table row holds all domain knowledge for a value.

### Step types

Four step types, defined as a tagged enum at runtime:

**Validate** — check for bad input, emit warnings, optionally short-circuit.
```toml
[[step]]
type = "validate"
label = "na_check"
pattern = '(?i)^(N/?A|{na_values})$'
warning = "na_address"
clear = true
```

**Rewrite** — transform the working string in place. No extraction, no fields.
```toml
[[step]]
type = "rewrite"
label = "unstick_suffix_unit"
pattern = '\b({suffix_common})({unit_type})\b'
replacement = '$1 $2'
```

**Extract** — match a pattern, pull the value into a field, remove from working string. No standardization.
```toml
[[step]]
type = "extract"
label = "po_box"
table = "po_box"
target = "po_box"
skip_if_filled = true
```

When a step has `table`, the extraction pattern comes from the table's pattern field. When a step has `pattern` directly, it uses that instead (for structural patterns that don't map to a single table, like city_state_zip).

**Standardize** — normalize an extracted field value using a table. Pure `(value, table) -> value`.
```toml
[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
matching_table = "suffix_all"
format_table = "suffix_usps"
```

### Extended table format

Tables gain an optional `pattern` field — a regex that recognizes all variants of this component. This is the matching pattern used by Extract steps that reference the table.

Current table shape:
```toml
[[entry]]
short = "ST"
long = "STREET"
```

New table shape:
```toml
[table]
pattern = '(?<!^)\b({suffix_common})\s*$'

[[entry]]
short = "ST"
long = "STREET"
```

The `pattern` field supports `{table_name}` template expansion, same as today. Tables without a pattern field work the same as before (standardization-only).

For tables like `po_box` where the pattern captures a value that needs reformatting:
```toml
[table]
pattern = '\b(?:P\W*O\W*BOX|POB)\W*(\w+)\b'

[[entry]]
short = "PO BOX"
long = "POST OFFICE BOX"
```

The table's entries define the canonical forms. The pattern defines how to recognize it in messy input. One place for all PO box knowledge — adding "POB" is editing one regex, not four rules.

### Step sequence

The full pipeline as a single ordered TOML array. Steps execute top to bottom.

```toml
# --- Validation ---
[[step]]
type = "validate"
label = "na_check"
pattern = '(?i)^(N/?A|{na_values})$'
warning = "na_address"
clear = true

# --- City / State / Zip (structural, no table) ---
[[step]]
type = "extract"
label = "city_state_zip"
pattern = ',\s*([A-Z][A-Z ]+)\W+{state}\W+(\d{5}(?:\W\d{4})?)(?:\s*US)?$'
target = "extra_back"

# --- PO Box ---
[[step]]
type = "extract"
label = "po_box"
table = "po_box"
target = "po_box"
skip_if_filled = true

# --- Pre-processing rewrites ---
[[step]]
type = "rewrite"
label = "unstick_suffix_unit"
pattern = '\b({suffix_common})({unit_type})\b'
replacement = '$1 $2'

[[step]]
type = "rewrite"
label = "st_to_saint"
pattern = '^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{suffix_all})\b)([A-Z]{3,20})'
replacement = '${1}SAINT $2'

# --- Extra front ---
[[step]]
type = "extract"
label = "extra_front"
pattern = '^(?:(?:[A-Z\W]+\s)+(?=(?:{direction})\s\d))|^(?:(?:[A-Z\W]+\s)+(?=\d))'
target = "extra_front"
skip_if_filled = true

# --- Street number ---
[[step]]
type = "extract"
label = "street_number_coords"
pattern = '^([NSEW])\W?(\d+)\W?([NSEW])\W?(\d+)\b'
target = "street_number"
skip_if_filled = true
replacement = '${1}${2} ${3}${4}'

[[step]]
type = "extract"
label = "street_number"
pattern = '^\d+\b'
target = "street_number"
skip_if_filled = true

[[step]]
type = "extract"
label = "unit_fraction"
pattern = '^[1-9]/\d+\b'
target = "unit"
skip_if_filled = true

# --- Unit ---
[[step]]
type = "extract"
label = "unit_type_value"
pattern = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$'
target = "unit"
skip_if_filled = true

[[step]]
type = "extract"
label = "unit_pound"
pattern = '#\W*(\w+)\s*$'
target = "unit"
skip_if_filled = true

[[step]]
type = "extract"
label = "unit_location"
table = "unit_location"
target = "unit"
skip_if_filled = true

# --- Direction / Suffix ---
[[step]]
type = "extract"
label = "post_direction"
table = "direction"
target = "post_direction"
skip_if_filled = true

[[step]]
type = "extract"
label = "suffix_common"
table = "suffix_common"
target = "suffix"
skip_if_filled = true

[[step]]
type = "extract"
label = "suffix_all"
table = "suffix_all"
target = "suffix"
skip_if_filled = true

[[step]]
type = "extract"
label = "pre_direction"
table = "direction"
target = "pre_direction"
skip_if_filled = true

# --- Street name cleanup ---
[[step]]
type = "rewrite"
label = "name_st_to_saint"
pattern = '(?:^|\s)ST\b(?=\s[A-Z]{3,})'
replacement = 'SAINT'

# --- remainder becomes street_name (handled by finalize) ---

# --- Standardization ---
[[step]]
type = "standardize"
label = "standardize_pre_direction"
target = "pre_direction"
matching_table = "direction"
format_table = "direction"

[[step]]
type = "standardize"
label = "standardize_post_direction"
target = "post_direction"
matching_table = "direction"
format_table = "direction"

[[step]]
type = "standardize"
label = "standardize_suffix"
target = "suffix"
matching_table = "suffix_all"
format_table = "suffix_usps"

[[step]]
type = "standardize"
label = "standardize_unit_location"
target = "unit"
matching_table = "unit_location"
format_table = "unit_location"

[[step]]
type = "standardize"
label = "standardize_po_box"
target = "po_box"
matching_table = "po_box"
format_table = "po_box"

[[step]]
type = "standardize"
label = "standardize_street_name_abbr"
target = "street_name"
matching_table = "street_name_abbr"
format_table = "street_name_abbr"
mode = "per_word"
```

### Rust data structures

```rust
enum Step {
    Validate {
        label: String,
        pattern: Regex,
        pattern_template: String,
        warning: String,
        clear: bool,
    },
    Rewrite {
        label: String,
        pattern: Regex,
        pattern_template: String,
        replacement: String,
    },
    Extract {
        label: String,
        pattern: Regex,
        pattern_template: String,
        target: Field,
        skip_if_filled: bool,
        /// Optional regex replacement on extracted value (for structural
        /// reformatting like coordinate street numbers).
        replacement: Option<(Regex, String)>,
    },
    Standardize {
        label: String,
        target: Field,
        matching_table: String,
        format_table: String,
        mode: StandardizeMode,
    },
}

enum StandardizeMode {
    /// Whole-field lookup (suffix, direction)
    WholeField,
    /// Per-word lookup within the field (street_name_abbr)
    PerWord,
}
```

Note: Extract keeps an optional `replacement` for structural reformatting (coordinate street numbers: `N123E456` → `N123 E456`). This is not standardization — it's restructuring captured groups. It doesn't reference a table.

### Pipeline execution

```rust
impl Pipeline {
    fn parse(&self, input: &str) -> Address {
        let prepared = match prepare(input) {
            Some(s) => s,
            None => return Address::na(),
        };

        let mut state = AddressState::new(prepared);

        for step in &self.steps {
            step.apply(&mut state, &self.tables, &self.output);
        }

        self.finalize(&mut state);
        state.fields
    }
}
```

`finalize()` shrinks to:
1. Assign remaining working string to street_name
2. Strip leading zeros from street_number
3. Clean `#` from unit
4. Promote unit to street_number if street_number is empty

These are structural fixups about the Address shape, not domain standardization.

### Loading

1. Default steps embedded via `include_str!("defaults/steps.toml")`
2. Default tables embedded via `include_str!("defaults/tables/*.toml")` (or current Rust-built tables)
3. User config can: disable steps by label, override step patterns, reorder steps, add new steps
4. `expand_template()` runs at load time to resolve `{table_name}` references in patterns
5. Regexes compiled once at load time, stored on the Step enum variants

### User customization

The existing config structure extends naturally:

```toml
[steps]
disabled = ["unit_pound"]
order = ["na_check", "city_state_zip", "po_box", ...]  # full reorder

[steps.override.po_box]
pattern = '...'  # override just the pattern

[steps.add]
# add a new step at a specific position
[[steps.add]]
after = "suffix_all"
type = "extract"
label = "my_custom_suffix"
pattern = '...'
target = "suffix"
```

### What disappears

- `Action` enum
- `Rule` struct
- `standardize` field (inline regex on rules)
- `standardize_pairs` field
- `build_rules()` function (replaced by TOML loading)
- `change_street_name_abbr` special-cased construction
- Most of `finalize()` (standardization moves to steps)
- `prepare.rs` Change-type rules (become Rewrite steps)

### What stays

- `prepare.rs` — raw input normalization (uppercase, punctuation) stays separate. It operates on raw input before parsing state exists.
- `Abbreviations` / `AbbrTable` / `Abbr` — unchanged, gains optional `pattern` field
- `Address` / `AddressState` / `Field` — unchanged
- `OutputConfig` — unchanged, referenced by Standardize steps
- `expand_template()` — unchanged, used at TOML load time
- `extract_remove()`, `replace_pattern()`, `squish()` in ops.rs — unchanged
- All existing tests — parsing results should not change

### Migration path

1. Add `pattern` field to table format, populate for tables that need it
2. Create `Step` enum alongside existing `Rule` (both can coexist)
3. Write TOML step definitions matching current rule behavior
4. Write TOML loader that produces `Vec<Step>`
5. Implement `Step::apply()` for each variant
6. Switch Pipeline to use `Vec<Step>` instead of `Vec<Rule>`
7. Verify all tests pass (golden tests are the key gate)
8. Remove `Rule`, `Action`, `build_rules()`, inline standardize logic
9. Shrink `finalize()` to structural fixups only
10. Update TUI to work with Step instead of Rule

### TUI impact

The TUI currently shows rules with pattern templates and enable/disable toggles. With steps:
- Step list replaces rule list (same display: label, type, pattern template, enabled)
- Pattern editing works the same (edit the template, rebuild regex)
- Step reordering becomes a new feature (move steps up/down)
- The "Output" tab for short/long toggles stays — it controls what Standardize steps produce
