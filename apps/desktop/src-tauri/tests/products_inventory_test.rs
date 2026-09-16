use desktop_lib::auth::{AuthService, AuthenticatedIdentity, PermissionKey};
use desktop_lib::db::DatabaseManager;
use desktop_lib::engine::{
    BusinessEngine, ConfirmSaleRequest, CreateProductBatchRequest, CreateProductRequest,
    EngineError, RemapBarcodeRequest, SaleItemRequest, TransactionEngine,
    UpdateProductRequest,
};
use rusqlite::params;

/// Helper to initialize in-memory SQLite database with V1 and V2 migrations,
/// and seed initial test businesses and administrator.
fn setup_test_db() -> (DatabaseManager, AuthenticatedIdentity) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");

    let admin = db
        .with_connection(|conn| {
            // Seed businesses
            conn.execute(
                "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
                 VALUES ('biz_1', 'Ramesh Kirana Store', '+919876543210', 'Main Bazaar, Delhi', '2026-09-13', '2026-09-13')
                 ON CONFLICT(id) DO NOTHING",
                [],
            )?;
            conn.execute(
                "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
                 VALUES ('biz_2', 'Sharma General Store', '+919876543211', 'Sector 14, Noida', '2026-09-13', '2026-09-13')
                 ON CONFLICT(id) DO NOTHING",
                [],
            )?;

            // Create initial admin
            let admin_id = AuthService::create_initial_admin(
                conn,
                "superadmin",
                "AdminSecret123!",
                "AdminSecret123!",
                "Security Question?",
                "Security Answer",
            )?;

            Ok(admin_id)
        })
        .expect("Failed to seed initial admin");

    (db, admin)
}

// ================================================================================================
// 1. PRODUCT CREATION TESTS (Scenarios 1 - 9, 44)
// ================================================================================================

#[test]
fn test_01_product_creation_packaged_and_loose_with_scale_and_zero_stock() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // 1. Valid Packaged Product
        let packaged_req = CreateProductRequest {
            product_id: Some("prod_soap".to_string()),
            business_id: "biz_1".to_string(),
            name: "Neem Bath Soap 100g".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: Some("8901234567890".to_string()),
            barcode_type: Some("MANUFACTURER".to_string()),
            cost_price_cents: 2500,
            selling_price_cents: 3500,
            initial_stock: 50000, // 50 units (scale: 1000)
            min_stock_level: 10000, // 10 units
        };

        let prep = BusinessEngine::prepare_create_product(conn, &admin, packaged_req)?;
        let conf = prep.confirm(&admin);
        let res = TransactionEngine::execute_create_product(conn, conf)?;

        assert_eq!(res.id, "prod_soap");
        assert_eq!(res.product_type, "PACKAGED");
        assert_eq!(res.unit, "packet");
        assert_eq!(res.cost_price_cents, 2500);
        assert_eq!(res.selling_price_cents, 3500);

        // Verify inventory row initialized with x1000 scale
        let inv_qty: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_soap'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(inv_qty, 50000);

        // Verify INITIAL_STOCK movement recorded
        let (mov_type, mov_ref, mov_qty): (String, String, i64) = conn.query_row(
            "SELECT movement_type, reference_type, quantity_change FROM stock_movements WHERE product_id = 'prod_soap'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(mov_type, "INITIAL_STOCK");
        assert_eq!(mov_ref, "PRODUCTS");
        assert_eq!(mov_qty, 50000);

        // 2. Valid Loose Product with Zero Initial Stock (Scenario 44)
        let loose_req = CreateProductRequest {
            product_id: Some("prod_sugar".to_string()),
            business_id: "biz_1".to_string(),
            name: "Loose White Sugar".to_string(),
            product_type: "LOOSE".to_string(),
            unit: "kg".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 3800,
            selling_price_cents: 4400,
            initial_stock: 0, // Zero initial stock
            min_stock_level: 25000, // 25.000 kg
        };

        let prep_loose = BusinessEngine::prepare_create_product(conn, &admin, loose_req)?;
        let conf_loose = prep_loose.confirm(&admin);
        let res_loose = TransactionEngine::execute_create_product(conn, conf_loose)?;

        assert_eq!(res_loose.id, "prod_sugar");
        assert_eq!(res_loose.product_type, "LOOSE");

        // Inventory exists with 0 quantity
        let loose_qty: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = 'prod_sugar'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(loose_qty, 0);

        // No fake purchases or unnecessary movements created for 0 stock
        let mov_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM stock_movements WHERE product_id = 'prod_sugar'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(mov_count, 0);

        let purch_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM purchases",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(purch_count, 0);

        Ok(())
    }).expect("Test 01 failed");
}

