#![cfg(feature = "duckdb")]

use duckdb::Connection;
use tempfile::TempDir;

fn setup_test_db() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.duckdb").to_str().unwrap().to_string();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE my_data (id INTEGER, address VARCHAR);
         INSERT INTO my_data VALUES (1, '123 MAIN ST');
         INSERT INTO my_data VALUES (2, '456 OAK AVE APT 2');
         INSERT INTO my_data VALUES (3, '789 ELM BLVD');",
    )
    .unwrap();
    (dir, path)
}

#[test]
fn test_validate_input_success() {
    let (_dir, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_input(&path, "my_data", "address");
    assert!(result.is_ok());
}

#[test]
fn test_validate_input_missing_table() {
    let (_dir, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_input(&path, "nonexistent", "address");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("my_data"), "should list available tables: {err}");
}

#[test]
fn test_validate_input_missing_column() {
    let (_dir, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_input(&path, "my_data", "addr");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("address"),
        "should list available columns: {err}"
    );
}

#[test]
fn test_validate_output_table_exists() {
    let (_dir, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_output(&path, "my_data");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already exists"));
}

#[test]
fn test_validate_output_table_new() {
    let (_dir, path) = setup_test_db();
    let result = addrust::duckdb_io::validate_output(&path, "my_data_parsed");
    assert!(result.is_ok());
}

#[test]
fn test_read_addresses() {
    let (_dir, path) = setup_test_db();
    let addresses = addrust::duckdb_io::read_addresses(&path, "my_data", "address").unwrap();
    assert_eq!(addresses.len(), 3);
    assert_eq!(addresses[0], "123 MAIN ST");
    assert_eq!(addresses[1], "456 OAK AVE APT 2");
    assert_eq!(addresses[2], "789 ELM BLVD");
}

#[test]
fn test_read_addresses_skips_nulls() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.duckdb").to_str().unwrap().to_string();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE t (address VARCHAR);
         INSERT INTO t VALUES ('123 MAIN ST');
         INSERT INTO t VALUES (NULL);
         INSERT INTO t VALUES ('456 OAK AVE');",
    )
    .unwrap();
    let addresses = addrust::duckdb_io::read_addresses(&path, "t", "address").unwrap();
    assert_eq!(addresses.len(), 2);
}
