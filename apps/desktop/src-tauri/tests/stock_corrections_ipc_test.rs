use desktop_lib::auth::AuthService;
use desktop_lib::commands::{
    confirm_stock_correction_inner, get_stock_corrections_form_data_inner,
    get_stock_corrections_summary_inner, prepare_stock_correction_inner, AuthSession,
    ConfirmStockCorrectionPayload, PrepareStockCorrectionPayload, PreparedStockCorrectionCache,
};
use desktop_lib::db::DatabaseManager;

/// Helper to set up a clean in-memory database with seeded business, users, products, and inventory.
fn setup_test_context() -> (
    DatabaseManager,
    AuthSession,
    PreparedStockCorrectionCache,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");
    let session = AuthSession::default();
    let cache = PreparedStockCorrectionCache::default();

    let (admin, admin_two, employee) = db
        .with_connection(|conn| {
            // 1. Seed Business
            conn.execute(
                "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
                 VALUES ('biz_test', 'Test Kirana Store', '+919876543210', 'Main Bazaar, Delhi', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')
                 ON CONFLICT(id) DO NOTHING",
                [],
            )?;

            // 2. Create Initial Admin
            let admin_id = AuthService::create_initial_admin(
                conn,
                "admin_super",
                "AdminSecret123!",
                "AdminSecret123!",
                "Security Question?",
                "Security Answer",
            )?;
            let admin = AuthService::authenticate(conn, "admin_super", "AdminSecret123!")?;

            // 3. Create Second Admin (for session isolation tests)
            let admin_two_id = AuthService::create_employee(
                conn,
                &admin_id,
                "admin_two",
                "AdminTwoPass123!",
            )?;
            conn.execute(
                "UPDATE users SET role = 'ADMIN' WHERE id = ?1",
                rusqlite::params![admin_two_id],
            )?;
            let admin_two = AuthService::authenticate(conn, "admin_two", "AdminTwoPass123!")?;

            // 4. Create Employee (cashier with no admin rights)
            let _emp_id = AuthService::create_employee(
                conn,
                &admin_id,
                "cashier_john",
                "CashierPass123!",
            )?;
            let employee = AuthService::authenticate(conn, "cashier_john", "CashierPass123!")?;

            // 5. Seed Catalog Products & Initial Inventory
            conn.execute(
                "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
                 VALUES ('prod_rice', NULL, 'Basmati Rice 5kg', 'kg', 25000, 32000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_rice', 'prod_rice', 20000, '2026-09-13T10:00:00Z')",
                [],
            )?; // 20.000 kg

            conn.execute(
                "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
                 VALUES ('prod_oil', NULL, 'Mustard Oil 1L', 'L', 14000, 18000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_oil', 'prod_oil', 10000, '2026-09-13T10:00:00Z')",
                [],
            )?; // 10.000 L

            conn.execute(
                "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
                 VALUES ('prod_inactive', NULL, 'Discontinued Soda', 'can', 3000, 5000, 0, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_inactive', 'prod_inactive', 5000, '2026-09-13T10:00:00Z')",
                [],
            )?;

            Ok((admin, admin_two, employee))
        })
        .expect("Failed to seed test context");

    (db, session, cache, admin, admin_two, employee)
}

#[test]
fn test_01_prepare_positive_stock_correction_success() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_rice".to_string(),
        quantity_change: 5000, // +5.000 kg (MISCOUNT found extra inventory)
        reason: "MISCOUNT".to_string(),
        note: "Physical stock audit found 1 extra unopened bag".to_string(),
    };

    let quote = prepare_stock_correction_inner(&db, &session, &cache, input)
        .expect("Failed to prepare positive stock correction");

    assert!(!quote.preparation_token.is_empty());
    assert_eq!(quote.product_id, "prod_rice");
    assert_eq!(quote.product_name, "Basmati Rice 5kg");
    assert_eq!(quote.quantity_change, 5000);
    assert_eq!(quote.quantity_before, 20000);
    assert_eq!(quote.quantity_after, 25000);
    assert_eq!(quote.reason, "MISCOUNT");
    assert_eq!(quote.admin_username, "admin_super");

    // Invariant: Read-only preparation boundary must not mutate database inventory
    db.with_connection(|conn| {
        let current_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(current_stock, 20000, "Preparation must not mutate database inventory");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_corrections", [], |r| r.get(0))?;
        assert_eq!(count, 0, "No stock_corrections row before confirmation");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_02_confirm_positive_stock_correction_success() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_rice".to_string(),
        quantity_change: 5000,
        reason: "MISCOUNT".to_string(),
        note: "Physical stock audit found extra bag".to_string(),
    };

    let quote = prepare_stock_correction_inner(&db, &session, &cache, input).unwrap();

    let confirm_input = ConfirmStockCorrectionPayload {
        preparation_token: quote.preparation_token.clone(),
    };

    let receipt = confirm_stock_correction_inner(&db, &session, &cache, confirm_input)
        .expect("Confirmation failed");

    assert_eq!(receipt.correction_id, quote.correction_id);
    assert_eq!(receipt.quantity_before, 20000);
    assert_eq!(receipt.quantity_after, 25000);
    assert_eq!(receipt.quantity_change, 5000);

    // Verify DB mutations: inventory updated, stock_corrections row inserted, stock_movements row inserted, audit_logs row inserted
    db.with_connection(|conn| {
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 25000, "Inventory must reflect +5000 delta");

        let (corr_product, corr_delta, corr_reason, corr_note, corr_admin): (String, i64, String, String, String) = conn.query_row(
            "SELECT product_id, quantity_change, reason, note, admin_user_id FROM stock_corrections WHERE id = ?1",
            rusqlite::params![receipt.correction_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        assert_eq!(corr_product, "prod_rice");
        assert_eq!(corr_delta, 5000);
        assert_eq!(corr_reason, "MISCOUNT");
        assert_eq!(corr_note, "Physical stock audit found extra bag");
        assert_eq!(corr_admin, admin.user_id());

        let (mov_type, ref_type, mov_delta, mov_before, mov_after): (String, String, i64, i64, i64) = conn.query_row(
            "SELECT movement_type, reference_type, quantity_change, quantity_before, quantity_after FROM stock_movements WHERE reference_id = ?1",
            rusqlite::params![receipt.correction_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        assert_eq!(mov_type, "CORRECTION");
        assert_eq!(ref_type, "STOCK_CORRECTIONS");
        assert_eq!(mov_delta, 5000);
        assert_eq!(mov_before, 20000);
        assert_eq!(mov_after, 25000);

        let audit_action: String = conn.query_row(
            "SELECT action FROM audit_logs WHERE entity_id = ?1",
            rusqlite::params![receipt.correction_id],
            |r| r.get(0),
        )?;
        assert_eq!(audit_action, "STOCK_CORRECTION");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_03_prepare_and_confirm_negative_stock_correction_damage() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_oil".to_string(),
        quantity_change: -3000, // -3.000 L (bottle broke during shelving)
        reason: "DAMAGED".to_string(),
        note: "3 bottles dropped and shattered during shelf stacking".to_string(),
    };

    let quote = prepare_stock_correction_inner(&db, &session, &cache, input).unwrap();
    assert_eq!(quote.quantity_before, 10000);
    assert_eq!(quote.quantity_after, 7000);
    assert_eq!(quote.quantity_change, -3000);
    assert_eq!(quote.reason, "DAMAGED");

    let receipt = confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: quote.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.quantity_after, 7000);

    db.with_connection(|conn| {
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 7000);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_04_rejection_insufficient_stock_negative_delta() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    // Available is 10000, trying to deduct 15000
    let input = PrepareStockCorrectionPayload {
        product_id: "prod_oil".to_string(),
        quantity_change: -15000,
        reason: "LOST".to_string(),
        note: "Attempted reduction exceeds available stock".to_string(),
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    let err_msg = result.err().unwrap();
    assert!(err_msg.contains("Insufficient stock") || err_msg.contains("insufficient"));

    // DB state must be untouched
    db.with_connection(|conn| {
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 10000);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_05_rejection_zero_delta() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_oil".to_string(),
        quantity_change: 0,
        reason: "MISCOUNT".to_string(),
        note: "Zero delta should fail".to_string(),
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("cannot be zero"));
}

#[test]
fn test_06_rejection_invalid_reason() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_rice".to_string(),
        quantity_change: -1000,
        reason: "STOLEN_BY_ALIENS".to_string(),
        note: "Invalid reason".to_string(),
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("Invalid correction reason"));
}

#[test]
fn test_07_rejection_empty_note() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_rice".to_string(),
        quantity_change: -1000,
        reason: "EXPIRED".to_string(),
        note: "   ".to_string(), // Blank
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("explanatory note is required"));
}

#[test]
fn test_08_rejection_inactive_product() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_inactive".to_string(),
        quantity_change: 1000,
        reason: "MISCOUNT".to_string(),
        note: "Testing inactive product".to_string(),
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("inactive"));
}

#[test]
fn test_09_rejection_nonexistent_product() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "nonexistent_prod".to_string(),
        quantity_change: 1000,
        reason: "MISCOUNT".to_string(),
        note: "Testing nonexistent product".to_string(),
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("not found"));
}