#[test]
fn test_02_product_creation_validation_rejects_invalid_inputs() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        let base_req = CreateProductRequest {
            product_id: None,
            business_id: "biz_1".to_string(),
            name: "Valid Product".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 1000,
            selling_price_cents: 1500,
            initial_stock: 5000,
            min_stock_level: 2000,
        };

        // 1. Empty name rejected
        let mut req = base_req.clone();
        req.name = "   ".to_string();
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::InvalidProductName(_)));

        // 2. Invalid product type rejected
        let mut req = base_req.clone();
        req.product_type = "DIGITAL_DOWNLOAD".to_string();
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::InvalidProductType(_)));

        // 3. Invalid empty unit rejected
        let mut req = base_req.clone();
        req.unit = "   ".to_string();
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::InvalidUnit(_)));

        // 4. Negative cost price rejected
        let mut req = base_req.clone();
        req.cost_price_cents = -500;
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::InvalidAmount { .. }));

        // 5. Negative selling price rejected
        let mut req = base_req.clone();
        req.selling_price_cents = -100;
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::InvalidAmount { .. }));

        // 6. Negative initial stock rejected
        let mut req = base_req.clone();
        req.initial_stock = -1000;
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::NegativeStock(-1000)));

        // 7. Negative minimum stock rejected
        let mut req = base_req.clone();
        req.min_stock_level = -500;
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::NegativeMinimumStock(-500)));

        // 8. Non-existent business rejected
        let mut req = base_req.clone();
        req.business_id = "biz_phantom_999".to_string();
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::EntityNotFound(_)));

        Ok(())
    }).expect("Test 02 failed");
}

// ================================================================================================
// 2. INVENTORY & LOW-STOCK DETECTION (Scenarios 10 - 15)
// ================================================================================================

#[test]
fn test_03_inventory_state_and_low_stock_detection() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // Product 1: Low stock (current <= min_stock)
        let req1 = CreateProductRequest {
            product_id: Some("prod_low".to_string()),
            business_id: "biz_1".to_string(),
            name: "Low Stock Tea".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 1000,
            selling_price_cents: 1500,
            initial_stock: 5000, // 5 units
            min_stock_level: 10000, // min 10 units
        };
        let p1 = BusinessEngine::prepare_create_product(conn, &admin, req1)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, p1)?;

        let status1 = BusinessEngine::get_product_stock_status(conn, &admin, "biz_1", "prod_low")?;
        assert_eq!(status1.stock_status, "LOW_STOCK");

        // Product 2: Normal stock (current > min_stock)
        let req2 = CreateProductRequest {
            product_id: Some("prod_normal".to_string()),
            business_id: "biz_1".to_string(),
            name: "Normal Stock Coffee".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 2000,
            selling_price_cents: 3000,
            initial_stock: 25000, // 25 units
            min_stock_level: 10000, // min 10 units
        };
        let p2 = BusinessEngine::prepare_create_product(conn, &admin, req2)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, p2)?;

        let status2 = BusinessEngine::get_product_stock_status(conn, &admin, "biz_1", "prod_normal")?;
        assert_eq!(status2.stock_status, "NORMAL");

        // List low stock products in biz_1 returns prod_low
        let low_list = BusinessEngine::list_low_stock_products(conn, &admin, "biz_1")?;
        assert_eq!(low_list.len(), 1);
        assert_eq!(low_list[0].product_id, "prod_low");

        Ok(())
    }).expect("Test 03 failed");
}

// ================================================================================================
// 3. PRODUCT EDITING & UNIT IMMUTABILITY (Scenarios 16 - 22, 43)
// ================================================================================================

