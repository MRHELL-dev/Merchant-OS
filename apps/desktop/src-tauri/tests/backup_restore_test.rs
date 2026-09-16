use base64::Engine;
use desktop_lib::auth::{hash_password, AuthService, AuthenticatedIdentity};
use desktop_lib::backup::{
    BackupPackage, BackupService, CURRENT_APP_VERSION, CURRENT_BACKUP_FORMAT_VERSION,
};
use desktop_lib::db::DatabaseManager;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

/// Helper to set up an isolated test database with admin and sample business data.
fn setup_test_db(test_name: &str) -> (DatabaseManager, PathBuf, PathBuf) {
    let temp_dir = std::env::temp_dir().join(format!(
        "mos_test_{}_{}",
        test_name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let _ = fs::create_dir_all(&temp_dir);
    let db_path = temp_dir.join("test_merchant_os.db");
    let backup_dir = temp_dir.join("backups");
    let _ = fs::create_dir_all(&backup_dir);

    let db = DatabaseManager::open(&db_path).expect("Failed to open test file database");

    // Seed admin user, employee, and sample product, category, customer, and supplier
    let now = format!("{:?}", std::time::SystemTime::now());
    let admin_hash = hash_password("admin123").expect("Failed to hash admin password");
    let emp_hash = hash_password("emp123").expect("Failed to hash employee password");

    db.with_connection(|conn| {
        // Admin
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('user_admin_1', 'admin', ?1, 'ADMIN', 1, ?2, ?2)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![admin_hash, now],
        )?;

        // Employee
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('user_emp_1', 'cashier1', ?1, 'EMPLOYEE', 1, ?2, ?2)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![emp_hash, now],
        )?;

        // Category
        conn.execute(
            "INSERT INTO categories (id, name, slug, created_at)
             VALUES ('cat_1', 'Groceries', 'groceries', ?1)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![now],
        )?;

        // Product
        conn.execute(
            "INSERT INTO products (id, business_id, name, category_id, unit, product_type, min_stock_level, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_rice_1', 'biz_default', 'Basmati Rice 1kg', 'cat_1', 'kg', 'PACKAGED', 5000, 10000, 15000, 1, ?1, ?1)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![now],
        )?;

        // Inventory
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_rice_1', 'prod_rice_1', 25000, ?1)
             ON CONFLICT(product_id) DO NOTHING",
            rusqlite::params![now],
        )?;

        // Customer
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_1', 'Ramesh Kumar', '9876543210', 'Market Street', 0, 1, ?1, ?1)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![now],
        )?;

        // Supplier
        conn.execute(
            "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
             VALUES ('supp_1', 'Apex Wholesale', '9123456780', 'Grain Market', 0, 1, ?1, ?1)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![now],
        )?;

        Ok(())
    })
    .expect("Failed to seed sample test data");

    (db, temp_dir, backup_dir)
}

fn get_admin(db: &DatabaseManager) -> AuthenticatedIdentity {
    db.with_connection(|conn| {
        AuthService::authenticate(conn, "admin", "admin123")
            .map_err(|e| desktop_lib::db::operations::BusinessError::DatabaseError(e.to_string()))
    })
    .expect("Admin authentication failed")
}

fn get_employee(db: &DatabaseManager) -> AuthenticatedIdentity {
    db.with_connection(|conn| {
        AuthService::authenticate(conn, "cashier1", "emp123")
            .map_err(|e| desktop_lib::db::operations::BusinessError::DatabaseError(e.to_string()))
    })
    .expect("Employee authentication failed")
}

// ==============================================================================================
// 1. OFFLINE VERIFICATION (GUARDRAIL 4)
// ==============================================================================================

