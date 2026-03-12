# TUI Refactor Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure the TUI so domain knowledge lives in data tables, repeated UI patterns are shared components, and the file layout reflects logical boundaries.

**Architecture:** Data model changes first (address columns, StepDef field renames, cleanup), then TUI module split with shared panel layout and data-driven step metadata. Each chunk produces a compiling, passing codebase.

**Tech Stack:** Rust, ratatui, serde (toml), fancy_regex, crossterm.

**Spec:** `docs/superpowers/specs/2026-03-12-tui-refactor-design.md`

---

## Chunk 1: ColDef Table + City/State/Zip

Consolidate the three copies of address column definitions into one `COL_DEFS` table in `src/address.rs`. Add City, State, Zip columns. Replace `parse_col()` with `Col::from_key()`.

### Task 1: Add ColDef table and Col methods

**Files:**
- Modify: `src/address.rs`

- [ ] **Step 1: Write test for Col::from_key and Col::label**

Add at bottom of `src/address.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_col_from_key_roundtrip() {
        for def in COL_DEFS {
            assert_eq!(Col::from_key(def.key).unwrap(), def.col);
            assert_eq!(def.col.label(), def.label);
            assert_eq!(def.col.key(), def.key);
        }
    }

    #[test]
    fn test_col_from_key_unknown() {
        assert!(Col::from_key("nonsense").is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_col_from_key -- --nocapture`
Expected: FAIL — `ColDef`, `COL_DEFS`, `from_key`, `label`, `key` don't exist

- [ ] **Step 3: Add ColDef, COL_DEFS, and Col methods**

Add to `src/address.rs` after the `Col` enum:

```rust
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

impl Col {
    pub fn from_key(key: &str) -> Result<Col, String> {
        COL_DEFS.iter()
            .find(|d| d.key == key)
            .map(|d| d.col)
            .ok_or_else(|| format!("Unknown column name: {}", key))
    }

    pub fn key(&self) -> &'static str {
        COL_DEFS.iter().find(|d| d.col == *self).unwrap().key
    }

    pub fn label(&self) -> &'static str {
        COL_DEFS.iter().find(|d| d.col == *self).unwrap().label
    }
}
```

Add `City`, `State`, `Zip` variants to the `Col` enum.

Add `city`, `state`, `zip` fields to the `Address` struct (all `Option<String>`).

Add the three new match arms to `field_mut()` and `field()`.

- [ ] **Step 4: Run tests**

Run: `cargo test test_col_from_key -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/address.rs
git commit -m "feat: COL_DEFS table, Col::from_key/key/label, add city/state/zip columns"
```

### Task 2: Replace parse_col() with Col::from_key()

**Files:**
- Modify: `src/step.rs` — remove `parse_col()`, replace all calls with `Col::from_key()`

- [ ] **Step 1: Replace parse_col with Col::from_key**

In `src/step.rs`, delete the `parse_col()` function (lines 438-452). Find all calls to `parse_col(...)` and replace with `Col::from_key(...)`. There are ~6 calls in `compile_step()`.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/step.rs
git commit -m "refactor: replace parse_col() with Col::from_key() lookup"
```

### Task 3: Replace ADDRESS_COLS with COL_DEFS in TUI

**Files:**
- Modify: `src/tui.rs` — remove `ADDRESS_COLS` constant, use `COL_DEFS` everywhere

- [ ] **Step 1: Remove ADDRESS_COLS and update references**

Delete the `ADDRESS_COLS` constant from `src/tui.rs`. Replace all `ADDRESS_COLS` references with `crate::address::COL_DEFS`. Update the usage pattern: where code previously did `ADDRESS_COLS[i].0` (key) and `ADDRESS_COLS[i].1` (label), use `COL_DEFS[i].key` and `COL_DEFS[i].label`. Also update `.len()` calls.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/tui.rs
git commit -m "refactor: replace ADDRESS_COLS with COL_DEFS from address.rs"
```

---

## Chunk 2: StepDef Renames + Cleanup

Rename `target`/`targets`/`source` → `output_col`/`input_col` on StepDef and StepOverride. Add `OutputCol` enum. Remove deprecated fields. Update `steps.toml`.

### Task 4: Add OutputCol enum and rename StepDef fields

**Files:**
- Modify: `src/step.rs` — add `OutputCol` enum, rename fields on `StepDef`
- Modify: `data/defaults/steps.toml` — rename `target`/`targets`/`source` keys
- Test: existing tests in `tests/config.rs` and unit tests

