# Future: TUI and Rule Reordering

Design notes for features deferred from the initial config system.

## Interactive TUI (`addrust configure`)

An interactive terminal editor for `.addrust.toml`, built with ratatui + crossterm.

### Pipeline Rules View
- List all rules in pipeline order: label, group, enabled/disabled
- Toggle enable/disable with Enter or Space
- (Future) Move rules up/down: press Enter to grab, arrow keys to move, Enter to drop
- Group rules visually by group name

### Dictionary Tables View
- Browse tables by name
- View entries (short ↔ long)
- Add new entries inline
- Remove entries with delete key
- Override: edit the long form of an existing entry

### Workflow
- On launch, reads `.addrust.toml` if present, otherwise starts from defaults
- Changes are previewed in-place
- Save writes back to `.addrust.toml`
- Cancel discards changes

### Dependencies
- `ratatui` — TUI framework
- `crossterm` — terminal backend

## Rule Reordering

### Config file format (when added)
```toml
[rules]
order = [
    "change_na_address",
    "city_state_zip",
    "po_box_number",
    # ... full list
]
```

If `order` is present, rules are arranged in that order. Rules not in the list are appended at the end in their default position. This means users only need to specify the list if they want to change order — omitting it preserves defaults.

### TUI interaction
- Select a rule with Enter (highlights it)
- Arrow up/down to move it
- Enter again to drop it in place
- Visual feedback showing the rule moving through the list

### Use cases
- Extract suffix before unit for datasets where unit designators look like suffixes
- Move pre-direction extraction earlier/later for coordinate-style addresses
- Run custom pre-check rules before standard extraction
