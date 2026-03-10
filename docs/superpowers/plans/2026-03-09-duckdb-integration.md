# DuckDB Integration Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users parse addresses directly from/to DuckDB tables with `addrust parse --duckdb <file> --input-table <name>`.

**Architecture:** New `src/duckdb_io.rs` module behind an optional cargo feature. CLI flags on the `Parse` subcommand trigger DuckDB mode instead of stdin mode. The duckdb_io module handles all database I/O; parsing still goes through the existing `Pipeline`.

**Tech Stack:** `duckdb` crate v1.4 (optional dependency), existing `Pipeline` and `Address` types.

**Spec:** `docs/superpowers/specs/2026-03-09-duckdb-integration-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` | Modify | Add optional `duckdb` dependency |
| `src/lib.rs` | Modify | Conditional `pub mod duckdb_io` |
| `src/duckdb_io.rs` | Create | All DuckDB read/write/validation logic |
| `src/main.rs` | Modify | New CLI flags, dispatch to duckdb module |
| `tests/duckdb_integration.rs` | Create | Integration tests with temp DuckDB files |

---

## Chunk 1: Foundation

### Task 1: Add duckdb dependency as optional feature

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add duckdb to Cargo.toml**

```toml
[dependencies]
duckdb = { version = "1.4", optional = true }

[features]
duckdb = ["dep:duckdb"]
```

- [ ] **Step 2: Add conditional module in lib.rs**

Add after the existing module declarations:

```rust
#[cfg(feature = "duckdb")]
pub mod duckdb_io;
```

- [ ] **Step 3: Create empty src/duckdb_io.rs**

```rust
//! DuckDB integration for reading/writing address tables.
```

- [ ] **Step 4: Verify it compiles with and without the feature**

Run: `cargo check && cargo check --features duckdb`
Expected: Both pass

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/lib.rs src/duckdb_io.rs
git commit -m "feat: add optional duckdb dependency"
```

---

### Task 2: DuckDB validation functions

**Files:**
- Modify: `src/duckdb_io.rs`
- Create: `tests/duckdb_integration.rs`

- [ ] **Step 1: Write failing test for validate_input**

In `tests/duckdb_integration.rs`:

```rust
#![cfg(feature = "duckdb")]

use duckdb::Connection;
use std::path::Path;
use tempfile::NamedTempFile;

fn setup_test_db() -> (NamedTempFile, String) {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE my_data (id INTEGER, address VARCHAR);
         INSERT INTO my_data VALUES (1, '123 MAIN ST');
         INSERT INTO my_data VALUES (2, '456 OAK AVE APT 2');
         INSERT INTO my_data VALUES (3, '789 ELM BLVD');",
    )
    .unwrap();
    (tmp, path)
}

