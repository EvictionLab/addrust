# Step Reordering in TUI — Design Spec

## Overview

Add the ability to reorder pipeline steps in the TUI's Steps tab. Users enter a "move mode" on a selected step, then use arrow keys to physically reposition it in the list. The new order persists to config and is respected by the pipeline at parse time.

## Interaction Model

### Entering Move Mode

Press `m` on the currently selected step in the Steps list view (not the detail view). The step is "grabbed" and ready to move. Both enabled and disabled steps can be moved.

### While in Move Mode

- `↑`/`↓` (and `j`/`k`) — swap the grabbed step with its neighbor in that direction. Clamps at boundaries (no-op when moving the first step up or last step down — no wrapping)
- `Enter` — confirm the new position, exit move mode, mark config dirty
- `Esc` — cancel, return step to its original position, exit move mode

### Visual Feedback

Two simultaneous indicators:

1. **Row highlight** — the grabbed step's row renders in a distinct style (yellow foreground or bold) to visually separate it from the rest of the list
2. **Status bar** — footer updates to show move-mode keybindings: `↑↓: move | Enter: confirm | Esc: cancel`

## Guardrails

None. Fully free-form reordering. The user can place any step at any position. No group constraints or structural warnings. Users can reset to defaults if they break something.

## Persistence

### Config Format

`StepsConfig` gains a new optional field `step_order`:

```toml
[steps]
disabled = ["na_check"]
step_order = [
  "city_state_zip",
  "po_box",
  "street_number",
  # ... all labels in user's desired order
]
```

### Serialization Rules

- `step_order` is `Vec<String>` — an ordered list of step labels
- Only written to config when order differs from the default step order
- When order matches the default, `step_order` is omitted entirely (minimal config)

### Forward/Backward Compatibility

- **New default steps** (label not in `step_order`): appended at the end of the ordered list
- **Removed steps** (label in `step_order` but not in defaults): silently ignored

## Pipeline Integration

`Pipeline::from_steps_config()` already compiles default steps and applies disabled/pattern overrides. It gains one additional phase:

1. Compile default steps from `steps.toml` (expands templates, compiles regexes)
2. Apply `pattern_overrides` to step defs before compilation (existing)
3. **New: if `step_order` is non-empty, reorder compiled steps to match**
4. Apply `disabled` list — sets `enabled = false` (existing, order-independent)

Reordering happens on compiled steps (post-override). Disabled steps are still reordered into position — they just won't execute. This preserves the user's visual ordering in the TUI.

The reorder operation:
- Build a position map from `step_order` labels
- Sort compiled steps by their position in `step_order`
- Steps not found in `step_order` are appended at the end (preserving their relative default order)

## TUI State Changes

### New Fields on `App`

```rust
moving_step: Option<usize>,      // index of step being moved (None = not in move mode)
moving_step_origin: Option<usize>, // original index for Esc cancel
```

### State Transitions

| State | Key | Action |
|-------|-----|--------|
| Normal | `m` | Set `moving_step = Some(selected)`, `moving_step_origin = Some(selected)` |
| Moving | `↑`/`k` | Swap `steps[i]` with `steps[i-1]`, update selection to `i-1` |
| Moving | `↓`/`j` | Swap `steps[i]` with `steps[i+1]`, update selection to `i+1` |
| Moving | `Enter` | Clear `moving_step`/`moving_step_origin`, set `dirty = true` |
| Moving | `Esc` | Remove step from current index, re-insert at `moving_step_origin`, clear both fields |

### Rendering

- When `moving_step.is_some()`, the step at that index renders with the move-mode style
- Footer/status bar shows move-mode keybindings instead of normal keybindings
- All other keys are ignored while in move mode (no tab switching, no detail view, no toggle)

## Config Round-Trip

### `app.to_config()`

Two changes from the current positional-zip approach:

1. **Pattern overrides:** Currently `to_config()` zips `self.steps` with `default_summaries` by position to detect pattern changes. With reordering, this zip would pair mismatched steps. Instead, build a `HashMap<&str, &str>` from default labels → default patterns, then compare each step's pattern by label lookup.

2. **Step order:** Compare current `steps` label order against the default label order. If they differ, populate `step_order` with the current label sequence. If they match, leave `step_order` empty.

### `App::new()` (loading from config)

Currently zips `config_summaries` with `default_summaries` by position to set `default_enabled`. With reordering, these may be in different orders. Instead, build a `HashMap<&str, bool>` from default labels → default enabled status, then look up each config summary's label to set `default_enabled`.

The pipeline already returns steps in the configured order (since `from_steps_config` applies `step_order`), so the `StepState` vec will reflect the user's order.

## Files to Modify

| File | Change |
|------|--------|
| `src/config.rs` | Add `step_order: Vec<String>` to `StepsConfig`, serde handling, update `is_empty()` |
| `src/pipeline.rs` | Apply `step_order` reordering in `from_steps_config()` |
| `src/tui.rs` | Move mode state, keybindings, rendering, `to_config()` order diff |
