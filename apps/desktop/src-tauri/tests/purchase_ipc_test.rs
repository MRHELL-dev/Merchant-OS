use desktop_lib::auth::AuthService;
use desktop_lib::commands::{
    confirm_purchase_inner, create_supplier_for_purchase_inner, get_purchase_form_data_inner,
    prepare_purchase_inner, resolve_barcode_for_purchase_inner, AuthSession,
    ConfirmPurchaseIpcInput, CreateSupplierForPurchaseRequest, PreparePurchaseIpcInput,
    PreparedPurchaseCache, PurchaseItemIpcInput,
};
use desktop_lib::db::DatabaseManager;
use rusqlite::params;

/// Setup test database with seed business, products, suppliers, and admin/employee users.
fn setup_test_context() -> (
    DatabaseManager,
    AuthSession,
    PreparedPurchaseCache,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");
    let session = AuthSession::default();
    let cache = PreparedPurchaseCache::default();

    let (admin, emp_purchases, emp_suppliers_only) = db
        .with_connection(|conn| {
            // 1. Seed Business
            conn.execute(
                "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
                 VALUES ('biz_test', 'Test Merchant Store', '+919876543210', 'Market St, Delhi', '2026-09-13', '2026-09-13')
                 ON CONFLICT(id) DO NOTHING",
                [],
            )?;

            // 2. Create Initial Admin
            let admin_id = AuthService::create_initial_admin(
                conn,
                "admin_test",
                "AdminPass123!",
                "AdminPass123!",
                "Secret Question?",
                "Secret Answer",
            )?;

            // 3. Create Employee with PURCHASES permission
            let emp_p_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_purchases",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_p_id,
                "PURCHASES",
                true,
            )?;
            let emp_purchases = AuthService::authenticate(conn, "emp_purchases", "EmpPass123!")?;

            // 4. Create Employee with SUPPLIERS permission ONLY (no PURCHASES)
            let emp_s_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_suppliers_only",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_s_id,
                "SUPPLIERS",
                true,
            )?;
            let emp_suppliers_only = AuthService::authenticate(conn, "emp_suppliers_only", "EmpPass123!")?;

            // 5. Seed Test Supplier
            conn.execute(
                "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
                 VALUES ('supp_grain_traders', 'Grain Traders Co', '+919999900001', 'Wholesale Yard, Delhi', 0, 1, '2026-09-13', '2026-09-13')",
                [],
            )?;

            // 6. Seed Test Products:
            // - Rice 25kg Bag (Packaged, pcs)
            // - Loose Mustard Oil (Loose, litre)
            conn.execute(
                "INSERT INTO products (id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
                 VALUES ('prod_rice_bag', 'biz_test', 'Basmati Rice 25kg Bag', 'PACKAGED', 'pcs', 180000, 220000, 5000, 1, '2026-09-13', '2026-09-13')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_prod_rice_bag', 'prod_rice_bag', 10000, '2026-09-13')", // 10 bags (scale 1000)
                [],
            )?;

            conn.execute(
                "INSERT INTO products (id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
                 VALUES ('prod_mustard_oil', 'biz_test', 'Mustard Oil Pure', 'LOOSE', 'litre', 14000, 17500, 20000, 1, '2026-09-13', '2026-09-13')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_prod_mustard_oil', 'prod_mustard_oil', 50000, '2026-09-13')", // 50.000 L
                [],
            )?;

            // 7. Seed Barcode Mapping
            conn.execute(
                "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, created_at)
                 VALUES ('bm_rice', '890100000001', 'prod_rice_bag', 'MANUFACTURER', '2026-09-13')",
                [],
            )?;

            Ok((admin_id, emp_purchases, emp_suppliers_only))
        })
        .expect("Test context setup failed");

    (db, session, cache, admin, emp_purchases, emp_suppliers_only)
}

// ================================================================================================
// TESTS
// ================================================================================================