- [ ] **Step 1: Add OutputCol enum to step.rs**

Add before `StepDef`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputCol {
    Single(String),
    Multi(HashMap<String, usize>),
}
```

- [ ] **Step 2: Rename StepDef fields**

In `StepDef`:
- Remove `target: Option<String>`
- Remove `targets: Option<HashMap<String, usize>>`
- Remove `source: Option<String>`
- Remove `matching_table: Option<String>` (deprecated)
- Remove `format_table: Option<String>` (deprecated)
- Add `output_col: Option<OutputCol>`
- Add `input_col: Option<String>`

Keep `#[serde(skip_serializing_if = "Option::is_none")]` on both new fields.

- [ ] **Step 3: Update compile_step()**

In `compile_step()`:
- Replace `def.source` → `def.input_col`
- Replace `def.target` / `def.targets` logic with `def.output_col` match:

```rust
let (target, targets) = match &def.output_col {
    Some(OutputCol::Single(name)) => (Some(Col::from_key(name)?), None),
    Some(OutputCol::Multi(map)) => {
        let mut parsed = std::collections::HashMap::new();
        for (col_name, group_num) in map {
            parsed.insert(Col::from_key(col_name)?, *group_num);
        }
        (None, Some(parsed))
    }
    None => (None, None),
};
```

For extract: require `target.is_some() || targets.is_some()`.
For standardize: require `target.is_some()`, reject `targets`.
For rewrite: `source` → `input_col`, no target needed.

Remove `matching_table` / `format_table` references (the `.or(def.matching_table.clone())` fallback in extract).

- [ ] **Step 4: Update steps.toml**

Mechanical rename in `data/defaults/steps.toml`:
- All `target = "..."` → `output_col = "..."`
- All `targets = { ... }` → `output_col = { ... }`
- All `source = "..."` → `input_col = "..."`

- [ ] **Step 5: Fix all compilation errors**

Update all files that reference `StepDef` fields:
- `src/pipeline.rs` — `from_config` / `from_steps_config` may reference `target`, `targets`, `source`
- `src/config.rs` — `StepOverride` and `apply_to()` — update field names and types
- `src/tui.rs` — all references to `def.target`, `def.targets`, `def.source`, `FormField::Target`, `FormField::Targets`, `FormField::Source`, `FormField::TargetMode`
- `tests/config.rs` — test assertions referencing `target`, `targets`, `source`

For `StepOverride`: rename `target`/`targets`/`source` to `output_col`/`input_col`. Change `output_col` type to `Option<OutputCol>`. Update `apply_to()`.

For the TUI `FormField` enum:
- Remove `Target`, `Targets`, `TargetMode`, `Source`
- Add `OutputCol`, `InputCol`
- Update all match arms in `visible_fields_for_type`, `form_field_display`, `field_key`, `handle_form_left_key`, `render_form_help_panel`, `render_form_targets_panel`, `handle_form_targets_key`, `render_form_right_panel`, `close_form`, etc.

This is a large mechanical change. Work through compilation errors one by one.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: Some tests will need updating for renamed fields. Fix assertion strings.

- [ ] **Step 7: Commit**

```bash
git add src/step.rs src/config.rs src/pipeline.rs src/tui.rs data/defaults/steps.toml tests/
git commit -m "refactor: rename target/targets/source → output_col/input_col, add OutputCol enum, remove deprecated fields"
```

---

## Chunk 3: TUI Module Split + Widgets

Split `tui.rs` into `src/tui/` module directory. Extract UI primitives into `widgets.rs`. No behavior changes — pure reorganization.

### Task 5: Create tui/ module directory and move code

**Files:**
- Delete: `src/tui.rs`
- Create: `src/tui/mod.rs` — App struct, run loop, top-level render, Tab enum, key dispatch
- Create: `src/tui/widgets.rs` — UI primitive helpers
- Create: `src/tui/tabs.rs` — tab content (steps, dict, output rendering + key handling)
- Create: `src/tui/panel.rs` — placeholder (empty for now, populated in Chunk 4)
- Create: `src/tui/meta.rs` — placeholder (empty for now, populated in Chunk 4)

- [ ] **Step 1: Create src/tui/ directory**

```bash
mkdir -p src/tui
```

- [ ] **Step 2: Extract widgets.rs**

