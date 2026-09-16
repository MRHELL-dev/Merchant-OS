use desktop_lib::db::operations::{
    confirm_purchase, confirm_sale, convert_order_to_sale, process_customer_return,
    process_supplier_return, record_payment, record_stock_correction, BusinessError,
    PurchaseItemInput, ReturnItemInput, SaleItemInput,
};
use desktop_lib::db::DatabaseManager;
use rusqlite::params;

/// Helper to seed minimal prerequisite admin, employee, product, customer, and supplier.
fn seed_base_data(db: &DatabaseManager) {
    db.with_connection(|conn| {
        // Business profile
        conn.execute(
            "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
             VALUES ('biz_1', 'Ramesh Grocery', '+919876543210', 'Main Bazaar, Delhi', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Admin and Employee
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('usr_admin', 'admin', 'hash_admin', 'ADMIN', 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('usr_emp', 'cashier1', 'hash_emp', 'EMPLOYEE', 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Permissions for employee (valid BUILD 04 permission keys)
        for (i, key) in ["SALES", "PURCHASES", "CUSTOMER_ORDERS", "CUSTOMERS", "SUPPLIERS", "PAYMENTS"].iter().enumerate() {
            conn.execute(
                "INSERT INTO permissions (id, user_id, feature_key, is_enabled, updated_at)
                 VALUES (?1, 'usr_emp', ?2, 1, '2026-09-13T10:00:00Z')",
                params![format!("perm_{}", i), key],
            )?;
        }

        // Categories
        conn.execute(
            "INSERT INTO categories (id, name, slug, description, created_at)
             VALUES ('cat_grains', 'Grains & Pulses', 'grains-pulses', 'Rice, Dal, Wheat', '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Products: Basmati Rice (loose goods in kg) and Masala Pouch (packaged in pcs)
        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_rice', 'cat_grains', 'Basmati Rice Premium', 'kg', 6000, 9000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_masala', NULL, 'Garam Masala 100g', 'pcs', 3000, 4500, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Initial inventory: 50.000 kg Rice (50,000 milli) and 20 pcs Masala (20,000 milli)
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_rice', 'prod_rice', 50000, '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_masala', 'prod_masala', 20000, '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Customers
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_sharma', 'Sunil Sharma', '+919811122233', 'Plot 12, Delhi', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Suppliers
        conn.execute(
            "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
             VALUES ('supp_agri', 'Kisan Agri Wholesale', '+919822233344', 'Grain Mandi, Punjab', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;

        Ok(())
    }).expect("Failed to seed base data");
}

// ------------------------------------------------------------------------------------------------
// TEST 1: ALL 26 TABLES EXIST AND SYSTEM METADATA PRESERVED
// ------------------------------------------------------------------------------------------------
#[test]
fn test_all_26_tables_exist() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    let tables = db.get_table_names().expect("Failed to get tables");
    assert_eq!(tables.len(), 26, "Expected exactly 26 tables");

    let required = [
        "businesses", "users", "permissions", "categories", "products",
        "barcode_mappings", "inventory", "stock_movements", "stock_corrections",
        "customers", "customer_ledger", "suppliers", "supplier_ledger",
        "customer_orders", "customer_order_items", "sales", "sale_items",
        "purchases", "purchase_items", "returns", "return_items",
        "payments", "expenses", "audit_logs", "system_metadata", "schema_migrations",
    ];

    for name in required {
        assert!(tables.contains(&name.to_string()), "Missing table: {}", name);
    }
}

// ------------------------------------------------------------------------------------------------
// TEST 2: FOREIGN KEY ENFORCEMENT
// ------------------------------------------------------------------------------------------------
#[test]
fn test_foreign_key_referential_integrity() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    let result = db.with_connection(|conn| {
        // Attempt to insert product referencing non-existent category
        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_bad', 'cat_non_existent', 'Bad Product', 'pcs', 100, 200, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        Ok(())
    });

    assert!(result.is_err(), "Foreign key violation must fail when category does not exist");
}

// ------------------------------------------------------------------------------------------------
// TEST 3: UNIQUE CONSTRAINTS
// ------------------------------------------------------------------------------------------------
#[test]
fn test_unique_constraints() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    let result = db.with_connection(|conn| {
        // Duplicate username
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES ('usr_admin2', 'admin', 'pass2', 'ADMIN', 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        Ok(())
    });

    assert!(result.is_err(), "Duplicate username must violate unique index");
}

// ------------------------------------------------------------------------------------------------
// TEST 4: BARCODE MAPPINGS DECOUPLED FROM PRODUCT IDENTITY
// ------------------------------------------------------------------------------------------------
#[test]
fn test_barcode_mapping_architecture() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    db.with_connection(|conn| {
        // Map manufacturer barcode and weighing scale barcode to same product
        conn.execute(
            "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, notes, created_at)
             VALUES ('bm_1', '8901234567890', 'prod_rice', 'MANUFACTURER', 'Packaged 1kg bag', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, notes, created_at)
             VALUES ('bm_2', '2000001025005', 'prod_rice', 'WEIGHING_SCALE', 'Loose scale sticker', '2026-09-13T10:00:00Z')",
            [],
        )?;

        // Verify product lookup from barcode
        let mut stmt = conn.prepare(
            "SELECT p.name, p.unit FROM barcode_mappings bm
             JOIN products p ON bm.product_id = p.id
             WHERE bm.barcode = ?1",
        )?;

        let (name, unit): (String, String) = stmt.query_row(params!["2000001025005"], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

        assert_eq!(name, "Basmati Rice Premium");
        assert_eq!(unit, "kg");
        Ok(())
    }).expect("Barcode resolution failed");
}

// ------------------------------------------------------------------------------------------------
// TEST 5: CONFIRM SALE ATOMICITY (CASH + CREDIT SPLIT)
// ------------------------------------------------------------------------------------------------
#[test]
fn test_confirm_sale_atomicity() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    // Sunil Sharma buys 2.500 kg Rice @ ₹90/kg (₹225.00 = 22500 cents)
    // and 2 pcs Garam Masala @ ₹45/pc (₹90.00 = 9000 cents)
    // Total = ₹315.00 (31500 cents).
    // Pays ₹200.00 (20000 cents) Cash; Credit Due = ₹115.00 (11500 cents).
    let sale_items = vec![
        SaleItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 2500, // 2.500 kg
            unit_price_cents: 9000,
        },
        SaleItemInput {
            product_id: "prod_masala".to_string(),
            quantity: 2000, // 2 pcs
            unit_price_cents: 4500,
        },
    ];

    db.with_connection(|conn| {
        confirm_sale(
            conn,
            "sale_101",
            "SALE-20260913-0001",
            Some("cust_sharma"),
            &sale_items,
            20000, // paid
            Some("CASH"),
            "usr_emp",
            "2026-09-13",
        )
    }).expect("Sale confirmation failed");

    db.with_connection(|conn| {
        // 1. Verify sale header
        let mut s_stmt = conn.prepare("SELECT total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status FROM sales WHERE id = 'sale_101'")?;
        let (total, paid, credit, status): (i64, i64, i64, String) = s_stmt.query_row([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        assert_eq!(total, 31500);
        assert_eq!(paid, 20000);
        assert_eq!(credit, 11500);
        assert_eq!(status, "PARTIAL");

        // 2. Verify historical prices in sale_items
        let mut si_stmt = conn.prepare("SELECT unit_price_cents, cost_price_cents, total_cents FROM sale_items WHERE sale_id = 'sale_101' AND product_id = 'prod_rice'")?;
        let (unit_p, cost_p, line_tot): (i64, i64, i64) = si_stmt.query_row([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        assert_eq!(unit_p, 9000);
        assert_eq!(cost_p, 6000);
        assert_eq!(line_tot, 22500);

        // 3. Verify inventory deducted
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 47500); // 50000 - 2500

        // 4. Verify stock_movements ledger
        let mut sm_stmt = conn.prepare("SELECT quantity_change, movement_type, reference_type FROM stock_movements WHERE reference_id = 'sale_101' AND product_id = 'prod_rice'")?;
        let (delta, m_type, r_type): (i64, String, String) = sm_stmt.query_row([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        assert_eq!(delta, -2500);
        assert_eq!(m_type, "SALE");
        assert_eq!(r_type, "SALES");

        // 5. Verify customer credit & ledger
        let mut cust_stmt = conn.prepare("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'")?;
        let cust_credit: i64 = cust_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(cust_credit, 11500);

        let mut cl_stmt = conn.prepare("SELECT entry_type, amount_cents, balance_after_cents FROM customer_ledger WHERE reference_id = 'sale_101'")?;
        let (c_type, c_amt, c_bal): (String, i64, i64) = cl_stmt.query_row([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        assert_eq!(c_type, "SALE_CREDIT");
        assert_eq!(c_amt, 11500);
        assert_eq!(c_bal, 11500);

        // 6. Verify payments
        let mut pmt_stmt = conn.prepare("SELECT amount_cents, payment_method FROM payments WHERE related_entity_id = 'sale_101'")?;
        let (p_amt, p_method): (i64, String) = pmt_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(p_amt, 20000);
        assert_eq!(p_method, "CASH");

        // 7. Verify audit log
        let mut aud_stmt = conn.prepare("SELECT action, entity_type FROM audit_logs WHERE entity_id = 'sale_101'")?;
        let (a_act, a_ent): (String, String) = aud_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(a_act, "SALE_CONFIRMED");
        assert_eq!(a_ent, "sales");

        Ok(())
    }).expect("Verification query failed");
}

// ------------------------------------------------------------------------------------------------
// TEST 6: CONFIRM PURCHASE ATOMICITY (STOCK INWARD + SUPPLIER PAYABLE)
// ------------------------------------------------------------------------------------------------
#[test]
fn test_confirm_purchase_atomicity() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    // Purchase 20.000 kg Rice @ ₹58/kg (cost = 5800 cents * 20 = 116000 cents)
    // Paid ₹500.00 (50000 cents) via UPI; Outstanding = ₹660.00 (66000 cents).
    let purchase_items = vec![
        PurchaseItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 20000, // 20 kg
            unit_cost_cents: 5800,
        },
    ];

    db.with_connection(|conn| {
        confirm_purchase(
            conn,
            "pur_201",
            "PUR-20260913-0001",
            "supp_agri",
            &purchase_items,
            50000,
            Some("UPI"),
            "usr_admin",
            "2026-09-13",
        )
    }).expect("Purchase confirmation failed");

    db.with_connection(|conn| {
        // 1. Verify stock increased
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 70000); // 50000 + 20000

        // 2. Verify stock movement
        let mut sm_stmt = conn.prepare("SELECT quantity_change, movement_type FROM stock_movements WHERE reference_id = 'pur_201'")?;
        let (delta, m_type): (i64, String) = sm_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(delta, 20000);
        assert_eq!(m_type, "PURCHASE");

        // 3. Verify supplier payable & ledger
        let mut supp_stmt = conn.prepare("SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_agri'")?;
        let outstanding: i64 = supp_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(outstanding, 66000);

        let mut sl_stmt = conn.prepare("SELECT entry_type, amount_cents, balance_after_cents FROM supplier_ledger WHERE reference_id = 'pur_201'")?;
        let (s_type, s_amt, s_bal): (String, i64, i64) = sl_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        assert_eq!(s_type, "PURCHASE_CREDIT");
        assert_eq!(s_amt, 66000);
        assert_eq!(s_bal, 66000);

        // 4. Verify payment
        let mut pmt_stmt = conn.prepare("SELECT amount_cents, payment_method FROM payments WHERE related_entity_id = 'pur_201'")?;
        let (p_amt, p_method): (i64, String) = pmt_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(p_amt, 50000);
        assert_eq!(p_method, "UPI");

        Ok(())
    }).expect("Purchase verification failed");
}

// ------------------------------------------------------------------------------------------------
// TEST 7: CUSTOMER ORDER DRAFT ISOLATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_customer_order_draft_isolation() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    db.with_connection(|conn| {
        // Create draft order for 10.000 kg Rice
        conn.execute(
            "INSERT INTO customer_orders (id, order_number, customer_id, status, converted_sale_id, notes, user_id, created_at, updated_at)
             VALUES ('ord_draft_1', 'ORD-20260913-0001', 'cust_sharma', 'DRAFT', NULL, 'Evening delivery', 'usr_emp', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO customer_order_items (id, order_id, product_id, quantity, unit_price_cents, notes)
             VALUES ('ordi_1', 'ord_draft_1', 'prod_rice', 10000, 9000, NULL)",
            [],
        )?;

        // Verify: Inventory is STILL 50.000 kg (unaffected by draft)
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 50000);

        // Verify: Zero sales recorded
        let mut sales_stmt = conn.prepare("SELECT COUNT(*) FROM sales")?;
        let sales_count: i64 = sales_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(sales_count, 0);

        // Verify: Customer credit remains 0
        let mut cust_stmt = conn.prepare("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'")?;
        let credit: i64 = cust_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(credit, 0);

        Ok(())
    }).expect("Draft isolation check failed");
}

// ------------------------------------------------------------------------------------------------
// TEST 8: ORDER CONVERSION ATOMICITY & STATUS CHANGE
// ------------------------------------------------------------------------------------------------
#[test]
fn test_customer_order_conversion_success() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO customer_orders (id, order_number, customer_id, status, converted_sale_id, notes, user_id, created_at, updated_at)
             VALUES ('ord_301', 'ORD-20260913-0301', 'cust_sharma', 'DRAFT', NULL, NULL, 'usr_emp', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO customer_order_items (id, order_id, product_id, quantity, unit_price_cents, notes)
             VALUES ('ordi_301', 'ord_301', 'prod_rice', 5000, 9000, NULL)",
            [],
        )?;
        Ok(())
    }).unwrap();

    // Convert to confirmed sale: 5.000 kg @ ₹90/kg = ₹450.00 (45000 cents). Customer pays ₹450.00 in full.
    db.with_connection(|conn| {
        convert_order_to_sale(
            conn,
            "ord_301",
            "sale_from_ord_301",
            "SALE-20260913-0301",
            45000,
            Some("UPI"),
            "usr_emp",
            "2026-09-13",
        )
    }).expect("Order conversion failed");

    db.with_connection(|conn| {
        // Verify order status is CONVERTED and converted_sale_id is set
        let mut o_stmt = conn.prepare("SELECT status, converted_sale_id FROM customer_orders WHERE id = 'ord_301'")?;
        let (status, sale_ref): (String, Option<String>) = o_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(status, "CONVERTED");
        assert_eq!(sale_ref, Some("sale_from_ord_301".to_string()));

        // Verify inventory reduced by 5.000 kg
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 45000); // 50000 - 5000

        Ok(())
    }).unwrap();
}

// ------------------------------------------------------------------------------------------------
// TEST 9: FAILED ORDER CONVERSION ROLLBACK (INSUFFICIENT STOCK)
// ------------------------------------------------------------------------------------------------
#[test]
fn test_failed_order_conversion_rollback() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    db.with_connection(|conn| {
        // Draft order requesting 60.000 kg (only 50.000 available!)
        conn.execute(
            "INSERT INTO customer_orders (id, order_number, customer_id, status, converted_sale_id, notes, user_id, created_at, updated_at)
             VALUES ('ord_fail', 'ORD-20260913-9999', 'cust_sharma', 'DRAFT', NULL, NULL, 'usr_emp', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO customer_order_items (id, order_id, product_id, quantity, unit_price_cents, notes)
             VALUES ('ordi_fail', 'ord_fail', 'prod_rice', 60000, 9000, NULL)",
            [],
        )?;
        Ok(())
    }).unwrap();

    let result = db.with_connection(|conn| {
        convert_order_to_sale(
            conn,
            "ord_fail",
            "sale_should_fail",
            "SALE-FAIL",
            540000,
            Some("CASH"),
            "usr_emp",
            "2026-09-13",
        )
    });

    assert!(result.is_err(), "Conversion must fail due to insufficient stock");

    db.with_connection(|conn| {
        // Order MUST remain DRAFT
        let mut o_stmt = conn.prepare("SELECT status, converted_sale_id FROM customer_orders WHERE id = 'ord_fail'")?;
        let (status, sale_ref): (String, Option<String>) = o_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(status, "DRAFT");
        assert_eq!(sale_ref, None);

        // No sale created
        let mut s_stmt = conn.prepare("SELECT COUNT(*) FROM sales WHERE id = 'sale_should_fail'")?;
        let s_count: i64 = s_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(s_count, 0);

        // Stock unaffected
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 50000);

        Ok(())
    }).unwrap();
}