#[test]
fn test_04_product_editing_and_strict_unit_immutability() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // Create product with initial stock 2.5 kg (2500 milli-units)
        let req = CreateProductRequest {
            product_id: Some("prod_grain".to_string()),
            business_id: "biz_1".to_string(),
            name: "Organic Wheat Grain".to_string(),
            product_type: "LOOSE".to_string(),
            unit: "kg".to_string(),
            barcode: Some("1122334455".to_string()),
            barcode_type: Some("INTERNAL".to_string()),
            cost_price_cents: 3000,
            selling_price_cents: 4000,
            initial_stock: 2500, // 2.500 kg
            min_stock_level: 1000,
        };
        let conf = BusinessEngine::prepare_create_product(conn, &admin, req)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, conf)?;

        // 1. Name, min stock, and prices can be safely updated
        let upd_req = UpdateProductRequest {
            product_id: "prod_grain".to_string(),
            business_id: "biz_1".to_string(),
            name: Some("Whole Wheat Grain 100% Organic".to_string()),
            unit: None,
            barcode: None,
            barcode_type: None,
            cost_price_cents: Some(3200),
            selling_price_cents: Some(4500),
            min_stock_level: Some(2000),
        };
        let conf_upd = BusinessEngine::prepare_update_product(conn, &admin, upd_req)?.confirm(&admin);
        TransactionEngine::execute_update_product(conn, conf_upd)?;

        let p_after = BusinessEngine::get_product(conn, &admin, "biz_1", "prod_grain")?;
        assert_eq!(p_after.name, "Whole Wheat Grain 100% Organic");
        assert_eq!(p_after.cost_price_cents, 3200);
        assert_eq!(p_after.selling_price_cents, 4500);
        assert_eq!(p_after.min_stock_level, 2000);

        // 2. Unit change AFTER stock history exists MUST BE REJECTED (Section 43)
        let unit_upd_req = UpdateProductRequest {
            product_id: "prod_grain".to_string(),
            business_id: "biz_1".to_string(),
            name: None,
            unit: Some("packet".to_string()), // Attempt to change kg -> packet
            barcode: None,
            barcode_type: None,
            cost_price_cents: None,
            selling_price_cents: None,
            min_stock_level: None,
        };
        let err = BusinessEngine::prepare_update_product(conn, &admin, unit_upd_req).unwrap_err();
        assert!(matches!(err, EngineError::UnitChangeForbidden(_)));

        // Verify stock remains exactly 2500 milli-units with unit 'kg'
        let inv = BusinessEngine::get_inventory(conn, &admin, "biz_1", "prod_grain")?;
        assert_eq!(inv.current_quantity, 2500);
        let p_check = BusinessEngine::get_product(conn, &admin, "biz_1", "prod_grain")?;
        assert_eq!(p_check.unit, "kg");

        // 3. Unit CAN be changed if stock is 0 and no movements exist
        let req_empty = CreateProductRequest {
            product_id: Some("prod_empty_history".to_string()),
            business_id: "biz_1".to_string(),
            name: "Unused Container".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "pcs".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 500,
            selling_price_cents: 1000,
            initial_stock: 0,
            min_stock_level: 0,
        };
        let c_empty = BusinessEngine::prepare_create_product(conn, &admin, req_empty)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, c_empty)?;

        let upd_empty_unit = UpdateProductRequest {
            product_id: "prod_empty_history".to_string(),
            business_id: "biz_1".to_string(),
            name: None,
            unit: Some("box".to_string()),
            barcode: None,
            barcode_type: None,
            cost_price_cents: None,
            selling_price_cents: None,
            min_stock_level: None,
        };
        let c_upd_empty = BusinessEngine::prepare_update_product(conn, &admin, upd_empty_unit)?.confirm(&admin);
        TransactionEngine::execute_update_product(conn, c_upd_empty)?;

        let p_updated = BusinessEngine::get_product(conn, &admin, "biz_1", "prod_empty_history")?;
        assert_eq!(p_updated.unit, "box");

        Ok(())
    }).expect("Test 04 failed");
}

// ================================================================================================
// 4. BARCODE ARCHITECTURE, LOOKUP, AND REMAPPING (Scenarios 23 - 31, 42)
// ================================================================================================

