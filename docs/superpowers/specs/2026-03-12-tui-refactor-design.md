# TUI Refactor Design

**Goal:** Restructure the TUI so domain knowledge lives in data tables, repeated UI patterns are shared components, and the file layout reflects logical boundaries. Make adding a new column, step type, or tab a matter of adding rows, not editing branches.

**Branch:** feat/step-editor-form (continues current work)

---

## 1. Address Columns — Define Once

The `Col` enum is the single source of truth for address output columns. A const table `COL_DEFS` pairs each variant with its config key and display label. Everything else derives from this table.

```rust
// src/address.rs

pub struct ColDef {
    pub col: Col,
    pub key: &'static str,
    pub label: &'static str,
}

pub const COL_DEFS: &[ColDef] = &[
    ColDef { col: Col::StreetNumber,  key: "street_number",  label: "Street Number" },
    ColDef { col: Col::PreDirection,  key: "pre_direction",  label: "Pre-Direction" },
    ColDef { col: Col::StreetName,    key: "street_name",    label: "Street Name" },
    ColDef { col: Col::Suffix,        key: "suffix",         label: "Suffix" },
    ColDef { col: Col::PostDirection, key: "post_direction",  label: "Post-Direction" },
    ColDef { col: Col::Unit,          key: "unit",           label: "Unit" },
    ColDef { col: Col::UnitType,      key: "unit_type",      label: "Unit Type" },
    ColDef { col: Col::PoBox,         key: "po_box",         label: "PO Box" },
    ColDef { col: Col::Building,      key: "building",       label: "Building" },
    ColDef { col: Col::ExtraFront,    key: "extra_front",    label: "Extra Front" },
    ColDef { col: Col::ExtraBack,     key: "extra_back",     label: "Extra Back" },
    ColDef { col: Col::City,          key: "city",           label: "City" },
    ColDef { col: Col::State,         key: "state",          label: "State" },
    ColDef { col: Col::Zip,           key: "zip",            label: "Zip" },
];
```

`Col::from_key()` and `Col::label()` are lookups into `COL_DEFS`. `ADDRESS_COLS` in the TUI is removed — pickers iterate `COL_DEFS` directly. `parse_col()` is replaced by `Col::from_key()`.

New columns (city, state, zip) are added to the `Col` enum, the `Address` struct, and `COL_DEFS` — one row each. No other code changes needed.

---

## 2. Rename target/source → output_col/input_col

`StepDef` fields are renamed for clarity:

| Old | New | Meaning |
|-----|-----|---------|
| `target: Option<String>` | removed | |
| `targets: Option<HashMap<String, usize>>` | removed | |
| `source: Option<String>` | `input_col: Option<String>` | Column to read from instead of working string |
| — | `output_col: Option<OutputCol>` | Column(s) to write to |

`target` and `targets` merge into a single `output_col` field using a serde-compatible enum:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputCol {
    Single(String),
    Multi(HashMap<String, usize>),
}
```

TOML examples:
```toml
output_col = "suffix"                          # single
output_col = { unit_type = 1, unit = 2 }       # multi with capture groups
```

`steps.toml` is updated mechanically: `target =` → `output_col =`, `targets =` → `output_col =`, `source =` → `input_col =`.

---

## 3. Step Type Metadata as Data

A const table defines per-step-type behavior. The TUI reads this table instead of branching on step type strings.

```rust
// src/tui/meta.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKey {
    Pattern,
    Table,
    OutputCol,
    Replacement,
    SkipIfFilled,
    Mode,
    InputCol,
    Label,
}

pub struct StepTypeMeta {
    pub name: &'static str,
    pub display: &'static str,
    pub visible: &'static [PropKey],
    pub required: fn(&StepDef) -> bool,
}