#[test]
fn test_01_prepare_purchase_is_read_only_with_zero_sqlite_mutations() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = PreparePurchaseIpcInput {
        supplier_id: "supp_grain_traders".to_string(),
        items: vec![
            PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 5000, // 5 bags
                unit_cost_cents: 175000, // actual buying price ₹1750.00
            },
            PurchaseItemIpcInput {
                product_id: "prod_mustard_oil".to_string(),
                quantity: 20500, // 20.500 L
                unit_cost_cents: 13500, // ₹135.00/L
            },
        ],
        paid_amount_cents: 1000000, // ₹10,000 paid
        payment_method: Some("BANK_TRANSFER".to_string()),
        purchase_date: Some("2026-09-13T10:00:00Z".to_string()),
    };

    let quote = prepare_purchase_inner(&db, &session, &cache, input)
        .expect("prepare_purchase should succeed");

    // 1. Verify Authoritative Calculation:
    // Line 1: 5 * 175000 = 875,000 cents
    // Line 2: round_half_up(20.5 * 13500) = 276,750 cents
    // Total: 875,000 + 276,750 = 1,151,750 cents (₹11,517.50)
    // Paid: 1,000,000 cents (₹10,000.00)
    // Credit / Due: 151,750 cents (₹1,517.50)
    assert_eq!(quote.total_amount_cents, 1151750);
    assert_eq!(quote.paid_amount_cents, 1000000);
    assert_eq!(quote.credit_amount_cents, 151750);
    assert_eq!(quote.payment_status, "PARTIAL");
    assert!(quote.preparation_token.starts_with("prep_"));
    assert_eq!(quote.preparation_token.len(), 69, "Token must have 256-bit cryptographically secure entropy (prep_ + 64 hex chars)");

    // 2. CRUCIAL ARCHITECTURE CHECK: ZERO SQLite mutations during preparation
    db.with_connection(|conn| {
        let p_count: i64 = conn.query_row("SELECT COUNT(*) FROM purchases", [], |r| r.get(0))?;
        assert_eq!(p_count, 0, "No purchase rows may be created during preparation");

        let pi_count: i64 = conn.query_row("SELECT COUNT(*) FROM purchase_items", [], |r| r.get(0))?;
        assert_eq!(pi_count, 0, "No purchase_item rows may be created during preparation");

        let mov_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(mov_count, 0, "No stock movements may be created during preparation");

        let pay_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(pay_count, 0, "No payment rows may be created during preparation");

        let sl_count: i64 = conn.query_row("SELECT COUNT(*) FROM supplier_ledger", [], |r| r.get(0))?;
        assert_eq!(sl_count, 0, "No supplier ledger entries may be created during preparation");

        let audit_count: i64 = conn.query_row("SELECT COUNT(*) FROM audit_logs WHERE action LIKE 'PURCHASE%'", [], |r| r.get(0))?;
        assert_eq!(audit_count, 0, "No purchase audit log rows may be created during preparation");

        let rice_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_bag'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rice_stock, 10000, "Inventory must remain unchanged during preparation");

        Ok(())
    }).expect("Database check failed");
}

#[test]
fn test_02_unauthenticated_and_unauthorized_preparation_rejected() {
    let (db, session, cache, _, _, emp_no_purchases) = setup_test_context();

    let input = PreparePurchaseIpcInput {
        supplier_id: "supp_grain_traders".to_string(),
        items: vec![PurchaseItemIpcInput {
            product_id: "prod_rice_bag".to_string(),
            quantity: 1000,
            unit_cost_cents: 180000,
        }],
        paid_amount_cents: 180000,
        payment_method: Some("CASH".to_string()),
        purchase_date: None,
    };

    // A. Unauthenticated session (None)
    session.set_identity(None);
    let err_unauth = prepare_purchase_inner(&db, &session, &cache, input.clone())
        .unwrap_err();
    assert!(err_unauth.contains("Unauthenticated"));

    // B. Authenticated employee WITHOUT 'PURCHASES' permission
    session.set_identity(Some(emp_no_purchases));
    let err_noperm = prepare_purchase_inner(&db, &session, &cache, input)
        .unwrap_err();
    assert!(err_noperm.contains("Permission denied") || err_noperm.contains("PURCHASES"));
}