#[test]
fn test_01_offline_operations_uninterrupted() {
    let (db, _temp_dir, backup_dir) = setup_test_db("offline");
    let admin = get_admin(&db);

    // 1. Core business operations function without any network
    let now = format!("{:?}", std::time::SystemTime::now());
    let sale_res = db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO sales (id, sale_number, customer_id, total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status, user_id, sale_date, created_at)
             VALUES ('sale_off_1', 'INV-OFF-01', 'cust_1', 15000, 15000, 0, 'PAID', 'user_admin_1', ?1, ?1)",
            rusqlite::params![now],
        )?;
        conn.execute(
            "INSERT INTO sale_items (id, sale_id, product_id, quantity, unit_price_cents, cost_price_cents, total_cents)
             VALUES ('item_off_1', 'sale_off_1', 'prod_rice_1', 1000, 15000, 10000, 15000)",
            [],
        )?;
        conn.execute(
            "UPDATE inventory SET current_quantity = current_quantity - 1000 WHERE product_id = 'prod_rice_1'",
            [],
        )?;
        Ok(())
    });
    assert!(sale_res.is_ok(), "Offline sale must succeed");

    // 2. Manual local backup works completely offline
    let backup_meta = BackupService::create_backup(
        &db,
        &admin,
        "MANUAL",
        Some("Offline backup"),
        Some(&backup_dir),
    )
    .expect("Manual backup must succeed completely offline");
    assert_eq!(backup_meta.backup_type, "MANUAL");
    assert!(backup_meta.total_records > 0);

    // 3. Trigger auto backup with internet = false must skip safely without error or blocking
    let auto_res = BackupService::trigger_auto_backup(&db, &admin, false, Some(&backup_dir))
        .expect("Auto backup skip must not return error");
    assert!(auto_res.is_none(), "Auto backup must be skipped when offline");
}

// ==============================================================================================
// 2. MANUAL BACKUP & LOGICAL STATE PRESERVATION (GUARDRAILS 3 & 5)
// ==============================================================================================

#[test]
fn test_02_manual_backup_creation_and_integrity() {
    let (db, _temp_dir, backup_dir) = setup_test_db("manual_backup");
    let admin = get_admin(&db);

    let backup_meta = BackupService::create_backup(
        &db,
        &admin,
        "MANUAL",
        Some("Initial state"),
        Some(&backup_dir),
    )
    .expect("Failed to create manual backup");

    assert_eq!(
        backup_meta.backup_format_version,
        CURRENT_BACKUP_FORMAT_VERSION
    );
    assert_eq!(backup_meta.app_version, CURRENT_APP_VERSION);
    assert_eq!(backup_meta.business_id, "biz_default");
    assert_eq!(
        backup_meta.checksum_sha256.len(),
        64,
        "SHA-256 must be 64 hex characters"
    );

    // Verify all 26 tables are present in table_counts
    assert_eq!(
        backup_meta.table_counts.len(),
        26,
        "Manifest must cover all 26 schema tables"
    );
    assert!(backup_meta.table_counts.contains_key("products"));
    assert!(backup_meta.table_counts.contains_key("inventory"));
    assert!(backup_meta.table_counts.contains_key("users"));
    assert!(backup_meta.table_counts.contains_key("audit_logs"));

    // Verify file exists on disk
    let file_path = backup_dir.join(&backup_meta.file_name);
    assert!(file_path.exists());

    // Validate backup file
    let report =
        BackupService::validate_backup_file(&file_path).expect("Validation execution failed");
    assert!(report.is_valid, "Fresh manual backup must be 100% valid");
    assert_eq!(report.compatibility_status, "COMPATIBLE");
    assert!(report.integrity_check_passed);
    assert!(report.foreign_key_check_passed);
    assert!(report.schema_tables_passed);
    assert!(report.business_invariants_passed);
}

// ==============================================================================================
// 3. PERSISTENT ID PRESERVATION & LOGICAL EQUALITY (GUARDRAIL 3)
// ==============================================================================================

