use desktop_lib::auth::AuthService;
use desktop_lib::commands::{
    cancel_customer_order_inner, confirm_order_conversion_inner, create_customer_order_inner,
    generate_order_number, get_customer_order_detail_inner, get_customer_orders_form_data_inner,
    get_customer_orders_summary_inner, prepare_order_conversion_inner, update_customer_order_inner,
    AuthSession, ConfirmOrderConversionIpcInput, CreateCustomerOrderIpcInput,
    CustomerOrderItemInput, PrepareOrderConversionIpcInput, PreparedOrderConversionCache,
    UpdateCustomerOrderIpcInput,
};
use desktop_lib::db::DatabaseManager;
use rusqlite::params;
use std::sync::Arc;
use std::thread;

/// Setup test database with seed business, products with stock, customers, and admin/employee identities.
fn setup_test_context() -> (
    DatabaseManager,
    AuthSession,
    PreparedOrderConversionCache,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");
    let session = AuthSession::default();
    let cache = PreparedOrderConversionCache::default();

    let (admin, emp_orders, emp_no_orders) = db
        .with_connection(|conn| {
            // 1. Seed Business
            conn.execute(
                "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
                 VALUES ('biz_test', 'Test Merchant Store', '+919876543210', 'Market St, Delhi', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')
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

            // 3. Create Employee with canonical CUSTOMER_ORDERS permission
            let emp_orders_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_orders",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_orders_id,
                "CUSTOMER_ORDERS",
                true,
            )?;
            let emp_orders = AuthService::authenticate(conn, "emp_orders", "EmpPass123!")?;

            // 4. Create Employee with NO Customer Orders permission
            let emp_no_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_no_orders",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_no_id,
                "CUSTOMERS",
                true,
            )?;
            let emp_no_orders = AuthService::authenticate(conn, "emp_no_orders", "EmpPass123!")?;

            // 5. Seed Category & Products
            conn.execute(
                "INSERT INTO categories (id, name, slug, description, created_at)
                 VALUES ('cat_groceries', 'Groceries', 'groceries', 'General Groceries', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Product A: Basmati Rice (50 kg stock = 50,000 milli-units, ₹60.00/kg = 6000 cents)
            conn.execute(
                "INSERT INTO products (id, name, category_id, product_type, unit, cost_price_cents, selling_price_cents, is_active, business_id, created_at, updated_at)
                 VALUES ('prod_rice', 'Basmati Rice', 'cat_groceries', 'WEIGHT_BASED', 'kg', 4500, 6000, 1, 'biz_test', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_rice', 'prod_rice', 50000, '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Product B: Mustard Oil (20 L stock = 20,000 milli-units, ₹150.00/L = 15000 cents)
            conn.execute(
                "INSERT INTO products (id, name, category_id, product_type, unit, cost_price_cents, selling_price_cents, is_active, business_id, created_at, updated_at)
                 VALUES ('prod_oil', 'Mustard Oil', 'cat_groceries', 'STANDARD', 'L', 12000, 15000, 1, 'biz_test', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_oil', 'prod_oil', 20000, '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Product C: Inactive Product
            conn.execute(
                "INSERT INTO products (id, name, category_id, product_type, unit, cost_price_cents, selling_price_cents, is_active, business_id, created_at, updated_at)
                 VALUES ('prod_inactive', 'Old Spices', 'cat_groceries', 'STANDARD', 'packet', 2000, 3000, 0, 'biz_test', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES ('inv_inactive', 'prod_inactive', 10000, '2026-09-13T10:00:00Z')",
                [],
            )?;

            // 6. Seed Customers
            // Active Customer
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_sharma', 'Ramesh Sharma', '+919811100001', 'Noida', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;
            // Inactive Customer
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_inactive', 'Inactive Customer', '+919833300003', 'Delhi', 0, 0, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            Ok((admin, emp_orders, emp_no_orders))
        })
        .expect("Failed to seed test context");

    (db, session, cache, admin, emp_orders, emp_no_orders)
}

#[test]
fn test_01_create_customer_order_draft_success() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![
            CustomerOrderItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 5000, // 5.0 kg
                notes: Some("Special polish".to_string()),
            },
            CustomerOrderItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 2000, // 2.0 L
                notes: None,
            },
        ],
        notes: Some("Please deliver before noon".to_string()),
    };

    let order = create_customer_order_inner(&db, &session, input).expect("Draft order creation failed");

    assert_eq!(order.status, "DRAFT");
    assert_eq!(order.customer_id, Some("cust_sharma".to_string()));
    assert_eq!(order.customer_name, Some("Ramesh Sharma".to_string()));
    assert_eq!(order.notes, Some("Please deliver before noon".to_string()));
    assert_eq!(order.items.len(), 2);

    // Rice: 5.0 kg * 60.00 = 300.00 (30,000 cents)
    // Oil: 2.0 L * 150.00 = 300.00 (30,000 cents)
    // Total: 60,000 cents (₹600.00)
    assert_eq!(order.total_amount_cents, 60000);

    // Check backend-generated order number format: ORD-YYYYMMDD-<6HEX>
    assert!(order.order_number.starts_with("ORD-"));
    assert_eq!(order.order_number.len(), 19); // "ORD-" (4) + "YYYYMMDD" (8) + "-" (1) + "HEXHEX" (6) = 19
}

#[test]
fn test_02_draft_creation_zero_side_effects_on_inventory_or_ledger() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 10000, // 10.0 kg
            notes: None,
        }],
        notes: None,
    };

    let order = create_customer_order_inner(&db, &session, input).expect("Draft order creation failed");
    assert_eq!(order.status, "DRAFT");

    // Strictly verify zero side effects across database
    db.with_connection(|conn| {
        // 1. Stock remains 50,000
        let rice_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rice_stock, 50000, "Inventory stock must not be decremented for draft orders");

        // 2. Sales table empty
        let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(sales_count, 0, "No sales should be created");

        // 3. Sale items table empty
        let sale_items_count: i64 = conn.query_row("SELECT COUNT(*) FROM sale_items", [], |r| r.get(0))?;
        assert_eq!(sale_items_count, 0, "No sale items should be created");

        // 4. Payments table empty
        let payments_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(payments_count, 0, "No payments should be recorded");

        // 5. Customer balance unchanged (0)
        let cust_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(cust_credit, 0, "Customer credit must remain untouched");

        // 6. Customer ledger empty
        let ledger_count: i64 = conn.query_row("SELECT COUNT(*) FROM customer_ledger", [], |r| r.get(0))?;
        assert_eq!(ledger_count, 0, "Customer ledger must remain untouched");

        // 7. Stock movements empty
        let sm_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(sm_count, 0, "Stock movements must remain untouched");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_03_order_number_collision_safety_and_backend_uniqueness() {
    let (db, _, _, _, _, _) = setup_test_context();

    db.with_connection(|conn| {
        // Generate multiple order numbers
        let mut numbers = std::collections::HashSet::new();
        for _ in 0..20 {
            let num = generate_order_number(conn).expect("Failed to generate order number");
            assert!(num.starts_with("ORD-"));
            assert!(!numbers.contains(&num), "Order number collision detected: {}", num);
            numbers.insert(num);
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_04_create_order_walkin_customer_optional() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: None, // Walk-in customer
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 3000,
            notes: None,
        }],
        notes: Some("Walk-in customer order".to_string()),
    };

    let order = create_customer_order_inner(&db, &session, input).expect("Walk-in order creation failed");
    assert_eq!(order.status, "DRAFT");
    assert_eq!(order.customer_id, None);
    assert_eq!(order.customer_name, None);
}

#[test]
fn test_05_create_order_inactive_customer_rejected() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_inactive".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };

    let err = create_customer_order_inner(&db, &session, input).expect_err("Must reject inactive customer");
    assert!(err.contains("inactive") || err.contains("Inactive"));
}

