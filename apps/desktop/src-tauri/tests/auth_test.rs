use desktop_lib::auth::{
    verify_password, AuthError, AuthService,
    AuthorizationService, PermissionKey, Role,
};
use desktop_lib::db::DatabaseManager;
use desktop_lib::engine::{
    BusinessEngine, ConfirmSaleRequest, EngineError, RecordStockCorrectionRequest,
    SaleItemRequest, TransactionEngine,
};

fn setup_test_db() -> DatabaseManager {
    let db = DatabaseManager::open_in_memory().expect("Failed to create in-memory database");
    db.with_connection(|conn| {
        // Seed default business
        conn.execute(
            "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
             VALUES ('biz_test_1', 'Test Kirana Store', '+919988776655', 'Test Market, New Delhi', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        Ok(())
    }).expect("Failed to seed test business");
    db
}

// ================================================================================================
// 1. AUTHENTICATION & PASSWORD HASHING TESTS (Scenarios 1 - 7)
// ================================================================================================

#[test]
fn test_01_password_is_argon2id_hash_and_no_plaintext_stored() {
    let db = setup_test_db();
    let raw_password = "SecretPassword@123";

    db.with_connection(|conn| {
        let identity = AuthService::create_initial_admin(
            conn,
            "admin_test",
            raw_password,
            raw_password,
            "What is your primary school?",
            "St Marys",
        )?;

        assert_eq!(identity.username(), "admin_test");
        assert_eq!(identity.role(), Role::Admin);

        // Verify stored hash in SQLite
        let hash: String = conn.query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            rusqlite::params![identity.user_id()],
            |r| r.get(0),
        )?;

        assert!(hash.starts_with("$argon2id$"), "Hash must be an Argon2id hash");
        assert_ne!(hash, raw_password, "Hash must never match plaintext");

        // Verify that raw plaintext password does not appear anywhere in database
        let count_in_users: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE password_hash = ?1",
            rusqlite::params![raw_password],
            |r| r.get(0),
        )?;
        assert_eq!(count_in_users, 0);

        // Verify password verification API
        assert!(verify_password(raw_password, &hash).unwrap());
        assert!(!verify_password("WrongPassword123", &hash).unwrap());

        Ok(())
    }).expect("Test 01 failed");
}

#[test]
fn test_02_password_verification_and_generic_failure_anti_enumeration() {
    let db = setup_test_db();
    let correct_pass = "AdminSecr3t!";

    db.with_connection(|conn| {
        AuthService::create_initial_admin(
            conn,
            "admin_enum",
            correct_pass,
            correct_pass,
            "What was your first car?",
            "Maruti 800",
        )?;

        // 1. Correct password succeeds
        let identity = AuthService::authenticate(conn, "admin_enum", correct_pass)?;
        assert_eq!(identity.username(), "admin_enum");

        // 2. Wrong password fails with InvalidCredentials
        let err_wrong_pass = AuthService::authenticate(conn, "admin_enum", "WrongPassword!").unwrap_err();
        assert_eq!(err_wrong_pass, AuthError::InvalidCredentials);

        // 3. Unknown username fails with the EXACT same InvalidCredentials error (prevents enumeration)
        let err_unknown_user = AuthService::authenticate(conn, "non_existent_user", correct_pass).unwrap_err();
        assert_eq!(err_unknown_user, AuthError::InvalidCredentials);

        Ok(())
    }).expect("Test 02 failed");
}

#[test]
fn test_03_inactive_user_cannot_authenticate() {
    let db = setup_test_db();
    let pass = "EmployeePass1!";

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "admin_act",
            "AdminPass123!",
            "AdminPass123!",
            "Security Question?",
            "Answer",
        )?;

        let emp_id = AuthService::create_employee(conn, &admin, "inactive_cashier", pass)?;

        // Deactivate employee account
        conn.execute(
            "UPDATE users SET is_active = 0 WHERE id = ?1",
            rusqlite::params![emp_id],
        )?;

        let err = AuthService::authenticate(conn, "inactive_cashier", pass).unwrap_err();
        assert_eq!(err, AuthError::UserInactive);

        Ok(())
    }).expect("Test 03 failed");
}

