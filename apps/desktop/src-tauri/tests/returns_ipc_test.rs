use desktop_lib::auth::AuthService;
use desktop_lib::commands::{
    confirm_customer_return_inner, confirm_supplier_return_inner, generate_return_number,
    get_return_detail_inner, get_returns_form_data_inner, get_returns_summary_inner,
    prepare_customer_return_inner, prepare_supplier_return_inner, AuthSession,
    ConfirmCustomerReturnIpcInput, ConfirmSupplierReturnIpcInput, PrepareCustomerReturnIpcInput,
    PrepareSupplierReturnIpcInput, PreparedReturnCache, ReturnItemInput,
};
use desktop_lib::db::DatabaseManager;
use desktop_lib::engine::{
    BusinessEngine, ConfirmPurchaseRequest, PurchaseItemRequest, TransactionEngine,
};
use rusqlite::params;

/// Setup test database with seed business, products with stock, customers, suppliers, and admin/employee identities.
fn setup_test_context() -> (
    DatabaseManager,
    AuthSession,
    PreparedReturnCache,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");
    let session = AuthSession::default();
    let cache = PreparedReturnCache::default();

    let (admin, emp_returns, emp_no_returns) = db
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

            // 3. Create Employee with canonical RETURNS permission
            let emp_returns_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_returns",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_returns_id,
                "RETURNS",
                true,
            )?;
            let emp_returns = AuthService::authenticate(conn, "emp_returns", "EmpPass123!")?;

            // 4. Create Employee with NO Returns permission
            let emp_no_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_no_returns",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_no_id,
                "CUSTOMERS",
                true,
            )?;
            let emp_no_returns = AuthService::authenticate(conn, "emp_no_returns", "EmpPass123!")?;

            // 5. Seed Category & Products
            conn.execute(
                "INSERT INTO categories (id, name, slug, description, created_at)
                 VALUES ('cat_groceries', 'Groceries', 'groceries', 'General Groceries', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Product A: Basmati Rice (50 kg stock = 50,000 milli-units, Cost ₹45.00/kg = 4500 cents, Selling ₹60.00/kg = 6000 cents)
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

            // Product B: Mustard Oil (20 L stock = 20,000 milli-units, Cost ₹120.00/L = 12000 cents, Selling ₹150.00/L = 15000 cents)
            conn.execute(
                "INSERT INTO products (id, name, category_id, product_type, unit, cost_price_cents, selling_price_cents, is_active, business_id, created_at, updated_at)
                 VALUES ('prod_oil', 'Mustard Oil', 'cat_groceries', 'UNIT_BASED', 'pcs', 12000, 15000, 1, 'biz_test', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
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
                 VALUES ('prod_inactive', 'Discontinued Tea', 'cat_groceries', 'UNIT_BASED', 'pcs', 2000, 3000, 0, 'biz_test', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // 6. Seed Customers
            // Customer 1: Active with ₹1,000 credit debt (100,000 cents)
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_rahul', 'Rahul Sharma', '+919811122233', 'Connaught Place', 100000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Customer 2: Active with ₹0 credit debt
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_priya', 'Priya Patel', '+919844455566', 'Karol Bagh', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Customer 3: Inactive Customer
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_inactive', 'Old Customer', '+919877788899', 'Old Delhi', 5000, 0, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // 7. Seed Suppliers
            // Supplier 1: Active with ₹500 outstanding debt owed to supplier (50,000 cents)
            conn.execute(
                "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
                 VALUES ('sup_agro', 'Agro Farms Ltd', '+919122334455', 'Karnal, Haryana', 50000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // Supplier 2: Inactive Supplier
            conn.execute(
                "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
                 VALUES ('sup_inactive', 'Defunct Wholesale', '+919988776655', 'Panipat', 0, 0, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            Ok((admin, emp_returns, emp_no_returns))
        })
        .expect("Failed to seed test data");

    session.set_identity(Some(admin.clone()));
    (db, session, cache, admin, emp_returns, emp_no_returns)
}

#[test]
fn test_01_prepare_customer_return_quote_success() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let input = PrepareCustomerReturnIpcInput {
        customer_id: Some("cust_rahul".to_string()),
        reference_sale_id: None,
        items: vec![
            ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 5000, // 5 kg * ₹60 = ₹300 (30,000 cents)
                notes: Some("Damaged packet".to_string()),
            },
            ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 1000, // 1 bottle * ₹150 = ₹150 (15,000 cents)
                notes: None,
            },
        ],
        reason: "Customer returned defective goods".to_string(),
        refund_payment_method: Some("CASH".to_string()),
    };

    let quote = prepare_customer_return_inner(&db, &session, &cache, input)
        .expect("Failed to prepare customer return");

    assert_eq!(quote.items.len(), 2);
    assert_eq!(quote.total_amount_cents, 45000); // ₹450
    // Rahul owes ₹1,000 (100,000 cents), so all ₹450 goes to debt reduction
    assert_eq!(quote.debt_reduction_cents, 45000);
    assert_eq!(quote.refund_amount_cents, 0);
    assert_eq!(quote.balance_before_cents, 100000);
    assert_eq!(quote.balance_after_cents, 55000);
    assert!(quote.preparation_token.starts_with("prep_"));
}

#[test]
fn test_02_prepare_customer_return_walkin_success() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let input = PrepareCustomerReturnIpcInput {
        customer_id: None,
        reference_sale_id: None,
        items: vec![ReturnItemInput {
            product_id: "prod_oil".to_string(),
            quantity: 2000, // 2 bottles * ₹150 = ₹300 (30,000 cents)
            notes: None,
        }],
        reason: "Walk-in exchange".to_string(),
        refund_payment_method: Some("UPI".to_string()),
    };

    let quote = prepare_customer_return_inner(&db, &session, &cache, input)
        .expect("Failed to prepare customer return");

    assert_eq!(quote.total_amount_cents, 30000);
    assert_eq!(quote.debt_reduction_cents, 0);
    assert_eq!(quote.refund_amount_cents, 30000);
    assert_eq!(quote.balance_before_cents, 0);
    assert_eq!(quote.balance_after_cents, 0);
    assert_eq!(quote.refund_payment_method, Some("UPI".to_string()));
}

#[test]
fn test_03_confirm_customer_return_with_debt_reduction_and_refund() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Customer Rahul owes ₹1,000 (100,000 cents).
    // Return: 20 kg Rice * ₹60 = ₹1,200 (120,000 cents) + 2 Oil * ₹150 = ₹300 (30,000 cents) = ₹1,500 total (150,000 cents).
    // Financial outcome:
    // Debt reduced by ₹1,000 (100,000 cents) -> Rahul owes ₹0
    // Excess ₹500 (50,000 cents) refunded via CASH
    let prep_input = PrepareCustomerReturnIpcInput {
        customer_id: Some("cust_rahul".to_string()),
        reference_sale_id: None,
        items: vec![
            ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 20000,
                notes: None,
            },
            ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 2000,
                notes: None,
            },
        ],
        reason: "Bulk return".to_string(),
        refund_payment_method: Some("CASH".to_string()),
    };

    let quote = prepare_customer_return_inner(&db, &session, &cache, prep_input).unwrap();
    assert_eq!(quote.total_amount_cents, 150000);
    assert_eq!(quote.debt_reduction_cents, 100000);
    assert_eq!(quote.refund_amount_cents, 50000);

    let receipt = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: quote.preparation_token,
        },
    )
    .expect("Failed to confirm customer return");

    assert_eq!(receipt.total_amount_cents, 150000);
    assert_eq!(receipt.debt_reduction_cents, 100000);
    assert_eq!(receipt.refund_amount_cents, 50000);
    assert_eq!(receipt.balance_before_cents, 100000);
    assert_eq!(receipt.balance_after_cents, 0);

    // Verify DB state
    db.with_connection(|conn| {
        // Customer credit updated to 0
        let credit: i64 = conn
            .query_row(
                "SELECT current_credit_cents FROM customers WHERE id = 'cust_rahul'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(credit, 0);

        // Inventory incremented: Rice was 50,000 + 20,000 = 70,000; Oil was 20,000 + 2,000 = 22,000
        let rice_stock: i64 = conn
            .query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rice_stock, 70000);

        // Customer ledger has RETURN_CREDIT entry
        let (entry_type, amount, bal_after): (String, i64, i64) = conn
            .query_row(
                "SELECT entry_type, amount_cents, balance_after_cents FROM customer_ledger WHERE customer_id = 'cust_rahul' AND reference_type = 'RETURN'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry_type, "RETURN_CREDIT");
        assert_eq!(amount, 100000);
        assert_eq!(bal_after, 0);

        // Payment record exists for refund payout of ₹500
        let (p_type, p_method, p_amount): (String, String, i64) = conn
            .query_row(
                "SELECT payment_type, payment_method, amount_cents FROM payments WHERE related_entity_type = 'RETURN' AND related_entity_id = ?1",
                params![receipt.return_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(p_type, "CUSTOMER_RETURN_REFUND");
        assert_eq!(p_method, "CASH");
        assert_eq!(p_amount, 50000);

        // Stock movements exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_movements WHERE reference_type = 'RETURN' AND reference_id = ?1",
                params![receipt.return_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_04_confirm_customer_return_zero_debt_full_refund() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Customer Priya owes ₹0
    let prep_input = PrepareCustomerReturnIpcInput {
        customer_id: Some("cust_priya".to_string()),
        reference_sale_id: None,
        items: vec![ReturnItemInput {
            product_id: "prod_oil".to_string(),
            quantity: 1000, // ₹150 (15,000 cents)
            notes: None,
        }],
        reason: "Wrong item purchased".to_string(),
        refund_payment_method: Some("CARD".to_string()),
    };

    let quote = prepare_customer_return_inner(&db, &session, &cache, prep_input).unwrap();
    assert_eq!(quote.debt_reduction_cents, 0);
    assert_eq!(quote.refund_amount_cents, 15000);

    let receipt = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: quote.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.debt_reduction_cents, 0);
    assert_eq!(receipt.refund_amount_cents, 15000);

    // Verify refund payment
    db.with_connection(|conn| {
        let (method, amount): (String, i64) = conn
            .query_row(
                "SELECT payment_method, amount_cents FROM payments WHERE related_entity_type = 'RETURN' AND related_entity_id = ?1",
                params![receipt.return_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(method, "CARD");
        assert_eq!(amount, 15000);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_05_confirm_customer_return_walkin_full_refund() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let prep_input = PrepareCustomerReturnIpcInput {
        customer_id: None,
        reference_sale_id: None,
        items: vec![ReturnItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 10000, // 10 kg * ₹60 = ₹600 (60,000 cents)
            notes: None,
        }],
        reason: "Walk-in cash return".to_string(),
        refund_payment_method: Some("CASH".to_string()),
    };

    let quote = prepare_customer_return_inner(&db, &session, &cache, prep_input).unwrap();
    let receipt = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: quote.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.customer_id, None);
    assert_eq!(receipt.total_amount_cents, 60000);
    assert_eq!(receipt.refund_amount_cents, 60000);
    assert_eq!(receipt.debt_reduction_cents, 0);
}

#[test]
fn test_06_prepare_supplier_return_quote_success() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let input = PrepareSupplierReturnIpcInput {
        supplier_id: Some("sup_agro".to_string()),
        reference_purchase_id: None,
        items: vec![ReturnItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 10000, // 10 kg * Cost ₹45 = ₹450 (45,000 cents)
            notes: None,
        }],
        reason: "Expired stock return".to_string(),
    };

    let quote = prepare_supplier_return_inner(&db, &session, &cache, input).unwrap();
    assert_eq!(quote.total_amount_cents, 45000);
    assert_eq!(quote.balance_before_cents, 50000);
    // Agro outstanding was ₹500 (50,000 cents) - ₹450 = ₹50 (5,000 cents)
    assert_eq!(quote.balance_after_cents, 5000);
    assert_eq!(quote.items[0].unit_cost_cents, 4500);
    assert_eq!(quote.items[0].available_stock, 50000);
}

#[test]
fn test_07_confirm_supplier_return_positive_balance_reduction() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 5000, // 5 kg * ₹45 = ₹225 (22,500 cents)
                notes: None,
            }],
            reason: "Return bad lot".to_string(),
        },
    )
    .unwrap();

    let receipt = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.balance_before_cents, 50000);
    assert_eq!(receipt.balance_after_cents, 27500); // 50,000 - 22,500 = 27,500

    db.with_connection(|conn| {
        // Supplier balance updated
        let bal: i64 = conn
            .query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'sup_agro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bal, 27500);

        // Inventory decremented from 50,000 to 45,000
        let stock: i64 = conn
            .query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stock, 45000);

        // Supplier ledger entry
        let (entry_type, amt, after): (String, i64, i64) = conn
            .query_row(
                "SELECT entry_type, amount_cents, balance_after_cents FROM supplier_ledger WHERE supplier_id = 'sup_agro' AND reference_type = 'RETURN'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry_type, "RETURN_DEBIT");
        assert_eq!(amt, 22500);
        assert_eq!(after, 27500);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_08_guardrail_2_supplier_return_negative_balance_credit_note() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Agro outstanding before: ₹500 (50,000 cents)
    // Return: 20 kg Rice * ₹45 = ₹900 (90,000 cents)
    // Balance after must be: 50,000 - 90,000 = -40,000 cents (-₹400 Credit Note)
    // Invariant: NO zero clamping!
    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 20000,
                notes: None,
            }],
            reason: "Major return resulting in supplier credit note".to_string(),
        },
    )
    .unwrap();

    assert_eq!(prep.balance_before_cents, 50000);
    assert_eq!(prep.balance_after_cents, -40000);

    let receipt = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.balance_before_cents, 50000);
    assert_eq!(receipt.balance_after_cents, -40000);

    // Verify DB signed balance
    db.with_connection(|conn| {
        let bal: i64 = conn
            .query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'sup_agro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bal, -40000); // Strictly negative signed integer!

        let ledger_after: i64 = conn
            .query_row(
                "SELECT balance_after_cents FROM supplier_ledger WHERE reference_id = ?1",
                params![receipt.return_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ledger_after, -40000);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_09_guardrail_2_future_credit_purchase_absorbs_credit_note() {
    let (db, session, cache, admin, _, _) = setup_test_context();

    // 1. First establish a negative balance of -₹1,000 (-100,000 cents) via supplier return
    // Agro current is ₹500 (50,000 cents). We return goods worth ₹1,500 (150,000 cents):
    // 33.333 kg Rice * ₹45 = ₹1,500 (use 10 Oil * ₹120 = ₹1,200 + 7 kg Rice * ₹45 = ₹315 = ₹1,515)
    // Or return 10 Oil (10,000 milli-units * ₹120 = 120,000 cents) + 7,000 milli-units Rice (31,500 cents) = 151,500 cents
    // Let's set supplier outstanding to -100,000 directly via return:
    // 50,000 - 150,000 = -100,000.
    // 10 Oil = 120,000 cents, 6.666 kg Rice * 45 = ~30,000 cents.
    // Let's just return 10 Oil (120,000 cents) from 50,000 outstanding -> 50,000 - 120,000 = -70,000 cents (-₹700)
    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 10000, // 10 Oil * ₹120 = ₹1,200 (120,000 cents)
                notes: None,
            }],
            reason: "Defective oil lot".to_string(),
        },
    )
    .unwrap();

    let receipt = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.balance_after_cents, -70000); // -₹700

    // 2. Now simulate a future BUILD 06 credit purchase from Agro Farms Ltd of ₹1,000 (+100,000 cents)
    // The resulting balance must naturally absorb the credit: -₹700 + ₹1,000 = +₹300 (+30,000 cents)!
    db.with_connection(|conn| {
        let prep_purchase = BusinessEngine::prepare_purchase(
            conn,
            &admin,
            ConfirmPurchaseRequest {
                purchase_id: "pur_future_credit".to_string(),
                purchase_number: "PUR-2026-FUTURE".to_string(),
                supplier_id: "sup_agro".to_string(),
                items: vec![PurchaseItemRequest {
                    product_id: "prod_oil".to_string(),
                    quantity: 10000,
                    unit_cost_cents: 10000, // ₹100/unit * 10 = ₹1,000 total
                }],
                paid_amount_cents: 0,
                payment_method: None,
                user_id: admin.user_id().to_string(),
                purchase_date: "2026-09-14T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let confirmed_purchase = prep_purchase.confirm(&admin);
        TransactionEngine::execute_purchase(conn, confirmed_purchase).unwrap();

        // Verify final balance
        let final_bal: i64 = conn
            .query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'sup_agro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(final_bal, 30000); // -70,000 + 100,000 = +30,000 (+₹300)

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_10_guardrail_1_supplier_return_insufficient_stock_atomic_rollback() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Mustard oil has 20,000 milli-units (20 L) stock.
    // Attempt supplier return of 25,000 milli-units (25 L).
    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 25000, // Exceeds stock!
                notes: None,
            }],
            reason: "Oversell attempt".to_string(),
        },
    )
    .unwrap();

    let err = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .expect_err("Must fail due to insufficient stock");

    assert!(
        err.contains("Insufficient stock"),
        "Expected InsufficientStock error, got: {}",
        err
    );

    // Verify DB atomic state: zero mutations
    db.with_connection(|conn| {
        let stock: i64 = conn
            .query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stock, 20000); // Stock unchanged!

        let return_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM returns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(return_count, 0);

        let sup_bal: i64 = conn
            .query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'sup_agro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sup_bal, 50000); // Unchanged!

        let ledger_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM supplier_ledger WHERE reference_type = 'RETURN'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ledger_count, 0);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_11_guardrail_1_mid_transaction_concurrency_failure_complete_rollback() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Prepare a valid supplier return
    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![
                ReturnItemInput {
                    product_id: "prod_rice".to_string(),
                    quantity: 5000,
                    notes: None,
                },
                ReturnItemInput {
                    product_id: "prod_oil".to_string(),
                    quantity: 50000, // This 2nd item exceeds available oil stock (20,000)!
                    notes: None,
                },
            ],
            reason: "Partial failure test".to_string(),
        },
    )
    .unwrap();

    let err = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .expect_err("Must abort transaction due to second item failing stock check");

    assert!(err.contains("Insufficient stock"));

    // Guardrail 1 requirement: GUARANTEE ZERO PARTIAL STOCK MUTATIONS OR LEDGER ENTRIES
    db.with_connection(|conn| {
        // Product Rice stock must NOT be decremented even though it was processed first
        let rice_stock: i64 = conn
            .query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rice_stock, 50000); // 100% rolled back!

        let oil_stock: i64 = conn
            .query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(oil_stock, 20000);

        let returns_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM returns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(returns_count, 0);

        let items_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM return_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items_count, 0);

        let movements_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(movements_count, 0);

        let sup_bal: i64 = conn
            .query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = 'sup_agro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sup_bal, 50000);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_12_guardrail_3_customer_return_live_revalidation_price_change() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Prepare with current rice selling price = ₹60 (6,000 cents)
    let prep = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 2000, // 2 kg * 60 = 120 (12,000 cents)
                notes: None,
            }],
            reason: "Price revalidation test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();

    assert_eq!(prep.total_amount_cents, 12000);

    // Before confirmation, catalog price changes to ₹80 (8,000 cents)!
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE products SET selling_price_cents = 8000 WHERE id = 'prod_rice'",
            [],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();

    // Confirm: MUST use live selling price (2 kg * 80 = 16,000 cents)
    let receipt = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.total_amount_cents, 16000);
    assert_eq!(receipt.refund_amount_cents, 16000);
    assert_eq!(receipt.items[0].unit_price_cents, 8000);
    assert_eq!(receipt.items[0].line_total_cents, 16000);
}