// ------------------------------------------------------------------------------------------------
// TEST 10: CUSTOMER RETURN SPLIT (DEBT REDUCTION + EXCESS CASH REFUND)
// ------------------------------------------------------------------------------------------------
#[test]
fn test_customer_return_debt_refund_split() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    // Customer Sunil Sharma owes ₹1,000.00 (100000 cents)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE customers SET current_credit_cents = 100000 WHERE id = 'cust_sharma'",
            [],
        )?;
        Ok(())
    }).unwrap();

    // Customer returns goods worth ₹1,500.00 (150000 cents):
    // 16.667 kg Rice @ ₹90/kg (approx 16667 milli * 9000 cents = 150003 cents) -> we test exact 150000 cents
    let return_items = vec![
        ReturnItemInput {
            product_id: "prod_rice".to_string(),
            quantity: 16667, // 16.667 kg
            unit_price_cents: 9000,
        },
    ];

    db.with_connection(|conn| {
        process_customer_return(
            conn,
            "ret_cust_1",
            "RET-20260913-0001",
            None,
            "cust_sharma",
            &return_items,
            "Customer overpurchased for banquet",
            "usr_admin", // authorized by Admin
        )
    }).expect("Customer return processing failed");

    db.with_connection(|conn| {
        // Total return = 150003 cents (approx ₹1,500)
        // 1. Debt of 100000 cents should be completely wiped (balance = 0)
        let mut c_stmt = conn.prepare("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'")?;
        let new_debt: i64 = c_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(new_debt, 0, "Debt must be completely reduced to 0");

        // 2. Excess refund of 50003 cents must be recorded in payments
        let mut pmt_stmt = conn.prepare("SELECT amount_cents, payment_type FROM payments WHERE related_entity_id = 'ret_cust_1'")?;
        let (ref_amt, p_type): (i64, String) = pmt_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(ref_amt, 50003);
        assert_eq!(p_type, "CUSTOMER_RETURN_REFUND");

        // 3. Inventory increased by 16.667 kg
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 66667); // 50000 + 16667

        Ok(())
    }).unwrap();
}