#[test]
fn test_04_duplicate_username_rejected() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "duplicate_user",
            "AdminPass123!",
            "AdminPass123!",
            "Security Question?",
            "Answer",
        )?;

        // Attempt to create employee with same username
        let err = AuthService::create_employee(conn, &admin, "duplicate_user", "Pass123456").unwrap_err();
        assert!(matches!(err, AuthError::DatabaseError(_)));

        Ok(())
    }).expect("Test 04 failed");
}

// ================================================================================================
// 2. INITIAL ADMIN & ADMIN AUTHORITY TESTS (Scenarios 8 - 12)
// ================================================================================================

#[test]
fn test_05_initial_admin_setup_authoritative_and_second_admin_rejected() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        // First admin succeeds
        let admin = AuthService::create_initial_admin(
            conn,
            "main_admin",
            "SuperSecretAdmin1!",
            "SuperSecretAdmin1!",
            "First pet?",
            "Sheru",
        )?;
        assert!(admin.is_admin());

        // Second initial admin attempt MUST be rejected
        let err = AuthService::create_initial_admin(
            conn,
            "imposter_admin",
            "ImposterPass1!",
            "ImposterPass1!",
            "First pet?",
            "Tommy",
        ).unwrap_err();
        assert_eq!(err, AuthError::AdminAlreadyExists);

        // Verify only 1 ADMIN exists in the database
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'ADMIN'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);

        Ok(())
    }).expect("Test 05 failed");
}

#[test]
fn test_06_admin_is_unrestricted_by_employee_permission_rows() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "unrestricted_admin",
            "AdminPass123!",
            "AdminPass123!",
            "First school?",
            "Delhi Public",
        )?;

        // Ensure 0 permission rows exist for this admin
        let perm_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM permissions WHERE user_id = ?1",
            rusqlite::params![admin.user_id()],
            |r| r.get(0),
        )?;
        assert_eq!(perm_count, 0, "Admin must not require permission rows");

        // Admin is authorized for ALL standard keys
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Dashboard.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Sales.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Purchases.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Inventory.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Prices.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Customers.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::CustomerCredits.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::CustomerOrders.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Suppliers.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Returns.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Correction.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Employees.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Permissions.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Expenses.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::TransactionHistory.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::Reports.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::BusinessProfile.as_str()).is_ok());
        assert!(AuthorizationService::authorize(conn, &admin, PermissionKey::BackupRestore.as_str()).is_ok());

        // Admin is authorized for any future custom feature
        assert!(AuthorizationService::authorize(conn, &admin, "FUTURE_AI_COPILOT").is_ok());

        // Admin passes require_admin check
        assert!(AuthorizationService::require_admin(&admin, "critical operation").is_ok());

        Ok(())
    }).expect("Test 06 failed");
}

// ================================================================================================
// 3. EMPLOYEE CREATION & PRIVILEGE ESCALATION REJECTION (Scenarios 13 - 21)
// ================================================================================================

#[test]
fn test_07_employee_lifecycle_and_privilege_escalation_impossible() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "boss_admin",
            "BossPassword123!",
            "BossPassword123!",
            "Childhood hero?",
            "Kalam",
        )?;

        // 1. Admin creates Employee
        let emp_id = AuthService::create_employee(conn, &admin, "clerk_ramu", "RamuPassword123!")?;
        let emp_identity = AuthService::authenticate(conn, "clerk_ramu", "RamuPassword123!")?;
        assert_eq!(emp_identity.role(), Role::Employee);
        assert!(!emp_identity.is_admin());

        // 2. Employee cannot create another employee
        let err_create = AuthService::create_employee(conn, &emp_identity, "clerk_shamu", "Shamu123!").unwrap_err();
        assert!(matches!(err_create, AuthError::AdminAuthorizationRequired(_)));

        // 3. Employee cannot grant permissions to self
        let err_perm = AuthService::set_employee_permission(
            conn,
            &emp_identity,
            &emp_id,
            PermissionKey::Sales.as_str(),
            true,
        ).unwrap_err();
        assert!(matches!(err_perm, AuthError::AdminAuthorizationRequired(_)));

        // 4. Employee cannot pass require_admin
        let err_admin_req = AuthorizationService::require_admin(&emp_identity, "any_admin_op").unwrap_err();
        assert!(matches!(err_admin_req, AuthError::AdminAuthorizationRequired(_)));

        // Verify clerk_shamu was NOT created in SQLite
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE username = 'clerk_shamu'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 0);

        Ok(())
    }).expect("Test 07 failed");
}