#[test]
fn test_10_authorization_employee_denied_prepare() {
    let (db, session, cache, _, _, employee) = setup_test_context();
    session.set_identity(Some(employee.clone()));

    let input = PrepareStockCorrectionPayload {
        product_id: "prod_rice".to_string(),
        quantity_change: -1000,
        reason: "DAMAGED".to_string(),
        note: "Employee attempting correction".to_string(),
    };

    let result = prepare_stock_correction_inner(&db, &session, &cache, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("Only administrators"));
}

#[test]
fn test_11_authorization_employee_denied_confirm() {
    let (db, session, cache, admin, _, employee) = setup_test_context();

    // Admin prepares quote
    session.set_identity(Some(admin.clone()));
    let quote = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_rice".to_string(),
            quantity_change: -1000,
            reason: "EXPIRED".to_string(),
            note: "Expired pack".to_string(),
        },
    )
    .unwrap();

    // Switch to employee session
    session.set_identity(Some(employee.clone()));
    let result = confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("Only administrators"));
}

#[test]
fn test_12_anti_replay_single_use_token() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let quote = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_rice".to_string(),
            quantity_change: 2000,
            reason: "MISCOUNT".to_string(),
            note: "Audit reconciliation".to_string(),
        },
    )
    .unwrap();

    // First confirmation succeeds
    let confirm_input = ConfirmStockCorrectionPayload {
        preparation_token: quote.preparation_token.clone(),
    };
    let res1 = confirm_stock_correction_inner(&db, &session, &cache, confirm_input.clone());
    assert!(res1.is_ok());

    // Replay attempt fails immediately because token is consumed single-use
    let res2 = confirm_stock_correction_inner(&db, &session, &cache, confirm_input);
    assert!(res2.is_err());
    assert!(res2.err().unwrap().contains("Invalid, expired, or already-used"));

    // Inventory must have incremented exactly once (+2000), NOT duplicated (+4000)
    db.with_connection(|conn| {
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 22000, "Replay must not duplicate stock delta");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_13_session_isolation_different_user_cannot_confirm() {
    let (db, session, cache, admin1, admin2, _) = setup_test_context();

    // Admin 1 prepares quote
    session.set_identity(Some(admin1.clone()));
    let quote = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_oil".to_string(),
            quantity_change: -1000,
            reason: "LOST".to_string(),
            note: "Shrinkage missing bottle".to_string(),
        },
    )
    .unwrap();

    // Admin 2 attempts to confirm Admin 1's token
    session.set_identity(Some(admin2.clone()));
    let result = confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: quote.preparation_token.clone(),
        },
    );
    assert!(result.is_err(), "Different user must not be able to confirm another user's quote");
    assert!(result.err().unwrap().contains("Invalid, expired, or already-used"));
}