Create `src/tui/widgets.rs` with these helpers extracted from repeated patterns in `tui.rs`:

```rust
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Style for a selected list item (bold white) vs unselected (default).
pub fn selected_style(selected: bool) -> Style {
    if selected {
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    }
}

/// Border style: cyan when focused, dark gray when not.
pub fn focus_border(focused: bool) -> Style {
    Style::new().fg(if focused { Color::Cyan } else { Color::DarkGray })
}

/// Render a checkbox: "[x]" or "[ ]".
pub fn checkbox(checked: bool) -> &'static str {
    if checked { "[x]" } else { "[ ]" }
}

/// Render text with a visible cursor at the given position.
pub fn cursor_line<'a>(text: &'a str, cursor: usize) -> Line<'a> {
    let pos = cursor.min(text.len());
    let (before, after) = text.split_at(pos);
    Line::from(vec![
        Span::styled(before, Style::new().fg(Color::White)),
        Span::styled(
            if after.is_empty() { "_".to_string() } else { after[..1].to_string() },
            Style::new().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(
            if after.len() > 1 { after[1..].to_string() } else { String::new() },
            Style::new().fg(Color::White),
        ),
    ])
}

/// Truncate text to fit within a given width, adding "..." if truncated.
pub fn truncate(text: &str, width: usize) -> String {
    if text.len() <= width {
        text.to_string()
    } else if width > 3 {
        format!("{}...", &text[..width - 3])
    } else {
        text[..width].to_string()
    }
}
```

- [ ] **Step 3: Extract tabs.rs**

Move all tab-specific rendering and key handling functions from `tui.rs` into `src/tui/tabs.rs`:

Functions to move:
- `render_steps()` and `handle_rules_key()`
- `render_dict()`, `handle_dict_key()`, `handle_input_mode()`, `text_edit()`, `render_text_with_cursor()`
- `render_output()`, `handle_output_key()`
- `render_step_form()`, `render_form_left_panel()`, `render_form_right_panel()`, `render_form_help_panel()`, `render_form_pattern_panel()`, `render_form_targets_panel()`, `render_form_table_panel()`, `render_form_text_edit_panel()`
- `handle_form_key()`, `handle_form_left_key()`, `handle_form_pattern_key()`, `handle_form_targets_key()`, `handle_form_table_key()`, `handle_form_text_edit()`
- `close_form()`, `validate_step_def()`, `visible_fields_for_type()`, `field_key()`, `form_field_display()`
- Supporting types: `FormState`, `FormFocus`, `FormField`, `DictGroupState`, `GroupStatus`, `OutputSettingState`, `StepState`, `InputMode`, `TextEditResult`
- Supporting constants: `TABLE_DESCRIPTIONS`

The functions take `&mut App` or `&App` — make `App` fields `pub(super)` so `tabs.rs` can access them.

- [ ] **Step 4: Create mod.rs with remaining code**

`src/tui/mod.rs` keeps:
- `mod widgets;`, `mod tabs;`, `mod panel;`, `mod meta;`
- `pub use` for the public `run()` function
- `Tab` enum
- `App` struct (with `pub(super)` fields)
- `App::new()`, `App::to_config()`, `App::save()`, helper methods
- `run()` and `run_loop()` — the main event loop
- `render()` — top-level render dispatch
- `centered_rect()` helper
- Tests

Update `run_loop()` to call `tabs::handle_rules_key()`, `tabs::handle_dict_key()`, etc.
Update `render()` to call `tabs::render_steps()`, `tabs::render_dict()`, etc.

- [ ] **Step 5: Create empty placeholder files**

```rust
// src/tui/panel.rs
// Shared panel layout — populated in Chunk 4.

// src/tui/meta.rs
// Step type metadata tables — populated in Chunk 4.
```

- [ ] **Step 6: Update src/lib.rs**

The `mod tui;` declaration should still work — Rust resolves `src/tui/mod.rs` automatically.

- [ ] **Step 7: Run full test suite**

Run: `cargo test`
Expected: All tests pass. This is a pure code move, no behavior changes.

- [ ] **Step 8: Replace inline styles with widget helpers**

Go through `tabs.rs` and replace repeated patterns with calls to `widgets::selected_style()`, `widgets::focus_border()`, `widgets::checkbox()`, etc. This is a search-and-replace within the file.

- [ ] **Step 9: Remove j/k key handling**