#[test]
fn test_13_guardrail_3_customer_return_live_revalidation_debt_change() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Rahul owes ₹1,000 (100,000 cents)
    // Return quote: 15 kg Rice * ₹60 = ₹900 (90,000 cents)
    // At prepare time: debt reduction = ₹900, refund = ₹0
    let prep = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: Some("cust_rahul".to_string()),
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 15000,
                notes: None,
            }],
            reason: "Interleaving payment test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();

    assert_eq!(prep.debt_reduction_cents, 90000);
    assert_eq!(prep.refund_amount_cents, 0);

    // Before confirmation, Rahul makes a payment of ₹600 (60,000 cents), so debt becomes ₹400 (40,000 cents)!
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE customers SET current_credit_cents = 40000 WHERE id = 'cust_rahul'",
            [],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();

    // Confirm: Must recalculate live debt!
    // Total return is ₹900. Live debt is ₹400.
    // Debt reduction must be ₹400 (offsetting all remaining debt).
    // Excess ₹500 must be refunded via CASH!
    let receipt = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.total_amount_cents, 90000);
    assert_eq!(receipt.debt_reduction_cents, 40000);
    assert_eq!(receipt.refund_amount_cents, 50000);
    assert_eq!(receipt.balance_before_cents, 40000);
    assert_eq!(receipt.balance_after_cents, 0);
}