#[test]
fn test_06_create_order_inactive_product_rejected() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_inactive".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };

    let err = create_customer_order_inner(&db, &session, input).expect_err("Must reject inactive product");
    assert!(err.contains("inactive") || err.contains("Inactive"));
}

#[test]
fn test_07_create_order_zero_items_rejected() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![],
        notes: None,
    };

    let err = create_customer_order_inner(&db, &session, input).expect_err("Must reject empty items");
    assert!(err.contains("zero items"));
}

#[test]
fn test_08_create_order_non_positive_quantity_rejected() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 0,
            notes: None,
        }],
        notes: None,
    };

    let err = create_customer_order_inner(&db, &session, input).expect_err("Must reject 0 quantity");
    assert!(err.contains("strictly positive"));
}

#[test]
fn test_09_create_order_consolidates_duplicate_products() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![
            CustomerOrderItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 2000,
                notes: None,
            },
            CustomerOrderItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 3000,
                notes: None,
            },
        ],
        notes: None,
    };

    let order = create_customer_order_inner(&db, &session, input).expect("Consolidation failed");
    assert_eq!(order.items.len(), 1);
    assert_eq!(order.items[0].quantity, 5000);
}

#[test]
fn test_10_get_customer_orders_summary_deterministic_ordering() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Create 3 orders
    for i in 1..=3 {
        let input = CreateCustomerOrderIpcInput {
            customer_id: Some("cust_sharma".to_string()),
            items: vec![CustomerOrderItemInput {
                product_id: "prod_rice".to_string(),
                quantity: i * 1000,
                notes: None,
            }],
            notes: Some(format!("Order #{}", i)),
        };
        create_customer_order_inner(&db, &session, input).unwrap();
    }

    let summaries = get_customer_orders_summary_inner(&db, &session, None).unwrap();
    assert_eq!(summaries.len(), 3);

    // Verify deterministic order: created_at DESC, id DESC
    assert_eq!(summaries[0].notes, Some("Order #3".to_string()));
    assert_eq!(summaries[1].notes, Some("Order #2".to_string()));
    assert_eq!(summaries[2].notes, Some("Order #1".to_string()));
}