#[test]
fn test_validate_input_success() {
    let (_tmp, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_input(&path, "my_data", "address");
    assert!(result.is_ok());
}

#[test]
fn test_validate_input_missing_table() {
    let (_tmp, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_input(&path, "nonexistent", "address");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("my_data"), "should list available tables: {err}");
}

#[test]
fn test_validate_input_missing_column() {
    let (_tmp, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_input(&path, "my_data", "addr");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("address"), "should list available columns: {err}");
}

#[test]
fn test_validate_output_table_exists() {
    let (_tmp, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_output(&path, "my_data");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already exists"));
}

#[test]
fn test_validate_output_table_new() {
    let (_tmp, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_output(&path, "my_data_parsed");
    assert!(result.is_ok());
}
```

Also add `tempfile` as a dev dependency in Cargo.toml:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --features duckdb --test duckdb_integration -- 2>&1 | head -20`
Expected: FAIL — functions don't exist yet

- [ ] **Step 3: Implement validate_input and validate_output**

In `src/duckdb_io.rs`:

```rust
//! DuckDB integration for reading/writing address tables.

use duckdb::Connection;

/// Validate that the input table and column exist in the database.
/// Returns Ok(()) or an error message listing available tables/columns.
pub fn validate_input(db_path: &str, table: &str, column: &str) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;

    // Check table exists
    let tables = list_tables(&conn)?;
    if !tables.iter().any(|t| t.eq_ignore_ascii_case(table)) {
        return Err(format!(
            "Table '{}' not found. Available tables: {}",
            table,
            tables.join(", ")
        ));
    }

    // Check column exists
    let columns = list_columns(&conn, table)?;
    if !columns.iter().any(|c| c.eq_ignore_ascii_case(column)) {
        return Err(format!(
            "Column '{}' not found in '{}'. Available columns: {}",
            column, table,
            columns.join(", ")
        ));
    }

    Ok(())
}

/// Validate that the output table does not already exist.
pub fn validate_output(db_path: &str, table: &str) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;

    let tables = list_tables(&conn)?;
    if tables.iter().any(|t| t.eq_ignore_ascii_case(table)) {
        return Err(format!(
            "Output table '{}' already exists. Drop it first or choose a different name.",
            table
        ));
    }

    Ok(())
}

fn list_tables(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT table_name FROM information_schema.tables WHERE table_schema = 'main'")
        .map_err(|e| format!("Failed to list tables: {e}"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to list tables: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to list tables: {e}"))
}

fn list_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = 'main' AND table_name = ?",
        )
        .map_err(|e| format!("Failed to list columns: {e}"))?;
    let rows = stmt
        .query_map([table], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to list columns: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to list columns: {e}"))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --features duckdb --test duckdb_integration -v`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/duckdb_io.rs tests/duckdb_integration.rs
git commit -m "feat: add DuckDB input/output validation"
```

---

### Task 3: Read addresses from DuckDB

**Files:**
- Modify: `src/duckdb_io.rs`
- Modify: `tests/duckdb_integration.rs`

- [ ] **Step 1: Write failing test for read_addresses**

Append to `tests/duckdb_integration.rs`:

```rust
#[test]
fn test_read_addresses() {
    let (_tmp, path) = setup_test_db();
    let addresses = addrust::duckdb_io::read_addresses(&path, "my_data", "address").unwrap();
    assert_eq!(addresses.len(), 3);
    assert_eq!(addresses[0], "123 MAIN ST");
    assert_eq!(addresses[1], "456 OAK AVE APT 2");
    assert_eq!(addresses[2], "789 ELM BLVD");
}

#[test]
fn test_read_addresses_skips_nulls() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE t (address VARCHAR);
         INSERT INTO t VALUES ('123 MAIN ST');
         INSERT INTO t VALUES (NULL);
         INSERT INTO t VALUES ('456 OAK AVE');",
    )
    .unwrap();
    let addresses = addrust::duckdb_io::read_addresses(path, "t", "address").unwrap();
    assert_eq!(addresses.len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --features duckdb --test duckdb_integration test_read -v`
Expected: FAIL

- [ ] **Step 3: Implement read_addresses**

Add to `src/duckdb_io.rs`:

```rust
/// Read address values from the specified table and column.
/// Skips NULL values. Returns (row_indices, address_strings) for
/// later alignment with the output table.
pub fn read_addresses(
    db_path: &str,
    table: &str,
    column: &str,
) -> Result<Vec<String>, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;

    let sql = format!(
        "SELECT \"{col}\" FROM \"{tbl}\" WHERE \"{col}\" IS NOT NULL",
        col = column,
        tbl = table,
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to query table: {e}"))?;

    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to read addresses: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read addresses: {e}"))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --features duckdb --test duckdb_integration test_read -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/duckdb_io.rs tests/duckdb_integration.rs
git commit -m "feat: add read_addresses from DuckDB"
```

---

## Chunk 2: Write and CLI

### Task 4: Write parsed addresses to DuckDB

**Files:**
- Modify: `src/duckdb_io.rs`
- Modify: `tests/duckdb_integration.rs`

- [ ] **Step 1: Write failing test for write_parsed**

Append to `tests/duckdb_integration.rs`:

```rust
use addrust::address::Address;

#[test]
fn test_write_parsed() {
    let (_tmp, path) = setup_test_db();

    let originals = vec!["123 MAIN ST".to_string(), "456 OAK AVE APT 2".to_string()];
    let parsed = vec![
        Address {
            street_number: Some("123".into()),
            street_name: Some("MAIN".into()),
            suffix: Some("ST".into()),
            ..Default::default()
        },
        Address {
            street_number: Some("456".into()),
            street_name: Some("OAK".into()),
            suffix: Some("AVE".into()),
            unit: Some("2".into()),
            unit_type: Some("APT".into()),
            ..Default::default()
        },
    ];

    addrust::duckdb_io::write_parsed(&path, "my_data_parsed", &originals, &parsed).unwrap();

    // Verify by reading back
    let conn = Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM my_data_parsed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);

    let street: String = conn
        .query_row(
            "SELECT street_name FROM my_data_parsed WHERE street_number = '123'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(street, "MAIN");
}

#[test]
fn test_write_parsed_empty_fields_are_null() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    let originals = vec!["PO BOX 100".to_string()];
    let parsed = vec![Address {
        po_box: Some("PO BOX 100".into()),
        ..Default::default()
    }];

    addrust::duckdb_io::write_parsed(path, "out", &originals, &parsed).unwrap();

    let conn = Connection::open(path).unwrap();
    // street_number should be NULL, not empty string
    let result: Option<String> = conn
        .query_row("SELECT street_number FROM out", [], |r| r.get(0))
        .unwrap();
    assert!(result.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --features duckdb --test duckdb_integration test_write -v`
Expected: FAIL

- [ ] **Step 3: Implement write_parsed**

Add to `src/duckdb_io.rs`:

```rust
use crate::address::Address;

/// Write original addresses and parsed components to a new table.
pub fn write_parsed(
    db_path: &str,
    output_table: &str,
    originals: &[String],
    parsed: &[Address],
) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;

    let create_sql = format!(
        "CREATE TABLE \"{}\" (
            address VARCHAR,
            street_number VARCHAR,
            pre_direction VARCHAR,
            street_name VARCHAR,
            suffix VARCHAR,
            post_direction VARCHAR,
            unit_type VARCHAR,
            unit VARCHAR,
            po_box VARCHAR,
            building VARCHAR
        )",
        output_table
    );

    conn.execute_batch(&create_sql)
        .map_err(|e| format!("Failed to create output table: {e}"))?;

    let insert_sql = format!(
        "INSERT INTO \"{}\" VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        output_table
    );

    let mut stmt = conn
        .prepare(&insert_sql)
        .map_err(|e| format!("Failed to prepare insert: {e}"))?;

    for (original, addr) in originals.iter().zip(parsed.iter()) {
        stmt.execute(duckdb::params![
            original,
            addr.street_number.as_deref(),
            addr.pre_direction.as_deref(),
            addr.street_name.as_deref(),
            addr.suffix.as_deref(),
            addr.post_direction.as_deref(),
            addr.unit_type.as_deref(),
            addr.unit.as_deref(),
            addr.po_box.as_deref(),
            addr.building.as_deref(),
        ])
        .map_err(|e| format!("Failed to insert row: {e}"))?;
    }

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --features duckdb --test duckdb_integration test_write -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/duckdb_io.rs tests/duckdb_integration.rs
git commit -m "feat: add write_parsed to DuckDB"
```

---

### Task 5: Top-level run_duckdb function

**Files:**
- Modify: `src/duckdb_io.rs`
- Modify: `tests/duckdb_integration.rs`

- [ ] **Step 1: Write failing integration test for full round-trip**

Append to `tests/duckdb_integration.rs`:

```rust
use addrust::config::Config;

#[test]
fn test_run_duckdb_full_roundtrip() {
    let (_tmp, path) = setup_test_db();

    let config = Config::default();
    addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data_parsed", "address").unwrap();

    let conn = Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM my_data_parsed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    // Verify a parsed address has components
    let suffix: String = conn
        .query_row(
            "SELECT suffix FROM my_data_parsed WHERE street_number = '123'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(suffix, "ST");
}

#[test]
fn test_run_duckdb_missing_input_table() {
    let (_tmp, path) = setup_test_db();
    let config = Config::default();

    let result = addrust::duckdb_io::run_duckdb(&config, &path, "nonexistent", "out", "address");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_run_duckdb_output_table_already_exists() {
    let (_tmp, path) = setup_test_db();
    let config = Config::default();

    let result = addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data", "address");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already exists"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --features duckdb --test duckdb_integration test_run -v`
Expected: FAIL

- [ ] **Step 3: Implement run_duckdb**

Add to `src/duckdb_io.rs`:

```rust
use crate::config::Config;
use crate::pipeline::Pipeline;

/// Run the full DuckDB parse pipeline: validate, read, parse, write.
pub fn run_duckdb(
    config: &Config,
    db_path: &str,
    input_table: &str,
    output_table: &str,
    column: &str,
) -> Result<(), String> {
    validate_input(db_path, input_table, column)?;
    validate_output(db_path, output_table)?;

    let addresses = read_addresses(db_path, input_table, column)?;
    eprintln!("Read {} addresses from '{}'", addresses.len(), input_table);

    let pipeline = Pipeline::from_config(config);
    let refs: Vec<&str> = addresses.iter().map(|s| s.as_str()).collect();
    let parsed = pipeline.parse_batch(&refs);
    eprintln!("Parsed {} addresses", parsed.len());

    write_parsed(db_path, output_table, &addresses, &parsed)?;
    eprintln!("Wrote results to '{}'", output_table);

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --features duckdb --test duckdb_integration -v`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/duckdb_io.rs tests/duckdb_integration.rs
git commit -m "feat: add run_duckdb orchestration function"
```

---

### Task 6: CLI flags on Parse subcommand

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add DuckDB flags to Parse variant**

Update the `Commands::Parse` variant:

```rust
    /// Parse addresses from stdin or DuckDB
    Parse {
        /// Output format: "clean" (default), "full", or "tsv" (stdin mode only)
        #[arg(long, default_value = "clean")]
        format: String,
        /// Show timing information
        #[arg(long)]
        time: bool,
        /// DuckDB database file path
        #[cfg(feature = "duckdb")]
        #[arg(long)]
        duckdb: Option<PathBuf>,
        /// Input table name (required with --duckdb)
        #[cfg(feature = "duckdb")]
        #[arg(long)]
        input_table: Option<String>,
        /// Output table name (default: {input_table}_parsed)
        #[cfg(feature = "duckdb")]
        #[arg(long)]
        output_table: Option<String>,
        /// Address column name (default: "address")
        #[cfg(feature = "duckdb")]
        #[arg(long, default_value = "address")]
        column: String,
    },
```

- [ ] **Step 2: Update the match arm in main()**

Replace the `Some(Commands::Parse { format, time })` arm:

```rust
        Some(Commands::Parse {
            format,
            time,
            #[cfg(feature = "duckdb")]
            duckdb,
            #[cfg(feature = "duckdb")]
            input_table,
            #[cfg(feature = "duckdb")]
            output_table,
            #[cfg(feature = "duckdb")]
            column,
        }) => {
            let config = load_config(&cli.config);

            #[cfg(feature = "duckdb")]
            if let Some(ref db_path) = duckdb {
                let input = match input_table {
                    Some(ref t) => t.clone(),
                    None => {
                        eprintln!("Error: --input-table is required when using --duckdb");
                        std::process::exit(1);
                    }
                };
                let output = output_table
                    .unwrap_or_else(|| format!("{}_parsed", input));
                let db_str = db_path.to_str().unwrap_or_else(|| {
                    eprintln!("Error: invalid database path");
                    std::process::exit(1);
                });
                if let Err(e) = addrust::duckdb_io::run_duckdb(
                    &config, db_str, &input, &output, &column,
                ) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }

            run_parse(&config, &format, time);
        }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --features duckdb && cargo check`
Expected: Both pass (feature-gated code compiles in both modes)

- [ ] **Step 4: Manual smoke tests**

Run: `cargo run --features duckdb -- parse --help`
Expected: Shows `--duckdb`, `--input-table`, `--output-table`, `--column` flags

Run: `cargo run --features duckdb -- parse --duckdb nonexistent.duckdb`
Expected: Error message "Error: --input-table is required when using --duckdb"

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add DuckDB CLI flags to parse subcommand"
```

---

### Task 7: End-to-end CLI test

**Files:**
- Modify: `tests/duckdb_integration.rs`

- [ ] **Step 1: Write end-to-end test using run_duckdb**

Append to `tests/duckdb_integration.rs`:

```rust
#[test]
fn test_end_to_end_with_default_output_name() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE parcels (address VARCHAR);
         INSERT INTO parcels VALUES ('100 N BROADWAY STE 200');
         INSERT INTO parcels VALUES ('PO BOX 555');
         INSERT INTO parcels VALUES ('42 W ELM ST APT 3B');",
    )
    .unwrap();
    drop(conn);

    let config = Config::default();
    addrust::duckdb_io::run_duckdb(&config, path, "parcels", "parcels_parsed", "address").unwrap();

    let conn = Connection::open(path).unwrap();

    // Check all rows made it
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM parcels_parsed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    // Check original address preserved
    let orig: String = conn
        .query_row(
            "SELECT address FROM parcels_parsed WHERE street_number = '100'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orig, "100 N BROADWAY STE 200");

    // Check components parsed correctly
    let pre_dir: String = conn
        .query_row(
            "SELECT pre_direction FROM parcels_parsed WHERE street_number = '100'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pre_dir, "N");

    // Check PO Box row
    let po: String = conn
        .query_row(
            "SELECT po_box FROM parcels_parsed WHERE address = 'PO BOX 555'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(po, "PO BOX 555");
}
```

- [ ] **Step 2: Run all DuckDB tests**

Run: `cargo test --features duckdb --test duckdb_integration -v`
Expected: All tests pass

- [ ] **Step 3: Run the full test suite to check for regressions**

Run: `cargo test --features duckdb`
Expected: All existing tests still pass + all new duckdb tests pass

- [ ] **Step 4: Commit**

```bash
git add tests/duckdb_integration.rs
git commit -m "test: add end-to-end DuckDB integration test"
```
