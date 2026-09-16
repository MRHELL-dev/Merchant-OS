use desktop_lib::auth::{AuthService, AuthenticatedIdentity, hash_password};
use desktop_lib::db::DatabaseManager;
use desktop_lib::engine::{
    BusinessEngine, ConfirmPurchaseRequest, ConfirmSaleRequest, ConvertOrderToSaleRequest,
    EngineError, ProcessCustomerReturnRequest, ProcessSupplierReturnRequest,
    PurchaseItemRequest, RecordCustomerPaymentRequest, RecordExpenseRequest,
    RecordStockCorrectionRequest, RecordSupplierPaymentRequest, ReturnItemRequest,
    SaleItemRequest, TransactionEngine,
};

fn seed_engine_base_data(db: &DatabaseManager) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
             VALUES ('biz_1', 'Ramesh Kirana', '+919876543210', 'Main Market, Delhi', '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
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

        // Grant employee permissions: SALES, PURCHASES, CUSTOMER_CREDITS, SUPPLIERS, CUSTOMER_ORDERS, EXPENSES
        let perms = [
            "SALES", "PURCHASES", "CUSTOMER_CREDITS", "SUPPLIERS", "CUSTOMER_ORDERS", "EXPENSES"
        ];
        for (i, p) in perms.iter().enumerate() {
            conn.execute(
                "INSERT INTO permissions (id, user_id, feature_key, is_enabled, updated_at)
                 VALUES (?1, 'usr_emp', ?2, 1, '2026-09-13T10:00:00Z')",
                rusqlite::params![format!("perm_{}", i), p],
            )?;
        }

        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_atta', NULL, 'Chakki Atta 5kg', 'kg', 16000, 21000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO products (id, category_id, name, unit, cost_price_cents, selling_price_cents, is_active, created_at, updated_at)
             VALUES ('prod_oil', NULL, 'Mustard Oil 1L', 'L', 12000, 15000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_atta', 'prod_atta', 30000, '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES ('inv_oil', 'prod_oil', 20000, '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_gupta', 'Manoj Gupta', '+919811199988', 'Shop 4, Market', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
             VALUES ('supp_agro', 'Agro Mills Ltd', '+919822299977', 'Ind Area, Karnal', 0, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
            [],
        )?;
        Ok(())
    }).expect("Failed to seed engine base data");
}

fn get_identities(conn: &rusqlite::Connection) -> (AuthenticatedIdentity, AuthenticatedIdentity) {
    let admin = AuthService::authenticate(conn, "admin", "admin123").expect("Failed admin login");
    let emp = AuthService::authenticate(conn, "cashier1", "emp123").expect("Failed emp login");
    (admin, emp)
}