In all key handlers in `tabs.rs`, remove `KeyCode::Char('j')` and `KeyCode::Char('k')` arms. Keep only `KeyCode::Up` and `KeyCode::Down`. Similarly remove `KeyCode::Char('h')` and `KeyCode::Char('l')` if present — keep only `KeyCode::Left` and `KeyCode::Right`.

- [ ] **Step 10: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 11: Commit**

```bash
git add src/tui.rs src/tui/ src/lib.rs
git commit -m "refactor: split tui.rs into tui/ module (mod, widgets, tabs, panel, meta)"
```

---

## Chunk 4: Step Type Metadata Tables

Move step type domain knowledge from code branches into data tables in `meta.rs`. Replace `visible_fields_for_type()`, `validate_step_def()`, and `render_form_help_panel()` with table lookups.

### Task 6: Implement meta.rs with StepTypeMeta and PropKey

**Files:**
- Modify: `src/tui/meta.rs`

- [ ] **Step 1: Write PropKey, StepTypeMeta, STEP_TYPES, PROP_HELP**

```rust
use crate::step::StepDef;

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
        visible: &[PropKey::Pattern, PropKey::Table, PropKey::OutputCol,
                   PropKey::SkipIfFilled, PropKey::Replacement, PropKey::InputCol,
                   PropKey::Label],
        required: |def| (def.pattern.is_some() || def.table.is_some())
            && def.output_col.is_some(),
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

pub fn find_step_type(name: &str) -> Option<&'static StepTypeMeta> {
    STEP_TYPES.iter().find(|m| m.name == name)
}

pub fn help_text(key: PropKey) -> &'static str {
    PROP_HELP.iter().find(|p| p.0 == key).map(|p| p.1).unwrap_or("")
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles (meta.rs doesn't import anything that changed yet).

- [ ] **Step 3: Commit**

```bash
git add src/tui/meta.rs
git commit -m "feat: step type metadata tables (StepTypeMeta, PropKey, STEP_TYPES, PROP_HELP)"
```

### Task 7: Replace visible_fields_for_type and validate_step_def with table lookups

**Files:**
- Modify: `src/tui/tabs.rs`

- [ ] **Step 1: Replace visible_fields_for_type()**

Replace the function body with a lookup into `meta::STEP_TYPES`. Map `PropKey` variants to `FormField` variants (or replace `FormField` with `PropKey` directly if practical). The visible list comes from `meta::find_step_type(step_type).visible`.

- [ ] **Step 2: Replace validate_step_def()**

Replace the match-on-step-type body with:

```rust
fn validate_step_def(def: &crate::step::StepDef) -> bool {
    super::meta::find_step_type(&def.step_type)
        .map(|m| (m.required)(def))
        .unwrap_or(false)
}
```

- [ ] **Step 3: Replace render_form_help_panel()**

Replace the large per-field match with a lookup into `meta::help_text()`. The right panel shows the help string for the currently selected `PropKey`.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tui/tabs.rs
git commit -m "refactor: replace form field branches with meta.rs table lookups"
```

---

## Chunk 5: Shared Panel + Table Layout

Replace the current list rendering in each tab with a shared table-based panel component. Steps, dict, and output all render as tables with a right detail panel.

### Task 8: Implement panel.rs shared layout

**Files:**
- Modify: `src/tui/panel.rs`

- [ ] **Step 1: Implement PanelState and render_panel**

