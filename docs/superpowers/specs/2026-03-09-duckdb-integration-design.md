# DuckDB Integration for `parse` Subcommand

## Problem

Parsing addresses from DuckDB requires a multi-step pipe chain that's hard to remember and easy to get wrong. Users want to point addrust at a DuckDB file and go.

## Design

### New CLI flags on `parse`

```
--duckdb <path>          DuckDB database file
--input-table <name>     Table to read addresses from
--output-table <name>    Table to write results to (default: {input_table}_parsed)
--column <name>          Address column name (default: "address")
```

When `--duckdb` is provided, `--input-table` is required. `--output-table` defaults to `{input_table}_parsed`.

These flags are mutually exclusive with stdin mode. If `--duckdb` is set, stdin is not read. The existing `--format` flag is ignored in duckdb mode (output goes to a table, not stdout).

### Behavior

1. Open the DuckDB file at `--duckdb` path
2. Validate: `--input-table` exists, `--column` exists in it, `--output-table` does not exist
3. Read all values from the address column
4. Parse each address through the pipeline
5. Write `--output-table` with columns:
   - `address` (original value, using whatever the source column was named, renamed to `address`)
   - `street_number`
   - `pre_direction`
   - `street_name`
   - `suffix`
   - `post_direction`
   - `unit_type`
   - `unit`
   - `po_box`
   - `building`
6. Report count to stderr

### Error cases

- `--duckdb` without `--input-table`: error with usage hint
- Input table doesn't exist: error listing available tables
- Column doesn't exist in input table: error listing available columns
- Output table already exists: error suggesting a different name or asking user to drop it

### Build setup

- `duckdb` Rust crate added as an optional dependency behind a `duckdb` cargo feature
- CLI flags only available when feature is enabled
- Default build does not include duckdb (keeps binary lean for stdin-only users)

### Example usage

```bash
# Minimal
addrust parse --duckdb data-requests.duckdb --input-table my_data

# With explicit output table and column
addrust parse --duckdb data-requests.duckdb --input-table my_data --output-table my_data_clean --column raw_addr

# Still works as before
echo "123 MAIN ST APT 4" | addrust parse
```

## What this does NOT include

- Reading/writing CSV, Parquet, or other file formats (future work)
- Preserving all original columns (only the address column is carried over)
- Appending to existing output tables