// ================================================================================================
// 4. PERMISSION ENFORCEMENT & REVOCATION TESTS (Scenarios 22 - 28)
// ================================================================================================

#[test]
fn test_08_employee_explicit_permissions_and_immediate_revocation() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "perm_admin",
            "AdminPass123!",
            "AdminPass123!",
            "City of birth?",
            "Patna",
        )?;

        let emp_id = AuthService::create_employee(conn, &admin, "sales_cashier", "Cashier123!")?;
        let emp = AuthService::authenticate(conn, "sales_cashier", "Cashier123!")?;

        // 1. Initially, employee has NO permissions enabled -> DENIED
        let err = AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).unwrap_err();
        assert!(matches!(err, AuthError::PermissionDenied { .. }));

        // 2. Admin grants SALES permission -> ALLOWED
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Sales.as_str(), true)?;
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).is_ok());

        // 3. Other permissions remain DENIED
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Purchases.as_str()).is_err());
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Correction.as_str()).is_err());

        // 4. Admin disables SALES permission -> immediately DENIED
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Sales.as_str(), false)?;
        let err_revoked = AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).unwrap_err();
        assert!(matches!(err_revoked, AuthError::PermissionDenied { .. }));

        // 5. Deleting row from permissions table -> immediately DENIED
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Sales.as_str(), true)?;
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).is_ok());
        conn.execute("DELETE FROM permissions WHERE user_id = ?1", rusqlite::params![emp_id])?;
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).is_err());

        Ok(())
    }).expect("Test 08 failed");
}

// ================================================================================================
// 5. PROTECTED BUSINESS OPERATIONS & TRANSACTION ENGINE INTEGRATION (Scenarios 29 - 32)
// ================================================================================================

#[test]
fn test_09_protected_operations_require_valid_authorization_zero_side_effects() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "biz_admin",
            "AdminPass123!",
            "AdminPass123!",
            "School?",
            "DAV",
        )?;

        // Seed inventory product
        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_tea', NULL, 'Assam Tea 500g', 'pack', 12000, 15000, 1, '2026-09-13', '2026-09-13')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_tea', 'prod_tea', 20000, '2026-09-13')",
            [],
        )?;

        let emp_id = AuthService::create_employee(conn, &admin, "tea_cashier", "Pass12345!")?;
        let emp = AuthService::authenticate(conn, "tea_cashier", "Pass12345!")?;

        let sale_req = ConfirmSaleRequest {
            sale_id: "sale_auth_1".to_string(),
            sale_number: "INV-AUTH-1".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_tea".to_string(),
                quantity: 2000,
                unit_price_cents: 15000,
            }],
            paid_amount_cents: 30000,
            payment_method: Some("CASH".to_string()),
            user_id: emp_id.clone(),
            sale_date: "2026-09-13".to_string(),
        };

        // A. Employee lacks SALES permission -> BusinessEngine denies operation
        let err = BusinessEngine::prepare_sale(conn, &emp, sale_req.clone()).unwrap_err();
        assert!(matches!(err, EngineError::PermissionDenied { .. }));

        // Verify zero SQLite mutations occurred
        let sale_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales WHERE id = 'sale_auth_1'", [], |r| r.get(0))?;
        assert_eq!(sale_count, 0);
        let stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_tea'", [], |r| r.get(0))?;
        assert_eq!(stock, 20000);

        // B. Admin grants SALES permission -> preparation and execution succeed
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Sales.as_str(), true)?;
        let prepared = BusinessEngine::prepare_sale(conn, &emp, sale_req)?;
        let confirmed = prepared.confirm(&emp);
        TransactionEngine::execute_sale(conn, confirmed)?;

        // Verify stock deducted to 18,000
        let new_stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_tea'", [], |r| r.get(0))?;
        assert_eq!(new_stock, 18000);

        // C. Employee attempts stock correction (Admin-only) -> DENIED
        let corr_req = RecordStockCorrectionRequest {
            correction_id: "corr_fail".to_string(),
            product_id: "prod_tea".to_string(),
            quantity_change: -1000,
            reason: "DAMAGED".to_string(),
            note: "Unauthorized correction".to_string(),
            admin_user_id: emp_id.clone(),
        };
        let err_corr = BusinessEngine::prepare_stock_correction(conn, &emp, corr_req).unwrap_err();
        assert!(matches!(err_corr, EngineError::AdminAuthorizationRequired(_)));

        // Verify stock remains exactly 18,000
        let unchanged_stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_tea'", [], |r| r.get(0))?;
        assert_eq!(unchanged_stock, 18000);

        Ok(())
    }).expect("Test 09 failed");
}