#[test]
fn test_11_get_customer_order_detail_includes_stock_info() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Test C — Multiple draft orders do not reserve stock
    // Initial stock: Rice = 50,000 (50 kg)
    // Order A requests 8,000 (8 kg)
    let input_a = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 8000,
            notes: Some("Order A".to_string()),
        }],
        notes: Some("Order A".to_string()),
    };
    let order_a = create_customer_order_inner(&db, &session, input_a).unwrap();

    // Order B requests 7,000 (7 kg)
    let input_b = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 7000,
            notes: Some("Order B".to_string()),
        }],
        notes: Some("Order B".to_string()),
    };
    let order_b = create_customer_order_inner(&db, &session, input_b).unwrap();

    // Both orders remain DRAFT
    assert_eq!(order_a.status, "DRAFT");
    assert_eq!(order_b.status, "DRAFT");

    // Authoritative stock in database remains exactly 50,000 (no reservation / no decrement)
    db.with_connection(|conn| {
        let rice_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rice_stock, 50000, "Multiple draft orders must not reserve or decrement inventory stock");

        let sm_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(sm_count, 0, "No stock movements may be created for draft orders");
        Ok(())
    })
    .unwrap();

    // Both order details display the full unreserved current stock (50,000), not a reduced available quantity
    let detail_a = get_customer_order_detail_inner(&db, &session, order_a.id).unwrap();
    assert_eq!(detail_a.items.len(), 1);
    assert_eq!(detail_a.items[0].available_stock, 50000);

    let detail_b = get_customer_order_detail_inner(&db, &session, order_b.id).unwrap();
    assert_eq!(detail_b.items.len(), 1);
    assert_eq!(detail_b.items[0].available_stock, 50000);
}

