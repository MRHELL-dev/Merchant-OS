use desktop_lib::auth::AuthService;
use desktop_lib::commands::{
    confirm_sale_inner, create_customer_for_sale_inner, get_sales_form_data_inner,
    prepare_sale_inner, resolve_barcode_for_sale_inner, AuthSession,
    ConfirmSaleIpcInput, CreateCustomerForSaleRequest, PrepareSaleIpcInput,
    PreparedSaleCache, SaleItemIpcInput,
};
use desktop_lib::db::DatabaseManager;
use rusqlite::params;
use std::sync::Arc;
use std::thread;

/// Setup test database with seed business, products, customer, and admin/employee users.
fn setup_test_context() -> (
    DatabaseManager,
    AuthSession,
    PreparedSaleCache,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");
    let session = AuthSession::default();
    let cache = PreparedSaleCache::default();

    let (admin, emp_sales, emp_unauthorized) = db
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
            let admin = AuthService::authenticate(conn, "admin_test", "AdminPass123!")?;

            // 3. Create Employee with SALES permission
            let emp_s_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_sales",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_s_id,
                "SALES",
                true,
            )?;
            let emp_sales = AuthService::authenticate(conn, "emp_sales", "EmpPass123!")?;

            // 4. Create Employee with PURCHASES permission ONLY (unauthorized for SALES)
            let emp_p_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_purchases_only",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_p_id,
                "PURCHASES",
                true,
            )?;
            let emp_unauthorized = AuthService::authenticate(conn, "emp_purchases_only", "EmpPass123!")?;

            // 5. Seed Test Customer
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_sharma', 'Ramesh Sharma', '+919811100001', 'Block B, Sector 4, Noida', 0, 1, '2026-09-13', '2026-09-13')",
                [],
            )?;

            // 6. Seed Test Products:
            // - Rice 25kg Bag (Packaged, pcs, cost 180000, selling 220000, 10 pcs in stock)
            // - Loose Mustard Oil (Loose, litre, cost 14000, selling 17500, 25.000 litres in stock)
            conn.execute(
                "INSERT INTO products (id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
                 VALUES ('prod_rice_bag', 'biz_test', 'Basmati Rice 25kg Bag', 'PACKAGED', 'pcs', 180000, 220000, 5000, 1, '2026-09-13', '2026-09-13')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_prod_rice_bag', 'prod_rice_bag', 10000, '2026-09-13')", // 10 units (scale 1000)
                [],
            )?;

            conn.execute(
                "INSERT INTO products (id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
                 VALUES ('prod_mustard_oil', 'biz_test', 'Mustard Oil Pure', 'LOOSE', 'litre', 14000, 17500, 20000, 1, '2026-09-13', '2026-09-13')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_prod_mustard_oil', 'prod_mustard_oil', 25000, '2026-09-13')", // 25.000 units (scale 1000)
                [],
            )?;

            // 7. Seed Barcode Mapping
            conn.execute(
                "INSERT INTO barcode_mappings (id, product_id, barcode, created_at)
                 VALUES ('bm_rice_1', 'prod_rice_bag', '890103000001', '2026-09-13')",
                [],
            )?;

            Ok((admin, emp_sales, emp_unauthorized))
        })
        .expect("Failed to seed test database context");

    (db, session, cache, admin, emp_sales, emp_unauthorized)
}

