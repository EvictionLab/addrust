//! DuckDB integration for reading/writing address tables.

use crate::address::Address;
use duckdb::Connection;

/// Validate that the input table and column exist in the database.
/// Returns Ok(()) or an error message listing available tables/columns.
pub fn validate_input(db_path: &str, table: &str, column: &str) -> Result<(), String> {
    let conn =
        Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;

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
            column,
            table,
            columns.join(", ")
        ));
    }

    Ok(())
}

/// Validate that the output table does not already exist.
pub fn validate_output(db_path: &str, table: &str) -> Result<(), String> {
    let conn =
        Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;

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

/// Read address values from the specified table and column.
/// Skips NULL values.
pub fn read_addresses(db_path: &str, table: &str, column: &str) -> Result<Vec<String>, String> {
    let conn =
        Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;

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

/// Write original addresses and parsed components to a new table.
pub fn write_parsed(
    db_path: &str,
    output_table: &str,
    originals: &[String],
    parsed: &[Address],
) -> Result<(), String> {
    let conn =
        Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;

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