#[test]
fn test_12_update_customer_order_draft_success() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Test B — Draft modification does not change stock
    // Initial stock: Rice = 50,000, Oil = 20,000
    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 2000,
            notes: None,
        }],
        notes: Some("Old note".to_string()),
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let update_input = UpdateCustomerOrderIpcInput {
        order_id: created.id.clone(),
        customer_id: Some("cust_sharma".to_string()),
        items: vec![
            CustomerOrderItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 4000, // 4.0 L = 600.00
                notes: Some("Oil instead".to_string()),
            },
        ],
        notes: Some("Updated note".to_string()),
    };

    let updated = update_customer_order_inner(&db, &session, update_input).unwrap();
    assert_eq!(updated.notes, Some("Updated note".to_string()));
    assert_eq!(updated.items.len(), 1);
    assert_eq!(updated.items[0].product_id, "prod_oil");
    assert_eq!(updated.items[0].quantity, 4000);
    assert_eq!(updated.total_amount_cents, 60000);
    assert_eq!(updated.status, "DRAFT");

    // Strictly verify inventory stock remains completely unchanged after modification
    db.with_connection(|conn| {
        let rice_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rice_stock, 50000, "Rice stock must remain completely unchanged after draft edit");

        let oil_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(oil_stock, 20000, "Oil stock must remain completely unchanged after draft edit");

        let sm_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(sm_count, 0, "No stock movements may be created during draft modification");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_13_cancel_customer_order_success() {
    let (db, session, _, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 2000,
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    cancel_customer_order_inner(&db, &session, created.id.clone()).expect("Cancellation failed");

    let detail = get_customer_order_detail_inner(&db, &session, created.id.clone()).unwrap();
    assert_eq!(detail.status, "CANCELLED");

    // Updating a cancelled order must fail
    let update_input = UpdateCustomerOrderIpcInput {
        order_id: created.id.clone(),
        customer_id: None,
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };
    let err = update_customer_order_inner(&db, &session, update_input).expect_err("Must reject edit on cancelled order");
    assert!(err.contains("cannot edit non-draft") || err.contains("status is 'CANCELLED'"));

    // Cancelling a cancelled order again must fail
    let err2 = cancel_customer_order_inner(&db, &session, created.id).expect_err("Must reject double cancellation");
    assert!(err2.contains("cannot cancel non-draft") || err2.contains("status is 'CANCELLED'"));
}

#[test]
fn test_14_prepare_order_conversion_read_only_quote() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 5000, // 5.0 kg
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("UPI".to_string()),
    };

    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).expect("Prepare quote failed");

    assert!(quote.preparation_token.starts_with("prep_"));
    assert_eq!(quote.order_id, created.id);
    assert_eq!(quote.order_number, created.order_number);
    assert_eq!(quote.settlement_mode, "PAID");
    assert_eq!(quote.payment_method, Some("UPI".to_string()));
    assert_eq!(quote.total_amount_cents, 30000);
    assert_eq!(quote.paid_amount_cents, 30000);
    assert_eq!(quote.credit_amount_cents, 0);

    // Verify zero database writes occurred
    db.with_connection(|conn| {
        let order_status: String = conn.query_row(
            "SELECT status FROM customer_orders WHERE id = ?1",
            params![created.id],
            |r| r.get(0),
        )?;
        assert_eq!(order_status, "DRAFT", "Order must still be DRAFT during preparation");

        let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(sales_count, 0, "No sales must exist during preparation");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_15_prepare_order_conversion_uses_current_catalog_pricing() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // 1. Create draft when rice is ₹60.00/kg (6000 cents)
    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 5000, // 5.0 kg
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    // 2. Price increases in catalog to ₹75.00/kg (7500 cents)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE products SET selling_price_cents = 7500 WHERE id = 'prod_rice'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // 3. Prepare order conversion
    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id,
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).unwrap();

    // Authoritative catalog pricing must be applied: 5.0 kg * ₹75.00 = ₹375.00 (37,500 cents)
    assert_eq!(quote.total_amount_cents, 37500);
    assert_eq!(quote.items[0].unit_price_cents, 7500);
    assert_eq!(quote.items[0].line_total_cents, 37500);
}

