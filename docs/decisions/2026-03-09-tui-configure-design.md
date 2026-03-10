# TUI Configuration Editor (`addrust configure`)

## Summary

An interactive terminal editor for `.addrust.toml`, built with ratatui + crossterm. Two top-level tabs: Rules and Dictionaries. Users can toggle rules on/off and patch dictionary tables (add/remove/override entries). Saves only the diff from defaults.

## Scope

**In scope:**
- View rules: label, group, action type, enabled/disabled
- Toggle rules enabled/disabled
- Browse dictionary tables and entries
- Add, remove, and override dictionary entries
- Save diff-only `.addrust.toml`

**Not in scope (future phases):**
- Rule reordering
- Custom user-defined rules
- Live preview of parsing results
- Viewing compiled regex patterns

## Layout

### Tab 1: Rules

A scrollable list of all pipeline rules in order. Each row shows:

```
[ ] change_na_address              na_check     Warn
[x] city_state_zip                 city_state   Extract
```

- `[ ]` = enabled, `[x]` = disabled
- Columns: status, label, group, action

### Tab 2: Dictionaries

Sub-tabs for each table name (all_suffix, common_suffix, direction, state, unit_location, unit_type, usps_suffix). Selected sub-tab shows entries:

```
  SHORT                LONG
  ST                   STREET
  AVE                  AVENUE
+ PSGE                 PASSAGE        (added)
- TRAILER              TRAILER PARK   (removed)
~ STE                  SUITE NUMBER   (overridden, was SUITE)
```

Markers (`+`, `-`, `~`) indicate pending changes not yet saved.

## Keybindings

### Global
- `Tab` / `Shift-Tab` — switch top-level tabs (Rules / Dictionaries)
- `s` — save to `.addrust.toml`
- `q` / `Esc` — quit (prompt if unsaved changes)

### Rules tab
- `j` / `k` / arrows — navigate
- `Space` / `Enter` — toggle enabled/disabled

### Dictionaries tab
- `1`-`7` or left/right arrows — switch sub-tab (table)
- `j` / `k` / arrows — navigate entries
- `a` — add new entry (inline prompt for short and long forms)
- `d` / `Delete` — mark/unmark entry for removal
- `Enter` — edit entry's long form (inline edit)

## Save Format

Diff-only. The file contains only what differs from defaults:

```toml
[rules]
disabled = ["city_state_zip"]

[dictionaries.all_suffix]
add = [{ short = "PSGE", long = "PASSAGE" }]
remove = ["TRAILER"]

[dictionaries.unit_type]
override = [{ short = "STE", long = "SUITE NUMBER" }]
```

If nothing is changed, no file is written (or existing file is deleted). This keeps the config as a patch, not a snapshot.

## Data Flow

```
Launch:
  load .addrust.toml (if exists) → Config
  build_default_tables() → Abbreviations
  apply Config patches → patched Abbreviations
  build_rules(patched) → rules with enable/disable applied
  → TUI state: rules list + table entries + pending changes

Edit:
  user toggles/edits → pending changes tracked in TUI state
  changes shown with markers (+/-/~)

Save:
  pending changes → Config struct → serialize as diff-only TOML → write .addrust.toml

Quit:
  if unsaved changes → prompt save/discard/cancel
```

## Dependencies

- `ratatui` — TUI framework
- `crossterm` — terminal backend

## Architecture

- `src/tui.rs` — main TUI module: app state, event loop, rendering
- `src/tui/` could be split into submodules if it grows (rules_view, dict_view, etc.) but start as one file

The TUI reads from and writes to the existing `Config` struct. No new data structures needed for serialization — just need to add TOML serialization (serde `Serialize`) to the config types, and a method to compute the diff between current state and defaults.