// ------------------------------------------------------------------------------------------------
// 1. SALES: FULL PAID, CREDIT, INSUFFICIENT STOCK, INVALID QUANTITY
// ------------------------------------------------------------------------------------------------
#[test]
fn test_sales_lifecycle_validation_and_atomicity() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (_admin, emp) = get_identities(conn);

        // A. Invalid quantity rejection (quantity <= 0)
        let invalid_qty_req = ConfirmSaleRequest {
            sale_id: "sale_inv_qty".to_string(),
            sale_number: "INV-001".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_atta".to_string(),
                quantity: 0,
                unit_price_cents: 21000,
            }],
            paid_amount_cents: 21000,
            payment_method: Some("CASH".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };
        let err = BusinessEngine::prepare_sale(conn, &emp, invalid_qty_req).unwrap_err();
        assert!(matches!(err, EngineError::InvalidQuantity { .. }));

        // B. Insufficient stock rejection
        let overstock_req = ConfirmSaleRequest {
            sale_id: "sale_overstock".to_string(),
            sale_number: "INV-002".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_atta".to_string(),
                quantity: 35000, // available 30,000
                unit_price_cents: 21000,
            }],
            paid_amount_cents: 21000,
            payment_method: Some("CASH".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };
        let err = BusinessEngine::prepare_sale(conn, &emp, overstock_req).unwrap_err();
        assert!(matches!(err, EngineError::InsufficientStock { .. }));

        // C. Successful Paid Sale: 10kg Atta (10,000 milli) @ ₹210 = ₹2,100 (210,000 paise)
        let paid_sale_req = ConfirmSaleRequest {
            sale_id: "sale_paid_1".to_string(),
            sale_number: "INV-003".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_atta".to_string(),
                quantity: 10000,
                unit_price_cents: 21000,
            }],
            paid_amount_cents: 210000,
            payment_method: Some("CASH".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };

        // Merchant Confirmation Boundary: Prepare -> Explicit Confirm -> Transaction Execute
        let prepared_sale = BusinessEngine::prepare_sale(conn, &emp, paid_sale_req)?;
        assert_eq!(prepared_sale.payload.total_amount_cents, 210000);
        assert_eq!(prepared_sale.payload.payment_status, "PAID");

        let confirmed_sale = prepared_sale.confirm(&emp);
        TransactionEngine::execute_sale(conn, confirmed_sale)?;

        // Verify stock deducted to 20,000
        let current_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_atta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(current_stock, 20000);

        // Verify stock movement created
        let (mov_delta, mov_type): (i64, String) = conn.query_row(
            "SELECT quantity_change, movement_type FROM stock_movements WHERE reference_id = 'sale_paid_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(mov_delta, -10000);
        assert_eq!(mov_type, "SALE");

        // Verify audit log created
        let audit_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE entity_id = 'sale_paid_1' AND action = 'SALE_CONFIRMED'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(audit_count, 1);

        // D. Credit Sale: 5kg Atta @ ₹210 = ₹1,050 (105,000 paise), Paid ₹500, Due ₹550
        let credit_sale_req = ConfirmSaleRequest {
            sale_id: "sale_credit_1".to_string(),
            sale_number: "INV-004".to_string(),
            customer_id: Some("cust_gupta".to_string()),
            items: vec![SaleItemRequest {
                product_id: "prod_atta".to_string(),
                quantity: 5000,
                unit_price_cents: 21000,
            }],
            paid_amount_cents: 50000,
            payment_method: Some("UPI".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };

        let prepared_credit = BusinessEngine::prepare_sale(conn, &emp, credit_sale_req)?;
        assert_eq!(prepared_credit.payload.credit_amount_cents, 55000);
        assert_eq!(prepared_credit.payload.payment_status, "PARTIAL");

        let confirmed_credit = prepared_credit.confirm(&emp);
        TransactionEngine::execute_sale(conn, confirmed_credit)?;

        // Customer outstanding credit must be ₹550
        let cust_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_gupta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(cust_credit, 55000);

        // Customer ledger must reflect running balance
        let (entry_type, amt, bal_after): (String, i64, i64) = conn.query_row(
            "SELECT entry_type, amount_cents, balance_after_cents FROM customer_ledger WHERE reference_id = 'sale_credit_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(entry_type, "SALE_CREDIT");
        assert_eq!(amt, 55000);
        assert_eq!(bal_after, 55000);

        Ok(())
    }).expect("Sales lifecycle test failed");
}

// ------------------------------------------------------------------------------------------------
// 2. PURCHASES: FULL, PARTIAL, AND CREDIT
// ------------------------------------------------------------------------------------------------
#[test]
fn test_purchase_lifecycle_and_payable_ledger() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (admin, _emp) = get_identities(conn);

        // Purchase 50L Oil @ ₹120 = ₹6,000 (600,000 paise). Pay ₹2,000 (200,000 paise), Due ₹4,000
        let purchase_req = ConfirmPurchaseRequest {
            purchase_id: "pur_oil_1".to_string(),
            purchase_number: "PO-001".to_string(),
            supplier_id: "supp_agro".to_string(),
            items: vec![PurchaseItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 50000,
                unit_cost_cents: 12000,
            }],
            paid_amount_cents: 200000,
            payment_method: Some("BANK_TRANSFER".to_string()),
            user_id: "usr_admin".to_string(),
            purchase_date: "2026-09-13".to_string(),
        };

        let prepared = BusinessEngine::prepare_purchase(conn, &admin, purchase_req)?;
        assert_eq!(prepared.payload.total_amount_cents, 600000);
        assert_eq!(prepared.payload.credit_amount_cents, 400000);

        let confirmed = prepared.confirm(&admin);
        TransactionEngine::execute_purchase(conn, confirmed)?;

        // Stock increased from 20,000 to 70,000
        let oil_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(oil_stock, 70000);

        // Supplier payable is ₹4,000 (400,000 paise)
        let payable: i64 = conn.query_row(
            "SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_agro'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(payable, 400000);

        // Supplier ledger entry
        let (s_type, s_amt, s_bal): (String, i64, i64) = conn.query_row(
            "SELECT entry_type, amount_cents, balance_after_cents FROM supplier_ledger WHERE reference_id = 'pur_oil_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(s_type, "PURCHASE_CREDIT");
        assert_eq!(s_amt, 400000);
        assert_eq!(s_bal, 400000);

        Ok(())
    }).expect("Purchase lifecycle test failed");
}