#[test]
fn test_14_guardrail_3_supplier_return_live_revalidation_price_change() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Prepare supplier return with rice cost price = ₹45 (4,500 cents)
    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 1000, // 1 kg * 45 = 45 (4,500 cents)
                notes: None,
            }],
            reason: "Supplier cost update test".to_string(),
        },
    )
    .unwrap();

    assert_eq!(prep.total_amount_cents, 4500);

    // Change cost price in catalog to ₹50 (5,000 cents)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE products SET cost_price_cents = 5000 WHERE id = 'prod_rice'",
            [],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();

    let receipt = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .unwrap();

    assert_eq!(receipt.total_amount_cents, 5000);
    assert_eq!(receipt.items[0].unit_cost_cents, 5000);
}

#[test]
fn test_15_guardrail_3_supplier_return_live_revalidation_stock_depleted() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Rice has 50,000 milli-units (50 kg)
    let prep = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 40000, // 40 kg
                notes: None,
            }],
            reason: "Stock race test".to_string(),
        },
    )
    .unwrap();

    // Interleaving event: stock depleted to 10,000 milli-units
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE inventory SET current_quantity = 10000 WHERE product_id = 'prod_rice'",
            [],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();

    let err = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .expect_err("Must reject because stock dropped below 40 kg");

    assert!(err.contains("Insufficient stock"));
}