// ------------------------------------------------------------------------------------------------
// TEST 11: SUPPLIER RETURN (COST PRICE, STOCK DECREASE, PAYABLE DEBIT)
// ------------------------------------------------------------------------------------------------
#[test]
fn test_supplier_return_admin_approved() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    // Supplier is owed ₹500.00 (50000 cents)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE suppliers SET current_outstanding_cents = 50000 WHERE id = 'supp_agri'",
            [],
        )?;
        Ok(())
    }).unwrap();

    // Return 5 pcs Garam Masala @ ₹30 cost price (3000 cents) = ₹150.00 (15000 cents)
    let return_items = vec![
        ReturnItemInput {
            product_id: "prod_masala".to_string(),
            quantity: 5000, // 5 pcs
            unit_price_cents: 3000,
        },
    ];

    db.with_connection(|conn| {
        process_supplier_return(
            conn,
            "ret_supp_1",
            "RET-SUPP-0001",
            None,
            "supp_agri",
            &return_items,
            "Damaged packaging on receipt",
            "usr_admin", // authorized by Admin
        )
    }).expect("Supplier return processing failed");

    db.with_connection(|conn| {
        // Stock decreased: 20 pcs - 5 pcs = 15 pcs (15000 milli)
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_masala'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 15000);

        // Supplier payable reduced: 50000 - 15000 = 35000 cents (₹350.00)
        let mut supp_stmt = conn.prepare("SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_agri'")?;
        let payable: i64 = supp_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(payable, 35000);

        // Supplier ledger debit recorded
        let mut sl_stmt = conn.prepare("SELECT entry_type, amount_cents FROM supplier_ledger WHERE reference_id = 'ret_supp_1'")?;
        let (entry, amt): (String, i64) = sl_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(entry, "RETURN_DEBIT");
        assert_eq!(amt, 15000);

        Ok(())
    }).unwrap();
}

