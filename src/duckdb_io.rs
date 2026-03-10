//! DuckDB integration for reading/writing address tables.

use crate::address::Address;
use crate::config::Config;
use crate::pipeline::Pipeline;
use duckdb::Connection;

/// Validate that the input table and column exist in the database.
/// Returns Ok(()) or an error message listing available tables/columns.
pub fn validate_input(conn: &Connection, table: &str, column: &str) -> Result<(), String> {
    // Check table exists
    let tables = list_tables(conn)?;
    if !tables.iter().any(|t| t.eq_ignore_ascii_case(table)) {
        return Err(format!(
            "Table '{}' not found. Available tables: {}",
            table,
            tables.join(", ")
        ));
    }

    // Check column exists
    let columns = list_columns(conn, table)?;
    if !columns.iter().any(|c| c.eq_ignore_ascii_case(column)) {
        return Err(format!(
            "Column '{}' not found in '{}'. Available columns: {}",
            column,
            table,
            columns.join(", ")
        ));
    }

    Ok(())
}

/// Validate that the output table does not already exist.
pub fn validate_output(conn: &Connection, table: &str) -> Result<(), String> {
    let tables = list_tables(conn)?;
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

/// Read address values from the specified table and column.
/// Skips NULL values.
pub fn read_addresses(conn: &Connection, table: &str, column: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT \"{col}\" FROM \"{tbl}\" WHERE \"{col}\" IS NOT NULL",
        col = column,
        tbl = table,
    );

    let mut stmt = conn.prepare(&sql)
        .map_err(|e| format!("Failed to query table: {e}"))?;

    let rows = stmt.query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to read addresses: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read addresses: {e}"))
}

/// Write original addresses and parsed components to a new table.
/// Sorts by street_name before inserting for better columnar compression.
/// Uses DuckDB's Appender API for bulk insert performance.
pub fn write_parsed(
    conn: &Connection,
    output_table: &str,
    originals: &[String],
    parsed: &[Address],
) -> Result<(), String> {
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

    // Sort by street_name for better columnar compression
    let mut indices: Vec<usize> = (0..originals.len()).collect();
    indices.sort_by(|&a, &b| {
        parsed[a]
            .street_name
            .as_deref()
            .unwrap_or("")
            .cmp(parsed[b].street_name.as_deref().unwrap_or(""))
    });

    let mut appender = conn
        .appender(output_table)
        .map_err(|e| format!("Failed to create appender: {e}"))?;

    for &i in &indices {
        appender
            .append_row(duckdb::params![
                originals[i].as_str(),
                parsed[i].street_number.as_deref(),
                parsed[i].pre_direction.as_deref(),
                parsed[i].street_name.as_deref(),
                parsed[i].suffix.as_deref(),
                parsed[i].post_direction.as_deref(),
                parsed[i].unit_type.as_deref(),
                parsed[i].unit.as_deref(),
                parsed[i].po_box.as_deref(),
                parsed[i].building.as_deref(),
            ])
            .map_err(|e| format!("Failed to append row: {e}"))?;
    }

    Ok(())
}

/// Run the full DuckDB parse pipeline: validate, read, parse, write.
pub fn run_duckdb(
    config: &Config,
    db_path: &str,
    input_table: &str,
    output_table: &str,
    column: &str,
    overwrite: bool,
) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;

    validate_input(&conn, input_table, column)?;

    if overwrite {
        drop_table_if_exists(&conn, output_table)?;
    } else {
        validate_output(&conn, output_table)?;
    }

    let addresses = read_addresses(&conn, input_table, column)?;
    eprintln!("Read {} addresses from '{}'", addresses.len(), input_table);

    let pipeline = Pipeline::from_config(config);
    let refs: Vec<&str> = addresses.iter().map(|s| s.as_str()).collect();
    let parsed = pipeline.parse_batch(&refs);
    eprintln!("Parsed {} addresses", parsed.len());

    write_parsed(&conn, output_table, &addresses, &parsed)?;
    eprintln!("Wrote results to '{}'", output_table);

    Ok(())
}

fn drop_table_if_exists(conn: &Connection, table: &str) -> Result<(), String> {
    conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{}\"", table))
        .map_err(|e| format!("Failed to drop table '{}': {e}", table))
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
