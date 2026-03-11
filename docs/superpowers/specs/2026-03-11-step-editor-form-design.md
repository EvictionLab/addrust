# Step Editor Form — Design Spec

## Problem

The TUI's step wizard and editor have three issues:

1. **Adding a step is a blind linear flow.** Each screen is a popup with no context about what comes next or what you've already chosen. New users don't know how many screens remain.
2. **Editing is pattern-only.** Once a step is created, you can only edit the regex pattern. Target, replacement, table, skip_if_filled, and label are all locked.
3. **Add vs edit asymmetry.** The wizard sets all fields, but edit mode only exposes one.

Additionally, the `prepare.rs` pre-cleaning rules are hardcoded and not visible or editable in the TUI.

## Design

Replace the linear wizard popup and the current detail view with a **unified two-panel form** that works for both adding and editing any step.

### Layout

**Left panel (44%):** All fields for the step, listed as rows. Navigate with j/k, edit simple fields inline (Space to toggle, Enter for text input). The currently selected field is highlighted with a cursor marker.

**Right panel (56%):** Adapts based on which field is selected:
- **Complex fields** (pattern, targets, table): Shows the interactive editor — pattern drill-down, target field picker, table picker.
- **Simple fields** (skip_if_filled, replacement, source, label): Shows contextual help explaining what the field does, its syntax, and current value.

### Step Types and Their Fields

Fields shown adapt to the step type. Irrelevant fields are hidden, not grayed out.

| Field | Extract | Rewrite | Standardize |
|-------|---------|---------|-------------|
| Pattern | yes | yes | optional (pattern-based only) |
| Target mode | yes (single/multi) | — | — |
| Target / Targets | yes | — | yes (single only) |
| Skip if filled | yes | — | — |
| Replacement | yes (optional) | yes (if not table-driven) | required if pattern-based (defaults to empty) |
| Table | — | yes (if table-driven) | yes (if table-based) |
| Source | yes | yes | — |
| Mode | — | — | yes (whole field / per word) |
| Label | yes | yes | yes |

### Header

Top of the form shows:
```
TYPE: EXTRACT     DEFAULT STEP  ● MODIFIED
```

- Type is shown but not editable (locked at creation).
- "DEFAULT STEP" or "CUSTOM STEP" indicates origin.
- "● MODIFIED" appears when any field on a default step differs from its default value.

### Adding a New Step

1. Press `a` in the step list → pick step type (extract / rewrite / standardize) from a 3-item popup.
2. Land in the two-panel form with empty/default values. Fields are pre-populated with sensible defaults (e.g., skip_if_filled defaults to false, source defaults to working string).
3. Fill in fields in any order — no forced sequence.
4. Label auto-generates from type + target (e.g., `custom_extract_unit`) but is editable.
5. Esc to finish. Validates required fields before closing (see Validation below). If required fields are missing, shows a confirmation prompt: "Discard incomplete step? (y/n)".

### Validation

Required fields per step type (matching constraints in `compile_step`):

| Step Type | Required Fields |
|-----------|----------------|
| Extract | pattern, and exactly one of target or targets |
| Rewrite | pattern, and exactly one of replacement or table |
| Standardize | target, and either (pattern + replacement) or table |

When Esc is pressed:
- **New step, all required fields set**: step is created, form closes.
- **New step, missing required fields**: confirmation prompt to discard.
- **Existing step**: changes apply to in-memory state immediately (persisted to disk only on Ctrl+S from main screen, same as all other TUI changes).

### Editing an Existing Step

Press Enter on any step in the step list → opens the two-panel form pre-populated with the step's current values. All fields are editable.

For **default steps**, each field tracks whether it differs from the default:
- Modified fields show a `*` marker in the left panel and "[modified]" in the right panel.
- The right panel shows both current and default values for modified fields.
- Press `r` on a modified field to reset it to default.
- Changes to default steps are persisted as config overrides (pattern_overrides for pattern, custom_steps entry for other field changes).

### Pattern Drill-Down

When the Pattern field is selected, the right panel shows the parsed pattern segments:
- **Literals**: gray text (not selectable)
- **Table references**: `{table_name}` in cyan, shows value count
- **Alternation groups**: shows enabled/disabled count, Enter to expand

When drilled into an alternation group:
- **Space**: toggle an alternative on/off
- **a**: add a new alternative (text input, same as dictionary variant add)
- **d**: delete the selected alternative
- **e**: edit raw regex (full pattern text input)
- **Esc**: close the drill-down

These keybindings match the dictionary variant editor for consistency.

### Target Picker

