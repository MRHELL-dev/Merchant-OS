use desktop_lib::auth::{AuthService, AuthenticatedIdentity, hash_password};
use desktop_lib::db::DatabaseManager;
use desktop_lib::engine::{
    BusinessEngine, ConfirmPurchaseRequest, ConfirmSaleRequest, EngineError,
    ProcessCustomerReturnRequest, PurchaseItemRequest, RecordStockCorrectionRequest,
    SaleItemRequest, TransactionEngine,
};

fn seed_architecture_base_data(db: &DatabaseManager) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
             VALUES ('biz_arch', 'Kirana Store', '+919876543210', 'Central Market', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        let admin_hash = hash_password("admin123").unwrap();
        let emp_hash = hash_password("emp123").unwrap();

        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('usr_admin', 'admin', ?1, 'ADMIN', 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            rusqlite::params![admin_hash],
        )?;
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('usr_emp', 'cashier1', ?1, 'EMPLOYEE', 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            rusqlite::params![emp_hash],
        )?;

        // Grant employee permissions for SALES and PURCHASES
        conn.execute(
            "INSERT INTO permissions (id, user_id, feature_key, is_enabled, updated_at)
             VALUES ('perm_sales', 'usr_emp', 'SALES', 1, '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO permissions (id, user_id, feature_key, is_enabled, updated_at)
             VALUES ('perm_purchases', 'usr_emp', 'PURCHASES', 1, '2026-09-13T10:00:00Z')",
            [],
        )?;

        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_sugar', NULL, 'Organic Sugar 1kg', 'kg', 3800, 4800, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_sugar', 'prod_sugar', 100000, '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_verma', 'Rajesh Verma', '+919811100011', 'Sector 14', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
             VALUES ('supp_sugar_mill', 'Mawana Sugar Works', '+919822200022', 'Meerut', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        Ok(())
    }).expect("Failed to seed architecture base data");
}

fn get_identities(conn: &rusqlite::Connection) -> (AuthenticatedIdentity, AuthenticatedIdentity) {
    let admin = AuthService::authenticate(conn, "admin", "admin123").expect("Failed admin login");
    let emp = AuthService::authenticate(conn, "cashier1", "emp123").expect("Failed emp login");
    (admin, emp)
}

// ------------------------------------------------------------------------------------------------
// ARCHITECTURE TEST 1: HISTORICAL SALE PRICES ARE IMMUTABLE SNAPSHOTS
// ------------------------------------------------------------------------------------------------
#[test]
fn test_historical_sale_prices_remain_immutable_when_catalog_prices_change() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_architecture_base_data(&db);

    db.with_connection(|conn| {
        let (_admin, emp) = get_identities(conn);

        // 1. Confirm a sale at ₹48.00 (4,800 paise) with cost price ₹38.00 (3,800 paise)
        let sale_req = ConfirmSaleRequest {
            sale_id: "sale_hist_1".to_string(),
            sale_number: "INV-H01".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_sugar".to_string(),
                quantity: 10000, // 10kg
                unit_price_cents: 4800,
            }],
            paid_amount_cents: 48000,
            payment_method: Some("CASH".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };

        let prepared = BusinessEngine::prepare_sale(conn, &emp, sale_req)?;
        let confirmed = prepared.confirm(&emp);
        TransactionEngine::execute_sale(conn, confirmed)?;

        // 2. Later, merchant updates catalog prices: selling price jumps to ₹60.00 and cost to ₹50.00
        conn.execute(
            "UPDATE products SET selling_price_cents = 6000, cost_price_cents = 5000 WHERE id = 'prod_sugar'",
            [],
        )?;

        // 3. Verify historical sale_items prices remain unchanged
        let (stored_selling, stored_cost): (i64, i64) = conn.query_row(
            "SELECT unit_price_cents, cost_price_cents FROM sale_items WHERE sale_id = 'sale_hist_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(stored_selling, 4800, "Historical selling price must remain 4800");
        assert_eq!(stored_cost, 3800, "Historical cost price must remain 3800");

        // Verify total sale record remains unchanged
        let total_cents: i64 = conn.query_row(
            "SELECT total_amount_cents FROM sales WHERE id = 'sale_hist_1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(total_cents, 48000);

        Ok(())
    }).expect("Historical price test failed");
}