#[test]
fn test_05_barcode_persistent_resolution_duplicate_protection_and_remapping() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // 1. Create Product A with Barcode X in Business 1
        let req_a = CreateProductRequest {
            product_id: Some("prod_a".to_string()),
            business_id: "biz_1".to_string(),
            name: "Product Alpha".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: Some("BARCODE_X_100".to_string()),
            barcode_type: Some("MANUFACTURER".to_string()),
            cost_price_cents: 1000,
            selling_price_cents: 1500,
            initial_stock: 10000,
            min_stock_level: 2000,
        };
        let ca = BusinessEngine::prepare_create_product(conn, &admin, req_a)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, ca)?;

        // 2. Persistent resolution returns Product A
        let res = BusinessEngine::resolve_barcode(conn, &admin, "biz_1", "BARCODE_X_100")?;
        assert_eq!(res.product_id, "prod_a");
        assert_eq!(res.product_name, "Product Alpha");
        assert_eq!(res.barcode_type, "MANUFACTURER");

        // 3. Attempt to create Product B with same Barcode X in Business 1 fails
        let req_b = CreateProductRequest {
            product_id: Some("prod_b".to_string()),
            business_id: "biz_1".to_string(),
            name: "Product Beta".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: Some("BARCODE_X_100".to_string()),
            barcode_type: Some("MANUFACTURER".to_string()),
            cost_price_cents: 1200,
            selling_price_cents: 1800,
            initial_stock: 5000,
            min_stock_level: 1000,
        };
        let err_dup = BusinessEngine::prepare_create_product(conn, &admin, req_b).unwrap_err();
        assert!(matches!(err_dup, EngineError::DuplicateBarcodeMapping { .. }));

        // 4. Create Product C in Business 1 without barcode
        let req_c = CreateProductRequest {
            product_id: Some("prod_c".to_string()),
            business_id: "biz_1".to_string(),
            name: "Product Gamma".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 1100,
            selling_price_cents: 1600,
            initial_stock: 4000,
            min_stock_level: 1000,
        };
        let cc = BusinessEngine::prepare_create_product(conn, &admin, req_c)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, cc)?;

        // Ordinary update of Product C trying to set Barcode X fails (cannot silently steal barcode)
        let silent_steal = UpdateProductRequest {
            product_id: "prod_c".to_string(),
            business_id: "biz_1".to_string(),
            name: None,
            unit: None,
            barcode: Some(Some("BARCODE_X_100".to_string())),
            barcode_type: None,
            cost_price_cents: None,
            selling_price_cents: None,
            min_stock_level: None,
        };
        let err_steal = BusinessEngine::prepare_update_product(conn, &admin, silent_steal).unwrap_err();
        assert!(matches!(err_steal, EngineError::DuplicateBarcodeMapping { .. }));

        // 5. Explicit Admin Remapping of Barcode X from Product A -> Product C succeeds
        let remap_req = RemapBarcodeRequest {
            business_id: "biz_1".to_string(),
            barcode: "BARCODE_X_100".to_string(),
            new_product_id: "prod_c".to_string(),
        };
        let conf_remap = BusinessEngine::prepare_remap_barcode(conn, &admin, remap_req)?.confirm(&admin);
        TransactionEngine::execute_remap_barcode(conn, conf_remap)?;

        // Now resolving Barcode X returns Product C
        let res_after = BusinessEngine::resolve_barcode(conn, &admin, "biz_1", "BARCODE_X_100")?;
        assert_eq!(res_after.product_id, "prod_c");
        assert_eq!(res_after.product_name, "Product Gamma");

        // 6. Loose / Weighing-Scale Barcode Metadata Foundation (Section 25 & 31)
        let req_loose = CreateProductRequest {
            product_id: Some("prod_loose_apple".to_string()),
            business_id: "biz_1".to_string(),
            name: "Shimla Apples Loose".to_string(),
            product_type: "LOOSE".to_string(),
            unit: "kg".to_string(),
            barcode: Some("200045012503".to_string()),
            barcode_type: Some("WEIGHING_SCALE".to_string()),
            cost_price_cents: 8000,
            selling_price_cents: 12000,
            initial_stock: 100000,
            min_stock_level: 10000,
        };
        let cl = BusinessEngine::prepare_create_product(conn, &admin, req_loose)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, cl)?;

        let res_loose = BusinessEngine::resolve_barcode(conn, &admin, "biz_1", "200045012503")?;
        assert_eq!(res_loose.barcode_type, "WEIGHING_SCALE");
        assert!(res_loose.weighing_metadata.is_some());
        assert_eq!(res_loose.weighing_metadata.unwrap().raw_payload, "200045012503");

        Ok(())
    }).expect("Test 05 failed");
}

// ================================================================================================
// 5. BUSINESS ISOLATION ENFORCEMENT (Scenarios 32 - 36, 42)
// ================================================================================================