#[test]
fn test_03_supplier_creation_permission_matrix() {
    let (db, session, _, admin, emp_purchases, emp_suppliers_only) = setup_test_context();

    let req = CreateSupplierForPurchaseRequest {
        name: "Punjab Agro Supplies".to_string(),
        phone: Some("+919811122233".to_string()),
    };

    // 1. Employee with SUPPLIERS only (no PURCHASES) -> DENIED
    session.set_identity(Some(emp_suppliers_only));
    let err_s_only = create_supplier_for_purchase_inner(&db, &session, req.clone())
        .unwrap_err();
    assert!(err_s_only.contains("Permission denied"));

    // 2. Employee with PURCHASES permission -> ALLOWED
    session.set_identity(Some(emp_purchases));
    let supp_emp = create_supplier_for_purchase_inner(&db, &session, req)
        .expect("Employee with PURCHASES permission should create supplier");
    assert_eq!(supp_emp.name, "Punjab Agro Supplies");

    // 3. Admin -> ALLOWED
    session.set_identity(Some(admin));
    let supp_admin = create_supplier_for_purchase_inner(&db, &session, CreateSupplierForPurchaseRequest {
        name: "Haryana Wholesale Dal Mills".to_string(),
        phone: None,
    }).expect("Admin should create supplier");
    assert_eq!(supp_admin.name, "Haryana Wholesale Dal Mills");
}

#[test]
fn test_04_session_bound_prepared_cache_isolation() {
    let (db, session, cache, admin, emp_purchases, _) = setup_test_context();

    // 1. User A (emp_purchases) prepares purchase
    session.set_identity(Some(emp_purchases.clone()));
    let quote = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 2000,
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 360000,
            payment_method: Some("CASH".to_string()),
            purchase_date: None,
        },
    ).expect("Preparation by emp_purchases should succeed");

    let prep_token = quote.preparation_token;

    // 2. User B (admin) attempts to confirm User A's token
    session.set_identity(Some(admin));
    let err_cross_user = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: prep_token.clone(),
        },
    ).unwrap_err();

    assert!(
        err_cross_user.contains("StaleOrInvalidPreparation") || err_cross_user.contains("not found") || err_cross_user.contains("another user session"),
        "A user must not be able to confirm a prepared purchase owned by another user: {}",
        err_cross_user
    );

    // 3. Verify User B's unauthorized attempt did NOT evict User A's token from cache
    session.set_identity(Some(emp_purchases));
    let receipt = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: prep_token,
        },
    ).expect("User A must still be able to confirm their own prepared purchase");
    assert_eq!(receipt.total_amount_cents, 360000);

    // Verify exactly one purchase was written
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM purchases", [], |r| r.get(0))?;
        assert_eq!(count, 1);
        Ok(())
    }).unwrap();
}

#[test]
fn test_05_single_use_consumption_and_anti_replay() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let quote = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 4000, // 4 bags
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 720000,
            payment_method: Some("CASH".to_string()),
            purchase_date: None,
        },
    ).unwrap();

    let prep_token = quote.preparation_token;

    // First confirmation -> SUCCEEDS (atomic execution)
    let receipt = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: prep_token.clone(),
        },
    ).expect("First confirmation must succeed");

    assert_eq!(receipt.total_amount_cents, 720000);
    assert_eq!(receipt.payment_status, "PAID");

    // Second confirmation attempt with same token -> REJECTED (anti-replay)
    let err_replay = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: prep_token,
        },
    ).unwrap_err();

    assert!(
        err_replay.contains("not found") || err_replay.contains("already confirmed"),
        "Replay must be rejected: {}",
        err_replay
    );

    // Verify stock only incremented ONCE (10,000 initial + 4,000 = 14,000)
    db.with_connection(|conn| {
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_bag'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 14000, "Stock must only be incremented once");
        Ok(())
    }).unwrap();
}

#[test]
fn test_06_stale_preparation_protection() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Prepare purchase
    let quote = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_mustard_oil".to_string(),
                quantity: 10000,
                unit_cost_cents: 14000,
            }],
            paid_amount_cents: 140000,
            payment_method: Some("UPI".to_string()),
            purchase_date: None,
        },
    ).unwrap();

    // Deactivate product before confirmation occurs
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE products SET is_active = 0 WHERE id = 'prod_mustard_oil'",
            [],
        )?;
        Ok(())
    }).unwrap();

    // Attempt confirmation -> MUST BE REJECTED by stale preparation protection
    let err_stale = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).unwrap_err();

    assert!(err_stale.contains("no longer active"));

    // Verify zero purchase rows created
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM purchases", [], |r| r.get(0))?;
        assert_eq!(count, 0);
        Ok(())
    }).unwrap();
}