// ------------------------------------------------------------------------------------------------
// TEST 12: STOCK CORRECTION REASONS & ADMIN AUTHORIZATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_stock_correction_admin_enforcement() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    // 1. Employee attempt must FAIL
    let emp_attempt = db.with_connection(|conn| {
        record_stock_correction(
            conn,
            "corr_emp",
            "prod_rice",
            -2000,
            "EXPIRED",
            "Found mold in bag",
            "usr_emp", // Employee, not Admin
        )
    });

    assert!(
        matches!(emp_attempt, Err(BusinessError::AdminAuthorizationRequired(_))),
        "Expected AdminAuthorizationRequired error, got: {:?}",
        emp_attempt
    );

    // 2. Admin attempt must SUCCEED
    db.with_connection(|conn| {
        record_stock_correction(
            conn,
            "corr_admin",
            "prod_rice",
            -2000,
            "EXPIRED",
            "Authorized waste write-off",
            "usr_admin", // Admin
        )
    }).expect("Admin stock correction failed");

    db.with_connection(|conn| {
        // Stock reduced by 2.000 kg: 50000 - 2000 = 48000 milli
        let mut inv_stmt = conn.prepare("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'")?;
        let qty: i64 = inv_stmt.query_row([], |r| r.get(0))?;
        assert_eq!(qty, 48000);

        // Stock movement recorded
        let mut sm_stmt = conn.prepare("SELECT movement_type, reference_type FROM stock_movements WHERE reference_id = 'corr_admin'")?;
        let (m_type, r_type): (String, String) = sm_stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(m_type, "CORRECTION");
        assert_eq!(r_type, "STOCK_CORRECTIONS");

        Ok(())
    }).unwrap();
}

