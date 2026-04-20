# Config Override Redesign

## Problem

The config override system has multiple independent merge/patch/load paths that silently drop data. Bugs found in a single session:

1. `patch()` uppercased tags, causing tag lookups to fail and producing empty regex alternations that hang the parser
2. `patch()` merged unrelated groups sharing a long form (HWY and GA HIGHWAY both expand to HIGHWAY, got merged into one group)
3. TUI load path didn't merge tags from config overrides into default entries
4. Canonical overrides with empty tags wiped existing tags from defaults
5. TUI change detection drifted from actual state due to "original" values being overwritten during merge

Root cause: the system tries to merge partial diffs on top of defaults, with separate merge logic in `patch()` and the TUI loader. Each merge path can silently drop fields.

## Design Decisions

1. **Config entries are complete rows, not diffs.** A config entry for MT includes short, long, variants, and tags. No field falls back to defaults.
2. **Canonical entries fully replace defaults.** No merge logic. The config entry IS the group.
3. **Non-canonical adds with the same long form are separate groups.** HWY -> HIGHWAY and GA HIGHWAY -> HIGHWAY are distinct entries.
4. **Drop the `canonical` field.** The system infers replacement vs addition by checking whether the short form exists in the defaults.
5. **Change detection compares against the default baseline**, not last-saved config.
6. **Single `patch()` function used everywhere.** Pipeline and TUI call the same code. The TUI wraps the result with status metadata.
7. **Duplicate short/long forms rejected in TUI.** Adding an entry with a short or long form that already exists in the table produces an error.

## New `patch()` Semantics

`AbbrTable::patch(overrides)`:

1. Clone default groups
2. Build a lookup set of default short forms
3. For each entry in `overrides.remove`: remove any group where short, long, or variant matches (case-insensitive)
4. For each entry in `overrides.add`:
   - If short form matches a default group -> replace that group entirely
   - Otherwise -> append as a new group
5. Return new table via `from_groups()`

No merge logic. No long-form matching. No canonical branching. Tags, variants, long form all come from the config entry as-is.

## TUI Load Path

1. Load raw defaults via `load_default_tables()`
2. Call `patch()` with config overrides -> final `AbbrTable`
3. Build `DictGroupState` from patched groups
4. Determine status for each group:
   - Short form in defaults AND identical to default -> `Default`
   - Short form in defaults AND differs -> `Modified`
   - Short form not in defaults -> `Added`
5. For removed entries: check the config's `remove` list against the raw defaults. For each default group that was removed, add a `DictGroupState` with `Removed` status (these groups aren't in the patched table, so the TUI re-creates them from defaults for display).
6. Store the **default version** of each group as `original_*` fields. For added entries, `original_*` copies the current values.

## TUI Save Path (`to_config()`)

Compare each entry against its default baseline:

- `Default` -> don't write (unchanged from defaults)
- `Modified` -> write full entry to `add` list
- `Added` -> write full entry to `add` list
- `Removed` -> write short form to `remove` list

No `canonical` field emitted. The system infers intent from short-form matching at load time.

## Duplicate Validation

On dict panel close (adding or editing an entry), check the short form and long form against all other entries in the current table. If either collides, reject the save with a status bar error. An entry being edited excludes itself from the collision check.

## Backward Compatibility

Existing config files with `canonical = true` continue to parse. The field is ignored via `#[serde(default)]` — the system determines replacement vs addition from short-form matching.

## Tag and Case Handling

- **Tags**: preserved exactly as written. No case transformation. Tags are semantic labels referenced by exact match in pattern templates (`{street_name:start}`).
- **Short/long**: uppercased during `AbbrGroup` construction (address components).
- **Variants**: preserved as-is (may contain regex patterns where case matters).

## Files Changed

### `src/tables/abbreviations.rs`
- Rewrite `AbbrTable::patch()` with replace-not-merge semantics
- Remove canonical-specific branches
- Update tests for new semantics

### `src/config.rs`
- Remove `canonical` field from `DictEntry`
- Update tests that reference `canonical`

### `src/tui/mod.rs`
- Rewrite `App::new()` dict loading: call `patch()`, then compute status by comparing against defaults
- Rewrite `to_config()`: compare against default baseline, write full entries
- Remove existing merge logic in load path

### `src/tui/panel.rs`
- Add duplicate validation on panel close (check short/long against other entries in table)

### `src/pipeline.rs`
- No changes. Already calls `patch()`, gets new semantics automatically.

### `src/step.rs`
- No changes. Pattern expansion reads from patched table.
