# addrust

A fast, configurable address parser for US addresses, built in Rust.

addrust uses a table-driven pipeline architecture: domain knowledge lives in lookup tables, not code branches. Parsing steps are defined in TOML configuration and can be reordered, customized, or extended without modifying source code.

## Features

- **Pipeline-based parsing** — 33 default steps (rewrite + extract) process addresses from outside-in, using positional context to resolve ambiguity
- **Table-driven standardization** — USPS suffixes, directionals, state abbreviations, unit types, and number words are all lookup tables
- **Interactive TUI** — built with [ratatui](https://ratatui.rs) for exploring and editing pipeline steps, abbreviation tables, and output settings
- **Configurable** — override default steps, tables, and output settings via TOML
- **Optional DuckDB integration** — read/write address data directly from DuckDB tables

## Installation

```sh
cargo install --path .
```

With DuckDB support:

```sh
cargo install --path . --features duckdb
```

## Usage

Parse addresses from a file (one per line):

```sh
addrust parse addresses.txt
```

Launch the interactive TUI:

```sh
addrust tui
```

## Configuration

addrust looks for `.addrust.toml` in the current directory for custom configuration. See `data/defaults/steps.toml` for the default pipeline steps.

## License

[MIT](LICENSE)