```rust
use ratatui::prelude::*;
use ratatui::widgets::*;
use super::widgets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    Table,
    Detail,
}

/// Render a two-panel layout: table on left, detail on right.
/// Returns the areas used for table and detail, for the caller to render content into.
pub fn render_panel_frame(
    frame: &mut Frame,
    area: Rect,
    headers: &[(&str, u16)],  // (header_name, column_width)
    focus: PanelFocus,
    title: &str,
) -> (Rect, Rect) {
    let [table_area, detail_area] = Layout::horizontal([
        Constraint::Percentage(55),
        Constraint::Percentage(45),
    ]).areas(area);

    // Table header
    let header_cells: Vec<Cell> = headers.iter()
        .map(|(name, _)| Cell::from(*name).style(Style::new().add_modifier(Modifier::BOLD)))
        .collect();
    let header_row = Row::new(header_cells)
        .style(Style::new().fg(Color::Cyan))
        .bottom_margin(1);

    // Render table border
    let table_block = Block::bordered()
        .title(title)
        .border_style(widgets::focus_border(focus == PanelFocus::Table));
    frame.render_widget(table_block, table_area);

    // Render detail border
    let detail_block = Block::bordered()
        .border_style(widgets::focus_border(focus == PanelFocus::Detail));
    frame.render_widget(detail_block, detail_area);

    // Return inner areas for content rendering
    let table_inner = table_area.inner(Margin::new(1, 1));
    let detail_inner = detail_area.inner(Margin::new(1, 1));
    (table_inner, detail_inner)
}

/// Handle arrow key navigation for a panel.
/// Returns true if the key was consumed.
pub fn handle_panel_nav(
    focus: &mut PanelFocus,
    selected: &mut usize,
    item_count: usize,
    code: crossterm::event::KeyCode,
) -> bool {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Up => {
            if *focus == PanelFocus::Table && item_count > 0 {
                *selected = if *selected == 0 { item_count - 1 } else { *selected - 1 };
            }
            true
        }
        KeyCode::Down => {
            if *focus == PanelFocus::Table && item_count > 0 {
                *selected = (*selected + 1) % item_count;
            }
            true
        }
        KeyCode::Right | KeyCode::Enter => {
            if *focus == PanelFocus::Table {
                *focus = PanelFocus::Detail;
            }
            true
        }
        KeyCode::Left | KeyCode::Esc => {
            if *focus == PanelFocus::Detail {
                *focus = PanelFocus::Table;
                true
            } else {
                false // Let caller handle (e.g., close form, quit)
            }
        }
        _ => false,
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/tui/panel.rs
git commit -m "feat: shared panel layout with table+detail, arrow key navigation"
```

### Task 9: Convert steps tab to table layout

**Files:**
- Modify: `src/tui/tabs.rs` — rewrite `render_steps()` to use panel and show columns

- [ ] **Step 1: Rewrite render_steps to use panel + table**

Replace the current list rendering with a table showing columns: Label, Function, Input, Output, Pattern.

Use `ratatui::widgets::Table` with `Row` items built from step data:
- Label: `step.label()`
- Function: `step.step_type()` (capitalize via `meta::find_step_type`)
- Input: `step.def.input_col` or "(working)"
- Output: display from `step.def.output_col` — `Single(name)` → name, `Multi(map)` → comma-separated keys, `None` → "—"
- Pattern: truncated `step.def.pattern` or table name

Use `panel::render_panel_frame()` for the layout. The detail panel on the right shows the step form (existing form rendering).

- [ ] **Step 2: Verify it compiles and renders correctly**

Run: `cargo build && cargo run -- configure --config /tmp/test-addrust.toml`
Visual check: steps show as a table with columns.

- [ ] **Step 3: Commit**

```bash
git add src/tui/tabs.rs
git commit -m "feat: steps tab as table with Label/Function/Input/Output/Pattern columns"
```

### Task 10: Convert dict and output tabs to table layout

**Files:**
- Modify: `src/tui/tabs.rs` — rewrite `render_dict()` and `render_output()`

- [ ] **Step 1: Rewrite render_dict as table**

Columns: Short, Long, Variants (count), Status.
Use `panel::render_panel_frame()`. Detail panel shows variant list with add/delete.

- [ ] **Step 2: Rewrite render_output as table**

Columns: Component, Format, Example.
Use `panel::render_panel_frame()`. Detail panel shows format picker.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Manual smoke test**

Run: `cargo run -- configure --config /tmp/test-addrust.toml`
Verify all three tabs render as tables with right panel detail.

- [ ] **Step 5: Commit**

```bash
git add src/tui/tabs.rs
git commit -m "feat: dict and output tabs as tables with shared panel layout"
```

### Task 11: Final cleanup and remove TABLE_DESCRIPTIONS

**Files:**
- Modify: `src/tui/tabs.rs` or `src/tui/meta.rs`

- [ ] **Step 1: Move TABLE_DESCRIPTIONS to meta.rs or derive from table registry**

If the table registry (Abbreviations) can provide names and descriptions, derive from it. Otherwise, move the constant to `meta.rs` as the single source.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Run golden tests**

Run: `cargo test golden`
Expected: PASS. If any golden tests fail due to field renames, update expected outputs.

- [ ] **Step 4: Commit**

```bash
git add src/tui/ tests/
git commit -m "cleanup: move TABLE_DESCRIPTIONS to meta.rs, final test fixes"
```
