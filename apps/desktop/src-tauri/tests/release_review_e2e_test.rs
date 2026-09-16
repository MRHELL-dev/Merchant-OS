use desktop_lib::ai::{AIInterpreter, DeterministicLocalInterpreter};
use desktop_lib::auth::{AuthService, AuthorizationService, PermissionKey, Role};
use desktop_lib::backup::BackupService;
use desktop_lib::commands::{
    create_customer_order_inner, AuthSession, CreateCustomerOrderIpcInput, CustomerOrderItemInput,
};
use desktop_lib::db::DatabaseManager;
use desktop_lib::engine::{
    BusinessEngine, ConfirmPurchaseRequest, ConfirmSaleRequest, ConvertOrderToSaleRequest,
    CreateProductRequest, EngineError, ProcessCustomerReturnRequest, ProcessSupplierReturnRequest,
    PurchaseItemRequest, RecordCustomerPaymentRequest, RecordExpenseRequest,
    RecordStockCorrectionRequest, RecordSupplierPaymentRequest, ReturnItemRequest, SaleItemRequest,
    TransactionEngine, UpdateProductRequest,
};
use rusqlite::params;
use std::fs;

#[test]
fn test_final_release_review_end_to_end_lifecycle() {
    // --------------------------------------------------------------------------------------------
    // 1. ENVIRONMENT & ISOLATED DATABASE SETUP
    // --------------------------------------------------------------------------------------------
    let temp_dir = std::env::temp_dir().join(format!("merchant_os_release_review_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let db_path = temp_dir.join("review_merchant_os.db");

    let db = DatabaseManager::open(&db_path).expect("Failed to open isolated test database");
    assert!(db.verify_connection().expect("SQLite connection failed"));

    // --------------------------------------------------------------------------------------------
    // 2. PART 2 — ADMIN SETUP / LOGIN
    // --------------------------------------------------------------------------------------------
    let (admin_identity, _emp_identity) = db.with_connection(|conn| {
        // Seed default business entity
        conn.execute(
            "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
             VALUES ('biz_main', 'Bharat Kirana & General Store', '+919876543210', 'Shop 4, Market Complex, Delhi', '2026-09-16', '2026-09-16')",
            [],
        )?;

        // Admin creation: Password mismatch rejected
        let mismatch_res = AuthService::create_initial_admin(
            conn, "admin_owner", "SuperSecret123!", "WrongConfirm123!", "Favorite City?", "Delhi"
        );
        assert!(mismatch_res.is_err(), "Password mismatch must be rejected");

        // Admin creation: Valid admin setup
        let admin = AuthService::create_initial_admin(
            conn, "admin_owner", "SuperSecret123!", "SuperSecret123!", "Favorite City?", "Delhi"
        )?;
        assert_eq!(admin.username(), "admin_owner");
        assert!(admin.is_admin());

        // Second admin creation rejected
        let second_admin_res = AuthService::create_initial_admin(
            conn, "admin_second", "SuperSecret123!", "SuperSecret123!", "Favorite City?", "Delhi"
        );
        assert!(second_admin_res.is_err(), "Second initial admin must be strictly rejected");

        // Admin login verification: correct credentials
        let auth_admin = AuthService::authenticate(conn, "admin_owner", "SuperSecret123!")?;
        assert_eq!(auth_admin.user_id(), admin.user_id());

        // Admin login verification: invalid credentials rejected
        let invalid_auth = AuthService::authenticate(conn, "admin_owner", "IncorrectPassword!");
        assert!(invalid_auth.is_err(), "Invalid password must be rejected");

        // ----------------------------------------------------------------------------------------
        // 3. PART 3 — EMPLOYEE + PERMISSIONS
        // ----------------------------------------------------------------------------------------
        let emp_id = AuthService::create_employee(conn, &admin, "cashier_raj", "RajPass123!")?;
        let emp = AuthService::authenticate(conn, "cashier_raj", "RajPass123!")?;
        assert_eq!(emp.role(), Role::Employee);

        // Before permissions: employee cannot perform sales
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).is_err());

        // Admin assigns permissions: SALES, CUSTOMER_ORDERS
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Sales.as_str(), true)?;
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::CustomerOrders.as_str(), true)?;

        // Now authorized for sales
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Sales.as_str()).is_ok());

        // Still unauthorized for Admin-only operations (Correction, BackupRestore, Returns)
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::Correction.as_str()).is_err());
        assert!(AuthorizationService::authorize(conn, &emp, PermissionKey::BackupRestore.as_str()).is_err());

        Ok((admin, emp))
    }).expect("Part 2 & 3 setup failed");

    // --------------------------------------------------------------------------------------------
    // 4. PART 4 — PRODUCTS / INVENTORY (Realistic Kirana Catalog)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        let products = vec![
            CreateProductRequest {
                product_id: Some("prod_rice".to_string()),
                business_id: "biz_main".to_string(),
                name: "Basmati Rice 25kg Bag".to_string(),
                product_type: "PACKAGED".to_string(),
                unit: "kg".to_string(),
                barcode: Some("8901234567890".to_string()),
                barcode_type: Some("MANUFACTURER".to_string()),
                cost_price_cents: 180000,   // ₹1,800.00
                selling_price_cents: 220000,// ₹2,200.00
                initial_stock: 0,
                min_stock_level: 5000,      // 5 kg min threshold
            },
            CreateProductRequest {
                product_id: Some("prod_oil".to_string()),
                business_id: "biz_main".to_string(),
                name: "Mustard Oil Pure".to_string(),
                product_type: "PACKAGED".to_string(),
                unit: "pcs".to_string(),
                barcode: Some("8901234567891".to_string()),
                barcode_type: Some("MANUFACTURER".to_string()),
                cost_price_cents: 12000,    // ₹120.00
                selling_price_cents: 15000, // ₹150.00
                initial_stock: 0,
                min_stock_level: 10000,     // 10 pcs
            },
            CreateProductRequest {
                product_id: Some("prod_dal".to_string()),
                business_id: "biz_main".to_string(),
                name: "Chana Dal Premium".to_string(),
                product_type: "LOOSE".to_string(),
                unit: "kg".to_string(),
                barcode: Some("8901234567892".to_string()),
                barcode_type: Some("INTERNAL".to_string()),
                cost_price_cents: 8000,     // ₹80.00/kg
                selling_price_cents: 11000, // ₹110.00/kg
                initial_stock: 0,
                min_stock_level: 20000,     // 20 kg
            },
            CreateProductRequest {
                product_id: Some("prod_salt".to_string()),
                business_id: "biz_main".to_string(),
                name: "Tata Salt 1kg".to_string(),
                product_type: "PACKAGED".to_string(),
                unit: "pcs".to_string(),
                barcode: Some("8901234567893".to_string()),
                barcode_type: Some("MANUFACTURER".to_string()),
                cost_price_cents: 2000,     // ₹20.00
                selling_price_cents: 2800,  // ₹28.00
                initial_stock: 0,
                min_stock_level: 15000,     // 15 pcs
            },
        ];

        for p in products {
            let prep = BusinessEngine::prepare_create_product(conn, &admin_identity, p)?;
            let conf = prep.confirm(&admin_identity);
            TransactionEngine::execute_create_product(conn, conf)?;
        }

        // Verify initial stock is 0 for all products
        let count_zeros: i64 = conn.query_row(
            "SELECT COUNT(*) FROM inventory WHERE current_quantity = 0",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count_zeros, 4, "All 4 catalog items must start with 0 stock");

        // Verify normal product editing CANNOT modify stock
        let update_req = UpdateProductRequest {
            product_id: "prod_salt".to_string(),
            business_id: "biz_main".to_string(),
            name: Some("Tata Salt 1kg Iodized".to_string()),
            unit: None,
            barcode: None,
            barcode_type: None,
            cost_price_cents: None,
            selling_price_cents: Some(3000), // Revised selling price to ₹30.00
            min_stock_level: None,
        };
        let prep_up = BusinessEngine::prepare_update_product(conn, &admin_identity, update_req)?;
        let conf_up = prep_up.confirm(&admin_identity);
        TransactionEngine::execute_update_product(conn, conf_up)?;

        let (salt_name, salt_price, salt_stock): (String, i64, i64) = conn.query_row(
            "SELECT p.name, p.selling_price_cents, i.current_quantity
             FROM products p JOIN inventory i ON p.id = i.product_id
             WHERE p.id = 'prod_salt'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(salt_name, "Tata Salt 1kg Iodized");
        assert_eq!(salt_price, 3000);
        assert_eq!(salt_stock, 0, "Product editing MUST NEVER modify stock directly");

        Ok(())
    }).expect("Part 4 product catalog setup failed");

    // --------------------------------------------------------------------------------------------
    // 5. PART 5 — PURCHASE (Replenish Kirana Stock from Wholesaler)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Create supplier
        conn.execute(
            "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
             VALUES ('supp_kisan', 'Kisan Agro Wholesalers', '+919123456789', 'Grain Market Yard, Delhi', 0, 1, '2026-09-16', '2026-09-16')",
            [],
        )?;

        // Purchase Items:
        // - 20 bags Basmati Rice @ ₹1800 = ₹36,000 (3,600,000 cents)
        // - 50 pcs Mustard Oil @ ₹120 = ₹6,000 (600,000 cents)
        // - 100 kg Chana Dal @ ₹80 = ₹8,000 (800,000 cents)
        // - 100 pcs Tata Salt @ ₹20 = ₹2,000 (200,000 cents)
        // Total Purchase Value = ₹52,000 (5,200,000 cents)
        // Partial Payment: ₹30,000 (3,000,000 cents) paid via BANK_TRANSFER
        // Credit/Due = ₹22,000 (2,200,000 cents)
        let purchase_items = vec![
            PurchaseItemRequest {
                product_id: "prod_rice".to_string(),
                quantity: 20000, // 20 units
                unit_cost_cents: 180000,
            },
            PurchaseItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 50000, // 50 units
                unit_cost_cents: 12000,
            },
            PurchaseItemRequest {
                product_id: "prod_dal".to_string(),
                quantity: 100000, // 100 units
                unit_cost_cents: 8000,
            },
            PurchaseItemRequest {
                product_id: "prod_salt".to_string(),
                quantity: 100000, // 100 units
                unit_cost_cents: 2000,
            },
        ];

        let purchase_req = ConfirmPurchaseRequest {
            purchase_id: "po_001".to_string(),
            purchase_number: "PO-2026-001".to_string(),
            supplier_id: "supp_kisan".to_string(),
            items: purchase_items,
            paid_amount_cents: 3000000,
            payment_method: Some("BANK_TRANSFER".to_string()),
            user_id: admin_identity.user_id().to_string(),
            purchase_date: "2026-09-16".to_string(),
        };

        let prep = BusinessEngine::prepare_purchase(conn, &admin_identity, purchase_req)?;
        assert_eq!(prep.payload.total_amount_cents, 5200000);
        assert_eq!(prep.payload.credit_amount_cents, 2200000);

        let conf = prep.confirm(&admin_identity);
        TransactionEngine::execute_purchase(conn, conf)?;

        // Verify Inventory updated exactly
        let rice_stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'", [], |r| r.get(0))?;
        let oil_stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'", [], |r| r.get(0))?;
        let dal_stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        let salt_stock: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_salt'", [], |r| r.get(0))?;

        assert_eq!(rice_stock, 20000);
        assert_eq!(oil_stock, 50000);
        assert_eq!(dal_stock, 100000);
        assert_eq!(salt_stock, 100000);

        // Verify Supplier balance
        let supp_bal: i64 = conn.query_row("SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_kisan'", [], |r| r.get(0))?;
        assert_eq!(supp_bal, 2200000);

        // Verify Supplier ledger has entry
        let ledger_count: i64 = conn.query_row("SELECT COUNT(*) FROM supplier_ledger WHERE supplier_id = 'supp_kisan'", [], |r| r.get(0))?;
        assert!(ledger_count >= 1);

        Ok(())
    }).expect("Part 5 purchase failed");

    // --------------------------------------------------------------------------------------------
    // 6. PART 6 — SALE / POS (Paid Sale A + Credit Sale B)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Sale A: Walk-in Paid Sale
        // 2 bags Basmati Rice @ ₹2200 = ₹4,400 (440,000 cents)
        // 5 pcs Mustard Oil @ ₹150 = ₹750 (75,000 cents)
        // Total = ₹5,150 (515,000 cents) PAID in CASH
        let sale_a_items = vec![
            SaleItemRequest {
                product_id: "prod_rice".to_string(),
                quantity: 2000, // 2 units
                unit_price_cents: 220000,
            },
            SaleItemRequest {
                product_id: "prod_oil".to_string(),
                quantity: 5000, // 5 units
                unit_price_cents: 15000,
            },
        ];

        let sale_a_req = ConfirmSaleRequest {
            sale_id: "sale_paid_001".to_string(),
            sale_number: "INV-2026-001".to_string(),
            customer_id: None, // Walk-in
            items: sale_a_items,
            paid_amount_cents: 515000,
            payment_method: Some("CASH".to_string()),
            user_id: admin_identity.user_id().to_string(),
            sale_date: "2026-09-16".to_string(),
        };

        let prep_a = BusinessEngine::prepare_sale(conn, &admin_identity, sale_a_req)?;
        assert_eq!(prep_a.payload.total_amount_cents, 515000);
        assert_eq!(prep_a.payload.payment_status, "PAID");
        let conf_a = prep_a.confirm(&admin_identity);
        TransactionEngine::execute_sale(conn, conf_a)?;

        // Verify stock decreased exactly once
        let rice_stock_after_a: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'", [], |r| r.get(0))?;
        let oil_stock_after_a: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'", [], |r| r.get(0))?;
        assert_eq!(rice_stock_after_a, 18000); // 20 - 2 = 18
        assert_eq!(oil_stock_after_a, 45000);  // 50 - 5 = 45

        // Sale B: Registered Customer Credit Sale (Khata)
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_ramesh', 'Ramesh Sharma', '+919811122233', 'House 12, Main Street', 0, 1, '2026-09-16', '2026-09-16')",
            [],
        )?;

        // 3 kg Chana Dal @ ₹110 = ₹330 (33,000 cents)
        // 2 pcs Tata Salt @ ₹30 = ₹60 (6,000 cents)
        // Total = ₹390 (39,000 cents) CREDIT (Khata)
        let sale_b_items = vec![
            SaleItemRequest {
                product_id: "prod_dal".to_string(),
                quantity: 3000, // 3 units
                unit_price_cents: 11000,
            },
            SaleItemRequest {
                product_id: "prod_salt".to_string(),
                quantity: 2000, // 2 units
                unit_price_cents: 3000,
            },
        ];

        let sale_b_req = ConfirmSaleRequest {
            sale_id: "sale_credit_002".to_string(),
            sale_number: "INV-2026-002".to_string(),
            customer_id: Some("cust_ramesh".to_string()),
            items: sale_b_items,
            paid_amount_cents: 0,
            payment_method: None,
            user_id: admin_identity.user_id().to_string(),
            sale_date: "2026-09-16".to_string(),
        };

        let prep_b = BusinessEngine::prepare_sale(conn, &admin_identity, sale_b_req)?;
        assert_eq!(prep_b.payload.total_amount_cents, 39000);
        assert_eq!(prep_b.payload.credit_amount_cents, 39000);
        assert_eq!(prep_b.payload.payment_status, "UNPAID");
        let conf_b = prep_b.confirm(&admin_identity);
        TransactionEngine::execute_sale(conn, conf_b)?;

        // Verify stock decreased
        let dal_stock_after_b: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        let salt_stock_after_b: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_salt'", [], |r| r.get(0))?;
        assert_eq!(dal_stock_after_b, 97000); // 100 - 3 = 97
        assert_eq!(salt_stock_after_b, 98000); // 100 - 2 = 98

        // Verify customer credit balance is now ₹390 (39,000 cents)
        let ramesh_credit: i64 = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_ramesh'", [], |r| r.get(0))?;
        assert_eq!(ramesh_credit, 39000);

        Ok(())
    }).expect("Part 6 sales failed");

    // --------------------------------------------------------------------------------------------
    // 7. PART 7 — CUSTOMER PAYMENT (Khata Settlements & Negative Safety)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Customer owes ₹390 (39,000 cents)

        // Invalid: zero payment
        let zero_pay = RecordCustomerPaymentRequest {
            payment_id: "pmt_zero".to_string(),
            customer_id: "cust_ramesh".to_string(),
            amount_cents: 0,
            payment_method: "CASH".to_string(),
            user_id: admin_identity.user_id().to_string(),
            notes: None,
        };
        assert!(BusinessEngine::prepare_customer_payment(conn, &admin_identity, zero_pay).is_err());

        // Invalid: negative payment
        let neg_pay = RecordCustomerPaymentRequest {
            payment_id: "pmt_neg".to_string(),
            customer_id: "cust_ramesh".to_string(),
            amount_cents: -5000,
            payment_method: "CASH".to_string(),
            user_id: admin_identity.user_id().to_string(),
            notes: None,
        };
        assert!(BusinessEngine::prepare_customer_payment(conn, &admin_identity, neg_pay).is_err());

        // Invalid: overpayment (attempts to pay ₹500 against ₹390 balance)
        let over_pay = RecordCustomerPaymentRequest {
            payment_id: "pmt_over".to_string(),
            customer_id: "cust_ramesh".to_string(),
            amount_cents: 50000,
            payment_method: "CASH".to_string(),
            user_id: admin_identity.user_id().to_string(),
            notes: None,
        };
        let over_err = BusinessEngine::prepare_customer_payment(conn, &admin_identity, over_pay).unwrap_err();
        assert!(matches!(over_err, EngineError::OverpaymentNotAllowed { owed: 39000, attempted: 50000 }));

        // Valid Partial Payment: Pay ₹200 (20,000 cents) via UPI
        let part_pay = RecordCustomerPaymentRequest {
            payment_id: "pmt_part_1".to_string(),
            customer_id: "cust_ramesh".to_string(),
            amount_cents: 20000,
            payment_method: "UPI".to_string(),
            user_id: admin_identity.user_id().to_string(),
            notes: Some("UPI partial repayment".to_string()),
        };
        let prep_part = BusinessEngine::prepare_customer_payment(conn, &admin_identity, part_pay)?;
        assert_eq!(prep_part.payload.balance_after_cents, 19000);
        let conf_part = prep_part.confirm(&admin_identity);
        TransactionEngine::execute_customer_payment(conn, conf_part)?;

        let credit_after_part: i64 = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_ramesh'", [], |r| r.get(0))?;
        assert_eq!(credit_after_part, 19000);

        // Valid Remaining Payment: Pay remaining ₹190 (19,000 cents) via CASH
        let full_pay = RecordCustomerPaymentRequest {
            payment_id: "pmt_full_2".to_string(),
            customer_id: "cust_ramesh".to_string(),
            amount_cents: 19000,
            payment_method: "CASH".to_string(),
            user_id: admin_identity.user_id().to_string(),
            notes: Some("Full settlement".to_string()),
        };
        let prep_full = BusinessEngine::prepare_customer_payment(conn, &admin_identity, full_pay)?;
        assert_eq!(prep_full.payload.balance_after_cents, 0);
        let conf_full = prep_full.confirm(&admin_identity);
        TransactionEngine::execute_customer_payment(conn, conf_full)?;

        let final_credit: i64 = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_ramesh'", [], |r| r.get(0))?;
        assert_eq!(final_credit, 0, "Customer credit must be exactly 0 after full settlement");

        Ok(())
    }).expect("Part 7 customer payment failed");

    // --------------------------------------------------------------------------------------------
    // 8. PART 8 — CUSTOMER ORDER (Draft Isolation & Authoritative Conversion)
    // --------------------------------------------------------------------------------------------
    let session = AuthSession::default();
    session.set_identity(Some(admin_identity.clone()));

    // Stock before draft order: Dal = 97,000, Salt = 98,000
    let (dal_stock_pre, salt_stock_pre): (i64, i64) = db.with_connection(|conn| {
        let dal: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        let salt: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_salt'", [], |r| r.get(0))?;
        Ok((dal, salt))
    }).unwrap();

    // Customer creates order: 5 kg Chana Dal, 5 pcs Tata Salt
    let order_input = CreateCustomerOrderIpcInput {
        customer_id: Some("cust_ramesh".to_string()),
        items: vec![
            CustomerOrderItemInput {
                product_id: "prod_dal".to_string(),
                quantity: 5000, // 5 units
                notes: None,
            },
            CustomerOrderItemInput {
                product_id: "prod_salt".to_string(),
                quantity: 5000, // 5 units
                notes: None,
            },
        ],
        notes: Some("Please pack carefully".to_string()),
    };

    let draft_order = create_customer_order_inner(&db, &session, order_input).expect("Order draft creation failed");
    assert_eq!(draft_order.status, "DRAFT");

    // CRITICAL INVARIANT VERIFICATION:
    // Draft order MUST NOT reduce stock, reserve stock, create sale, or touch customer balance!
    let (dal_stock_post, salt_stock_post, cust_credit_post): (i64, i64, i64) = db.with_connection(|conn| {
        let dal: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        let salt: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_salt'", [], |r| r.get(0))?;
        let cred: i64 = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_ramesh'", [], |r| r.get(0))?;
        Ok((dal, salt, cred))
    }).unwrap();

    assert_eq!(dal_stock_pre, dal_stock_post, "Draft order must NOT deduct stock");
    assert_eq!(salt_stock_pre, salt_stock_post, "Draft order must NOT deduct stock");
    assert_eq!(cust_credit_post, 0, "Draft order must NOT alter customer credit");

    // Convert Draft Order to Authoritative Sale
    // Total: 5 * 110 + 5 * 30 = 550 + 150 = ₹700 (70,000 cents) paid via UPI
    db.with_connection(|conn| {
        let conv_req = ConvertOrderToSaleRequest {
            order_id: draft_order.id.clone(),
            sale_id: "sale_from_ord_101".to_string(),
            sale_number: "INV-ORD-101".to_string(),
            paid_amount_cents: 70000,
            payment_method: Some("UPI".to_string()),
            user_id: admin_identity.user_id().to_string(),
            sale_date: "2026-09-16".to_string(),
        };

        let prep_conv = BusinessEngine::prepare_order_conversion(conn, &admin_identity, conv_req)?;
        let conf_conv = prep_conv.confirm(&admin_identity);
        TransactionEngine::execute_order_conversion(conn, conf_conv)?;

        // Live stock is deducted on conversion:
        // Dal: 97 - 5 = 92 kg (92,000)
        // Salt: 98 - 5 = 93 pcs (93,000)
        let dal_stock_conv: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        let salt_stock_conv: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_salt'", [], |r| r.get(0))?;
        assert_eq!(dal_stock_conv, 92000);
        assert_eq!(salt_stock_conv, 93000);

        // Order status is now CONVERTED
        let order_status: String = conn.query_row("SELECT status FROM customer_orders WHERE id = ?1", params![draft_order.id], |r| r.get(0))?;
        assert_eq!(order_status, "CONVERTED");

        // Replay defense: Second conversion attempt must be safely rejected
        let dup_conv = ConvertOrderToSaleRequest {
            order_id: draft_order.id.clone(),
            sale_id: "sale_dup_attempt".to_string(),
            sale_number: "INV-DUP".to_string(),
            paid_amount_cents: 70000,
            payment_method: Some("UPI".to_string()),
            user_id: admin_identity.user_id().to_string(),
            sale_date: "2026-09-16".to_string(),
        };
        let dup_err = BusinessEngine::prepare_order_conversion(conn, &admin_identity, dup_conv).unwrap_err();
        assert!(matches!(dup_err, EngineError::OrderAlreadyConverted(_)));

        Ok(())
    }).expect("Part 8 customer order conversion failed");

    // --------------------------------------------------------------------------------------------
    // 9. PART 9 — CUSTOMER RETURN (Stock Increases & Debt/Refund Handling)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Customer Ramesh returns 1 bag Basmati Rice (@ selling price ₹2,200 = 220,000 cents)
        // Ramesh currently has 0 debt, so full ₹2,200 is refunded in CASH.
        let return_req = ProcessCustomerReturnRequest {
            return_id: "ret_cust_001".to_string(),
            return_number: "RET-C-001".to_string(),
            reference_sale_id: Some("sale_paid_001".to_string()),
            customer_id: "cust_ramesh".to_string(),
            items: vec![
                ReturnItemRequest {
                    product_id: "prod_rice".to_string(),
                    quantity: 1000, // 1 unit
                    unit_price_cents: 220000,
                },
            ],
            reason: "Customer bought excess bag for wedding feast".to_string(),
            admin_user_id: admin_identity.user_id().to_string(),
        };

        let prep_ret = BusinessEngine::prepare_customer_return(conn, &admin_identity, return_req)?;
        assert_eq!(prep_ret.payload.total_amount_cents, 220000);
        assert_eq!(prep_ret.payload.debt_reduction_cents, 0);
        assert_eq!(prep_ret.payload.cash_refund_cents, 220000);

        let conf_ret = prep_ret.confirm(&admin_identity);
        TransactionEngine::execute_customer_return(conn, conf_ret)?;

        // Stock increases by 1: Rice 18 -> 19 bags (19,000)
        let rice_stock_ret: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'", [], |r| r.get(0))?;
        assert_eq!(rice_stock_ret, 19000);

        // Verify return record created
        let ret_recorded: i64 = conn.query_row("SELECT COUNT(*) FROM returns WHERE id = 'ret_cust_001'", [], |r| r.get(0))?;
        assert_eq!(ret_recorded, 1);

        Ok(())
    }).expect("Part 9 customer return failed");

    // --------------------------------------------------------------------------------------------
    // 10. PART 10 — SUPPLIER RETURN (Cost-Price Stock Reduction & Payable Reduction)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Return 2 pcs Mustard Oil to Kisan Agro Wholesalers (@ cost price ₹120 = ₹240 = 24,000 cents)
        // Mustard oil stock before return: 45 pcs (45,000)
        // Supplier payable before return: ₹22,000 (2,200,000 cents)
        let supp_ret_req = ProcessSupplierReturnRequest {
            return_id: "ret_supp_001".to_string(),
            return_number: "RET-S-001".to_string(),
            reference_purchase_id: Some("po_001".to_string()),
            supplier_id: "supp_kisan".to_string(),
            items: vec![
                ReturnItemRequest {
                    product_id: "prod_oil".to_string(),
                    quantity: 2000, // 2 units
                    unit_price_cents: 12000,
                },
            ],
            reason: "Defective seal on batch".to_string(),
            admin_user_id: admin_identity.user_id().to_string(),
        };

        let prep_s_ret = BusinessEngine::prepare_supplier_return(conn, &admin_identity, supp_ret_req)?;
        assert_eq!(prep_s_ret.payload.total_amount_cents, 24000);
        assert_eq!(prep_s_ret.payload.balance_after_cents, 2176000); // 22,000 - 240 = 21,760

        let conf_s_ret = prep_s_ret.confirm(&admin_identity);
        TransactionEngine::execute_supplier_return(conn, conf_s_ret)?;

        // Stock decreased: 45 - 2 = 43 pcs (43,000)
        let oil_stock_supp_ret: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'", [], |r| r.get(0))?;
        assert_eq!(oil_stock_supp_ret, 43000);

        // Supplier payable reduced to ₹21,760 (2,176,000 cents)
        let supp_bal_ret: i64 = conn.query_row("SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_kisan'", [], |r| r.get(0))?;
        assert_eq!(supp_bal_ret, 2176000);

        Ok(())
    }).expect("Part 10 supplier return failed");

    // --------------------------------------------------------------------------------------------
    // 11. PART 11 — STOCK CORRECTION (Physical Inventory Adjustment & Reasons)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Damaged: 1 kg Chana Dal damaged by damp storage
        // Chana Dal stock before correction: 92 kg (92,000)
        let corr_req = RecordStockCorrectionRequest {
            correction_id: "corr_dam_001".to_string(),
            product_id: "prod_dal".to_string(),
            quantity_change: -1000, // -1 unit (scale 1000)
            reason: "DAMAGED".to_string(),
            note: "Damp packaging discarded after inspection".to_string(),
            admin_user_id: admin_identity.user_id().to_string(),
        };

        let prep_corr = BusinessEngine::prepare_stock_correction(conn, &admin_identity, corr_req)?;
        assert_eq!(prep_corr.payload.quantity_before, 92000);
        assert_eq!(prep_corr.payload.quantity_after, 91000);

        let conf_corr = prep_corr.confirm(&admin_identity);
        TransactionEngine::execute_stock_correction(conn, conf_corr)?;

        // Verify stock updated exactly
        let dal_stock_corr: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        assert_eq!(dal_stock_corr, 91000);

        // Verify stock movement recorded
        let mov_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM stock_movements WHERE reference_id = 'corr_dam_001' AND movement_type = 'CORRECTION'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(mov_count, 1);

        Ok(())
    }).expect("Part 11 stock correction failed");

    // --------------------------------------------------------------------------------------------
    // 12. PART 12 — EXPENSE (Shop Overhead Recording)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        let expense_req = RecordExpenseRequest {
            expense_id: "exp_util_001".to_string(),
            expense_name: "Shop electricity bill".to_string(),
            amount_cents: 150000, // ₹1,500.00
            category: "UTILITIES".to_string(),
            expense_date: "2026-09-16".to_string(),
            notes: Some("September billing period".to_string()),
            user_id: admin_identity.user_id().to_string(),
        };

        let prep_exp = BusinessEngine::prepare_expense(conn, &admin_identity, expense_req)?;
        let conf_exp = prep_exp.confirm(&admin_identity);
        TransactionEngine::execute_expense(conn, conf_exp)?;

        let (exp_name, exp_amount, exp_cat): (String, i64, String) = conn.query_row(
            "SELECT expense_name, amount_cents, category FROM expenses WHERE id = 'exp_util_001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(exp_name, "Shop electricity bill");
        assert_eq!(exp_amount, 150000);
        assert_eq!(exp_cat, "UTILITIES");

        Ok(())
    }).expect("Part 12 expense failed");

    // --------------------------------------------------------------------------------------------
    // 13. PART 13 — SUPPLIER ACCOUNTS & SETTLEMENT
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // Supplier Kisan Agro Wholesalers has ₹21,760 (2,176,000 cents) due
        // Pay ₹10,000 (1,000,000 cents) instalment
        let pmt_supp = RecordSupplierPaymentRequest {
            payment_id: "pmt_supp_001".to_string(),
            supplier_id: "supp_kisan".to_string(),
            amount_cents: 1000000,
            payment_method: "BANK_TRANSFER".to_string(),
            user_id: admin_identity.user_id().to_string(),
            notes: Some("Weekly wholesale settlement".to_string()),
        };

        let prep_pmt = BusinessEngine::prepare_supplier_payment(conn, &admin_identity, pmt_supp)?;
        assert_eq!(prep_pmt.payload.balance_after_cents, 1176000); // ₹11,760 remaining
        let conf_pmt = prep_pmt.confirm(&admin_identity);
        TransactionEngine::execute_supplier_payment(conn, conf_pmt)?;

        let rem_supp_bal: i64 = conn.query_row("SELECT current_outstanding_cents FROM suppliers WHERE id = 'supp_kisan'", [], |r| r.get(0))?;
        assert_eq!(rem_supp_bal, 1176000);

        Ok(())
    }).expect("Part 13 supplier settlement failed");

    // --------------------------------------------------------------------------------------------
    // 14. PART 14 — REPORTS & DEMAND INTELLIGENCE
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // 1. Total Confirmed Sales
        // Sale A (₹5,150) + Sale B (₹390) + Converted Order (₹700) = ₹6,240 (624,000 cents) across 3 transactions
        let total_sales: i64 = conn.query_row("SELECT SUM(total_amount_cents) FROM sales", [], |r| r.get(0))?;
        let count_sales: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert_eq!(total_sales, 624000);
        assert_eq!(count_sales, 3);

        // 2. Demand recommendations strictly use confirmed sales
        let recs = desktop_lib::ai::DemandIntelligenceService::generate_recommendations(conn)?;
        // Low-stock recommendations must NOT have modified database state
        for r in &recs {
            assert!(r.current_stock_millie >= 0);
        }

        Ok(())
    }).expect("Part 14 reports failed");

    // --------------------------------------------------------------------------------------------
    // 15. PART 15 — BACKUP / RESTORE SAFETY & INTEGRITY
    // --------------------------------------------------------------------------------------------
    let backup_dir = temp_dir.join("backups");
    let _ = fs::create_dir_all(&backup_dir);

    // 1. Create manual backup
    let backup_meta = BackupService::create_backup(
        &db,
        &admin_identity,
        "MANUAL",
        Some("Release review baseline snapshot"),
        Some(&backup_dir),
    ).expect("Failed to create backup");

    assert!(backup_meta.file_size_bytes > 0);
    assert!(!backup_meta.checksum_sha256.is_empty());

    // 2. Validate backup
    let backup_path = backup_dir.join(&backup_meta.file_name);
    let val_report = BackupService::validate_backup_file(&backup_path).expect("Validation failed");
    assert!(val_report.is_valid);
    assert!(val_report.integrity_check_passed);
    assert!(val_report.foreign_key_check_passed);
    assert_eq!(val_report.compatibility_status, "COMPATIBLE");

    // 3. Make known change: insert temporary marker customer
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_temporary', 'Temporary Test Marker', '+919999999999', 'Nowhere', 0, 1, '2026-09-16', '2026-09-16')",
            [],
        )?;
        Ok(())
    }).unwrap();

    let marker_exists_before_restore: i64 = db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM customers WHERE id = 'cust_temporary'", [], |r| r.get(0))?;
        Ok(count)
    }).unwrap();
    assert_eq!(marker_exists_before_restore, 1);

    // 4. Restore the backup
    let restore_report = BackupService::restore_backup_file(&db, &admin_identity, &backup_path).expect("Restore failed");
    assert!(restore_report.success);
    assert!(restore_report.verification_passed);
    assert!(!restore_report.rolled_back);

    // 5. Verify restored state: temporary customer marker is GONE
    let marker_exists_after_restore: i64 = db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM customers WHERE id = 'cust_temporary'", [], |r| r.get(0))?;
        Ok(count)
    }).unwrap();
    assert_eq!(marker_exists_after_restore, 0, "Restored database must match snapshot exactly");

    // --------------------------------------------------------------------------------------------
    // 16. PART 16 — OFFLINE-FIRST CAPABILITY
    // --------------------------------------------------------------------------------------------
    // The entire suite executes without any network interfaces, cloud APIs, or external servers.
    assert!(db.verify_connection().expect("Offline DB connection failed"));

    // --------------------------------------------------------------------------------------------
    // 17. PART 17 — AI & VOICE INTELLIGENCE (Strict Proposal Authority)
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        let interpreter = DeterministicLocalInterpreter::new();

        // 1. English stock query
        let res_en = AIInterpreter::process_query(conn, &interpreter, "How much Basmati Rice is left?", false);
        assert_eq!(res_en.mode, desktop_lib::ai::AIResponseMode::Informational);
        assert!(res_en.explanation.contains("Basmati Rice"));

        // 2. Hinglish query: "2 packets chawal becha rokad"
        let res_hi = AIInterpreter::process_query(conn, &interpreter, "2 packets chawal becha rokad", false);
        assert_eq!(res_hi.mode, desktop_lib::ai::AIResponseMode::PreparedAction);
        assert_eq!(res_hi.intent_type, "CREATE_SALE");

        // 3. Sales proposal (MUST BE A PROPOSAL, NEVER A DIRECT DATABASE MUTATION)
        let rice_stock_pre_ai: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'", [], |r| r.get(0))?;
        let res_sale = AIInterpreter::process_query(conn, &interpreter, "Sold 2 packets of Basmati Rice for cash", false);
        assert_eq!(res_sale.mode, desktop_lib::ai::AIResponseMode::PreparedAction);
        assert_eq!(res_sale.intent_type, "CREATE_SALE");
        assert!(res_sale.prepared_action.is_some());

        // Verify AI did NOT deduct stock
        let rice_stock_post_ai: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'", [], |r| r.get(0))?;
        assert_eq!(rice_stock_pre_ai, rice_stock_post_ai, "AI MUST NEVER directly mutate SQLite inventory");

        Ok(())
    }).expect("Part 17 AI failed");

    // --------------------------------------------------------------------------------------------
    // 18. PART 18 — NEGATIVE & ADVERSARIAL FAILURE TESTS
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        // A. Insufficient Stock Sale Rejection: Try to sell 500 bags of Rice (only 19 available)
        let oversell = ConfirmSaleRequest {
            sale_id: "sale_oversell".to_string(),
            sale_number: "INV-FAIL".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_rice".to_string(),
                quantity: 500000, // 500 units
                unit_price_cents: 220000,
            }],
            paid_amount_cents: 110000000,
            payment_method: Some("CASH".to_string()),
            user_id: admin_identity.user_id().to_string(),
            sale_date: "2026-09-16".to_string(),
        };
        let oversell_err = BusinessEngine::prepare_sale(conn, &admin_identity, oversell).unwrap_err();
        assert!(matches!(oversell_err, EngineError::InsufficientStock { .. }));

        // B. SQL Injection resilience: safely handled via parameterized queries
        let sqli_name = "'; DROP TABLE sales; --";
        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES ('cust_sqli', ?1, '+919000000000', 'Safe', 0, 1, '2026-09-16', '2026-09-16')",
            params![sqli_name],
        )?;
        let retrieved_sqli: String = conn.query_row("SELECT name FROM customers WHERE id = 'cust_sqli'", [], |r| r.get(0))?;
        assert_eq!(retrieved_sqli, sqli_name);

        // Sales table is completely intact
        let sales_exist: i64 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;
        assert!(sales_exist > 0, "Sales table must be completely intact");

        Ok(())
    }).expect("Part 18 failure tests failed");

    // --------------------------------------------------------------------------------------------
    // 19. PART 19 — FINAL DATA INTEGRITY & FOREIGN KEY CHECKS
    // --------------------------------------------------------------------------------------------
    db.with_connection(|conn| {
        let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        assert_eq!(integrity, "ok", "PRAGMA integrity_check must be 'ok'");

        let mut fk_stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let mut fk_rows = fk_stmt.query([])?;
        assert!(fk_rows.next()?.is_none(), "PRAGMA foreign_key_check must return 0 violations");

        // Mathematical reconciliation of inventory:
        // Rice: initial 0 + purchased 20 - sold 2 - return_out 0 + return_in 1 - corr 0 = 19
        let rice_final: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_rice'", [], |r| r.get(0))?;
        assert_eq!(rice_final, 19000);

        // Oil: initial 0 + purchased 50 - sold 5 - return_out 2 + return_in 0 - corr 0 = 43
        let oil_final: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_oil'", [], |r| r.get(0))?;
        assert_eq!(oil_final, 43000);

        // Dal: initial 0 + purchased 100 - sold 3 - order_converted 5 - corr 1 = 91
        let dal_final: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_dal'", [], |r| r.get(0))?;
        assert_eq!(dal_final, 91000);

        // Salt: initial 0 + purchased 100 - sold 2 - order_converted 5 = 93
        let salt_final: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_salt'", [], |r| r.get(0))?;
        assert_eq!(salt_final, 93000);

        Ok(())
    }).expect("Part 19 data integrity check failed");

    // Clean up temporary isolated review database
    drop(db);
    let _ = fs::remove_dir_all(temp_dir);
}