#[test]
fn test_07_payment_status_and_ledger_credit_rules() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // A. CREDIT Purchase: Paid = 0
    let quote_credit = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000, // 1 bag
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 0,
            payment_method: None, // No payment method when paid = 0
            purchase_date: None,
        },
    ).unwrap();

    assert_eq!(quote_credit.payment_status, "CREDIT");
    assert_eq!(quote_credit.credit_amount_cents, 180000);

    let receipt_credit = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: quote_credit.preparation_token,
        },
    ).unwrap();

    assert_eq!(receipt_credit.payment_status, "CREDIT");
    assert_eq!(receipt_credit.credit_amount_cents, 180000);

    // Verify supplier ledger recorded 180,000 credit dues
    db.with_connection(|conn| {
        let dues: i64 = conn.query_row(
            "SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_grain_traders'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(dues, 180000);

        let pay_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM payments WHERE related_entity_id = ?1",
            params![receipt_credit.purchase_id],
            |r| r.get(0),
        )?;
        assert_eq!(pay_count, 0, "Credit purchase with 0 paid must not record payment");
        Ok(())
    }).unwrap();
}

#[test]
fn test_08_form_data_and_barcode_resolution_ipc() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // 1. Get purchase form data
    let form_data = get_purchase_form_data_inner(&db, &session)
        .expect("get_purchase_form_data should succeed");
    assert!(!form_data.suppliers.is_empty());
    assert!(!form_data.products.is_empty());

    // 2. Barcode resolution for rapid purchase scanning
    let prod = resolve_barcode_for_purchase_inner(&db, &session, "890100000001".to_string())
        .expect("resolve_barcode should resolve mapped barcode");
    assert_eq!(prod.id, "prod_rice_bag");
    assert_eq!(prod.name, "Basmati Rice 25kg Bag");
}

#[test]
fn test_09_race_safe_concurrent_confirmation() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    // Prepare a purchase
    let quote = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 5000, // 5 bags
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 900000,
            payment_method: Some("BANK_TRANSFER".to_string()),
            purchase_date: None,
        },
    ).unwrap();

    let prep_token = quote.preparation_token;

    let db_arc = Arc::new(db);
    let session_arc = Arc::new(session);
    let cache_arc = Arc::new(cache);
    let barrier = Arc::new(Barrier::new(2));

    let t1_db = Arc::clone(&db_arc);
    let t1_session = Arc::clone(&session_arc);
    let t1_cache = Arc::clone(&cache_arc);
    let t1_barrier = Arc::clone(&barrier);
    let t1_token = prep_token.clone();

    let handle1 = thread::spawn(move || {
        t1_barrier.wait();
        confirm_purchase_inner(
            &t1_db,
            &t1_session,
            &t1_cache,
            ConfirmPurchaseIpcInput {
                preparation_token: t1_token,
            },
        )
    });

    let t2_db = Arc::clone(&db_arc);
    let t2_session = Arc::clone(&session_arc);
    let t2_cache = Arc::clone(&cache_arc);
    let t2_barrier = Arc::clone(&barrier);
    let t2_token = prep_token;

    let handle2 = thread::spawn(move || {
        t2_barrier.wait();
        confirm_purchase_inner(
            &t2_db,
            &t2_session,
            &t2_cache,
            ConfirmPurchaseIpcInput {
                preparation_token: t2_token,
            },
        )
    });

    let res1 = handle1.join().expect("Thread 1 panicked");
    let res2 = handle2.join().expect("Thread 2 panicked");

    // Invariant: Exactly ONE thread succeeds, exactly ONE thread fails with StaleOrInvalidPreparation
    let success_count = (if res1.is_ok() { 1 } else { 0 }) + (if res2.is_ok() { 1 } else { 0 });
    let fail_count = (if res1.is_err() { 1 } else { 0 }) + (if res2.is_err() { 1 } else { 0 });

    assert_eq!(success_count, 1, "Exactly one concurrent confirmation may succeed");
    assert_eq!(fail_count, 1, "Exactly one concurrent confirmation must fail");

    let err_msg = if let Err(e) = res1 { e } else { res2.unwrap_err() };
    assert!(
        err_msg.contains("StaleOrInvalidPreparation") || err_msg.contains("not found") || err_msg.contains("already confirmed"),
        "The failing request must report stale/invalid preparation: {}",
        err_msg
    );

    // Verify database state: Exactly ONE purchase committed
    db_arc.with_connection(|conn| {
        let p_count: i64 = conn.query_row("SELECT COUNT(*) FROM purchases", [], |r| r.get(0))?;
        assert_eq!(p_count, 1, "Exactly one purchase row must exist in SQLite");

        let mov_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(mov_count, 1, "Exactly one stock movement must exist");

        let pay_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(pay_count, 1, "Exactly one payment must exist");
        Ok(())
    }).unwrap();
}