#[test]
fn test_06_business_isolation_across_products_inventory_and_barcodes() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // Create Product in Business 1
        let req_biz1 = CreateProductRequest {
            product_id: Some("prod_biz1_item".to_string()),
            business_id: "biz_1".to_string(),
            name: "Store 1 Specialty".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: Some("SHARED_BARCODE_999".to_string()),
            barcode_type: Some("INTERNAL".to_string()),
            cost_price_cents: 1000,
            selling_price_cents: 2000,
            initial_stock: 5000,
            min_stock_level: 1000,
        };
        let c1 = BusinessEngine::prepare_create_product(conn, &admin, req_biz1)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, c1)?;

        // 1. Business 2 attempting to read Product from Business 1 -> CrossBusinessAccessDenied
        let err_get = BusinessEngine::get_product(conn, &admin, "biz_2", "prod_biz1_item").unwrap_err();
        assert!(matches!(err_get, EngineError::CrossBusinessAccessDenied { .. }));

        // 2. Business 2 attempting to read Inventory from Business 1 -> CrossBusinessAccessDenied
        let err_inv = BusinessEngine::get_inventory(conn, &admin, "biz_2", "prod_biz1_item").unwrap_err();
        assert!(matches!(err_inv, EngineError::CrossBusinessAccessDenied { .. }));

        // 3. Business 2 attempting to resolve Barcode belonging to Business 1 -> CrossBusinessAccessDenied
        let err_bar = BusinessEngine::resolve_barcode(conn, &admin, "biz_2", "SHARED_BARCODE_999").unwrap_err();
        assert!(matches!(err_bar, EngineError::CrossBusinessAccessDenied { .. }));

        // 4. Business 2 attempting to update Product from Business 1 -> CrossBusinessAccessDenied
        let upd_cross = UpdateProductRequest {
            product_id: "prod_biz1_item".to_string(),
            business_id: "biz_2".to_string(), // Cross-business claim
            name: Some("Hijacked Name".to_string()),
            unit: None,
            barcode: None,
            barcode_type: None,
            cost_price_cents: None,
            selling_price_cents: None,
            min_stock_level: None,
        };
        let err_upd = BusinessEngine::prepare_update_product(conn, &admin, upd_cross).unwrap_err();
        assert!(matches!(err_upd, EngineError::CrossBusinessAccessDenied { .. }));

        // 5. Cross-business barcode remapping -> CrossBusinessAccessDenied
        // Create Product in Business 2
        let req_biz2 = CreateProductRequest {
            product_id: Some("prod_biz2_item".to_string()),
            business_id: "biz_2".to_string(),
            name: "Store 2 Product".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 1500,
            selling_price_cents: 2500,
            initial_stock: 5000,
            min_stock_level: 1000,
        };
        let c2 = BusinessEngine::prepare_create_product(conn, &admin, req_biz2)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, c2)?;

        // Remap barcode from Business 1 to Business 2 product rejected
        let cross_remap = RemapBarcodeRequest {
            business_id: "biz_1".to_string(),
            barcode: "SHARED_BARCODE_999".to_string(),
            new_product_id: "prod_biz2_item".to_string(), // Belongs to biz_2
        };
        let err_remap = BusinessEngine::prepare_remap_barcode(conn, &admin, cross_remap).unwrap_err();
        assert!(matches!(err_remap, EngineError::CrossBusinessAccessDenied { .. }));

        Ok(())
    }).expect("Test 06 failed");
}

// ================================================================================================
// 6. PERMISSIONS & ROLE ENFORCEMENT (Scenarios 37 - 41)
// ================================================================================================

