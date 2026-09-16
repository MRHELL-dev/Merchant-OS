use desktop_lib::db::DatabaseManager;
use std::fs;

#[test]
fn test_sqlite_in_memory_connection_live() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open in-memory SQLite");
    let is_connected = db.verify_connection().expect("Connection check failed");
    assert!(is_connected, "SQLite connection must be verified");

    let version = db.get_sqlite_version().expect("Failed to read sqlite_version");
    assert!(!version.is_empty(), "SQLite version must not be empty");
    println!("VERIFIED: SQLite Version is {}", version);
}

#[test]
fn test_sqlite_foundation_schema_live() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open in-memory SQLite");
    let is_connected = db.verify_connection().expect("Connection check failed");
    assert!(is_connected);

    let version = db.get_sqlite_version().expect("Failed to get version");
    assert!(version.starts_with("3."), "SQLite major version must be 3");
    println!("VERIFIED: SQLite foundation schema operational on version {}", version);
}

#[test]
fn test_sqlite_file_database_persistence_live() {
    let temp_dir = std::env::temp_dir().join(format!("merchant_os_test_{}", std::process::id()));
    let db_path = temp_dir.join("live_merchant_os.db");

    let db = DatabaseManager::open(&db_path).expect("Failed to open file database");
    assert!(db.verify_connection().expect("Connection check failed"));
    assert!(db_path.exists(), "Database file must be physically created on disk");

    let version = db.get_sqlite_version().expect("Failed to get version");
    println!("VERIFIED: Physical SQLite database created at {:?}, version {}", db_path, version);

    // Clean up
    drop(db);
    let _ = fs::remove_dir_all(temp_dir);
}