#[test]
fn test_03_identity_preservation_and_logical_state_equality() {
    let (db, _temp_dir, backup_dir) = setup_test_db("id_preservation");
    let admin = get_admin(&db);

    // 1. Create a manual backup of initial state
    let backup_meta = BackupService::create_backup(&db, &admin, "MANUAL", None, Some(&backup_dir))
        .expect("Backup creation failed");
    let file_path = backup_dir.join(&backup_meta.file_name);

    // 2. Mutate live database state drastically
    let now = format!("{:?}", std::time::SystemTime::now());
    db.with_connection(|conn| {
        // Change product price and name
        conn.execute(
            "UPDATE products SET selling_price_cents = 99999, name = 'Mutated Rice' WHERE id = 'prod_rice_1'",
            [],
        )?;
        // Add new customer
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_mutated_2', 'Temp User', '1111111111', 'Nowhere', 50000, 1, ?1, ?1)",
            rusqlite::params![now],
        )?;
        // Decrement inventory
        conn.execute(
            "UPDATE inventory SET current_quantity = 0 WHERE product_id = 'prod_rice_1'",
            [],
        )?;
        Ok(())
    })
    .expect("Mutation failed");

    // Verify live state changed
    let (mutated_price, mutated_name): (i64, String) = db
        .with_connection(|conn| {
            let p = conn.query_row(
                "SELECT selling_price_cents, name FROM products WHERE id = 'prod_rice_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(p)
        })
        .unwrap();
    assert_eq!(mutated_price, 99999);
    assert_eq!(mutated_name, "Mutated Rice");

    // 3. Restore the original backup
    let restore_report = BackupService::restore_backup_file(&db, &admin, &file_path)
        .expect("Restore must succeed");
    assert!(restore_report.success);
    assert!(restore_report.verification_passed);
    assert!(!restore_report.rolled_back);

    // 4. Verify LOGICAL-STATE-EXACT RESTORATION (Guardrail 3)
    let (restored_price, restored_name, restored_qty): (i64, String, i64) = db
        .with_connection(|conn| {
            let (p, n): (i64, String) = conn.query_row(
                "SELECT selling_price_cents, name FROM products WHERE id = 'prod_rice_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let q: i64 = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            Ok((p, n, q))
        })
        .unwrap();

    // Persistent IDs and business attributes remain unchanged
    assert_eq!(
        restored_price, 15000,
        "Price must be restored to original 15000 cents"
    );
    assert_eq!(
        restored_name, "Basmati Rice 1kg",
        "Name must be restored to original"
    );
    assert_eq!(
        restored_qty, 25000,
        "Inventory must be restored to original 25000 millie-units"
    );

    // Mutated customer must no longer exist
    let mutated_cust_count: i64 = db
        .with_connection(|conn| {
            let c = conn.query_row(
                "SELECT COUNT(*) FROM customers WHERE id = 'cust_mutated_2'",
                [],
                |r| r.get(0),
            )?;
            Ok(c)
        })
        .unwrap();
    assert_eq!(
        mutated_cust_count, 0,
        "Entities created after backup must be purged on restore"
    );
}

// ==============================================================================================
// 4. AUTO BACKUP RETENTION & RETENTION ISOLATION (GUARDRAIL 6 & SECTION 13)
// ==============================================================================================

#[test]
fn test_04_auto_backup_retention_and_manual_isolation() {
    let (db, _temp_dir, backup_dir) = setup_test_db("retention");
    let admin = get_admin(&db);

    // Enable auto-backup
    let _ = BackupService::update_auto_backup_setting(&db, true, &backup_dir);

    // Create 3 manual backups
    let m1 = BackupService::create_backup(
        &db,
        &admin,
        "MANUAL",
        Some("Manual 1"),
        Some(&backup_dir),
    )
    .unwrap();
    let m2 = BackupService::create_backup(
        &db,
        &admin,
        "MANUAL",
        Some("Manual 2"),
        Some(&backup_dir),
    )
    .unwrap();
    let m3 = BackupService::create_backup(
        &db,
        &admin,
        "MANUAL",
        Some("Manual 3"),
        Some(&backup_dir),
    )
    .unwrap();

    // Create 7 automatic backups
    let mut auto_metas = Vec::new();
    for i in 1..=7 {
        thread::sleep(std::time::Duration::from_millis(15));
        let auto_res = BackupService::create_backup(
            &db,
            &admin,
            "AUTO",
            Some(&format!("Auto {}", i)),
            Some(&backup_dir),
        )
        .unwrap();
        let _ = BackupService::apply_auto_retention(&backup_dir, 7);
        auto_metas.push(auto_res);
    }

    let initial_backups = BackupService::list_backups(&backup_dir).unwrap();
    let auto_count_initial = initial_backups
        .iter()
        .filter(|b| b.backup_type == "AUTO")
        .count();
    let manual_count_initial = initial_backups
        .iter()
        .filter(|b| b.backup_type == "MANUAL")
        .count();
    assert_eq!(auto_count_initial, 7, "Must have exactly 7 auto backups");
    assert_eq!(manual_count_initial, 3, "Must have exactly 3 manual backups");

    // Now create the 8th automatic backup
    thread::sleep(std::time::Duration::from_millis(15));
    let auto_8 = BackupService::create_backup(
        &db,
        &admin,
        "AUTO",
        Some("Auto 8"),
        Some(&backup_dir),
    )
    .unwrap();
    let removed_oldest = BackupService::apply_auto_retention(&backup_dir, 7).unwrap();

    assert!(
        removed_oldest.is_some(),
        "Oldest auto backup must be deleted when 8th is created"
    );
    assert_eq!(
        removed_oldest.unwrap(),
        auto_metas[0].file_name,
        "The 1st auto backup must have been deleted"
    );

    // Verify retention counts
    let final_backups = BackupService::list_backups(&backup_dir).unwrap();
    let auto_count_final = final_backups
        .iter()
        .filter(|b| b.backup_type == "AUTO")
        .count();
    let manual_count_final = final_backups
        .iter()
        .filter(|b| b.backup_type == "MANUAL")
        .count();

    assert_eq!(auto_count_final, 7, "Exactly 7 newest auto backups must remain");
    assert_eq!(
        manual_count_final, 3,
        "Manual backups must NOT be touched by auto retention"
    );

    // Verify all 3 manual backup files still exist on disk
    assert!(
        backup_dir.join(&m1.file_name).exists(),
        "Manual backup 1 must still exist"
    );
    assert!(
        backup_dir.join(&m2.file_name).exists(),
        "Manual backup 2 must still exist"
    );
    assert!(
        backup_dir.join(&m3.file_name).exists(),
        "Manual backup 3 must still exist"
    );
    assert!(
        backup_dir.join(&auto_8.file_name).exists(),
        "Auto backup 8 must exist"
    );
}

// ==============================================================================================
// 5. RESTORE ROLLBACK SAFETY ON VERIFICATION FAILURE (GUARDRAIL 1)
// ==============================================================================================

#[test]
fn test_05_restore_failure_preserves_original_state() {
    let (db, _temp_dir, backup_dir) = setup_test_db("rollback_safety");
    let admin = get_admin(&db);

    // Verify original product price
    let original_price: i64 = db
        .with_connection(|conn| {
            let p = conn.query_row(
                "SELECT selling_price_cents FROM products WHERE id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            Ok(p)
        })
        .unwrap();
    assert_eq!(original_price, 15000);

    // Create a corrupted backup package:
    // Package JSON with valid format, valid base64, but modified manifest checksum to simulate tampering
    let backup_meta = BackupService::create_backup(&db, &admin, "MANUAL", None, Some(&backup_dir))
        .unwrap();
    let valid_path = backup_dir.join(&backup_meta.file_name);
    let valid_content = fs::read_to_string(&valid_path).unwrap();
    let mut package: BackupPackage = serde_json::from_str(&valid_content).unwrap();

    // Tamper with checksum
    package.manifest.checksum_sha256 =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let tampered_path = backup_dir.join("tampered_backup.mosbackup");
    fs::write(
        &tampered_path,
        serde_json::to_string_pretty(&package).unwrap(),
    )
    .unwrap();

    // Attempt restore of tampered backup
    let restore_err = BackupService::restore_backup_file(&db, &admin, &tampered_path);
    assert!(
        restore_err.is_err(),
        "Restore of tampered backup must be rejected"
    );

    // Verify live database was NEVER modified
    let current_price: i64 = db
        .with_connection(|conn| {
            let p = conn.query_row(
                "SELECT selling_price_cents FROM products WHERE id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            Ok(p)
        })
        .unwrap();
    assert_eq!(
        current_price, original_price,
        "Original live database must remain 100% intact"
    );
}

// ==============================================================================================
// 6. SNAPSHOT CONCURRENCY TEST (GUARDRAIL 2)
// ==============================================================================================

#[test]
fn test_06_snapshot_concurrency_safe_under_writes() {
    let (db, _temp_dir, backup_dir) = setup_test_db("concurrency");
    let db_arc = Arc::new(db);
    let admin = get_admin(&db_arc);

    // Spawn background thread performing rapid business writes
    let db_clone = Arc::clone(&db_arc);
    let stop_handle = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop_handle);

    let writer_thread = thread::spawn(move || {
        let mut counter = 0;
        while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
            counter += 1;
            let now = format!("{:?}", std::time::SystemTime::now());
            let _ = db_clone.with_connection(|conn| {
                let tx = conn.transaction()?;
                tx.execute(
                    "UPDATE inventory SET current_quantity = current_quantity + 1 WHERE product_id = 'prod_rice_1'",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO stock_movements (id, product_id, movement_type, quantity_change, quantity_before, quantity_after, reference_type, reference_id, created_at)
                     VALUES (?1, 'prod_rice_1', 'CORRECTION', 1, 25000, 25001, 'STOCK_CORRECTIONS', 'ref_test', ?2)",
                    rusqlite::params![format!("mov_conc_{}", counter), now],
                )?;
                tx.commit()?;
                Ok(())
            });
            thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    // Create backup while concurrent writes are active
    thread::sleep(std::time::Duration::from_millis(10));
    let backup_meta = BackupService::create_backup(
        &db_arc,
        &admin,
        "MANUAL",
        Some("Concurrent write snapshot"),
        Some(&backup_dir),
    )
    .expect("Backup creation must succeed during concurrent operations");

    // Stop writer thread
    stop_handle.store(true, std::sync::atomic::Ordering::Relaxed);
    writer_thread.join().expect("Writer thread failed");

    // Verify the snapshot created under concurrency is 100% valid and consistent
    let backup_file = backup_dir.join(&backup_meta.file_name);
    let report =
        BackupService::validate_backup_file(&backup_file).expect("Validation must succeed");

    assert!(
        report.is_valid,
        "Snapshot taken during concurrent writes must be 100% valid"
    );
    assert!(
        report.integrity_check_passed,
        "PRAGMA integrity_check must pass"
    );
    assert!(
        report.foreign_key_check_passed,
        "PRAGMA foreign_key_check must pass (no dangling relations)"
    );
    assert_eq!(report.compatibility_status, "COMPATIBLE");

    // Verify coherent committed state: snapshot must never contain a partial transaction
    let snap_content = fs::read_to_string(&backup_file).unwrap();
    let pkg: BackupPackage = serde_json::from_str(&snap_content).unwrap();
    let raw_bytes = base64::engine::general_purpose::STANDARD
        .decode(&pkg.sqlite_snapshot_base64)
        .unwrap();
    let temp_snap_path = _temp_dir.join("verify_snap.db");
    fs::write(&temp_snap_path, &raw_bytes).unwrap();
    let snap_conn = rusqlite::Connection::open(&temp_snap_path).unwrap();

    let snap_inventory_qty: i64 = snap_conn
        .query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let snap_movements_count: i64 = snap_conn
        .query_row(
            "SELECT COUNT(*) FROM stock_movements WHERE product_id = 'prod_rice_1'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Baseline was 25000. Each transaction atomically adds 1 to inventory and 1 row to stock_movements.
    // If a transaction was partially completed, snap_inventory_qty != 25000 + snap_movements_count.
    assert_eq!(
        snap_inventory_qty,
        25000 + snap_movements_count,
        "Snapshot must represent one coherent committed state: inventory ({}) must match baseline + movements ({})",
        snap_inventory_qty,
        25000 + snap_movements_count
    );
}

// ==============================================================================================
// 7. AUTHORIZATION ENFORCEMENT (SECTION 11)
// ==============================================================================================

#[test]
fn test_07_authorization_admin_allowed_employee_denied() {
    let (db, _temp_dir, backup_dir) = setup_test_db("auth");
    let admin = get_admin(&db);
    let employee = get_employee(&db);

    // 1. Admin creates a valid backup
    let backup_meta =
        BackupService::create_backup(&db, &admin, "MANUAL", None, Some(&backup_dir)).unwrap();
    let backup_path = backup_dir.join(&backup_meta.file_name);

    // 2. Employee without BACKUP_RESTORE permission cannot create manual backup
    let emp_backup_res =
        BackupService::create_backup(&db, &employee, "MANUAL", None, Some(&backup_dir));
    assert!(
        emp_backup_res.is_err(),
        "Unauthorized employee must be denied backup creation"
    );

    // 3. Employee attempts restore: MUST BE STRICTLY REJECTED
    let emp_restore_res = BackupService::restore_backup_file(&db, &employee, &backup_path);
    assert!(
        emp_restore_res.is_err(),
        "Unauthorized employee must be strictly denied restore"
    );

    // 4. Admin performs restore: MUST BE ALLOWED
    let admin_restore_res = BackupService::restore_backup_file(&db, &admin, &backup_path);
    assert!(
        admin_restore_res.is_ok(),
        "Admin must be authorized to restore"
    );
}

// ==============================================================================================
// 8. AUDIT LOG INTEGRITY (SECTION 12)
// ==============================================================================================

#[test]
fn test_08_audit_log_records_events() {
    let (db, _temp_dir, backup_dir) = setup_test_db("audit");
    let admin = get_admin(&db);

    // 1. Create manual backup
    let meta =
        BackupService::create_backup(&db, &admin, "MANUAL", None, Some(&backup_dir)).unwrap();
    let backup_path = backup_dir.join(&meta.file_name);

    // Verify BACKUP_MANUAL_CREATE was logged
    let backup_audit_count: i64 = db
        .with_connection(|conn| {
            let c = conn.query_row(
                "SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_MANUAL_CREATE' AND entity_type = 'BACKUP'",
                [],
                |r| r.get(0),
            )?;
            Ok(c)
        })
        .unwrap();
    assert!(
        backup_audit_count > 0,
        "BACKUP_MANUAL_CREATE must be recorded in audit_logs"
    );

    // 2. Perform restore
    let _ = BackupService::restore_backup_file(&db, &admin, &backup_path).unwrap();

    // Verify RESTORE_ATTEMPT and RESTORE_SUCCESS were logged
    let (attempt_count, success_count): (i64, i64) = db
        .with_connection(|conn| {
            let att: i64 = conn.query_row(
                "SELECT COUNT(*) FROM audit_logs WHERE action = 'RESTORE_ATTEMPT' AND entity_type = 'BACKUP'",
                [],
                |r| r.get(0),
            )?;
            let succ: i64 = conn.query_row(
                "SELECT COUNT(*) FROM audit_logs WHERE action = 'RESTORE_SUCCESS' AND entity_type = 'BACKUP'",
                [],
                |r| r.get(0),
            )?;
            Ok((att, succ))
        })
        .unwrap();

    assert!(
        attempt_count > 0,
        "RESTORE_ATTEMPT must be recorded in audit_logs"
    );
    assert!(
        success_count > 0,
        "RESTORE_SUCCESS must be recorded in audit_logs"
    );
}

// ==============================================================================================
// 9. FAILED-RESTORE POST-SWITCH ROLLBACK SAFETY (GUARDRAIL 1)
// ==============================================================================================

#[test]
fn test_09_failed_restore_post_switch_rollback_preserves_original_state() {
    let (db, temp_dir, _backup_dir) = setup_test_db("post_switch_rollback");

    // 1. Establish initial business state with distinct transactions, stock, and ledgers
    let now = format!("{:?}", std::time::SystemTime::now());
    db.with_connection(|conn| {
        // Create an initial sale transaction
        conn.execute(
            "INSERT INTO sales (id, sale_number, customer_id, total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status, user_id, sale_date, created_at)
             VALUES ('sale_orig_1', 'INV-ORIG-01', 'cust_1', 15000, 12500, 2500, 'PARTIAL', 'user_admin_1', ?1, ?1)",
            rusqlite::params![now],
        )?;
        conn.execute(
            "INSERT INTO sale_items (id, sale_id, product_id, quantity, unit_price_cents, cost_price_cents, total_cents)
             VALUES ('item_orig_1', 'sale_orig_1', 'prod_rice_1', 1000, 15000, 10000, 15000)",
            [],
        )?;
        // Deduct inventory (25000 - 1000 = 24000)
        conn.execute(
            "UPDATE inventory SET current_quantity = 24000 WHERE product_id = 'prod_rice_1'",
            [],
        )?;
        // Customer credit ledger updated to 2500 cents
        conn.execute(
            "UPDATE customers SET current_credit_cents = 2500 WHERE id = 'cust_1'",
            [],
        )?;
        // Supplier ledger remains 0
        conn.execute(
            "UPDATE suppliers SET current_outstanding_cents = 0 WHERE id = 'supp_1'",
            [],
        )?;
        Ok(())
    })
    .expect("Initial transaction seeding failed");

    // Capture baseline values before restore attempt
    let (orig_stock, orig_credit, orig_supplier_debt, orig_sale_count) = db
        .with_connection(|conn| {
            let stock: i64 = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            let credit: i64 = conn.query_row(
                "SELECT current_credit_cents FROM customers WHERE id = 'cust_1'",
                [],
                |r| r.get(0),
            )?;
            let supp_debt: i64 = conn.query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_1'",
                [],
                |r| r.get(0),
            )?;
            let sale_cnt: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sales WHERE id = 'sale_orig_1'",
                [],
                |r| r.get(0),
            )?;
            Ok((stock, credit, supp_debt, sale_cnt))
        })
        .unwrap();

    assert_eq!(orig_stock, 24000);
    assert_eq!(orig_credit, 2500);
    assert_eq!(orig_supplier_debt, 0);
    assert_eq!(orig_sale_count, 1);

    // 2. Construct a candidate database that switches into the live database but intentionally fails
    // post-switch verification (e.g., foreign key violations and deactivated admin users).
    let candidate_path = temp_dir.join("candidate_violates_post_switch.db");
    {
        // Snapshot the current database as the base candidate
        db.create_consistent_snapshot(&candidate_path)
            .expect("Failed to snapshot base candidate");
        let cand_conn =
            rusqlite::Connection::open(&candidate_path).expect("Failed to open candidate db");
        // Disable foreign keys temporarily in candidate connection to inject foreign key violation
        cand_conn
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 INSERT INTO sale_items (id, sale_id, product_id, quantity, unit_price_cents, cost_price_cents, total_cents)
                 VALUES ('item_fk_violation', 'non_existent_sale_999', 'prod_rice_1', 1000, 15000, 10000, 15000);
                 UPDATE users SET is_active = 0 WHERE role = 'ADMIN';",
            )
            .expect("Failed to inject post-switch verification failures");
    }

    // 3. Directly trigger restore_from_snapshot_file to execute the page switch and verify post-switch rollback
    let restore_result = db.restore_from_snapshot_file(&candidate_path);

    // VERIFICATION CHECK 1: Restore must fail
    assert!(
        restore_result.is_err(),
        "Restore must fail when post-switch verification fails"
    );
    let err_string = restore_result.err().unwrap().to_string();
    assert!(
        err_string.contains("Post-switch verification failed")
            || err_string.contains("Original database successfully recovered"),
        "Error message must indicate post-switch verification failure and rollback: {}",
        err_string
    );

    // VERIFICATION CHECK 2: Original database remains intact
    // VERIFICATION CHECK 3: Original persistent IDs remain intact
    let (rice_exists, cust_exists, supp_exists, user_exists) = db
        .with_connection(|conn| {
            let r: i64 = conn.query_row(
                "SELECT COUNT(*) FROM products WHERE id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            let c: i64 = conn.query_row(
                "SELECT COUNT(*) FROM customers WHERE id = 'cust_1'",
                [],
                |r| r.get(0),
            )?;
            let s: i64 = conn.query_row(
                "SELECT COUNT(*) FROM suppliers WHERE id = 'supp_1'",
                [],
                |r| r.get(0),
            )?;
            let u: i64 = conn.query_row(
                "SELECT COUNT(*) FROM users WHERE id = 'user_admin_1'",
                [],
                |r| r.get(0),
            )?;
            Ok((r, c, s, u))
        })
        .unwrap();
    assert_eq!(
        rice_exists, 1,
        "Original product persistent ID 'prod_rice_1' must remain intact"
    );
    assert_eq!(
        cust_exists, 1,
        "Original customer persistent ID 'cust_1' must remain intact"
    );
    assert_eq!(
        supp_exists, 1,
        "Original supplier persistent ID 'supp_1' must remain intact"
    );
    assert_eq!(
        user_exists, 1,
        "Original user persistent ID 'user_admin_1' must remain intact"
    );

    // VERIFICATION CHECK 4: Original stock remains intact
    let post_rollback_stock: i64 = db
        .with_connection(|conn| {
            let q: i64 = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            Ok(q)
        })
        .unwrap();
    assert_eq!(
        post_rollback_stock, 24000,
        "Original stock quantity must be exactly preserved at 24000"
    );

    // VERIFICATION CHECK 5: Original ledgers remain intact
    let (post_rollback_credit, post_rollback_debt): (i64, i64) = db
        .with_connection(|conn| {
            let c = conn.query_row(
                "SELECT current_credit_cents FROM customers WHERE id = 'cust_1'",
                [],
                |r| r.get(0),
            )?;
            let d = conn.query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_1'",
                [],
                |r| r.get(0),
            )?;
            Ok((c, d))
        })
        .unwrap();
    assert_eq!(
        post_rollback_credit, 2500,
        "Original customer credit ledger must remain 2500"
    );
    assert_eq!(
        post_rollback_debt, 0,
        "Original supplier outstanding ledger must remain 0"
    );

    // VERIFICATION CHECK 6: Original transactions remain intact
    let (post_rollback_sales, fk_item_count): (i64, i64) = db
        .with_connection(|conn| {
            let s = conn.query_row(
                "SELECT COUNT(*) FROM sales WHERE id = 'sale_orig_1'",
                [],
                |r| r.get(0),
            )?;
            let fk = conn.query_row(
                "SELECT COUNT(*) FROM sale_items WHERE id = 'item_fk_violation'",
                [],
                |r| r.get(0),
            )?;
            Ok((s, fk))
        })
        .unwrap();
    assert_eq!(
        post_rollback_sales, 1,
        "Original sale transaction 'sale_orig_1' must remain intact"
    );
    assert_eq!(
        fk_item_count, 0,
        "Candidate violating row must NOT exist in the rolled-back database"
    );

    // VERIFICATION CHECK 7: Original database can still be opened and queried successfully
    let db_reopened = DatabaseManager::open(db.db_path().unwrap())
        .expect("Original database must open successfully after rollback");
    let active_admin_count: i64 = db_reopened
        .with_connection(|conn| {
            let c: i64 = conn.query_row(
                "SELECT COUNT(*) FROM users WHERE role = 'ADMIN' AND is_active = 1",
                [],
                |r| r.get(0),
            )?;
            Ok(c)
        })
        .unwrap();
    assert!(
        active_admin_count >= 1,
        "Reopened database must contain the original active admin user"
    );
}

// ==============================================================================================
// 10. CANCELLATION ZERO DATABASE CHANGES (SECTION 13 & GUARDRAILS)
// ==============================================================================================

#[test]
fn test_10_restore_cancellation_preserves_zero_database_changes() {
    let (db, _temp_dir, backup_dir) = setup_test_db("cancellation");
    let admin = get_admin(&db);

    // 1. Create a backup
    let backup_meta =
        BackupService::create_backup(&db, &admin, "MANUAL", None, Some(&backup_dir)).unwrap();
    let backup_file = backup_dir.join(&backup_meta.file_name);

    // 2. Capture baseline before user inspection / preparation
    let baseline_audit_count: i64 = db
        .with_connection(|conn| {
            let c: i64 = conn.query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))?;
            Ok(c)
        })
        .unwrap();

    // 3. User inspects and runs pre-flight validation (same as handleOpenRestoreModal in UI)
    let val_report = BackupService::validate_backup_file(&backup_file).unwrap();
    assert!(val_report.is_valid);

    // 4. User cancels the modal: no restore command is invoked
    // Verify that the live database had ZERO mutations, zero page replacements, and zero state alterations
    let (stock, audit_count): (i64, i64) = db
        .with_connection(|conn| {
            let s = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_1'",
                [],
                |r| r.get(0),
            )?;
            let a = conn.query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))?;
            Ok((s, a))
        })
        .unwrap();

    assert_eq!(
        stock, 25000,
        "Live stock must be completely unaffected by cancelled restore flow"
    );
    assert_eq!(
        audit_count, baseline_audit_count,
        "No audit logs or DB records should be created on cancel"
    );
}