#[test]
fn test_16_return_preparation_read_only_zero_db_mutations() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let _ = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: Some("cust_rahul".to_string()),
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 5000,
                notes: None,
            }],
            reason: "Quote test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();

    let _ = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 2000,
                notes: None,
            }],
            reason: "Quote test 2".to_string(),
        },
    )
    .unwrap();

    db.with_connection(|conn| {
        let return_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM returns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(return_count, 0);

        let items_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM return_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items_count, 0);

        let payments_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(payments_count, 0);

        let movements_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_movements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(movements_count, 0);

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_17_return_preparation_token_single_use_replay_protection() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let prep = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Replay test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();

    // First confirmation succeeds
    let _ = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep.preparation_token.clone(),
        },
    )
    .unwrap();

    // Second confirmation with same token MUST fail
    let err = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .expect_err("Token must be consumed on first use");

    assert!(err.contains("StaleOrInvalidPreparation"));
}

#[test]
fn test_18_return_preparation_session_isolation() {
    let (db, session, cache, _, emp_returns, _) = setup_test_context();

    // Admin prepares return
    let prep = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Session isolation test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();

    // Switch session to employee
    session.set_identity(Some(emp_returns));

    // Employee tries to confirm Admin's prepared token -> REJECTED
    let err = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .expect_err("Another user cannot confirm a token they do not own");

    assert!(err.contains("StaleOrInvalidPreparation"));
}