pub const STEP_TYPES: &[StepTypeMeta] = &[
    StepTypeMeta {
        name: "extract",
        display: "Extract",
        visible: &[PropKey::Pattern, PropKey::OutputCol, PropKey::SkipIfFilled,
                   PropKey::Replacement, PropKey::InputCol, PropKey::Label],
        required: |def| def.pattern.is_some() && def.output_col.is_some(),
    },
    StepTypeMeta {
        name: "rewrite",
        display: "Rewrite",
        visible: &[PropKey::Pattern, PropKey::Table, PropKey::Replacement,
                   PropKey::InputCol, PropKey::Label],
        required: |def| def.pattern.is_some()
            && (def.replacement.is_some() || def.table.is_some()),
    },
    StepTypeMeta {
        name: "standardize",
        display: "Standardize",
        visible: &[PropKey::Pattern, PropKey::Table, PropKey::Replacement,
                   PropKey::OutputCol, PropKey::Mode, PropKey::Label],
        required: |def| def.output_col.is_some()
            && (def.table.is_some() || (def.pattern.is_some() && def.replacement.is_some())),
    },
];
```

Help text is a flat table keyed by `PropKey`:

```rust
pub const PROP_HELP: &[(PropKey, &str)] = &[
    (PropKey::Pattern, "Regex pattern to match. Use {table_name} for table references."),
    (PropKey::Table, "Abbreviation table for lookups."),
    (PropKey::OutputCol, "Output column(s) to write the match to."),
    (PropKey::Replacement, "Replacement text. Use $1, $2 for capture groups."),
    (PropKey::SkipIfFilled, "Skip this step if the output column already has a value."),
    (PropKey::Mode, "Match mode: whole field or per word."),
    (PropKey::InputCol, "Read from this column instead of the working string."),
    (PropKey::Label, "Unique identifier for this step."),
];
```

Functions replaced by table lookups:
- `visible_fields_for_type()` → find step type in `STEP_TYPES`, return `.visible`
- `validate_step_def()` → find step type, call `.required(def)`
- `render_form_help_panel()` → find `PropKey` in `PROP_HELP`, render the string

---

## 4. Shared Panel Layout

All three tabs use the same interaction pattern: table on the left, detail panel on the right. A shared `Panel` component handles the chrome.

### Panel responsibilities:
- Split area into left table + right detail panel
- Draw borders with focus indicators (cyan when focused, gray when not)
- Arrow key navigation: Up/Down move rows, Right/Enter drill into detail, Left/Esc back out
- Track focus state (left vs right) and selected row

### Tab responsibilities (provided to the panel):
- Table column headers
- Row content (given an index, return styled column values)
- Right panel content (given selected row, render the detail)
- Detail panel key handling (Enter to edit a property, Space to toggle, etc.)

### Per-tab table columns:

**Steps tab:**
| Label | Function | Input | Output | Pattern |
|-------|----------|-------|--------|---------|
| suffix_common | Extract | (working) | suffix | {suffix_common} |
| prep_fix_ampersand | Rewrite | (working) | — | &AMP; → & |

**Dict tab:**
| Short | Long | Variants | Status |
|-------|------|----------|--------|
| AVE | AVENUE | 4 variants | Default |
| BLVD | BOULEVARD | 3 variants | Modified |

**Output tab:**
| Component | Format | Example |
|-----------|--------|---------|
| Suffix | Long | AVENUE |
| Direction | Short | N |

### Right panel behavior:
- **Steps:** Property list from `StepTypeMeta.visible`, with editors (pattern drill-down, column picker, table picker, text input, toggles)
- **Dict:** Variant list with add/delete/edit
- **Output:** Format picker (Short/Long/Default) with preview

---

## 5. UI Primitives

Repeated patterns extracted into shared helper functions in `widgets.rs`:

| Helper | Replaces | Usage |
|--------|----------|-------|
| `selected_style(selected: bool)` | 15+ inline style checks | Consistent bold/highlight for selected items |
| `focus_border(focused: bool)` | 4+ inline color checks | Cyan when focused, gray when not |
| `checkbox(checked: bool)` | 5+ inline `[x]/[ ]` formatting | Consistent checkbox rendering |
| `cursor_line(text, cursor_pos)` | 3+ cursor rendering implementations | Text with visible cursor position |
| `truncate(text, width)` | Ad-hoc truncation | Fit text into table columns |

---

## 6. Navigation

Arrow keys only — no j/k/h/l. Consistent everywhere:

| Key | Action |
|-----|--------|
| Up / Down | Move selection in list/table |
| Right / Enter | Drill into detail panel (or edit a property) |
| Left / Esc | Back out (detail → list, or close form) |
| Space | Toggle (enabled/disabled, skip_if_filled, mode) |
| Tab / BackTab | Switch top-level tabs |
| `a` | Add new item |
| `d` | Delete (custom steps, dict entries) |
| `m` | Move step (reorder pipeline) |
| `s` | Save config |
| `q` | Quit |

---

## 7. Module Layout

```
src/tui/
  mod.rs        — App struct, run loop, top-level render, Tab enum, key dispatch
  panel.rs      — shared two-panel table+detail layout, focus state, nav
  widgets.rs    — UI primitives (styling, checkbox, cursor, truncation)
  tabs.rs       — content providers for all three tabs (headers, rows, detail, keys)
  meta.rs       — StepTypeMeta, PropKey, STEP_TYPES, PROP_HELP
```

`meta.rs` holds data tables only — no rendering, no key handling.
`panel.rs` holds the shared frame — no tab-specific knowledge.
`widgets.rs` holds pure rendering helpers — no state, no logic.
`tabs.rs` holds the tab-specific content — what to show, how to edit.
`mod.rs` holds the app state and wiring.

If `tabs.rs` grows unwieldy, individual tabs can split out. Start consolidated.

---

## 8. Cleanup

Since there is no backward compatibility requirement:

- Remove deprecated `matching_table` and `format_table` fields from `StepDef`
- Remove `pattern_overrides` from `StepsConfig` (already superseded by `step_overrides`)
- Remove `InputMode::EditPattern` remnants if any survive
- Remove `TABLE_DESCRIPTIONS` constant (derive from table registry or move to `meta.rs`)
- Remove `ADDRESS_COLS` constant (use `COL_DEFS` from address.rs)
