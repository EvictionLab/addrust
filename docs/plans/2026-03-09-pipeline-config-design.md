# Pipeline Configuration System

## Summary

Add a TOML-based config file (`.addrust.toml`) that lets users enable/disable pipeline rules and patch dictionary tables (add, remove, override entries) without touching Rust code. Target user knows regex but not Rust.

## Config File

Per-project `.addrust.toml` in working directory. Three concerns in one file:

```toml
[rules]
disabled = ["po_box_number", "unit_location"]
disabled_groups = ["po_box"]

[dictionaries.suffix.add]
entries = [
    { short = "PSGE", long = "PASSAGE" },
]

[dictionaries.suffix.remove]
entries = ["TRAILER", "TRAILR"]

[dictionaries.unit_type.override]
entries = [
    { short = "STE", long = "SUITE NUMBER" },
]
```

Dictionary patches apply to built-in tables: adds append, removes filter out, overrides replace the long form for a matching short. Patches happen before rule building so new entries automatically appear in regex patterns.

## CLI Subcommands

Shift from flags-only to subcommand structure:

- `addrust parse` — parse addresses from stdin (bare stdin also works for backwards compat). Reads `.addrust.toml` if present.
- `addrust init` — generate a fully commented `.addrust.toml` with all rules (enabled by default) and all dictionary entries. Self-documenting reference.
- `addrust list rules` — print pipeline in order: label, group, action, enabled/disabled status.
- `addrust list tables [name]` — print dictionary tables and entry counts, or entries for a specific table.
- `addrust configure` — placeholder for future TUI.

## Pipeline API

`Pipeline` gains config-aware constructors:

```rust
Pipeline::new(rules, &config)      // explicit (existing)
Pipeline::from_config(path)        // from config file
Pipeline::default()                // built-in defaults, no config file
```

Config is read once at startup. No per-address overhead.

## Architecture

```
load .addrust.toml
  → patch abbreviation tables (add/remove/override)
  → build_rules() using patched tables
  → apply rule enable/disable
  → Pipeline ready
```

New modules:
- `src/config.rs` — TOML deserialization, config struct, table patching logic
- `src/cli.rs` — subcommand definitions (extracted from main.rs)

New dependencies:
- `toml` + `serde` for config parsing

## Not In Scope (this phase)

- Rule reordering (see future-tui-and-reordering.md)
- New dictionary table types
- Interactive TUI
- Per-user config or config merging

## Future: TUI and Rule Reordering

See `docs/plans/future-tui-and-reordering.md` for design notes on:
- Interactive `addrust configure` with ratatui
- Rule reordering via up/down controls
- The TUI reads/writes the same `.addrust.toml` format