// ================================================================================================
// 6. SPOOFING & ATTACK ATTEMPT TESTS (Scenario 32)
// ================================================================================================

#[test]
fn test_10_spoofing_attacks_and_audit_actor_integrity() {
    let db = setup_test_db();

    db.with_connection(|conn| {
        let admin = AuthService::create_initial_admin(
            conn,
            "real_admin",
            "AdminPass123!",
            "AdminPass123!",
            "Security?",
            "Yes",
        )?;

        let emp_id = AuthService::create_employee(conn, &admin, "malicious_actor", "ActorPass123!")?;
        let emp = AuthService::authenticate(conn, "malicious_actor", "ActorPass123!")?;
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Sales.as_str(), true)?;

        // Also create an innocent victim employee who exists in the system
        let victim_emp_id = AuthService::create_employee(conn, &admin, "innocent_victim", "VictimPass123!")?;

        // Seed product & inventory
        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_rice', NULL, 'Basmati Rice 1kg', 'kg', 8000, 10000, 1, '2026-09-13', '2026-09-13')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_rice', 'prod_rice', 50000, '2026-09-13')",
            [],
        )?;

        // ATTACK 1: Audit & sale actor spoofing.
        // Malicious user tries to set user_id: victim_emp_id in request payload.
        let sale_req = ConfirmSaleRequest {
            sale_id: "sale_spoof_audit".to_string(),
            sale_number: "INV-SPF-1".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_rice".to_string(),
                quantity: 5000,
                unit_price_cents: 10000,
            }],
            paid_amount_cents: 50000,
            payment_method: Some("CASH".to_string()),
            user_id: victim_emp_id.clone(), // Spoofed user_id in payload
            sale_date: "2026-09-13".to_string(),
        };

        let prepared = BusinessEngine::prepare_sale(conn, &emp, sale_req)?;
        let confirmed = prepared.confirm(&emp); // Confirmed with real authenticated identity
        TransactionEngine::execute_sale(conn, confirmed)?;

        // Verify the sales table recorded the authoritative authenticated user ID, NOT victim_emp_id!
        let sale_user: String = conn.query_row(
            "SELECT user_id FROM sales WHERE id = 'sale_spoof_audit'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(sale_user, emp.user_id(), "Sale record MUST record authenticated identity, not spoofed payload user");
        assert_ne!(sale_user, victim_emp_id);

        // Verify the audit log recorded the authoritative authenticated user ID, NOT victim_emp_id!
        let audit_user: String = conn.query_row(
            "SELECT user_id FROM audit_logs WHERE entity_id = 'sale_spoof_audit' AND action = 'SALE_CONFIRMED'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(audit_user, emp.user_id(), "Audit log MUST record authenticated identity, not spoofed payload user");
        assert_ne!(audit_user, victim_emp_id);

        // ATTACK 2: Admin ID spoofing.
        // Employee provides admin's user_id in payload to attempt stock correction.
        let corr_req = RecordStockCorrectionRequest {
            correction_id: "corr_spoof".to_string(),
            product_id: "prod_rice".to_string(),
            quantity_change: -5000,
            reason: "LOST".to_string(),
            note: "Attempting to spoof admin".to_string(),
            admin_user_id: admin.user_id().to_string(), // Spoofed admin user_id
        };

        // Caller identity is emp. Even with admin_user_id in payload, BusinessEngine checks caller's identity!
        let err = BusinessEngine::prepare_stock_correction(conn, &emp, corr_req).unwrap_err();
        assert!(matches!(err, EngineError::AdminAuthorizationRequired(_)));

        Ok(())
    }).expect("Test 10 failed");
}