When Targets is selected in multi-target mode, the right panel shows all 11 fields:
- `[ ]` = not assigned
- `[N]` = assigned to capture group N
- **Space**: toggle assignment (prompts for group number on first assign)
- **1-9**: directly set capture group number
- **d**: remove assignment
- Navigate with j/k within the picker

### Contextual Help

When a simple field is selected, the right panel shows a description. Content per field:

- **Skip if filled**: "When yes, this step is skipped if the target field(s) already have a value from a previous step. Use this for extraction steps that should only fire once."
- **Replacement**: "Text that replaces the matched pattern. Supports backreferences: `$1` (capture group), `${N:table}` (table lookup), `${N/M:fraction}` (fraction expansion)."
- **Source**: "Which text this step operates on. 'working string' is the main address being parsed. Selecting a field (e.g., 'unit') makes the step operate on that extracted field instead."
- **Table**: "The abbreviation table used for lookups. For rewrite steps, matched text is looked up in this table. For standardize steps, the target field value is standardized against this table."
- **Mode**: "How standardization is applied. 'Whole field' standardizes the entire field value as one lookup. 'Per word' splits on spaces and standardizes each word independently."
- **Label**: "Unique identifier for this step. Used in config files for overrides, ordering, and disable lists."

### Prepare Rules Migration

The hardcoded `prepare.rs` rules become regular rewrite steps at the top of `steps.toml`, in a "prepare" group. They are visible and editable in the TUI like any other step.

Each `PrepRule` maps directly to a rewrite `StepDef`:
- `pattern` → `pattern`
- `replacement` → `replacement`
- `source` → None (operates on working string)

The `prepare()` function is replaced by the pipeline running these steps first. The `squish` and `to_uppercase` operations remain as fixed pre-processing that runs before any steps (not configurable — they're structural, not domain-specific). The migrated rewrite steps assume input is already uppercased. Each rewrite step already calls `replace_all` and `squish` internally, so no new semantics are needed.

The 11 target fields available in the target picker are: street_number, pre_direction, street_name, suffix, post_direction, unit, unit_type, po_box, building, extra_front, extra_back.

### Data Model Changes

`StepState` needs to carry the full `StepDef` so the form can read and write all fields:

```rust
struct StepState {
    enabled: bool,
    default_enabled: bool,
    is_custom: bool,
    def: StepDef,                  // full definition — source of truth for all fields
    default_def: Option<StepDef>,  // original default for reset (None for custom steps)
}
```

Display-only fields (`label`, `group`/step type, `action_desc`, `pattern_template`) are derived from `def` on demand rather than cached separately, avoiding drift. The `custom_step_defs` HashMap is no longer needed — `StepState.def` replaces it.

### Config Persistence

Introduce a `step_overrides` config section — a label-keyed map of partial `StepDef` overrides, generalizing `pattern_overrides` to all fields. This preserves default step identity so future addrust updates to defaults flow through for non-overridden fields.

```toml
[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
skip_if_filled = false

[steps.step_overrides.unit_type_value]
targets = { unit_type = 1, unit = 2, building = 3 }
```

When saving, for each step:
- **Custom steps**: Serialize `def` directly into `custom_steps` array.
- **Default steps with modifications**: Diff `def` against `default_def`. Write only changed fields into `step_overrides.<label>`. Pattern-only changes can also be written here (deprecating the separate `pattern_overrides` section, but still reading it for backward compat).
- **Default steps with no modifications**: No config output needed.

The `pattern_overrides` section is kept for backward compatibility (read but not written). New saves use `step_overrides` exclusively.

### Keybinding Summary

In the step form (left panel):
- **j/k**: navigate fields
- **Enter**: open editor in right panel (for complex fields) or start inline edit (for text fields)
- **Space**: toggle boolean fields inline
- **r**: reset field to default (default steps only, modified fields only)
- **Esc**: close form, return to step list

In the right panel (complex editors):
- Pattern drill-down: **j/k** navigate, **Space** toggle, **a** add, **d** delete, **e** edit raw, **Esc** close
- Target picker: **j/k** navigate, **Space** toggle, **1-9** set group, **d** remove, **Esc** close
- Table picker: **j/k** navigate, **Enter** select, **Esc** close

### Scope Boundaries

**In scope:**
- Two-panel form for add and edit
- All fields editable for all steps (custom and default)
- Modified markers + per-field reset for default steps
- Pattern drill-down with add/delete alternatives
- Contextual help for simple fields
- Prepare rules migration to steps.toml

**Out of scope (future work):**
- Live preview of step effect on a sample address
- Undo/redo within the form
- Drag-and-drop reordering (existing move mode with `m` is retained)