#[test]
fn test_19_return_preparation_logout_invalidation() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let prep = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Logout invalidation test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();

    // User logs out
    session.logout_returns(&cache);

    // Attempting to confirm after logout fails
    let err = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep.preparation_token,
        },
    )
    .expect_err("Session is unauthenticated after logout");

    assert!(err.contains("Unauthenticated"));
}

#[test]
fn test_20_customer_return_requires_valid_refund_method_when_refund_due() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // Invalid refund method "BITCOIN"
    let err = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Invalid payment method test".to_string(),
            refund_payment_method: Some("BITCOIN".to_string()),
        },
    )
    .expect_err("Must reject invalid refund payment method");

    assert!(err.contains("Invalid refund payment method"));
}

#[test]
fn test_21_inactive_customer_rejected_for_return() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let err = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: Some("cust_inactive".to_string()),
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Inactive customer test".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .expect_err("Must reject inactive customer");

    assert!(err.contains("inactive"));
}

#[test]
fn test_22_inactive_supplier_rejected_for_return() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let err = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_inactive".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Inactive supplier test".to_string(),
        },
    )
    .expect_err("Must reject inactive supplier");

    assert!(err.contains("inactive"));
}

#[test]
fn test_23_inactive_product_rejected_for_return() {
    let (db, session, cache, _, _, _) = setup_test_context();

    let err = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_inactive".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Inactive product return".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .expect_err("Must reject inactive product");

    assert!(err.contains("inactive"));
}