#[test]
fn test_07_product_permissions_inventory_and_price_separation() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // Create employee
        let emp_id = AuthService::create_employee(conn, &admin, "cashier_inv", "CashierPass123!")?;
        let emp = AuthService::authenticate(conn, "cashier_inv", "CashierPass123!")?;

        let prod_req = CreateProductRequest {
            product_id: Some("prod_perm_test".to_string()),
            business_id: "biz_1".to_string(),
            name: "Permission Test Product".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 1000,
            selling_price_cents: 1500,
            initial_stock: 10000,
            min_stock_level: 2000,
        };

        // 1. Employee without INVENTORY permission fails
        let err_no_perm = BusinessEngine::prepare_create_product(conn, &emp, prod_req.clone()).unwrap_err();
        assert!(matches!(err_no_perm, EngineError::PermissionDenied { .. }));

        // 2. Grant employee INVENTORY permission -> product creation succeeds
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Inventory.as_str(), true)?;
        let c_emp = BusinessEngine::prepare_create_product(conn, &emp, prod_req)?.confirm(&emp);
        TransactionEngine::execute_create_product(conn, c_emp)?;

        // 3. Employee with INVENTORY can update name & min_stock
        let upd_meta = UpdateProductRequest {
            product_id: "prod_perm_test".to_string(),
            business_id: "biz_1".to_string(),
            name: Some("Permission Test Product Renamed".to_string()),
            unit: None,
            barcode: None,
            barcode_type: None,
            cost_price_cents: None,
            selling_price_cents: None,
            min_stock_level: Some(3000),
        };
        let c_meta = BusinessEngine::prepare_update_product(conn, &emp, upd_meta)?.confirm(&emp);
        TransactionEngine::execute_update_product(conn, c_meta)?;

        // 4. Employee with INVENTORY but WITHOUT PRICES attempts price change -> REJECTED
        let upd_price = UpdateProductRequest {
            product_id: "prod_perm_test".to_string(),
            business_id: "biz_1".to_string(),
            name: None,
            unit: None,
            barcode: None,
            barcode_type: None,
            cost_price_cents: Some(1200),
            selling_price_cents: Some(1800),
            min_stock_level: None,
        };
        let err_price = BusinessEngine::prepare_update_product(conn, &emp, upd_price.clone()).unwrap_err();
        assert!(matches!(err_price, EngineError::PermissionDenied { feature, .. } if feature == "PRICES"));

        // 5. Grant PRICES permission -> price update succeeds
        AuthService::set_employee_permission(conn, &admin, &emp_id, PermissionKey::Prices.as_str(), true)?;
        let c_price = BusinessEngine::prepare_update_product(conn, &emp, upd_price)?.confirm(&emp);
        TransactionEngine::execute_update_product(conn, c_price)?;

        let p_check = BusinessEngine::get_product(conn, &admin, "biz_1", "prod_perm_test")?;
        assert_eq!(p_check.cost_price_cents, 1200);
        assert_eq!(p_check.selling_price_cents, 1800);

        // 6. Employee attempts Barcode Remap (Admin Only) -> REJECTED
        let remap_emp = RemapBarcodeRequest {
            business_id: "biz_1".to_string(),
            barcode: "ANY_BARCODE".to_string(),
            new_product_id: "prod_perm_test".to_string(),
        };
        let err_remap = BusinessEngine::prepare_remap_barcode(conn, &emp, remap_emp).unwrap_err();
        assert!(matches!(err_remap, EngineError::AdminAuthorizationRequired(_)));

        Ok(())
    }).expect("Test 07 failed");
}

// ================================================================================================
// 7. ATOMICITY, CONFIRMATION BOUNDARY & SECURITY SPOOFING (Scenarios 40, 41, 45 - 50)
// ================================================================================================

#[test]
fn test_08_batch_creation_atomicity_and_all_or_nothing_rollback() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // Valid Item 1 and Item 2, but Item 3 has duplicate barcode with Item 1
        let batch_req = CreateProductBatchRequest {
            business_id: "biz_1".to_string(),
            products: vec![
                CreateProductRequest {
                    product_id: Some("prod_batch_1".to_string()),
                    business_id: "biz_1".to_string(),
                    name: "Batch Item 1".to_string(),
                    product_type: "PACKAGED".to_string(),
                    unit: "packet".to_string(),
                    barcode: Some("BATCH_BARCODE_COMMON".to_string()),
                    barcode_type: Some("MANUFACTURER".to_string()),
                    cost_price_cents: 100,
                    selling_price_cents: 200,
                    initial_stock: 1000,
                    min_stock_level: 500,
                },
                CreateProductRequest {
                    product_id: Some("prod_batch_2".to_string()),
                    business_id: "biz_1".to_string(),
                    name: "Batch Item 2".to_string(),
                    product_type: "PACKAGED".to_string(),
                    unit: "packet".to_string(),
                    barcode: Some("BATCH_BARCODE_2".to_string()),
                    barcode_type: Some("MANUFACTURER".to_string()),
                    cost_price_cents: 100,
                    selling_price_cents: 200,
                    initial_stock: 1000,
                    min_stock_level: 500,
                },
                CreateProductRequest {
                    product_id: Some("prod_batch_3".to_string()),
                    business_id: "biz_1".to_string(),
                    name: "Batch Item 3 (Collides)".to_string(),
                    product_type: "PACKAGED".to_string(),
                    unit: "packet".to_string(),
                    barcode: Some("BATCH_BARCODE_COMMON".to_string()), // Collision!
                    barcode_type: Some("MANUFACTURER".to_string()),
                    cost_price_cents: 100,
                    selling_price_cents: 200,
                    initial_stock: 1000,
                    min_stock_level: 500,
                },
            ],
        };

        // Batch validation catches duplicate barcode
        let err_batch = BusinessEngine::prepare_create_product_batch(conn, &admin, batch_req).unwrap_err();
        assert!(matches!(err_batch, EngineError::DuplicateBarcodeMapping { .. }));

        // Verify zero items exist in database
        let count_p: i64 = conn.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        assert_eq!(count_p, 0);

        // Now create valid batch of 2 products
        let valid_batch = CreateProductBatchRequest {
            business_id: "biz_1".to_string(),
            products: vec![
                CreateProductRequest {
                    product_id: Some("prod_b_ok_1".to_string()),
                    business_id: "biz_1".to_string(),
                    name: "Batch OK 1".to_string(),
                    product_type: "PACKAGED".to_string(),
                    unit: "packet".to_string(),
                    barcode: Some("BATCH_BARCODE_A".to_string()),
                    barcode_type: Some("MANUFACTURER".to_string()),
                    cost_price_cents: 1000,
                    selling_price_cents: 1500,
                    initial_stock: 2000,
                    min_stock_level: 500,
                },
                CreateProductRequest {
                    product_id: Some("prod_b_ok_2".to_string()),
                    business_id: "biz_1".to_string(),
                    name: "Batch OK 2".to_string(),
                    product_type: "LOOSE".to_string(),
                    unit: "kg".to_string(),
                    barcode: None,
                    barcode_type: None,
                    cost_price_cents: 2000,
                    selling_price_cents: 2500,
                    initial_stock: 0,
                    min_stock_level: 1000,
                },
            ],
        };

        let conf_batch = BusinessEngine::prepare_create_product_batch(conn, &admin, valid_batch)?.confirm(&admin);
        let results = TransactionEngine::execute_create_product_batch(conn, conf_batch)?;
        assert_eq!(results.len(), 2);

        // Both products and their inventories exist
        let inv1: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_b_ok_1'", [], |r| r.get(0))?;
        assert_eq!(inv1, 2000);
        let inv2: i64 = conn.query_row("SELECT current_quantity FROM inventory WHERE product_id = 'prod_b_ok_2'", [], |r| r.get(0))?;
        assert_eq!(inv2, 0);

        Ok(())
    }).expect("Test 08 failed");
}

