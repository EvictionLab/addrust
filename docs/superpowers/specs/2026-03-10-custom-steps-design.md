# Custom Steps Design

## Problem

Users can disable, reorder, and override patterns on existing pipeline steps, but cannot add entirely new steps. When the default steps don't cover a pattern — like `\bBOX (\d+)` for digit-only PO Box extraction alongside the broader default `\w+` pattern — there's no way to fill the gap without modifying the embedded defaults.

## Design

Custom steps are first-class pipeline steps. They use the same `StepDef` structure, compile through `compile_step()`, and execute via `apply_step()`. No new step types or execution paths.

### Remove the Validate Step Type

The `validate` type is removed. The sole validate step (`na_check`) becomes a rewrite with empty replacement:

```toml
[[step]]
type = "rewrite"
label = "na_check"
pattern = '(?i)^(N/?A|{na_values})$'
replacement = ''
```

Since the pattern is anchored to the full string, replacing the match with `''` empties the working string — same effect as the old validate+clear behavior. Warnings are dropped; empty output is sufficient signal that the input was junk.

This reduces the `Step` enum from 4 variants to 3: **Rewrite**, **Extract**, **Standardize**. The `Validate` variant, `warning` field, and `clear` field are removed from `Step`, `StepDef`, `compile_step()`, and `apply_step()`.

### Config

New `custom_steps` field on `StepsConfig`. Each entry is a `StepDef` — same schema as entries in `data/defaults/steps.toml`. `StepsConfig` holds the `Vec<StepDef>` directly (not wrapped in a `StepsDef`).

```toml
[steps]
disabled = ["suffix_all"]
step_order = ["na_check", "po_box", "custom_po_box_digits", "street_number", "..."]

[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)'
target = "po_box"
skip_if_filled = true
```

Labels must not collide with default step labels or other custom step labels.

Custom steps appear in `step_order` by label alongside defaults. If `step_order` is not set, custom steps append after all defaults in the order they appear in the config.

### Pipeline

`from_steps_config()` changes:

1. Compile default steps as before.
2. Compile custom steps from `config.steps.custom_steps` using the same `compile_step()`. Use graceful error handling (log warning and skip) rather than panicking on invalid custom step definitions — users may hand-edit the TOML and introduce typos.
3. Merge custom steps into the step list.
4. Apply `step_order` for final ordering (unchanged logic).
5. Apply `disabled` list (unchanged logic).
6. Apply `pattern_overrides` (unchanged logic — works on custom steps by label too).

### Patterns

Custom step patterns go through `expand_template()` like defaults. Users can reference any loaded dictionary with `{table_name}` or `{table_name$short}` placeholders. User-added dictionary entries are available in these expansions since tables are built before step compilation.

### Target Fields

Custom steps target existing `Field` variants only: `street_number`, `pre_direction`, `street_name`, `suffix`, `post_direction`, `unit`, `unit_type`, `po_box`, `building`, `extra_front`, `extra_back`. New output columns are out of scope.

`parse_field()` must return `Result` instead of panicking, so that invalid target names in hand-edited TOML produce a graceful error rather than a crash.

### TUI Interaction

**Adding a step:** On the Steps tab, press `a` to start the guided wizard. The new step inserts after the currently selected step.

**Wizard flow** — sequential prompts, branching by type:

1. **Pick type:** extract / rewrite / standardize (list selection)
2. **Type-specific prompts:**
   - **Extract:** pattern → target field (pick from list) → skip_if_filled? (y/n) → replacement? (optional post-extraction transformation, skip with Enter)
   - **Rewrite:** pattern → replacement text or table name (prompt asks which)
   - **Standardize:** target field → choose approach (pattern+replacement or matching_table+format_table) → if table-based, also pick mode (whole_field or per_word) → fill in chosen fields
3. **Label:** auto-suggested from type and target (e.g., `custom_extract_po_box`), editable before confirming.

Note: The extract wizard prompts for `pattern` only (not `table` reference). Extract steps that derive their pattern from a table's `pattern_template` are a power-user scenario handled by editing the TOML directly.

Pattern input uses the existing `EditPattern` mode with real-time regex validation and `{table}` expansion preview.

**After creation:** The new step appears in the step list at the insertion point. It can be moved (`m`), disabled (`Space`), pattern-edited (`e`), and deleted (`d`) like any step. Deleting a custom step removes it from `custom_steps` in config; deleting a default step just disables it.

**Delete behavior:** Pressing `d` on a default step does nothing (use `Space` to disable). Pressing `d` on a custom step prompts for confirmation, then removes it from `custom_steps` and `step_order` in config.

**Visual distinction:** Custom steps show a marker (e.g., `[+]` prefix or distinct color) so users can tell them apart from defaults.

### How `is_custom` Flows to the TUI

The TUI infers `is_custom` by checking whether a step's label exists in the default steps list. No changes to the `Step` enum needed — the TUI already builds `StepState` from `StepSummary` and has access to the default pipeline's summaries for comparison. A label present in the config pipeline but absent from defaults is custom.

### Validation

- Label uniqueness enforced at creation time (no collisions with defaults or other custom steps).
- Pattern must compile as valid regex after template expansion.
- Extract and standardize steps must have a valid target field.
- Type-specific required fields enforced by the wizard flow (same rules as `compile_step()`).

### What Changes

| File | Change |
|------|--------|
| `src/config.rs` | Add `custom_steps: Vec<StepDef>` to `StepsConfig` with `skip_serializing_if = "Vec::is_empty"`. Update `is_empty()` to check `custom_steps`. |
| `src/step.rs` | Remove `Validate` variant from `Step` enum. Remove `warning` and `clear` fields from `StepDef`. Add `Serialize` derive to `StepDef`. Change `parse_field()` to return `Result` instead of panicking. |
| `src/pipeline.rs` | `from_steps_config()` compiles and merges custom steps with graceful error handling. |
| `src/tui.rs` | Add wizard mode, `a` keybinding, `d` for delete (custom only), `[+]` marker, `StepState.is_custom` field inferred from default label set. |
| `data/defaults/steps.toml` | Change `na_check` from `type = "validate"` to `type = "rewrite"` with `replacement = ''`. Remove `warning` and `clear` fields. |

### What Doesn't Change

- `apply_step()` — no new execution logic (validate removal simplifies it)
- `compile_step()` — already handles all remaining step types from `StepDef` (validate branch removed)
- `expand_template()` — already expands any `{table}` reference