// ------------------------------------------------------------------------------------------------
// ARCHITECTURE TEST 2: HISTORICAL PURCHASE COSTS ARE IMMUTABLE SNAPSHOTS
// ------------------------------------------------------------------------------------------------
#[test]
fn test_historical_purchase_costs_remain_immutable_when_catalog_cost_changes() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_architecture_base_data(&db);

    db.with_connection(|conn| {
        let (admin, _emp) = get_identities(conn);

        // Record a purchase at negotiated rate ₹37.50 (3,750 paise)
        let pur_req = ConfirmPurchaseRequest {
            purchase_id: "pur_hist_1".to_string(),
            purchase_number: "BILL-H01".to_string(),
            supplier_id: "supp_sugar_mill".to_string(),
            items: vec![PurchaseItemRequest {
                product_id: "prod_sugar".to_string(),
                quantity: 50000, // 50kg
                unit_cost_cents: 3750,
            }],
            paid_amount_cents: 187500,
            payment_method: Some("BANK_TRANSFER".to_string()),
            user_id: "usr_admin".to_string(),
            purchase_date: "2026-09-13".to_string(),
        };

        let prepared = BusinessEngine::prepare_purchase(conn, &admin, pur_req)?;
        let confirmed = prepared.confirm(&admin);
        TransactionEngine::execute_purchase(conn, confirmed)?;

        // Later update product cost in catalog to ₹45.00
        conn.execute(
            "UPDATE products SET cost_price_cents = 4500 WHERE id = 'prod_sugar'",
            [],
        )?;

        // Verify purchase_items record remains untouched
        let (unit_cost, line_total): (i64, i64) = conn.query_row(
            "SELECT unit_cost_cents, total_cents FROM purchase_items WHERE purchase_id = 'pur_hist_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(unit_cost, 3750);
        assert_eq!(line_total, 187500);

        Ok(())
    }).expect("Purchase price snapshot test failed");
}

// ------------------------------------------------------------------------------------------------
// ARCHITECTURE TEST 3: LEDGERS ARE APPEND-ONLY WITH STRICT RUNNING BALANCE CONSISTENCY
// ------------------------------------------------------------------------------------------------
#[test]
fn test_customer_ledger_append_only_running_balances() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_architecture_base_data(&db);

    db.with_connection(|conn| {
        let (_admin, emp) = get_identities(conn);

        // Event 1: Credit sale of ₹100
        let sale_req = ConfirmSaleRequest {
            sale_id: "s_ledg_1".to_string(),
            sale_number: "INV-L01".to_string(),
            customer_id: Some("cust_verma".to_string()),
            items: vec![SaleItemRequest {
                product_id: "prod_sugar".to_string(),
                quantity: 2000,
                unit_price_cents: 5000,
            }],
            paid_amount_cents: 0,
            payment_method: None,
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };
        let prep1 = BusinessEngine::prepare_sale(conn, &emp, sale_req)?;
        let conf1 = prep1.confirm(&emp);
        TransactionEngine::execute_sale(conn, conf1)?;

        // Event 2: Credit sale of ₹50
        let sale_req_2 = ConfirmSaleRequest {
            sale_id: "s_ledg_2".to_string(),
            sale_number: "INV-L02".to_string(),
            customer_id: Some("cust_verma".to_string()),
            items: vec![SaleItemRequest {
                product_id: "prod_sugar".to_string(),
                quantity: 1000,
                unit_price_cents: 5000,
            }],
            paid_amount_cents: 0,
            payment_method: None,
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };
        let prep2 = BusinessEngine::prepare_sale(conn, &emp, sale_req_2)?;
        let conf2 = prep2.confirm(&emp);
        TransactionEngine::execute_sale(conn, conf2)?;

        // Verify ledger entries are strictly append-only and consecutive
        let mut stmt = conn.prepare(
            "SELECT amount_cents, balance_before_cents, balance_after_cents FROM customer_ledger WHERE customer_id = 'cust_verma' ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?;

        let history: Vec<(i64, i64, i64)> = rows.map(|r| r.unwrap()).collect();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], (10000, 0, 10000));
        assert_eq!(history[1], (5000, 10000, 15000));

        // Customer's current balance must match the last entry's balance_after_cents
        let current_bal: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_verma'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(current_bal, 15000);

        Ok(())
    }).expect("Customer ledger consistency test failed");
}

// ------------------------------------------------------------------------------------------------
// ARCHITECTURE TEST 4: ADMIN AUTHORITY BOUNDARY STRICTLY ENFORCED
// ------------------------------------------------------------------------------------------------
#[test]
fn test_admin_authority_boundary_strictly_enforced() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_architecture_base_data(&db);

    db.with_connection(|conn| {
        let (_admin, emp) = get_identities(conn);

        // Attempt admin-only stock correction as an EMPLOYEE
        let corr_req = RecordStockCorrectionRequest {
            correction_id: "corr_arch_emp".to_string(),
            product_id: "prod_sugar".to_string(),
            quantity_change: -1000,
            reason: "DAMAGED".to_string(),
            note: "Employee unauthorized attempt".to_string(),
            admin_user_id: "usr_emp".to_string(),
        };
        let err = BusinessEngine::prepare_stock_correction(conn, &emp, corr_req).unwrap_err();
        assert!(matches!(err, EngineError::AdminAuthorizationRequired(_)));

        // Attempt customer return authorization as an EMPLOYEE
        let ret_req = ProcessCustomerReturnRequest {
            return_id: "ret_arch_emp".to_string(),
            return_number: "RET-A01".to_string(),
            reference_sale_id: None,
            customer_id: "cust_verma".to_string(),
            items: vec![desktop_lib::engine::ReturnItemRequest {
                product_id: "prod_sugar".to_string(),
                quantity: 1000,
                unit_price_cents: 4800,
            }],
            reason: "Defective".to_string(),
            admin_user_id: "usr_emp".to_string(),
        };
        let err = BusinessEngine::prepare_customer_return(conn, &emp, ret_req).unwrap_err();
        assert!(matches!(err, EngineError::AdminAuthorizationRequired(_)));

        Ok(())
    }).expect("Admin authority boundary test failed");
}