#[test]
fn test_16_prepare_order_conversion_insufficient_stock_rejected() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Test E — Conversion uses current stock at conversion time
    // 1. Initial Stock: Rice has 50,000 (50.0 kg).
    // 2. Create draft order for 50,000 (50.0 kg)
    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 50000, // 50.0 kg
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();
    assert_eq!(created.status, "DRAFT");

    // 3. Before conversion, another confirmed sale / stock decrement reduces stock to 20,000 (20.0 kg)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE inventory SET current_quantity = 20000 WHERE product_id = 'prod_rice'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // 4. Attempt conversion of the 50,000 order
    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };

    let err = prepare_order_conversion_inner(&db, &session, &cache, prep_input)
        .expect_err("Must reject conversion when current stock is insufficient");
    assert!(err.contains("Insufficient stock") || err.contains("InsufficientStock"));

    // 5. Verify database invariants:
    // - Stock remains 20,000
    // - Order remains DRAFT
    // - No sale created
    // - No stock movement from failed conversion
    // - No payment created
    // - No customer credit created
    db.with_connection(|conn| {
        let current_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(current_stock, 20000, "Stock must remain 20,000");

        let order_status: String = conn.query_row(
            "SELECT status FROM customer_orders WHERE id = ?1",
            params![created.id],
            |r| r.get(0),
        )?;
        assert_eq!(order_status, "DRAFT", "Order must remain in DRAFT status");

        let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(sales_count, 0, "No sale may be created from failed conversion");

        let sm_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(sm_count, 0, "No stock movement may be created from failed conversion");

        let payments_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(payments_count, 0, "No payment may be recorded from failed conversion");

        let cust_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(cust_credit, 0, "No customer credit change from failed conversion");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_17_confirm_order_conversion_paid_settlement_success() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 10000, // 10.0 kg
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("UPI".to_string()),
    };
    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).unwrap();

    let confirm_input = ConfirmOrderConversionIpcInput {
        preparation_token: quote.preparation_token,
    };
    let receipt = confirm_order_conversion_inner(&db, &session, &cache, confirm_input).expect("Conversion confirmation failed");

    assert_eq!(receipt.total_amount_cents, 60000);
    assert_eq!(receipt.paid_amount_cents, 60000);
    assert_eq!(receipt.credit_amount_cents, 0);
    assert_eq!(receipt.settlement_mode, "PAID");
    assert_eq!(receipt.payment_method, Some("UPI".to_string()));

    // Verify database state:
    db.with_connection(|conn| {
        // 1. Order status is CONVERTED and converted_sale_id is set
        let (status, conv_sale_id): (String, Option<String>) = conn.query_row(
            "SELECT status, converted_sale_id FROM customer_orders WHERE id = ?1",
            params![created.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(status, "CONVERTED");
        assert_eq!(conv_sale_id, Some(receipt.sale_id.clone()));

        // 2. Stock decremented: 50,000 - 10,000 = 40,000
        let new_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(new_stock, 40000);

        // 3. Sale and sale_items created
        let sale_total: i64 = conn.query_row(
            "SELECT total_amount_cents FROM sales WHERE id = ?1",
            params![receipt.sale_id],
            |r| r.get(0),
        )?;
        assert_eq!(sale_total, 60000);

        // 4. Payment recorded
        let pay_amt: i64 = conn.query_row(
            "SELECT amount_cents FROM payments WHERE related_entity_type = 'SALE' AND related_entity_id = ?1",
            params![receipt.sale_id],
            |r| r.get(0),
        )?;
        assert_eq!(pay_amt, 60000);

        // 5. Customer balance remains 0
        let cust_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(cust_credit, 0);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_18_confirm_order_conversion_credit_settlement_success() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_oil".to_string(),
            quantity: 2000, // 2.0 L = ₹300.00 (30,000 cents)
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "CREDIT".to_string(),
        payment_method: None,
    };
    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).unwrap();

    let confirm_input = ConfirmOrderConversionIpcInput {
        preparation_token: quote.preparation_token,
    };
    let receipt = confirm_order_conversion_inner(&db, &session, &cache, confirm_input).expect("Credit conversion confirmation failed");

    assert_eq!(receipt.total_amount_cents, 30000);
    assert_eq!(receipt.paid_amount_cents, 0);
    assert_eq!(receipt.credit_amount_cents, 30000);
    assert_eq!(receipt.settlement_mode, "CREDIT");

    // Verify customer balance and ledger
    db.with_connection(|conn| {
        let cust_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(cust_credit, 30000, "Customer credit must increase by 30,000 cents");

        let ledger_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM customer_ledger WHERE customer_id = 'cust_sharma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(ledger_count, 1, "Customer ledger entry must be created");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_19_conversion_replay_protection() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    // Test F — Converted order cannot be converted twice
    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).unwrap();

    let confirm_input = ConfirmOrderConversionIpcInput {
        preparation_token: quote.preparation_token.clone(),
    };
    confirm_order_conversion_inner(&db, &session, &cache, confirm_input.clone()).unwrap();

    // 1. Second confirmation attempt with same token must fail immediately (anti-replay)
    let err = confirm_order_conversion_inner(&db, &session, &cache, confirm_input).expect_err("Must reject replay");
    assert!(err.contains("StaleOrInvalidPreparation"));

    // 2. Second preparation attempt on already-converted order must fail immediately
    let prep_input_2 = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let err2 = prepare_order_conversion_inner(&db, &session, &cache, prep_input_2)
        .expect_err("Must reject conversion preparation on already-converted order");
    assert!(err2.contains("OrderAlreadyConverted") || err2.contains("already converted"));

    // 3. Verify exactly 1 Sale, exactly 1 stock deduction, exactly 1 stock movement, exactly 1 payment
    db.with_connection(|conn| {
        let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(sales_count, 1, "Exactly one sale must exist");

        let rice_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rice_stock, 49000, "Exactly one stock deduction of 1000 milli-units");

        let sm_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))?;
        assert_eq!(sm_count, 1, "Exactly one stock movement must exist");

        let payments_count: i64 = conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))?;
        assert_eq!(payments_count, 1, "Exactly one payment record must exist");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_20_conversion_session_isolation() {
    let (db, session, cache, admin, emp_orders, _) = setup_test_context();

    // Admin prepares conversion
    session.set_identity(Some(admin));
    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id,
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).unwrap();

    // Employee tries to confirm Admin's token
    session.set_identity(Some(emp_orders));
    let confirm_input = ConfirmOrderConversionIpcInput {
        preparation_token: quote.preparation_token,
    };
    let err = confirm_order_conversion_inner(&db, &session, &cache, confirm_input).expect_err("Must reject other session's token");
    assert!(err.contains("StaleOrInvalidPreparation"));
}