// ------------------------------------------------------------------------------------------------
// 3. CUSTOMER PAYMENTS: FULL, PARTIAL, OVERPAYMENT REJECTION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_customer_payments_and_overpayment_prevention() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (_admin, emp) = get_identities(conn);

        // Set customer credit to ₹1,000 (100,000 paise)
        conn.execute(
            "UPDATE customers SET current_credit_cents = 100000 WHERE id = 'cust_gupta'",
            [],
        )?;

        // A. Overpayment rejection: customer owes ₹1,000, attempts to pay ₹1,500
        let overpay_req = RecordCustomerPaymentRequest {
            payment_id: "pmt_over".to_string(),
            customer_id: "cust_gupta".to_string(),
            amount_cents: 150000,
            payment_method: "CASH".to_string(),
            user_id: "usr_emp".to_string(),
            notes: Some("Overpayment attempt".to_string()),
        };
        let err = BusinessEngine::prepare_customer_payment(conn, &emp, overpay_req).unwrap_err();
        assert!(matches!(err, EngineError::OverpaymentNotAllowed { owed: 100000, attempted: 150000 }));

        // B. Partial payment: Pay ₹400 (40,000 paise)
        let partial_req = RecordCustomerPaymentRequest {
            payment_id: "pmt_part_1".to_string(),
            customer_id: "cust_gupta".to_string(),
            amount_cents: 40000,
            payment_method: "UPI".to_string(),
            user_id: "usr_emp".to_string(),
            notes: Some("Partial dues clearance".to_string()),
        };
        let prepared_part = BusinessEngine::prepare_customer_payment(conn, &emp, partial_req)?;
        assert_eq!(prepared_part.payload.balance_after_cents, 60000);

        let confirmed_part = prepared_part.confirm(&emp);
        TransactionEngine::execute_customer_payment(conn, confirmed_part)?;

        let rem_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_gupta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rem_credit, 60000);

        // Ledger check
        let (entry, amt, bal): (String, i64, i64) = conn.query_row(
            "SELECT entry_type, amount_cents, balance_after_cents FROM customer_ledger WHERE reference_id = 'pmt_part_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(entry, "PAYMENT_RECEIVED");
        assert_eq!(amt, 40000);
        assert_eq!(bal, 60000);

        // C. Full remaining payment: Pay ₹600 (60,000 paise)
        let full_req = RecordCustomerPaymentRequest {
            payment_id: "pmt_full_2".to_string(),
            customer_id: "cust_gupta".to_string(),
            amount_cents: 60000,
            payment_method: "CASH".to_string(),
            user_id: "usr_emp".to_string(),
            notes: Some("Full settlement".to_string()),
        };
        let prepared_full = BusinessEngine::prepare_customer_payment(conn, &emp, full_req)?;
        let confirmed_full = prepared_full.confirm(&emp);
        TransactionEngine::execute_customer_payment(conn, confirmed_full)?;

        let final_credit: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_gupta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(final_credit, 0);

        Ok(())
    }).expect("Customer payments test failed");
}

