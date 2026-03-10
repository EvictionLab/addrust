#![cfg(feature = "duckdb")]

use addrust::address::Address;
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

#[test]
fn test_write_parsed() {
    let (_dir, path) = setup_test_db();

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
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.duckdb").to_str().unwrap().to_string();

    let originals = vec!["PO BOX 100".to_string()];
    let parsed = vec![Address {
        po_box: Some("PO BOX 100".into()),
        ..Default::default()
    }];

    addrust::duckdb_io::write_parsed(&path, "out", &originals, &parsed).unwrap();

    let conn = Connection::open(&path).unwrap();
    // street_number should be NULL, not empty string
    let result: Option<String> = conn
        .query_row("SELECT street_number FROM out", [], |r| r.get(0))
        .unwrap();
    assert!(result.is_none());
}

use addrust::config::Config;

#[test]
fn test_run_duckdb_full_roundtrip() {
    let (_dir, path) = setup_test_db();

    let config = Config::default();
    addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data_parsed", "address", false).unwrap();

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
    assert_eq!(suffix, "STREET");
}

#[test]
fn test_run_duckdb_missing_input_table() {
    let (_dir, path) = setup_test_db();
    let config = Config::default();

    let result = addrust::duckdb_io::run_duckdb(&config, &path, "nonexistent", "out", "address", false);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_run_duckdb_output_table_already_exists() {
    let (_dir, path) = setup_test_db();
    let config = Config::default();

    let result = addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data", "address", false);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already exists"));
}

#[test]
fn test_run_duckdb_overwrite() {
    let (_dir, path) = setup_test_db();
    let config = Config::default();

    // First run creates the table
    addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data_parsed", "address", false).unwrap();

    // Without overwrite, should fail
    let result = addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data_parsed", "address", false);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already exists"));

    // With overwrite, should succeed
    addrust::duckdb_io::run_duckdb(&config, &path, "my_data", "my_data_parsed", "address", true).unwrap();

    let conn = Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM my_data_parsed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_end_to_end_with_varied_addresses() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.duckdb").to_str().unwrap().to_string();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE parcels (address VARCHAR);
         INSERT INTO parcels VALUES ('100 N BROADWAY STE 200');
         INSERT INTO parcels VALUES ('PO BOX 555');
         INSERT INTO parcels VALUES ('42 W ELM ST APT 3B');",
    )
    .unwrap();
    drop(conn);

    let config = Config::default();
    addrust::duckdb_io::run_duckdb(&config, &path, "parcels", "parcels_parsed", "address", false)
        .unwrap();

    let conn = Connection::open(&path).unwrap();

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

    // Check pre_direction parsed
    let pre_dir: String = conn
        .query_row(
            "SELECT pre_direction FROM parcels_parsed WHERE street_number = '100'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pre_dir, "N");

    // Check PO Box row has po_box filled
    let po: String = conn
        .query_row(
            "SELECT po_box FROM parcels_parsed WHERE address = 'PO BOX 555'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(po, "PO BOX 555");
}