// ================================================================================================
// 7. OFFLINE PASSWORD RECOVERY & CREDENTIAL SAFETY IN AUDIT LOGS (Scenarios 33 - 42)
// ================================================================================================

#[test]
fn test_11_offline_password_recovery_and_credential_sanitization() {
    let db = setup_test_db();
    let old_pass = "OldAdminPass123!";
    let new_pass = "NewAdminPass456!";
    let security_answer = "  HyDeRaBaD  "; // Mixed casing and spaces

    db.with_connection(|conn| {
        AuthService::create_initial_admin(
            conn,
            "recoverable_admin",
            old_pass,
            old_pass,
            "What is your hometown?",
            security_answer,
        )?;

        // 1. Security answer is stored hashed; plaintext does NOT exist in system_metadata
        let stored_answer_hash: String = conn.query_row(
            "SELECT value FROM system_metadata WHERE key = 'admin_security_answer_hash'",
            [],
            |r| r.get(0),
        )?;
        assert!(stored_answer_hash.starts_with("$argon2id$"));
        assert!(!stored_answer_hash.contains("HyDeRaBaD"));
        assert!(!stored_answer_hash.contains("hyderabad"));

        // 2. Incorrect security answer is rejected
        let err_wrong_ans = AuthService::reset_admin_password(
            conn,
            "recoverable_admin",
            "Mumbai",
            new_pass,
            new_pass,
        ).unwrap_err();
        assert_eq!(err_wrong_ans, AuthError::IncorrectSecurityAnswer);

        // 3. Correct answer with different whitespace/casing succeeds (normalized: trim -> lowercase -> Argon2id)
        AuthService::reset_admin_password(
            conn,
            "recoverable_admin",
            "hyderabad",
            new_pass,
            new_pass,
        )?;

        // 4. Old password no longer works
        let old_auth = AuthService::authenticate(conn, "recoverable_admin", old_pass).unwrap_err();
        assert_eq!(old_auth, AuthError::InvalidCredentials);

        // 5. New password succeeds
        let new_identity = AuthService::authenticate(conn, "recoverable_admin", new_pass)?;
        assert_eq!(new_identity.username(), "recoverable_admin");

        // 6. Verify PASSWORD_RESET audit entry exists and contains ZERO passwords or secrets
        let (audit_action, audit_details): (String, String) = conn.query_row(
            "SELECT action, details FROM audit_logs WHERE action = 'PASSWORD_RESET' ORDER BY created_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(audit_action, "PASSWORD_RESET");
        assert!(!audit_details.contains(old_pass));
        assert!(!audit_details.contains(new_pass));
        assert!(!audit_details.contains("hyderabad"));
        assert!(!audit_details.contains("$argon2id$"));

        // 7. Verify all audit logs in database contain no credentials or hashes
        let mut stmt = conn.prepare("SELECT details FROM audit_logs")?;
        let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
        for row in rows {
            if let Some(details) = row? {
                assert!(!details.contains(old_pass));
                assert!(!details.contains(new_pass));
                assert!(!details.contains("$argon2id$"));
                assert!(!details.contains("HyDeRaBaD"));
            }
        }

        Ok(())
    }).expect("Test 11 failed");
}