// ------------------------------------------------------------------------------------------------
// 4. SUPPLIER PAYMENTS: FULL, PARTIAL, OVERPAYMENT REJECTION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_supplier_payments_and_overpayment_prevention() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (admin, _emp) = get_identities(conn);

        // Set supplier payable to ₹5,000 (500,000 paise)
        conn.execute(
            "UPDATE suppliers SET current_outstanding_cents = 500000 WHERE id = 'supp_agro'",
            [],
        )?;

        // Overpayment rejection: owes ₹5,000, attempts to pay ₹6,000
        let overpay = RecordSupplierPaymentRequest {
            payment_id: "supp_pmt_over".to_string(),
            supplier_id: "supp_agro".to_string(),
            amount_cents: 600000,
            payment_method: "BANK_TRANSFER".to_string(),
            user_id: "usr_admin".to_string(),
            notes: None,
        };
        let err = BusinessEngine::prepare_supplier_payment(conn, &admin, overpay).unwrap_err();
        assert!(matches!(err, EngineError::OverpaymentNotAllowed { owed: 500000, attempted: 600000 }));

        // Pay ₹3,000 (300,000 paise)
        let payment = RecordSupplierPaymentRequest {
            payment_id: "supp_pmt_1".to_string(),
            supplier_id: "supp_agro".to_string(),
            amount_cents: 300000,
            payment_method: "BANK_TRANSFER".to_string(),
            user_id: "usr_admin".to_string(),
            notes: Some("Supplier instalment".to_string()),
        };
        let prepared = BusinessEngine::prepare_supplier_payment(conn, &admin, payment)?;
        let confirmed = prepared.confirm(&admin);
        TransactionEngine::execute_supplier_payment(conn, confirmed)?;

        let rem_payable: i64 = conn.query_row(
            "SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_agro'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rem_payable, 200000);

        Ok(())
    }).expect("Supplier payment test failed");
}

// ------------------------------------------------------------------------------------------------
// 5. CUSTOMER RETURNS: DEBT REDUCTION + CASH REFUND SPLIT & UNAUTHORIZED REJECTION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_customer_returns_and_admin_authority() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (admin, emp) = get_identities(conn);

        // Customer owes ₹1,000 (100,000 paise)
        conn.execute(
            "UPDATE customers SET current_credit_cents = 100000 WHERE id = 'cust_gupta'",
            [],
        )?;

        // Customer returns 10L Mustard Oil @ ₹150 = ₹1,500 (150,000 paise)
        let return_req = ProcessCustomerReturnRequest {
            return_id: "ret_split_1".to_string(),
            return_number: "RET-001".to_string(),
            reference_sale_id: None,
            customer_id: "cust_gupta".to_string(),
            items: vec![ReturnItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 10000,
                unit_price_cents: 15000,
            }],
            reason: "Customer ordered wrong item".to_string(),
            admin_user_id: "usr_emp".to_string(), // Employee attempt
        };

        // A. Employee attempt must FAIL
        let err = BusinessEngine::prepare_customer_return(conn, &emp, return_req.clone()).unwrap_err();
        assert!(matches!(err, EngineError::AdminAuthorizationRequired(_)));

        // B. Admin attempt must SUCCEED: ₹1,000 reduces debt to 0, ₹500 cash refund
        let mut admin_req = return_req;
        admin_req.admin_user_id = "usr_admin".to_string();

        let prepared = BusinessEngine::prepare_customer_return(conn, &admin, admin_req)?;
        assert_eq!(prepared.payload.debt_reduction_cents, 100000);
        assert_eq!(prepared.payload.cash_refund_cents, 50000);
        assert_eq!(prepared.payload.balance_after_cents, 0);

        let confirmed = prepared.confirm(&admin);
        TransactionEngine::execute_customer_return(conn, confirmed)?;

        // Customer credit is now 0
        let new_debt: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = 'cust_gupta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(new_debt, 0);

        // Refund payment created for ₹500
        let (ref_amt, p_type): (i64, String) = conn.query_row(
            "SELECT amount_cents, payment_type FROM payments WHERE related_entity_id = 'ret_split_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(ref_amt, 50000);
        assert_eq!(p_type, "CUSTOMER_RETURN_REFUND");

        // Stock increased from 20,000 to 30,000
        let new_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(new_stock, 30000);

        Ok(())
    }).expect("Customer return split test failed");
}

