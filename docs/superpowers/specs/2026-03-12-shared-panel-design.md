# Shared Panel Design

## Overview

Replace the dead code in `panel.rs` with a shared panel system used by both the Steps and Dictionary tabs. The panel is a centered auto-height overlay that opens when the user presses Enter on a table row. It provides a single-column field editor with two interaction modes: inline edit for single values and dropdown for multi-row content.

## Goals

- Consistent editing experience across Steps and Dictionary tabs
- Auto-height overlay sized to content (not fixed 80%x80%)
- Inline editing for simple fields, dropdown for lists
- Step type selector with Left/Right cycling
- Checkbox-based enable/disable for list items (pattern groups, variants)
- Extract form code from `tabs.rs` into `panel.rs`

## Panel Frame

Shared renderer for both step and dictionary panels:

- **Position**: centered horizontally, vertically centered
- **Width**: 70% of terminal width (clamped to min 50, max 100 columns)
- **Height**: computed from content — header (3 lines) + field rows (1 line each) + expanded dropdown rows + footer (2 lines) + border (2 lines). Clamped to terminal height minus 4.
- **Header**: title (step label or dict entry short form) left, status (DEFAULT/CUSTOM, MODIFIED indicator) right
- **Body**: single-column rows, each row is `label` left-aligned + `value` right-aligned
- **Footer**: context-sensitive keybinding hints that change based on current focus/mode

## Field Editing Modes

### Inline edit

For single-value fields. Enter activates a cursor in the value position on the same row. The value text is replaced by an editable text with a block cursor. Enter confirms, Esc cancels.

Inline fields (steps): Label, Replacement, Input Column
Inline fields (dictionary): Short form, Long form

Toggle fields (steps): Skip if filled (Space toggles yes/no)

### Dropdown

For multi-row content. Enter on the field expands a list below the row. The list is indented with a thin left border. Items have `[x]`/`[ ]` checkboxes toggled with Space. Other fields remain visible below the expanded dropdown. Esc collapses.

Dropdown fields (steps):
- **Pattern**: alternation groups with `[x]`/`[ ]` checkboxes. Space toggles a group. Enter on a group opens inline text edit for that alternative.
- **Output**: when `OutputCol::Multi`, shows capture group → column mappings. Enter on a mapping edits the column name inline. When `OutputCol::Single`, this is an inline edit field instead.
- **Table**: list of available tables from `TABLE_DESCRIPTIONS`. Shows `[x]` for selected, `[ ]` for others. Space to select (single-select).

Dropdown fields (dictionary):
- **Variants**: list with `[x]`/`[ ]` checkboxes. Space toggles. `a` adds a new variant (inline text entry appended to list). Enter on a variant edits its text inline.

## Step Type Selector

- Displayed at top of step panel body, above the field rows
- Shows all types from `meta::STEP_TYPES`: extract, rewrite, standardize
- Left/Right arrows cycle selection, current type highlighted
- Changing type recomputes `visible_fields` from `StepTypeMeta.visible` — fields appear/disappear dynamically
- Works on both new and existing steps
- The step type is determined from `FormState.def.step_type` (already stored as a string)

## Restore to Default

- Available on existing default steps (not custom-added steps)
- Keybinding `r` resets all fields to their original default values
- Resets step type as well

## Step Panel Fields

Visible fields determined by step type via `meta::STEP_TYPES`:

| Field | extract | rewrite | standardize |
|-------|---------|---------|-------------|
| Label | yes | yes | yes |
| Pattern | yes | yes | yes |
| Table | yes | yes | yes |
| Output | yes | — | yes |
| Skip if filled | yes | — | — |
| Replacement | yes | yes | yes |
| Input Col | yes | yes | — |
| Mode | — | — | yes |

## Dictionary Panel Fields

- Short form (inline edit)
- Long form (inline edit)
- Variants (dropdown with checkboxes)

## State Changes

### `FormFocus` (simplified)

Current: `Left`, `RightPattern`, `RightOutputCol`, `RightTable`, `EditingText(name, cursor, text)`

New:
- `Navigating` — cursor on field list, up/down to move
- `InlineEdit { cursor: usize, buffer: String }` — editing a single-value field in place
- `Dropdown { cursor: usize }` — navigating items in an expanded dropdown
- `DropdownEdit { item: usize, cursor: usize, buffer: String }` — editing an item within a dropdown

The field being edited/expanded is always `visible_fields[field_cursor]`, so it doesn't need to be stored in the focus enum.

### `InputMode` removal

The dictionary's `InputMode` variants (`AddShort`, `AddLong`, `EditLong`, `EditVariants`, `AddVariant`) are replaced by the panel system. `InputMode` can be removed or reduced to just the non-dict modes if any remain.

### `FormState` reuse

`FormState` stays mostly the same but is used for both steps and dictionary entries. For dictionary entries, `def` is not used — instead a new `DictFormState` or a `PanelKind` enum distinguishes the two:

```
enum PanelKind {
    Step(FormState),       // existing FormState with StepDef
    Dict(DictFormState),   // short, long, variants, field_cursor, focus
}
```

## File Changes

- **`panel.rs`**: gutted and rewritten — `PanelKind`, shared panel frame renderer (`render_panel`), inline edit logic, dropdown logic, field rendering, key handling (`handle_panel_key`)
- **`tabs.rs`**: remove all `render_step_form`, `render_form_left_panel`, `render_form_right_panel`, `render_form_*` functions and `handle_form_key`. Step/dict rendering calls `panel::render_panel`. Significant line reduction (~500-700 lines removed).
- **`mod.rs`**: `App.form_state` replaced by `App.panel: Option<PanelKind>`. `FormFocus` simplified as above. `InputMode` dict variants removed.

## Navigation Summary

| Context | Up/Down | Left/Right | Enter | Esc | Space | r |
|---------|---------|------------|-------|-----|-------|---|
| Field list | move cursor | cycle type (on type row) | edit/expand | close panel | toggle (booleans) | restore default |
| Inline edit | — | move cursor in text | confirm | cancel | — | — |
| Dropdown | navigate items | — | edit item text | collapse | toggle checkbox | — |
| Dropdown edit | — | move cursor in text | confirm | cancel | — | — |