#[test]
fn test_14_logout_cache_invalidation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let quote = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_oil".to_string(),
            quantity_change: -2000,
            reason: "EXPIRED".to_string(),
            note: "Discarded expired oil".to_string(),
        },
    )
    .unwrap();

    // Explicit logout clears stock corrections for this user
    session.logout_stock_corrections(&cache);

    // Confirmation now fails
    session.set_identity(Some(admin.clone()));
    let result = confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("Invalid, expired, or already-used"));
}

#[test]
fn test_15_live_revalidation_concurrent_stock_depletion() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    // Available is 10000. Prepare to remove 8000 (leaves 2000).
    let quote = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_oil".to_string(),
            quantity_change: -8000,
            reason: "DAMAGED".to_string(),
            note: "Bulk damage in transport".to_string(),
        },
    )
    .unwrap();

    // Concurrent event: A sale or POS transaction drops stock to 4000 before confirmation
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE inventory SET current_quantity = 4000 WHERE product_id = 'prod_oil'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // Confirming quote (-8000 on current 4000) would yield -4000.
    // Live revalidation must detect this and reject the confirmation atomically!
    let result = confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(result.is_err(), "Confirmation must fail live revalidation if stock dropped");
    let err_str = result.err().unwrap();
    assert!(err_str.contains("Insufficient stock") || err_str.contains("Available"));

    // DB state must be maintained at 4000, no partial changes
    db.with_connection(|conn| {
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 4000);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_corrections", [], |r| r.get(0))?;
        assert_eq!(count, 0);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_16_get_stock_corrections_form_data() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let form_data = get_stock_corrections_form_data_inner(&db, &session)
        .expect("Failed to fetch form data");

    // Only active products returned ('prod_rice' and 'prod_oil'), 'prod_inactive' omitted
    assert_eq!(form_data.products.len(), 2);
    let rice = form_data.products.iter().find(|p| p.id == "prod_rice").unwrap();
    assert_eq!(rice.name, "Basmati Rice 5kg");
    assert_eq!(rice.current_quantity, 20000);
    assert_eq!(rice.unit, "kg");
}