#[test]
fn test_09_forced_transaction_failure_rolls_back_everything_cleanly() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // Pre-insert a barcode mapping directly to force a collision at execution time
        conn.execute(
            "INSERT INTO businesses (id, name, phone, address, created_at, updated_at)
             VALUES ('biz_dummy', 'Dummy Store', '00000', 'Dummy', '2026-09-13', '2026-09-13')
             ON CONFLICT(id) DO NOTHING",
            [],
        )?;
        conn.execute(
            "INSERT INTO products (id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
             VALUES ('prod_preexisting', 'biz_dummy', 'Pre-existing Dummy', 'PACKAGED', 'pcs', 10, 20, 0, 1, '2026-09-13', '2026-09-13')",
            [],
        )?;
        conn.execute(
            "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, notes, created_at)
             VALUES ('bm_dummy', 'COLLIDING_BARCODE_777', 'prod_preexisting', 'MANUFACTURER', NULL, '2026-09-13')",
            [],
        )?;

        // Attempt a product creation where barcode mapping will collide
        let req = CreateProductRequest {
            product_id: Some("prod_doomed".to_string()),
            business_id: "biz_1".to_string(),
            name: "Doomed Product".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: Some("COLLIDING_BARCODE_777".to_string()),
            barcode_type: Some("MANUFACTURER".to_string()),
            cost_price_cents: 1000,
            selling_price_cents: 2000,
            initial_stock: 5000,
            min_stock_level: 1000,
        };

        // Preparation catches collision
        let err = BusinessEngine::prepare_create_product(conn, &admin, req).unwrap_err();
        assert!(matches!(err, EngineError::DuplicateBarcodeMapping { .. }));

        // Verify product was not inserted
        let p_count: i64 = conn.query_row("SELECT COUNT(*) FROM products WHERE id = 'prod_doomed'", [], |r| r.get(0))?;
        assert_eq!(p_count, 0);

        // Verify inventory was not inserted
        let inv_count: i64 = conn.query_row("SELECT COUNT(*) FROM inventory WHERE product_id = 'prod_doomed'", [], |r| r.get(0))?;
        assert_eq!(inv_count, 0);

        // Verify stock movement was not inserted
        let mov_count: i64 = conn.query_row("SELECT COUNT(*) FROM stock_movements WHERE product_id = 'prod_doomed'", [], |r| r.get(0))?;
        assert_eq!(mov_count, 0);

        Ok(())
    }).expect("Test 09 failed");
}