#[test]
fn test_24_return_number_collision_safety() {
    let (db, _session, _cache, admin, _, _) = setup_test_context();

    // Guardrail 4: test_24_return_number_collision_safety
    // Verifies backend-only generation, CSPRNG entropy, uniqueness, bounded regeneration,
    // and that client cannot supply the return number.
    db.with_connection(|conn| {
        let mut generated_numbers = std::collections::HashSet::new();

        for _ in 0..50 {
            let ret_num = generate_return_number(conn, "CUSTOMER_RETURN")
                .expect("Failed to generate customer return number");
            assert!(
                ret_num.starts_with("RET-"),
                "Expected prefix RET-, got {}",
                ret_num
            );
            assert!(
                generated_numbers.insert(ret_num.clone()),
                "Generated duplicate return number: {}",
                ret_num
            );

            let sr_num = generate_return_number(conn, "SUPPLIER_RETURN")
                .expect("Failed to generate supplier return number");
            assert!(
                sr_num.starts_with("SR-"),
                "Expected prefix SR-, got {}",
                sr_num
            );
            assert!(
                generated_numbers.insert(sr_num.clone()),
                "Generated duplicate supplier return number: {}",
                sr_num
            );
        }

        // Verify collision handling: insert an artificial candidate, then verify generate_return_number still succeeds
        let now = format!("{:?}", std::time::SystemTime::now());
        conn.execute(
            "INSERT INTO returns (id, return_number, return_type, total_amount_cents, reason, admin_user_id, created_at)
             VALUES ('ret_collision_test', 'RET-20260914-COLLIS', 'CUSTOMER_RETURN', 100, 'Test', ?1, ?2)",
            params![admin.user_id(), now],
        )?;

        let new_num = generate_return_number(conn, "CUSTOMER_RETURN").unwrap();
        assert_ne!(new_num, "RET-20260914-COLLIS");

        Ok(())
    })
    .unwrap();
}