// ------------------------------------------------------------------------------------------------
// TEST 13: INVALID POLYMORPHIC PAYMENT REFERENCE REJECTION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_invalid_payment_reference_rejection() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    // Attempt to record payment with related_entity_type 'CUSTOMER' but non-existent customer id
    let result = db.with_connection(|conn| {
        record_payment(
            conn,
            "pmt_invalid",
            "CUSTOMER_CREDIT_SETTLEMENT",
            "CUSTOMER",
            "cust_non_existent_999",
            5000,
            "UPI",
            "usr_emp",
            Some("Attempt payment on phantom customer"),
        )
    });

    assert!(
        matches!(result, Err(BusinessError::InvalidPaymentReference { .. })),
        "Polymorphic reference check must reject payment referencing non-existent entity"
    );

    // Attempt to record payment with non-existent sale
    let result_sale = db.with_connection(|conn| {
        record_payment(
            conn,
            "pmt_invalid_sale",
            "CUSTOMER_SALE",
            "SALE",
            "sale_ghost_123",
            5000,
            "CASH",
            "usr_emp",
            None,
        )
    });

    assert!(
        matches!(result_sale, Err(BusinessError::InvalidPaymentReference { .. })),
        "Must reject non-existent sale reference"
    );
}

// ------------------------------------------------------------------------------------------------
// TEST 14: EXPENSE RECORDING
// ------------------------------------------------------------------------------------------------
#[test]
fn test_expenses_recording() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_base_data(&db);

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO expenses (id, expense_name, amount_cents, category, expense_date, notes, user_id, created_at)
             VALUES ('exp_1', 'Evening Chai and Samosas for Staff', 15000, 'TEA_SNACKS', '2026-09-13', 'Daily tea', 'usr_admin', '2026-09-13T10:00:00Z')",
            [],
        )?;

        let mut stmt = conn.prepare("SELECT amount_cents, category FROM expenses WHERE id = 'exp_1'")?;
        let (amt, cat): (i64, String) = stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(amt, 15000);
        assert_eq!(cat, "TEA_SNACKS");

        Ok(())
    }).expect("Expense recording failed");
}