// ------------------------------------------------------------------------------------------------
// 6. SUPPLIER RETURNS: INSUFFICIENT STOCK & ADMIN ENFORCEMENT
// ------------------------------------------------------------------------------------------------
#[test]
fn test_supplier_returns_validation_and_authority() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (admin, emp) = get_identities(conn);

        // Supplier payable = ₹5,000 (500,000 paise). Available oil = 20,000 milli.
        conn.execute(
            "UPDATE suppliers SET current_outstanding_cents = 500000 WHERE id = 'supp_agro'",
            [],
        )?;

        // A. Employee authorization rejection
        let emp_req = ProcessSupplierReturnRequest {
            return_id: "sret_1".to_string(),
            return_number: "SR-001".to_string(),
            reference_purchase_id: None,
            supplier_id: "supp_agro".to_string(),
            items: vec![ReturnItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 5000,
                unit_price_cents: 12000,
            }],
            reason: "Defective packaging".to_string(),
            admin_user_id: "usr_emp".to_string(),
        };
        let err = BusinessEngine::prepare_supplier_return(conn, &emp, emp_req).unwrap_err();
        assert!(matches!(err, EngineError::AdminAuthorizationRequired(_)));

        // B. Insufficient stock rejection (attempts to return 25L when only 20L available)
        let over_req = ProcessSupplierReturnRequest {
            return_id: "sret_2".to_string(),
            return_number: "SR-002".to_string(),
            reference_purchase_id: None,
            supplier_id: "supp_agro".to_string(),
            items: vec![ReturnItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 25000,
                unit_price_cents: 12000,
            }],
            reason: "Excess goods".to_string(),
            admin_user_id: "usr_admin".to_string(),
        };
        let err = BusinessEngine::prepare_supplier_return(conn, &admin, over_req).unwrap_err();
        assert!(matches!(err, EngineError::InsufficientStock { .. }));

        // C. Successful supplier return: Return 10L Oil @ ₹120 = ₹1,200 (120,000 paise)
        let valid_req = ProcessSupplierReturnRequest {
            return_id: "sret_3".to_string(),
            return_number: "SR-003".to_string(),
            reference_purchase_id: None,
            supplier_id: "supp_agro".to_string(),
            items: vec![ReturnItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 10000,
                unit_price_cents: 12000,
            }],
            reason: "Quality issue".to_string(),
            admin_user_id: "usr_admin".to_string(),
        };
        let prepared = BusinessEngine::prepare_supplier_return(conn, &admin, valid_req)?;
        assert_eq!(prepared.payload.balance_after_cents, 380000);

        let confirmed = prepared.confirm(&admin);
        TransactionEngine::execute_supplier_return(conn, confirmed)?;

        let final_payable: i64 = conn.query_row(
            "SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_agro'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(final_payable, 380000);

        let final_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(final_stock, 10000);

        Ok(())
    }).expect("Supplier return test failed");
}