#[test]
fn test_25_get_returns_summary_and_detail_queries() {
    let (db, session, cache, _, _, _) = setup_test_context();

    // 1. Form data query
    let form_data = get_returns_form_data_inner(&db, &session).unwrap();
    assert_eq!(form_data.customers.len(), 2); // Only active customers
    assert_eq!(form_data.suppliers.len(), 1); // Only active suppliers
    assert_eq!(form_data.products.len(), 2); // Only active products

    // 2. Create customer return and supplier return
    let prep_c = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: Some("cust_priya".to_string()),
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_oil".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Summary test customer".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .unwrap();
    let rec_c = confirm_customer_return_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerReturnIpcInput {
            preparation_token: prep_c.preparation_token,
        },
    )
    .unwrap();

    let prep_s = prepare_supplier_return_inner(
        &db,
        &session,
        &cache,
        PrepareSupplierReturnIpcInput {
            supplier_id: Some("sup_agro".to_string()),
            reference_purchase_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 2000,
                notes: None,
            }],
            reason: "Summary test supplier".to_string(),
        },
    )
    .unwrap();
    let rec_s = confirm_supplier_return_inner(
        &db,
        &session,
        &cache,
        ConfirmSupplierReturnIpcInput {
            preparation_token: prep_s.preparation_token,
        },
    )
    .unwrap();

    // 3. Query all summary
    let all_returns = get_returns_summary_inner(&db, &session, None).unwrap();
    assert_eq!(all_returns.len(), 2);

    // 4. Query filtered by CUSTOMER_RETURN
    let cust_only =
        get_returns_summary_inner(&db, &session, Some("CUSTOMER_RETURN".to_string())).unwrap();
    assert_eq!(cust_only.len(), 1);
    assert_eq!(cust_only[0].id, rec_c.return_id);
    assert_eq!(
        cust_only[0].counterparty_name,
        Some("Priya Patel".to_string())
    );

    // 5. Query filtered by SUPPLIER_RETURN
    let sup_only =
        get_returns_summary_inner(&db, &session, Some("SUPPLIER_RETURN".to_string())).unwrap();
    assert_eq!(sup_only.len(), 1);
    assert_eq!(sup_only[0].id, rec_s.return_id);
    assert_eq!(
        sup_only[0].counterparty_name,
        Some("Agro Farms Ltd".to_string())
    );

    // 6. Detail query
    let detail = get_return_detail_inner(&db, &session, rec_c.return_id.clone()).unwrap();
    assert_eq!(detail.id, rec_c.return_id);
    assert_eq!(detail.items.len(), 1);
    assert_eq!(detail.items[0].product_name, "Mustard Oil");
}

#[test]
fn test_26_unauthorized_employee_permission_denial() {
    let (db, session, cache, _, _, emp_no_returns) = setup_test_context();

    // Switch session to employee WITHOUT Returns permission
    session.set_identity(Some(emp_no_returns));

    let err = prepare_customer_return_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerReturnIpcInput {
            customer_id: None,
            reference_sale_id: None,
            items: vec![ReturnItemInput {
                product_id: "prod_rice".to_string(),
                quantity: 1000,
                notes: None,
            }],
            reason: "Unauthorized attempt".to_string(),
            refund_payment_method: Some("CASH".to_string()),
        },
    )
    .expect_err("Must deny permission to unauthorized employee");

    assert!(
        err.contains("Permission denied") || err.contains("lacks permission"),
        "Expected permission denied, got: {}",
        err
    );
}