#[test]
fn test_21_conversion_logout_invalidation() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    let prep_input = PrepareOrderConversionIpcInput {
        order_id: created.id,
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let quote = prepare_order_conversion_inner(&db, &session, &cache, prep_input).unwrap();

    // Explicit logout
    session.logout_order_conversions(&cache);

    // Confirmation must fail
    let confirm_input = ConfirmOrderConversionIpcInput {
        preparation_token: quote.preparation_token,
    };
    let err = confirm_order_conversion_inner(&db, &session, &cache, confirm_input).expect_err("Must reject after logout");
    assert!(err.contains("Unauthenticated"));
}

#[test]
fn test_22_concurrent_conversion_race_atomic_conditional() {
    let (db, session, cache, admin, _, _) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 5000, // 5.0 kg
            notes: None,
        }],
        notes: None,
    };
    let created = create_customer_order_inner(&db, &session, input).unwrap();

    // Prepare two separate quotes for the same order
    let prep_input_1 = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("CASH".to_string()),
    };
    let quote_1 = prepare_order_conversion_inner(&db, &session, &cache, prep_input_1).unwrap();

    let prep_input_2 = PrepareOrderConversionIpcInput {
        order_id: created.id.clone(),
        settlement_mode: "PAID".to_string(),
        payment_method: Some("UPI".to_string()),
    };
    let quote_2 = prepare_order_conversion_inner(&db, &session, &cache, prep_input_2).unwrap();

    // Concurrent race: execute confirmation in 2 threads
    let db_arc = Arc::new(db);
    let cache_arc = Arc::new(cache);
    let admin_arc = Arc::new(admin);

    let db_1 = Arc::clone(&db_arc);
    let cache_1 = Arc::clone(&cache_arc);
    let admin_1 = Arc::clone(&admin_arc);
    let token_1 = quote_1.preparation_token;
    let handle_1 = thread::spawn(move || {
        let session_1 = AuthSession::default();
        session_1.set_identity(Some((*admin_1).clone()));
        confirm_order_conversion_inner(
            &db_1,
            &session_1,
            &cache_1,
            ConfirmOrderConversionIpcInput {
                preparation_token: token_1,
            },
        )
    });

    let db_2 = Arc::clone(&db_arc);
    let cache_2 = Arc::clone(&cache_arc);
    let admin_2 = Arc::clone(&admin_arc);
    let token_2 = quote_2.preparation_token;
    let handle_2 = thread::spawn(move || {
        let session_2 = AuthSession::default();
        session_2.set_identity(Some((*admin_2).clone()));
        confirm_order_conversion_inner(
            &db_2,
            &session_2,
            &cache_2,
            ConfirmOrderConversionIpcInput {
                preparation_token: token_2,
            },
        )
    });

    let res_1 = handle_1.join().unwrap();
    let res_2 = handle_2.join().unwrap();

    // Exactly one thread MUST succeed, and the other MUST fail!
    let successes = (if res_1.is_ok() { 1 } else { 0 }) + (if res_2.is_ok() { 1 } else { 0 });
    assert_eq!(successes, 1, "Exactly one concurrent conversion must succeed");

    // Stock must be decremented ONCE only (50,000 - 5,000 = 45,000)
    db_arc
        .with_connection(|conn| {
            let current_stock: i64 = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(current_stock, 45000, "Stock must be decremented exactly once");

            let sales_count: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
            assert_eq!(sales_count, 1, "Exactly one sale must be created");

            Ok(())
        })
        .unwrap();
}

#[test]
fn test_23_unauthorized_employee_permission_denial() {
    let (db, session, _cache, _, _, emp_no_orders) = setup_test_context();
    session.set_identity(Some(emp_no_orders));

    let input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_sharma".to_string()),
        items: vec![CustomerOrderItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 1000,
            notes: None,
        }],
        notes: None,
    };

    let err = create_customer_order_inner(&db, &session, input).expect_err("Must deny unauthorized employee");
    assert!(err.contains("CUSTOMER_ORDERS") || err.contains("Unauthorized") || err.contains("Permission denied"));

    let err_summary = get_customer_orders_summary_inner(&db, &session, None).expect_err("Must deny summary query");
    assert!(err_summary.contains("CUSTOMER_ORDERS") || err_summary.contains("Unauthorized"));

    let err_form = get_customer_orders_form_data_inner(&db, &session).expect_err("Must deny form data query");
    assert!(err_form.contains("CUSTOMER_ORDERS") || err_form.contains("Unauthorized"));
}