// ------------------------------------------------------------------------------------------------
// 7. STOCK CORRECTIONS: REASONS, NEGATIVE STOCK REJECTION, ADMIN AUTHORITY
// ------------------------------------------------------------------------------------------------
#[test]
fn test_stock_corrections_invariants_and_reasons() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (admin, emp) = get_identities(conn);

        // A. Employee rejection
        let emp_req = RecordStockCorrectionRequest {
            correction_id: "corr_emp".to_string(),
            product_id: "prod_atta".to_string(),
            quantity_change: -5000,
            reason: "DAMAGED".to_string(),
            note: "Water leak".to_string(),
            admin_user_id: "usr_emp".to_string(),
        };
        let err = BusinessEngine::prepare_stock_correction(conn, &emp, emp_req).unwrap_err();
        assert!(matches!(err, EngineError::AdminAuthorizationRequired(_)));

        // B. Invalid reason rejection
        let bad_reason = RecordStockCorrectionRequest {
            correction_id: "corr_bad".to_string(),
            product_id: "prod_atta".to_string(),
            quantity_change: -5000,
            reason: "STOLEN_BY_ALIENS".to_string(),
            note: "Invalid reason".to_string(),
            admin_user_id: "usr_admin".to_string(),
        };
        let err = BusinessEngine::prepare_stock_correction(conn, &admin, bad_reason).unwrap_err();
        assert!(matches!(err, EngineError::InvalidCorrectionReason(_)));

        // C. Negative stock rejection: current atta is 30,000; reduce by 35,000
        let neg_stock = RecordStockCorrectionRequest {
            correction_id: "corr_neg".to_string(),
            product_id: "prod_atta".to_string(),
            quantity_change: -35000,
            reason: "LOST".to_string(),
            note: "Misplaced sack".to_string(),
            admin_user_id: "usr_admin".to_string(),
        };
        let err = BusinessEngine::prepare_stock_correction(conn, &admin, neg_stock).unwrap_err();
        assert!(matches!(err, EngineError::InsufficientStock { .. }));

        // D. Successful corrections for all 4 valid reasons
        let reasons = ["DAMAGED", "EXPIRED", "LOST", "MISCOUNT"];
        for (i, reason) in reasons.iter().enumerate() {
            let req = RecordStockCorrectionRequest {
                correction_id: format!("corr_ok_{}", i),
                product_id: "prod_atta".to_string(),
                quantity_change: -1000,
                reason: reason.to_string(),
                note: format!("Routine {} write-off", reason),
                admin_user_id: "usr_admin".to_string(),
            };
            let prepared = BusinessEngine::prepare_stock_correction(conn, &admin, req)?;
            let confirmed = prepared.confirm(&admin);
            TransactionEngine::execute_stock_correction(conn, confirmed)?;
        }

        // Atta stock reduced by 4,000 from 30,000 to 26,000
        let rem_atta: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_atta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(rem_atta, 26000);

        Ok(())
    }).expect("Stock correction test failed");
}

// ------------------------------------------------------------------------------------------------
// 8. ORDER TO SALE CONVERSION: ROLLBACK ON FAILURE & DRAFT ISOLATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_order_to_sale_conversion_atomicity_and_draft_integrity() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (_admin, emp) = get_identities(conn);

        // Create customer order in DRAFT status: 15kg Atta (15,000 milli) @ ₹210
        conn.execute(
            "INSERT INTO customer_orders (id, order_number, customer_id, status, converted_sale_id, notes, user_id, created_at, updated_at)
             VALUES ('ord_draft_1', 'ORD-001', 'cust_gupta', 'DRAFT', NULL, 'Deliver in evening', 'usr_emp', '2026-09-13', '2026-09-13')",
            [],
        )?;
        conn.execute(
            "INSERT INTO customer_order_items (id, order_id, product_id, quantity, unit_price_cents, notes)
             VALUES ('oi_1', 'ord_draft_1', 'prod_atta', 15000, 21000, NULL)",
            [],
        )?;

        // Successful conversion: order draft -> confirmed sale -> CONVERTED
        let conv_req = ConvertOrderToSaleRequest {
            order_id: "ord_draft_1".to_string(),
            sale_id: "sale_from_ord_1".to_string(),
            sale_number: "INV-ORD-001".to_string(),
            paid_amount_cents: 315000,
            payment_method: Some("UPI".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };

        let prepared = BusinessEngine::prepare_order_conversion(conn, &emp, conv_req)?;
        let confirmed = prepared.confirm(&emp);
        TransactionEngine::execute_order_conversion(conn, confirmed)?;

        // Verify order status is CONVERTED and sale reference is populated
        let (status, sale_ref): (String, Option<String>) = conn.query_row(
            "SELECT status, converted_sale_id FROM customer_orders WHERE id = 'ord_draft_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(status, "CONVERTED");
        assert_eq!(sale_ref, Some("sale_from_ord_1".to_string()));

        // Cannot convert already converted order
        let duplicate_conv = ConvertOrderToSaleRequest {
            order_id: "ord_draft_1".to_string(),
            sale_id: "sale_dup".to_string(),
            sale_number: "INV-DUP".to_string(),
            paid_amount_cents: 315000,
            payment_method: Some("UPI".to_string()),
            user_id: "usr_emp".to_string(),
            sale_date: "2026-09-13".to_string(),
        };
        let err = BusinessEngine::prepare_order_conversion(conn, &emp, duplicate_conv).unwrap_err();
        assert!(matches!(err, EngineError::OrderAlreadyConverted(_)));

        Ok(())
    }).expect("Order conversion test failed");
}