#[test]
fn test_10_actor_spoofing_prevention_and_audit_integrity() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        let emp_id = AuthService::create_employee(conn, &admin, "victim_cashier", "CashierPass123!")?;

        let req = CreateProductRequest {
            product_id: Some("prod_spoof_test".to_string()),
            business_id: "biz_1".to_string(),
            name: "Audit Integrity Test Product".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 500,
            selling_price_cents: 1000,
            initial_stock: 1000,
            min_stock_level: 200,
        };

        // Admin confirms the creation
        let prep = BusinessEngine::prepare_create_product(conn, &admin, req)?;
        let conf = prep.confirm(&admin);
        TransactionEngine::execute_create_product(conn, conf)?;

        // Verify audit log recorded Admin's user_id, NOT emp_id
        let audit_user: String = conn.query_row(
            "SELECT user_id FROM audit_logs WHERE action = 'PRODUCT_CREATED' AND entity_id = 'prod_spoof_test'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(audit_user, admin.user_id(), "Authoritative confirmed actor must be logged");
        assert_ne!(audit_user, emp_id);

        Ok(())
    }).expect("Test 10 failed");
}

// ================================================================================================
// 8. HISTORICAL PRICE IMMUTABILITY REGRESSION TEST (Scenarios 51, 52)
// ================================================================================================

#[test]
fn test_11_historical_transaction_prices_remain_immutable_when_product_price_changes() {
    let (db, admin) = setup_test_db();

    db.with_connection(|conn| {
        // 1. Create product initially selling at ₹18.00 (1800 cents), cost ₹12.00 (1200 cents)
        let req = CreateProductRequest {
            product_id: Some("prod_historic".to_string()),
            business_id: "biz_1".to_string(),
            name: "Historic Pricing Biscuit".to_string(),
            product_type: "PACKAGED".to_string(),
            unit: "packet".to_string(),
            barcode: None,
            barcode_type: None,
            cost_price_cents: 1200,
            selling_price_cents: 1800,
            initial_stock: 50000,
            min_stock_level: 5000,
        };
        let conf = BusinessEngine::prepare_create_product(conn, &admin, req)?.confirm(&admin);
        TransactionEngine::execute_create_product(conn, conf)?;

        // 2. Record historical sale at ₹18.00
        let sale_id = "sale_hist_01";
        let sale_req = ConfirmSaleRequest {
            sale_id: sale_id.to_string(),
            sale_number: "INV-HIST-1".to_string(),
            customer_id: None,
            items: vec![SaleItemRequest {
                product_id: "prod_historic".to_string(),
                quantity: 2000, // 2 packets
                unit_price_cents: 1800, // ₹18 historical snapshot
            }],
            paid_amount_cents: 3600,
            payment_method: Some("CASH".to_string()),
            user_id: admin.user_id().to_string(),
            sale_date: "2026-09-12".to_string(),
        };
        let prep_sale = BusinessEngine::prepare_sale(conn, &admin, sale_req)?;
        let conf_sale = prep_sale.confirm(&admin);
        TransactionEngine::execute_sale(conn, conf_sale)?;

        // Verify historical sale records ₹18.00 (1800 cents)
        let hist_price: i64 = conn.query_row(
            "SELECT unit_price_cents FROM sale_items WHERE sale_id = ?1",
            params![sale_id],
            |r| r.get(0),
        )?;
        assert_eq!(hist_price, 1800);

        // 3. Today, update current reference product selling price to ₹25.00 (2500 cents) and cost to ₹16.00 (1600 cents)
        let upd = UpdateProductRequest {
            product_id: "prod_historic".to_string(),
            business_id: "biz_1".to_string(),
            name: None,
            unit: None,
            barcode: None,
            barcode_type: None,
            cost_price_cents: Some(1600),
            selling_price_cents: Some(2500),
            min_stock_level: None,
        };
        let conf_upd = BusinessEngine::prepare_update_product(conn, &admin, upd)?.confirm(&admin);
        TransactionEngine::execute_update_product(conn, conf_upd)?;

        // Verify current product price is updated to ₹25.00
        let p_curr = BusinessEngine::get_product(conn, &admin, "biz_1", "prod_historic")?;
        assert_eq!(p_curr.selling_price_cents, 2500);
        assert_eq!(p_curr.cost_price_cents, 1600);

        // 4. CRITICAL INVARIANT: Historical sale line item MUST STILL BE ₹18.00 (1800 cents)!
        let hist_price_after: i64 = conn.query_row(
            "SELECT unit_price_cents FROM sale_items WHERE sale_id = ?1",
            params![sale_id],
            |r| r.get(0),
        )?;
        assert_eq!(
            hist_price_after, 1800,
            "Historical sale price must remain strictly immutable when catalog price changes"
        );

        Ok(())
    }).expect("Test 11 failed");
}