#[test]
fn test_10_logout_session_invalidation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // 1. Prepare purchase while logged in
    let quote = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 2000,
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 360000,
            payment_method: Some("CASH".to_string()),
            purchase_date: None,
        },
    ).unwrap();

    let prep_token = quote.preparation_token;

    // 2. User logs out / session destroyed
    session.logout(&cache);

    // 3. Attempt confirmation with old token while logged out -> fails (Unauthenticated)
    let err_logged_out = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: prep_token.clone(),
        },
    ).unwrap_err();
    assert!(err_logged_out.contains("Unauthenticated"));

    // 4. Log in again as the same user and try to confirm old token -> fails (cache was evicted on logout)
    let admin_identity = db.with_connection(|conn| Ok(AuthService::authenticate(conn, "admin_test", "AdminPass123!")?)).unwrap();
    session.set_identity(Some(admin_identity));

    let err_purged = confirm_purchase_inner(
        &db,
        &session,
        &cache,
        ConfirmPurchaseIpcInput {
            preparation_token: prep_token,
        },
    ).unwrap_err();
    assert!(
        err_purged.contains("StaleOrInvalidPreparation") || err_purged.contains("not found"),
        "Old token must be purged on logout: {}",
        err_purged
    );

    // Verify zero database changes
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM purchases", [], |r| r.get(0))?;
        assert_eq!(count, 0, "No purchases committed after logout");
        Ok(())
    }).unwrap();
}

#[test]
fn test_11_payment_model_strict_validation_and_overpayment_rejection() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // A. Overpayment: Total is 180,000, Paid is 200,000 -> REJECTED
    let err_overpay = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 200000, // Exceeds 180,000 total
            payment_method: Some("CASH".to_string()),
            purchase_date: None,
        },
    ).unwrap_err();
    assert!(err_overpay.contains("Overpayment rejected") || err_overpay.contains("exceeds total"));

    // B. CREDIT purchase (paid = 0) with a payment method specified -> REJECTED
    let err_credit_pm = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 0,
            payment_method: Some("UPI".to_string()), // Forbidden when paid is 0
            purchase_date: None,
        },
    ).unwrap_err();
    assert!(err_credit_pm.contains("Payment method cannot be specified when paid amount is zero"));

    // C. PAID purchase (paid > 0) without payment method -> REJECTED
    let err_no_pm = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: 180000,
            payment_method: None, // Required when paid > 0
            purchase_date: None,
        },
    ).unwrap_err();
    assert!(err_no_pm.contains("Payment method is required"));

    // D. Negative paid amount -> REJECTED
    let err_neg = prepare_purchase_inner(
        &db,
        &session,
        &cache,
        PreparePurchaseIpcInput {
            supplier_id: "supp_grain_traders".to_string(),
            items: vec![PurchaseItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
                unit_cost_cents: 180000,
            }],
            paid_amount_cents: -500,
            payment_method: Some("CASH".to_string()),
            purchase_date: None,
        },
    ).unwrap_err();
    assert!(err_neg.contains("negative"));
}