// ------------------------------------------------------------------------------------------------
// 1. SALES FORM DATA & ZERO-MUTATION READ-ONLY PREPARATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_01_sales_form_data_and_read_only_preparation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // 1. Fetch sales form data
    let form_data = get_sales_form_data_inner(&db, &session).expect("Failed to fetch sales form data");
    assert_eq!(form_data.customers.len(), 1);
    assert_eq!(form_data.customers[0].name, "Ramesh Sharma");
    assert_eq!(form_data.products.len(), 2);

    // 2. Barcode resolution
    let scanned = resolve_barcode_for_sale_inner(&db, &session, "890103000001".to_string())
        .expect("Failed to resolve barcode");
    assert_eq!(scanned.id, "prod_rice_bag");
    assert_eq!(scanned.selling_price_cents, 220000);

    // 3. Prepare a PAID sale
    let prep_input = PrepareSaleIpcInput {
        customer_id: None,
        items: vec![
            SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 2000, // 2 bags
            },
            SaleItemIpcInput {
                product_id: "prod_mustard_oil".to_string(),
                quantity: 1500, // 1.500 litres
            },
        ],
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };

    let quote = prepare_sale_inner(&db, &session, &cache, prep_input)
        .expect("prepare_sale_inner failed");

    // 2 bags @ 220000 = 440000 cents; 1.5 litres @ 17500 = 26250 cents. Total = 466250 cents.
    assert_eq!(quote.total_amount_cents, 466250);
    assert_eq!(quote.paid_amount_cents, 466250);
    assert_eq!(quote.credit_amount_cents, 0);
    assert_eq!(quote.settlement_mode, "PAID");
    assert!(quote.preparation_token.starts_with("prep_"));

    // 4. Verify ZERO database mutations during preparation
    db.with_connection(|conn| {
        let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(sales_count, 0, "No rows in sales table after preparation");

        let items_count: i64 = conn.query_row("SELECT COUNT(*) FROM sale_items", [], |r| r.get(0))?;
        assert_eq!(items_count, 0, "No rows in sale_items table after preparation");

        let stock_mov_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(stock_mov_count, 0, "No stock movements during preparation");

        let payments_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(payments_count, 0, "No payments during preparation");

        // Verify inventory quantities unchanged
        let rice_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_bag'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rice_stock, 10000, "Inventory must remain completely unchanged");

        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 2. PERMISSION REJECTION FOR UNAUTHORIZED USERS
// ------------------------------------------------------------------------------------------------
#[test]
fn test_02_permission_denial_unauthorized_user() {
    let (db, session, cache, _, _, emp_unauthorized) = setup_test_context();
    session.set_identity(Some(emp_unauthorized));

    // Attempt to access sales form data without SALES permission
    let form_res = get_sales_form_data_inner(&db, &session);
    assert!(form_res.is_err());
    assert!(form_res.unwrap_err().contains("Permission denied"));

    // Attempt to prepare sale
    let prep_input = PrepareSaleIpcInput {
        customer_id: None,
        items: vec![SaleItemIpcInput {
            product_id: "prod_rice_bag".to_string(),
            quantity: 1000,
        }],
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let prep_res = prepare_sale_inner(&db, &session, &cache, prep_input);
    assert!(prep_res.is_err());
    assert!(prep_res.unwrap_err().contains("Permission denied"));

    // Attempt to confirm sale
    let confirm_res = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: "prep_fake_token".to_string(),
        },
    );
    assert!(confirm_res.is_err());
}

// ------------------------------------------------------------------------------------------------
// 3. FULL PAID SALE FLOW
// ------------------------------------------------------------------------------------------------
#[test]
fn test_03_full_paid_sale_flow() {
    let (db, session, cache, _, emp_sales, _) = setup_test_context();
    session.set_identity(Some(emp_sales));

    // Prepare PAID sale (2 bags of rice @ 220000 = 440000 cents)
    let prep_input = PrepareSaleIpcInput {
        customer_id: None,
        items: vec![SaleItemIpcInput {
            product_id: "prod_rice_bag".to_string(),
            quantity: 2000,
        }],
        settlement_mode: "PAID".to_string(),
        payment_method: Some("UPI".to_string()),
    };

    let quote = prepare_sale_inner(&db, &session, &cache, prep_input).expect("Preparation failed");
    assert_eq!(quote.total_amount_cents, 440000);
    assert_eq!(quote.paid_amount_cents, 440000);
    assert_eq!(quote.credit_amount_cents, 0);

    // Confirm sale
    let receipt = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token.clone(),
        },
    )
    .expect("Confirmation failed");

    // Verify receipt
    assert_eq!(receipt.total_amount_cents, 440000);
    assert_eq!(receipt.paid_amount_cents, 440000);
    assert_eq!(receipt.credit_amount_cents, 0);
    assert_eq!(receipt.settlement_mode, "PAID");
    assert_eq!(receipt.payment_method.as_deref(), Some("UPI"));
    assert_eq!(receipt.items.len(), 1);
    assert_eq!(receipt.items[0].quantity, 2000);
    assert_eq!(receipt.items[0].unit_price_cents, 220000);

    // Verify SQLite mutations
    db.with_connection(|conn| {
        // 1. Sales row
        let (total, paid, credit, status): (i64, i64, i64, String) = conn.query_row(
            "SELECT total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status FROM sales WHERE id = ?1",
            params![receipt.sale_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert_eq!(total, 440000);
        assert_eq!(paid, 440000);
        assert_eq!(credit, 0);
        assert_eq!(status, "PAID");

        // 2. Inventory decremented by 2000 (from 10000 to 8000)
        let stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_bag'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock, 8000);

        // 3. Stock movement recorded
        let (qty_change, qty_before, qty_after, mov_type): (i64, i64, i64, String) = conn.query_row(
            "SELECT quantity_change, quantity_before, quantity_after, movement_type FROM stock_movements WHERE reference_id = ?1",
            params![receipt.sale_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert_eq!(qty_change, -2000);
        assert_eq!(qty_before, 10000);
        assert_eq!(qty_after, 8000);
        assert_eq!(mov_type, "SALE");

        // 4. Payment record created
        let (amt, method, pmt_type): (i64, String, String) = conn.query_row(
            "SELECT amount_cents, payment_method, payment_type FROM payments WHERE related_entity_id = ?1",
            params![receipt.sale_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(amt, 440000);
        assert_eq!(method, "UPI");
        assert_eq!(pmt_type, "CUSTOMER_SALE");

        // 5. Customer ledger untouched
        let cleg_count: i64 = conn.query_row("SELECT COUNT(*) FROM customer_ledger", [], |r| r.get(0))?;
        assert_eq!(cleg_count, 0);

        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 4. FULL CREDIT SALE FLOW
// ------------------------------------------------------------------------------------------------
#[test]
fn test_04_full_credit_sale_flow() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Prepare CREDIT sale for customer Sharma (10.000 litres mustard oil @ 17500 = 175000 cents)
    let prep_input = PrepareSaleIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![SaleItemIpcInput {
            product_id: "prod_mustard_oil".to_string(),
            quantity: 10000,
        }],
        settlement_mode: "CREDIT".to_string(),
        payment_method: None,
    };

    let quote = prepare_sale_inner(&db, &session, &cache, prep_input).expect("Preparation failed");
    assert_eq!(quote.total_amount_cents, 175000);
    assert_eq!(quote.paid_amount_cents, 0);
    assert_eq!(quote.credit_amount_cents, 175000);
    assert_eq!(quote.settlement_mode, "CREDIT");
    assert_eq!(quote.customer_name.as_deref(), Some("Ramesh Sharma"));

    // Confirm credit sale
    let receipt = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token.clone(),
        },
    )
    .expect("Confirmation failed");

    assert_eq!(receipt.total_amount_cents, 175000);
    assert_eq!(receipt.paid_amount_cents, 0);
    assert_eq!(receipt.credit_amount_cents, 175000);
    assert_eq!(receipt.settlement_mode, "CREDIT");
    assert!(receipt.payment_method.is_none());

    // Verify SQLite state
    db.with_connection(|conn| {
        // 1. Customer credit balance increased from 0 to 175000
        let bal: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(bal, 175000);

        // 2. Customer ledger entry created
        let (entry_type, amt, before, after): (String, i64, i64, i64) = conn.query_row(
            "SELECT entry_type, amount_cents, balance_before_cents, balance_after_cents FROM customer_ledger WHERE customer_id = 'cust_sharma'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert_eq!(entry_type, "SALE_CREDIT");
        assert_eq!(amt, 175000);
        assert_eq!(before, 0);
        assert_eq!(after, 175000);

        // 3. Stock decremented from 25000 to 15000
        let oil_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_mustard_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(oil_stock, 15000);

        // 4. Payments table has 0 rows (no cash was paid)
        let pmt_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(pmt_count, 0);

        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 5. INLINE CUSTOMER REGISTRATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_05_inline_customer_creation() {
    let (db, session, cache, _, emp_sales, _) = setup_test_context();
    session.set_identity(Some(emp_sales));

    // Register new customer inline from POS
    let new_cust = create_customer_for_sale_inner(
        &db,
        &session,
        CreateCustomerForSaleRequest {
            name: "Sunita Verma".to_string(),
            phone: Some("+919877712345".to_string()),
        },
    )
    .expect("Failed to create customer");

    assert_eq!(new_cust.name, "Sunita Verma");
    assert_eq!(new_cust.current_balance_cents, 0);

    // Immediately execute a CREDIT sale for this new customer
    let prep_input = PrepareSaleIpcInput {
        customer_id: Some(new_cust.id.clone()),
        items: vec![SaleItemIpcInput {
            product_id: "prod_rice_bag".to_string(),
            quantity: 1000,
        }],
        settlement_mode: "CREDIT".to_string(),
        payment_method: None,
    };

    let quote = prepare_sale_inner(&db, &session, &cache, prep_input).expect("Prepare failed");
    let receipt = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token,
        },
    )
    .expect("Confirm failed");

    assert_eq!(receipt.customer_id.as_deref(), Some(new_cust.id.as_str()));
    assert_eq!(receipt.credit_amount_cents, 220000);

    // Check customer credit balance in SQLite
    db.with_connection(|conn| {
        let bal: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = ?1",
            params![new_cust.id],
            |r| r.get(0),
        )?;
        assert_eq!(bal, 220000);
        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 6. SESSION-BOUND CACHE ISOLATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_06_session_bound_cache_isolation() {
    let (db, session, cache, admin, emp_sales, _) = setup_test_context();

    // 1. Admin prepares a sale
    session.set_identity(Some(admin.clone()));
    let quote = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    )
    .expect("Prepare failed");

    // 2. Switch session to emp_sales
    session.set_identity(Some(emp_sales));

    // 3. emp_sales attempts to confirm admin's prepared token -> must be rejected
    let stolen_res = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token.clone(),
        },
    );
    assert!(stolen_res.is_err());
    let err = stolen_res.unwrap_err();
    assert!(
        err.contains("StaleOrInvalidPreparation"),
        "Another session must not confirm another user's token: {}",
        err
    );

    // 4. Switch back to admin -> confirmation succeeds
    session.set_identity(Some(admin));
    let admin_res = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(admin_res.is_ok());
}

// ------------------------------------------------------------------------------------------------
// 7. ANTI-REPLAY: SINGLE-USE TOKEN ENFORCEMENT
// ------------------------------------------------------------------------------------------------
#[test]
fn test_07_anti_replay_single_use_token() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let quote = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    )
    .expect("Prepare failed");

    // First confirmation -> OK
    let res1 = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token.clone(),
        },
    );
    assert!(res1.is_ok());

    // Replay attempt with same token -> Fails immediately
    let res2 = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(res2.is_err());
    assert!(res2.unwrap_err().contains("StaleOrInvalidPreparation"));

    // Exactly 1 sale record in SQLite
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(count, 1);
        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 8. INSUFFICIENT STOCK REJECTION AT PREPARATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_08_insufficient_stock_rejection_at_preparation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Rice bag stock is 10 units (10000 milli-units). Requesting 11 units (11000 milli-units).
    let prep_res = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 11000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    );

    assert!(prep_res.is_err());
    assert!(prep_res.unwrap_err().contains("Insufficient stock"));
}