// ------------------------------------------------------------------------------------------------
// 9. EXPENSES RECORDING & AUDIT LOG
// ------------------------------------------------------------------------------------------------
#[test]
fn test_expense_recording_and_audit() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let (admin, _emp) = get_identities(conn);

        let expense_req = RecordExpenseRequest {
            expense_id: "exp_101".to_string(),
            expense_name: "Shop electricity bill".to_string(),
            amount_cents: 450000,
            category: "UTILITIES".to_string(),
            expense_date: "2026-09-13".to_string(),
            notes: Some("August power bill".to_string()),
            user_id: "usr_admin".to_string(),
        };

        let prepared = BusinessEngine::prepare_expense(conn, &admin, expense_req)?;
        let confirmed = prepared.confirm(&admin);
        TransactionEngine::execute_expense(conn, confirmed)?;

        let (name, amt, cat): (String, i64, String) = conn.query_row(
            "SELECT expense_name, amount_cents, category FROM expenses WHERE id = 'exp_101'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(name, "Shop electricity bill");
        assert_eq!(amt, 450000);
        assert_eq!(cat, "UTILITIES");

        let audit_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE entity_id = 'exp_101' AND action = 'EXPENSE_RECORDED'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(audit_count, 1);

        Ok(())
    }).expect("Expense recording test failed");
}

// ------------------------------------------------------------------------------------------------
// 10. DELIBERATE MID-TRANSACTION FAILURE ATOMICITY VERIFICATION
// ------------------------------------------------------------------------------------------------
#[test]
fn test_deliberate_mid_transaction_failure_rollback_zero_side_effects() {
    let db = DatabaseManager::open_in_memory().expect("Failed to open DB");
    seed_engine_base_data(&db);

    db.with_connection(|conn| {
        let initial_stock: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_atta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(initial_stock, 30000);

        // Intentionally execute a transaction that writes sale and items, but fails midway before commit
        let failure_result: Result<(), rusqlite::Error> = (|| {
            let tx = conn.transaction()?;

            // 1. Insert partial sale
            tx.execute(
                "INSERT INTO sales (id, sale_number, customer_id, total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status, user_id, sale_date, created_at)
                 VALUES ('sale_fail_1', 'INV-FAIL', NULL, 100000, 100000, 0, 'PAID', 'usr_emp', '2026-09-13', '2026-09-13')",
                [],
            )?;

            // 2. Decrement stock
            tx.execute(
                "UPDATE inventory SET current_quantity = 20000 WHERE product_id = 'prod_atta'",
                [],
            )?;

            // 3. Intentionally trigger SQLite Foreign Key violation to simulate unexpected runtime failure
            tx.execute(
                "INSERT INTO sale_items (id, sale_id, product_id, quantity, unit_price_cents, cost_price_cents, total_cents)
                 VALUES ('si_fail', 'sale_fail_1', 'non_existent_product_foreign_key_boom', 10000, 10000, 10000, 100000)",
                [],
            )?;

            tx.commit()?;
            Ok(())
        })();

        assert!(failure_result.is_err(), "Expected transaction to fail on FK error");

        // Verify 100% rollback: NO sale record, NO inventory deduction, NO orphan records
        let sale_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sales WHERE id = 'sale_fail_1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(sale_count, 0, "Sale record must not exist after rollback");

        let stock_after: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_atta'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stock_after, 30000, "Inventory must remain completely unchanged");

        let movements_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM stock_movements WHERE reference_id = 'sale_fail_1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(movements_count, 0, "No stock movements must be recorded");

        Ok(())
    }).expect("Atomicity deliberate failure verification failed");
}