#[test]
fn test_17_get_stock_corrections_summary() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Initially summary is empty
    let summary_before = get_stock_corrections_summary_inner(&db, &session).unwrap();
    assert_eq!(summary_before.total_corrections_count, 0);

    // Apply two corrections
    let q1 = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_rice".to_string(),
            quantity_change: 3000,
            reason: "MISCOUNT".to_string(),
            note: "First correction".to_string(),
        },
    )
    .unwrap();
    confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: q1.preparation_token,
        },
    )
    .unwrap();

    let q2 = prepare_stock_correction_inner(
        &db,
        &session,
        &cache,
        PrepareStockCorrectionPayload {
            product_id: "prod_oil".to_string(),
            quantity_change: -1000,
            reason: "EXPIRED".to_string(),
            note: "Second correction".to_string(),
        },
    )
    .unwrap();
    confirm_stock_correction_inner(
        &db,
        &session,
        &cache,
        ConfirmStockCorrectionPayload {
            preparation_token: q2.preparation_token,
        },
    )
    .unwrap();

    let summary_after = get_stock_corrections_summary_inner(&db, &session).unwrap();
    assert_eq!(summary_after.total_corrections_count, 2);
    assert_eq!(summary_after.corrections.len(), 2);

    // Verify ordering: newest first
    assert_eq!(summary_after.corrections[0].product_id, "prod_oil");
    assert_eq!(summary_after.corrections[0].reason, "EXPIRED");
    assert_eq!(summary_after.corrections[1].product_id, "prod_rice");
    assert_eq!(summary_after.corrections[1].reason, "MISCOUNT");
}