// ------------------------------------------------------------------------------------------------
// 9. STALE PREPARATION STOCK EXHAUSTION AT CONFIRMATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_09_stale_preparation_stock_exhaustion_at_confirmation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // 1. Prepare sale for 8 bags of rice (available is 10)
    let quote = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 8000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    )
    .expect("Prepare failed");

    // 2. Intervening event: Stock is reduced to 3 bags in database
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE inventory SET current_quantity = 3000 WHERE product_id = 'prod_rice_bag'",
            [],
        )?;
        Ok(())
    })
    .expect("Stock update failed");

    // 3. Confirm prepared sale -> Must fail due to stale stock
    let confirm_res = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token,
        },
    );

    assert!(confirm_res.is_err());
    let err = confirm_res.unwrap_err();
    assert!(err.contains("Insufficient stock"), "Expected Insufficient stock error, got: {}", err);

    // Verify sale was not committed and inventory remains 3000
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(count, 0);
        let stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_bag'", [], |r| r.get(0))?;
        assert_eq!(stock, 3000);
        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 10. CONCURRENT MULTI-THREADED STOCK RACE: ZERO-OVERSELL PROTECTION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_10_concurrent_multi_threaded_stock_race() {
    let (db, session, cache, admin, emp_sales, _) = setup_test_context();

    // Set stock of rice to exactly 5 bags (5000 milli-units)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE inventory SET current_quantity = 5000 WHERE product_id = 'prod_rice_bag'",
            [],
        )?;
        Ok(())
    })
    .expect("Failed to set stock");

    // Admin prepares sale for 4 bags
    session.set_identity(Some(admin.clone()));
    let quote_admin = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 4000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    )
    .expect("Admin prepare failed");

    // Employee prepares sale for 4 bags
    session.set_identity(Some(emp_sales.clone()));
    let quote_emp = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 4000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("UPI".to_string()),
        },
    )
    .expect("Emp prepare failed");

    // Two threads race to confirm simultaneously
    let db_arc = Arc::new(db);
    let cache_arc = Arc::new(cache);

    let db1 = Arc::clone(&db_arc);
    let cache1 = Arc::clone(&cache_arc);
    let admin_ident = admin;
    let token_admin = quote_admin.preparation_token;
    let handle1 = thread::spawn(move || {
        let sess = AuthSession::default();
        sess.set_identity(Some(admin_ident));
        confirm_sale_inner(&db1, &sess, &cache1, ConfirmSaleIpcInput { preparation_token: token_admin })
    });

    let db2 = Arc::clone(&db_arc);
    let cache2 = Arc::clone(&cache_arc);
    let emp_ident = emp_sales;
    let token_emp = quote_emp.preparation_token;
    let handle2 = thread::spawn(move || {
        let sess = AuthSession::default();
        sess.set_identity(Some(emp_ident));
        confirm_sale_inner(&db2, &sess, &cache2, ConfirmSaleIpcInput { preparation_token: token_emp })
    });

    let res1 = handle1.join().unwrap();
    let res2 = handle2.join().unwrap();

    let success_count = (res1.is_ok() as i32) + (res2.is_ok() as i32);
    let failure_count = (res1.is_err() as i32) + (res2.is_err() as i32);

    assert_eq!(success_count, 1, "Exactly ONE sale must succeed");
    assert_eq!(failure_count, 1, "Exactly ONE sale must fail");

    // Inventory must be exactly 5000 - 4000 = 1000, never negative!
    db_arc.with_connection(|conn| {
        let remaining_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice_bag'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(remaining_stock, 1000, "Inventory must never oversell");

        let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(sales_count, 1, "Exactly one sale committed to database");
        Ok(())
    })
    .expect("DB validation failed");
}

