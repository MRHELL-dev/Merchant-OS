pub mod math;
pub mod migration;
pub mod operations;

use rusqlite::{Connection, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Authoritative runtime owner of the local SQLite database for Merchant OS.
///
/// Architecture Boundary:
/// - Only Rust backend code interacts directly with this struct and SQLite.
/// - React UI never receives raw database handles or directly executes SQL.
pub struct DatabaseManager {
    conn: Mutex<Connection>,
    db_path: Option<PathBuf>,
}

impl DatabaseManager {
    /// Opens or creates a local SQLite database at the specified path.
    /// Configures WAL mode, busy timeout, and enforces foreign key constraints.
    /// Runs embedded Drizzle V1 migrations automatically and transactionally.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
                })?;
            }
        }

        let mut conn = Connection::open(path_ref)?;
        Self::configure_connection(&conn)?;

        // Run V1 Drizzle migrations idempotently
        migration::run_migrations(&mut conn)?;

        let manager = Self {
            conn: Mutex::new(conn),
            db_path: Some(path_ref.to_path_buf()),
        };

        Ok(manager)
    }

    /// Opens an in-memory database for testing and verification.
    /// Configures foreign keys and runs all V1 migrations.
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure_connection(&conn)?;

        // Run V1 Drizzle migrations idempotently
        migration::run_migrations(&mut conn)?;

        let manager = Self {
            conn: Mutex::new(conn),
            db_path: None,
        };

        Ok(manager)
    }

    /// Applies runtime SQLite pragmas (WAL mode, foreign keys, synchronous normal, busy timeout).
    fn configure_connection(conn: &Connection) -> Result<()> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        Ok(())
    }

    /// Provides access to the connection for executing operations.
    pub fn with_connection<F, T>(&self, f: F) -> std::result::Result<T, operations::BusinessError>
    where
        F: FnOnce(&mut Connection) -> std::result::Result<T, operations::BusinessError>,
    {
        let mut conn = self.conn.lock().unwrap();
        f(&mut conn)
    }

    /// Queries the live SQLite version directly from the database engine.
    pub fn get_sqlite_version(&self) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT sqlite_version()")?;
        let version: String = stmt.query_row([], |row| row.get(0))?;
        Ok(version)
    }

    /// Tests connectivity by executing a quick ping statement.
    pub fn verify_connection(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT 1")?;
        let result: i32 = stmt.query_row([], |row| row.get(0))?;
        Ok(result == 1)
    }

    /// Returns a list of all user-defined tables in the database.
    pub fn get_table_names(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        migration::get_all_tables(&conn)
    }

    /// Returns the total count of user-defined tables in the database.
    pub fn count_tables(&self) -> Result<usize> {
        let tables = self.get_table_names()?;
        Ok(tables.len())
    }

    /// Retrieves the path of the database file if not in-memory.
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Creates a consistent, standalone SQLite database snapshot in the specified file.
    /// Uses SQLite's `VACUUM INTO` mechanism while holding the connection lock.
    /// This flushes WAL pages, guarantees that all committed transactions are included,
    /// excludes any uncommitted transactions, and produces a single self-contained database file.
    pub fn create_consistent_snapshot<P: AsRef<Path>>(&self, dest_path: P) -> std::result::Result<(), operations::BusinessError> {
        let dest = dest_path.as_ref();
        if dest.exists() {
            let _ = fs::remove_file(dest);
        }
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }

        let conn = self.conn.lock().unwrap();
        // Passive WAL checkpoint ensures committed WAL frames are reflected
        let _ = conn.execute("PRAGMA wal_checkpoint(PASSIVE)", []);

        let dest_str = dest.to_str().ok_or_else(|| {
            operations::BusinessError::DatabaseError("Invalid destination path for snapshot".to_string())
        })?;

        conn.execute("VACUUM INTO ?1", rusqlite::params![dest_str])
            .map_err(|e| operations::BusinessError::DatabaseError(format!("Failed to create snapshot via VACUUM INTO: {}", e)))?;

        Ok(())
    }

    /// Atomically restores the active database from a candidate database file with durable rollback protection.
    /// Follows Guardrail 1:
    /// 1. Holds the exclusive connection lock throughout the operation.
    /// 2. Preserves a durable copy of the currently active database in a rollback file before switching.
    /// 3. Atomically replaces the active database pages using SQLite Online Backup API.
    /// 4. Runs post-switch verification (integrity check, foreign key check, user invariant check).
    /// 5. If verification fails: immediately restores from the durable rollback copy and returns error.
    /// 6. If verification succeeds: cleans up the rollback file and returns Ok.
    pub fn restore_from_snapshot_file<P: AsRef<Path>>(&self, candidate_path: P) -> std::result::Result<(), operations::BusinessError> {
        let candidate_file = candidate_path.as_ref();
        if !candidate_file.exists() {
            return Err(operations::BusinessError::EntityNotFound(format!(
                "Candidate restore file not found: {:?}", candidate_file
            )));
        }

        let mut conn = self.conn.lock().unwrap();

        // 1. Checkpoint current WAL
        let _ = conn.execute("PRAGMA wal_checkpoint(PASSIVE)", []);

        // 2. Determine durable rollback file path
        let temp_dir = std::env::temp_dir();
        let rollback_path = match &self.db_path {
            Some(live_path) => live_path.with_extension("rollback_durable.db"),
            None => temp_dir.join(format!("merchant_os_rollback_{}.db", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis())),
        };

        if rollback_path.exists() {
            let _ = fs::remove_file(&rollback_path);
        }

        // 3. Preserve original DB in durable recoverable form before modifying live connection
        let rollback_str = rollback_path.to_str().ok_or_else(|| {
            operations::BusinessError::DatabaseError("Invalid rollback path".to_string())
        })?;
        conn.execute("VACUUM INTO ?1", rusqlite::params![rollback_str])
            .map_err(|e| operations::BusinessError::DatabaseError(format!("Failed to create durable rollback copy: {}", e)))?;

        // 4. Open candidate connection
        let candidate_conn = Connection::open(candidate_file)
            .map_err(|e| operations::BusinessError::DatabaseError(format!("Failed to open candidate restore file: {}", e)))?;

        // 5. Atomically replace live connection pages from candidate using SQLite Online Backup API
        let backup_result = {
            let backup = rusqlite::backup::Backup::new(&candidate_conn, &mut conn);
            match backup {
                Ok(b) => b.run_to_completion(100, std::time::Duration::from_millis(5), None),
                Err(e) => Err(e),
            }
        };

        drop(candidate_conn);

        if let Err(e) = backup_result {
            // Restore immediately from durable rollback
            let _ = Self::perform_rollback(&rollback_path, &mut conn);
            return Err(operations::BusinessError::DatabaseError(format!("Online backup page transfer failed: {}. Original DB preserved.", e)));
        }

        // Reapply runtime pragmas to live connection
        if let Err(e) = Self::configure_connection(&conn) {
            let _ = Self::perform_rollback(&rollback_path, &mut conn);
            return Err(operations::BusinessError::DatabaseError(format!("Failed to reconfigure live connection pragmas: {}. Original DB preserved.", e)));
        }

        // 6. Post-switch verification on live connection
        let verify_res = Self::verify_live_state(&conn);
        if let Err(e) = verify_res {
            // Rollback immediately to preserved original DB
            let rb_res = Self::perform_rollback(&rollback_path, &mut conn);
            let rb_msg = match rb_res {
                Ok(_) => "Original database successfully recovered and verified.",
                Err(rb_err) => return Err(operations::BusinessError::DatabaseError(format!("CRITICAL: Post-switch verification failed ({}) AND rollback failed: {}", e, rb_err))),
            };
            return Err(operations::BusinessError::DatabaseError(format!("Post-switch verification failed: {}. {}", e, rb_msg)));
        }

        // 7. Success: clean up durable rollback file
        let _ = fs::remove_file(&rollback_path);

        Ok(())
    }

    fn perform_rollback(rollback_path: &Path, live_conn: &mut Connection) -> std::result::Result<(), operations::BusinessError> {
        if !rollback_path.exists() {
            return Err(operations::BusinessError::DatabaseError(format!("Rollback file missing: {:?}", rollback_path)));
        }
        let rb_conn = Connection::open(rollback_path)
            .map_err(|e| operations::BusinessError::DatabaseError(format!("Failed to open rollback file: {}", e)))?;
        {
            let backup = rusqlite::backup::Backup::new(&rb_conn, live_conn)
                .map_err(|e| operations::BusinessError::DatabaseError(format!("Failed to init rollback backup: {}", e)))?;
            backup.run_to_completion(100, std::time::Duration::from_millis(5), None)
                .map_err(|e| operations::BusinessError::DatabaseError(format!("Failed to complete rollback backup: {}", e)))?;
        }
        drop(rb_conn);
        let _ = Self::configure_connection(live_conn);
        let _ = fs::remove_file(rollback_path);
        Ok(())
    }

    fn verify_live_state(conn: &Connection) -> std::result::Result<(), String> {
        // Integrity check
        let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| format!("integrity_check query failed: {}", e))?;
        if integrity != "ok" {
            return Err(format!("SQLite integrity_check returned: {}", integrity));
        }

        // Foreign key check
        let mut fk_stmt = conn.prepare("PRAGMA foreign_key_check")
            .map_err(|e| format!("foreign_key_check query failed: {}", e))?;
        let fk_violations = fk_stmt.query_map([], |_| Ok(()))
            .map_err(|e| format!("foreign_key_check execution failed: {}", e))?
            .count();
        if fk_violations > 0 {
            return Err(format!("foreign_key_check found {} violations", fk_violations));
        }

        // Check required tables
        let tables = migration::get_all_tables(conn)
            .map_err(|e| format!("failed to read table names: {}", e))?;
        if !tables.contains(&"users".to_string()) || !tables.contains(&"businesses".to_string()) {
            return Err("Critical tables (users, businesses) missing from restored database".to_string());
        }

        // Check at least one user exists
        let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .map_err(|e| format!("Failed to query users count: {}", e))?;
        if user_count == 0 {
            return Err("Restored database contains zero users".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_in_memory_connection_and_version() {
        let db = DatabaseManager::open_in_memory().expect("Failed to open in-memory SQLite");
        assert!(db.verify_connection().expect("Connection check failed"));

        let version = db.get_sqlite_version().expect("Failed to read sqlite_version");
        assert!(version.starts_with("3."), "SQLite version must start with 3.");
        println!("Verified SQLite Version: {}", version);
    }

    #[test]
    fn test_sqlite_v1_migration_execution_all_tables() {
        let db = DatabaseManager::open_in_memory().expect("Failed to open in-memory SQLite");
        let tables = db.get_table_names().expect("Failed to get table names");

        println!("Created tables count: {}", tables.len());
        for table in &tables {
            println!(" - Table: {}", table);
        }

        // Must have all 26 tables
        assert_eq!(tables.len(), 26, "Expected 26 user tables from V1 migration");

        assert!(tables.contains(&"businesses".to_string()));
        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"permissions".to_string()));
        assert!(tables.contains(&"categories".to_string()));
        assert!(tables.contains(&"products".to_string()));
        assert!(tables.contains(&"barcode_mappings".to_string()));
        assert!(tables.contains(&"inventory".to_string()));
        assert!(tables.contains(&"stock_movements".to_string()));
        assert!(tables.contains(&"stock_corrections".to_string()));
        assert!(tables.contains(&"customers".to_string()));
        assert!(tables.contains(&"customer_ledger".to_string()));
        assert!(tables.contains(&"suppliers".to_string()));
        assert!(tables.contains(&"supplier_ledger".to_string()));
        assert!(tables.contains(&"customer_orders".to_string()));
        assert!(tables.contains(&"customer_order_items".to_string()));
        assert!(tables.contains(&"sales".to_string()));
        assert!(tables.contains(&"sale_items".to_string()));
        assert!(tables.contains(&"purchases".to_string()));
        assert!(tables.contains(&"purchase_items".to_string()));
        assert!(tables.contains(&"returns".to_string()));
        assert!(tables.contains(&"return_items".to_string()));
        assert!(tables.contains(&"payments".to_string()));
        assert!(tables.contains(&"expenses".to_string()));
        assert!(tables.contains(&"audit_logs".to_string()));
        assert!(tables.contains(&"system_metadata".to_string()));
        assert!(tables.contains(&"schema_migrations".to_string()));
    }

    #[test]
    fn test_sqlite_migration_idempotency() {
        let mut conn = Connection::open_in_memory().unwrap();
        DatabaseManager::configure_connection(&conn).unwrap();

        // Apply once
        migration::run_migrations(&mut conn).unwrap();
        let tables_first = migration::get_all_tables(&conn).unwrap();

        // Apply second time
        migration::run_migrations(&mut conn).unwrap();
        let tables_second = migration::get_all_tables(&conn).unwrap();

        assert_eq!(tables_first, tables_second);
        assert_eq!(tables_first.len(), 26);
    }
}
