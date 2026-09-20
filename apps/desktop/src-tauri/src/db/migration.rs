use rusqlite::{params, Connection, Result};

pub const MIGRATION_VERSION_V1: &str = "0000_ordinary_skaar";
pub const MIGRATION_SQL_V1: &str = include_str!("../../../../../database/migrations/0000_ordinary_skaar.sql");

pub const MIGRATION_VERSION_V2: &str = "0001_build05_products_inventory";
pub const MIGRATION_SQL_V2: &str = include_str!("../../../../../database/migrations/0001_build05_products_inventory.sql");

fn apply_migration_if_needed(conn: &mut Connection, version: &str, sql: &str) -> Result<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
        params![version],
        |row| row.get(0),
    )?;

    if count > 0 {
        return Ok(());
    }

    let tx = conn.transaction()?;

    for statement in sql.split("--> statement-breakpoint") {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("CREATE TABLE `schema_migrations`")
            || trimmed.starts_with("CREATE UNIQUE INDEX `schema_migrations_version_unique`")
        {
            continue;
        }
        tx.execute_batch(trimmed)?;
    }

    let now = format!("{:?}", std::time::SystemTime::now());
    let checksum = format!("len:{}", sql.len());
    tx.execute(
        "INSERT INTO schema_migrations (version, applied_at, checksum) VALUES (?1, ?2, ?3)",
        params![version, now, checksum],
    )?;

    tx.commit()?;
    println!("[Merchant OS] Successfully applied migration: {}", version);
    Ok(())
}

/// Ensures the schema_migrations table exists and applies unapplied migrations in order.
pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    // 1. Ensure schema_migrations table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            version TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL,
            checksum TEXT NOT NULL
        );",
        [],
    )?;

    // 2. Apply V1 migration if needed
    apply_migration_if_needed(conn, MIGRATION_VERSION_V1, MIGRATION_SQL_V1)?;

    let now = format!("{:?}", std::time::SystemTime::now());

    // 3. Apply V2 migration if needed
    apply_migration_if_needed(conn, MIGRATION_VERSION_V2, MIGRATION_SQL_V2)?;

    // 5. Ensure foundation metadata is seeded if not present
    conn.execute(
        "INSERT INTO system_metadata (key, value, updated_at)
         VALUES ('system_status', 'ONLINE', ?1)
         ON CONFLICT(key) DO NOTHING",
        params![now],
    )?;

    Ok(())
}

/// Helper to check if a specific table exists in the database.
pub fn table_exists(conn: &Connection, table_name: &str) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
    )?;
    let count: i64 = stmt.query_row(params![table_name], |row| row.get(0))?;
    Ok(count > 0)
}

/// Helper to get a sorted list of all user tables in the database.
pub fn get_all_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    let mut tables = Vec::new();
    for table in rows {
        tables.push(table?);
    }
    Ok(tables)
}