// ------------------------------------------------------------------------------------------------
// 11. LOGOUT INVALIDATION OF PREPARED SALES
// ------------------------------------------------------------------------------------------------
#[test]
fn test_11_logout_cache_invalidation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let quote = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    )
    .expect("Prepare failed");

    // Merchant logs out
    session.logout_sales(&cache);

    // Merchant logs back in
    session.set_identity(Some(admin));

    // Attempt to confirm old token -> must fail
    let confirm_res = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token,
        },
    );

    assert!(confirm_res.is_err());
    assert!(confirm_res.unwrap_err().contains("StaleOrInvalidPreparation"));
}

// ------------------------------------------------------------------------------------------------
// 12. HISTORICAL PRICE SNAPSHOT & STRICT BINARY SETTLEMENT REJECTION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_12_historical_price_snapshot_and_binary_settlement_rejection() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // 1. Prepare and confirm a sale at current prices (rice selling = 220000, cost = 180000)
    let quote = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: Some("CARD".to_string()),
        },
    )
    .expect("Prepare failed");

    let receipt = confirm_sale_inner(
        &db,
        &session,
        &cache,
        ConfirmSaleIpcInput {
            preparation_token: quote.preparation_token,
        },
    )
    .expect("Confirm failed");

    // 2. Price change: Admin increases selling price to 300000 and cost price to 250000
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE products SET selling_price_cents = 300000, cost_price_cents = 250000 WHERE id = 'prod_rice_bag'",
            [],
        )?;
        Ok(())
    })
    .expect("Price update failed");

    // 3. Verify committed sale record preserved original snapshot prices
    db.with_connection(|conn| {
        let (saved_selling, saved_cost): (i64, i64) = conn.query_row(
            "SELECT unit_price_cents, cost_price_cents FROM sale_items WHERE sale_id = ?1",
            params![receipt.sale_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(saved_selling, 220000, "Historical selling price snapshot must be preserved");
        assert_eq!(saved_cost, 180000, "Historical cost price snapshot must be preserved");
        Ok(())
    })
    .expect("DB validation failed");

    // 4. Strict Binary Settlement Rejections:
    // A. "PARTIAL" is completely rejected
    let partial_res = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: Some("cust_sharma".to_string()),
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "PARTIAL".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    );
    assert!(partial_res.is_err());
    assert!(partial_res.unwrap_err().contains("Invalid settlement mode"));

    // B. CREDIT sale without customer is rejected
    let credit_no_cust = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "CREDIT".to_string(),
            payment_method: None,
        },
    );
    assert!(credit_no_cust.is_err());
    assert!(credit_no_cust.unwrap_err().contains("Credit sale requires a registered customer"));

    // C. CREDIT sale with payment method is rejected
    let credit_with_pm = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: Some("cust_sharma".to_string()),
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "CREDIT".to_string(),
            payment_method: Some("CASH".to_string()),
        },
    );
    assert!(credit_with_pm.is_err());
    assert!(credit_with_pm.unwrap_err().contains("Payment method cannot be specified for CREDIT sale"));

    // D. PAID sale without payment method is rejected
    let paid_no_pm = prepare_sale_inner(
        &db,
        &session,
        &cache,
        PrepareSaleIpcInput {
            customer_id: None,
            items: vec![SaleItemIpcInput {
                product_id: "prod_rice_bag".to_string(),
                quantity: 1000,
            }],
            settlement_mode: "PAID".to_string(),
            payment_method: None,
        },
    );
    assert!(paid_no_pm.is_err());
    assert!(paid_no_pm.unwrap_err().contains("Payment method is required for PAID sale"));
}
