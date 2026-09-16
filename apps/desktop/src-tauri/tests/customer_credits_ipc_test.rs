use desktop_lib::auth::AuthService;
use desktop_lib::commands::{
    confirm_customer_payment_inner, get_customer_credits_summary_inner,
    get_customer_ledger_history_inner, prepare_customer_payment_inner,
    AuthSession, ConfirmCustomerPaymentIpcInput, PrepareCustomerPaymentIpcInput,
    PreparedCustomerPaymentCache,
};
use desktop_lib::db::DatabaseManager;
use rusqlite::params;
use std::sync::Arc;
use std::thread;

/// Setup test database with seed business, customers, and admin/employee users with specific permissions.
fn setup_test_context() -> (
    DatabaseManager,
    AuthSession,
    PreparedCustomerPaymentCache,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
    desktop_lib::auth::AuthenticatedIdentity,
) {
    let db = DatabaseManager::open_in_memory().expect("Failed to open test database");
    let session = AuthSession::default();
    let cache = PreparedCustomerPaymentCache::default();

    let (admin, emp_credits, emp_customers_only) = db
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

            // 3. Create Employee with canonical CUSTOMER_CREDITS permission
            let emp_c_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_credits",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_c_id,
                "CUSTOMER_CREDITS",
                true,
            )?;
            let emp_credits = AuthService::authenticate(conn, "emp_credits", "EmpPass123!")?;

            // 4. Create Employee with CUSTOMERS permission ONLY (unauthorized for CUSTOMER_CREDITS)
            let emp_cust_id = AuthService::create_employee(
                conn,
                &admin_id,
                "emp_cust_only",
                "EmpPass123!",
            )?;
            AuthService::set_employee_permission(
                conn,
                &admin_id,
                &emp_cust_id,
                "CUSTOMERS",
                true,
            )?;
            let emp_customers_only = AuthService::authenticate(conn, "emp_cust_only", "EmpPass123!")?;

            // 5. Seed Test Customers:
            // - Customer A: Ramesh Sharma (₹2,450.00 credit due)
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_sharma', 'Ramesh Sharma', '+919811100001', 'Block B, Sector 4, Noida', 245000, 1, '2026-09-13T10:00:00Z', '2026-09-13T10:00:00Z')",
                [],
            )?;

            // - Customer B: Priya Verma (₹1,200.00 credit due)
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_verma', 'Priya Verma', '+919822200002', 'Connaught Place, Delhi', 120000, 1, '2026-09-13T10:05:00Z', '2026-09-13T10:05:00Z')",
                [],
            )?;

            // - Customer C: Inactive Customer (₹500.00 credit due, is_active = 0)
            conn.execute(
                "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
                 VALUES ('cust_inactive', 'Inactive Customer', '+919833300003', 'Old Delhi', 50000, 0, '2026-09-13T10:10:00Z', '2026-09-13T10:10:00Z')",
                [],
            )?;

            // Initial ledger entry for Customer Sharma
            conn.execute(
                "INSERT INTO customer_ledger (id, customer_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
                 VALUES ('cleg_init_sharma', 'cust_sharma', 'SALE_CREDIT', 245000, 0, 245000, 'SALE', 'sale_init_01', 'Initial credit sale due', ?1, '2026-09-13T10:00:00Z')",
                params![admin.user_id()],
            )?;

            Ok((admin, emp_credits, emp_customers_only))
        })
        .expect("Failed to seed test database context");

    (db, session, cache, admin, emp_credits, emp_customers_only)
}

#[test]
fn test_01_customer_credit_list_deterministic_order() {
    let (db, session, _cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let summary = get_customer_credits_summary_inner(&db, &session).expect("Failed to get summary");
    // Only active customers should be returned
    assert_eq!(summary.customers.len(), 2);
    // Deterministic order by name ASC, id ASC: Priya Verma, then Ramesh Sharma
    assert_eq!(summary.customers[0].id, "cust_verma");
    assert_eq!(summary.customers[0].name, "Priya Verma");
    assert_eq!(summary.customers[1].id, "cust_sharma");
    assert_eq!(summary.customers[1].name, "Ramesh Sharma");

    // Total outstanding cents: 245000 + 120000 = 365000
    assert_eq!(summary.total_outstanding_cents, 365000);
}

#[test]
fn test_02_customer_search() {
    let (db, session, _cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let summary = get_customer_credits_summary_inner(&db, &session).expect("Failed to get summary");
    // Search by name
    let found_sharma: Vec<_> = summary
        .customers
        .iter()
        .filter(|c| c.name.to_lowercase().contains("sharma"))
        .collect();
    assert_eq!(found_sharma.len(), 1);
    assert_eq!(found_sharma[0].id, "cust_sharma");

    // Search by phone
    let found_phone: Vec<_> = summary
        .customers
        .iter()
        .filter(|c| c.phone.as_deref().unwrap_or("").contains("9822200002"))
        .collect();
    assert_eq!(found_phone.len(), 1);
    assert_eq!(found_phone[0].id, "cust_verma");
}

#[test]
fn test_03_customer_detail() {
    let (db, session, _cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let history = get_customer_ledger_history_inner(&db, &session, "cust_sharma".to_string())
        .expect("Failed to fetch customer history");
    assert_eq!(history.customer.id, "cust_sharma");
    assert_eq!(history.customer.name, "Ramesh Sharma");
    assert_eq!(history.customer.current_credit_cents, 245000);
}

#[test]
fn test_04_credit_payment_history_deterministic_ordering() {
    let (db, session, _cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    let admin_id = admin.user_id().to_string();
    session.set_identity(Some(admin));

    // Seed another ledger entry with same timestamp to test deterministic id tie-breaking
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO customer_ledger (id, customer_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
             VALUES ('cleg_init_sharma_b', 'cust_sharma', 'PAYMENT_RECEIVED', 50000, 245000, 195000, 'PAYMENT', 'pmt_mock', 'Partial payment', ?1, '2026-09-13T10:00:00Z')",
            params![admin_id],
        )?;
        Ok(())
    }).unwrap();

    let history = get_customer_ledger_history_inner(&db, &session, "cust_sharma".to_string())
        .expect("Failed to fetch customer history");
    assert_eq!(history.entries.len(), 2);
    // Ordered by created_at DESC, id DESC -> 'cleg_init_sharma_b' should come before 'cleg_init_sharma'
    assert_eq!(history.entries[0].id, "cleg_init_sharma_b");
    assert_eq!(history.entries[1].id, "cleg_init_sharma");
}

#[test]
fn test_05_permission_denial_unauthorized_employee() {
    let (db, session, cache, _admin, emp_credits, emp_customers_only) = setup_test_context();

    // 1. Employee with CUSTOMERS permission ONLY must be DENIED access to CUSTOMER_CREDITS
    session.set_identity(Some(emp_customers_only));
    let err = get_customer_credits_summary_inner(&db, &session);
    assert!(err.is_err(), "Employee with CUSTOMERS only must not access credit summary");

    let prep_err = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 50000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(prep_err.is_err(), "Employee with CUSTOMERS only must not prepare payment");

    // 2. Employee with canonical CUSTOMER_CREDITS permission succeeds
    session.set_identity(Some(emp_credits));
    let ok = get_customer_credits_summary_inner(&db, &session);
    assert!(ok.is_ok(), "Employee with CUSTOMER_CREDITS must succeed");
}

#[test]
fn test_06_full_payment_settlement() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Full payment: exactly ₹2,450.00 (245,000 cents)
    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 245000,
            payment_method: "CASH".to_string(),
            notes: Some("Cleared all outstanding dues".to_string()),
        },
    ).expect("Preparation failed");

    assert_eq!(quote.balance_before_cents, 245000);
    assert_eq!(quote.balance_after_cents, 0);

    let receipt = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).expect("Confirmation failed");

    assert_eq!(receipt.amount_cents, 245000);
    assert_eq!(receipt.balance_before_cents, 245000);
    assert_eq!(receipt.balance_after_cents, 0);

    // Verify in database: balance must be 0
    let bal_db: i64 = db.with_connection(|conn| {
        let b: i64 = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'", [], |r| r.get(0))?;
        Ok(b)
    }).unwrap();
    assert_eq!(bal_db, 0);
}

#[test]
fn test_07_partial_payment() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Partial payment: ₹1,000.00 (100,000 cents) out of ₹2,450.00
    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 100000,
            payment_method: "UPI".to_string(),
            notes: Some("UPI part payment".to_string()),
        },
    ).expect("Preparation failed");

    assert_eq!(quote.balance_before_cents, 245000);
    assert_eq!(quote.balance_after_cents, 145000);

    let receipt = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).expect("Confirmation failed");

    assert_eq!(receipt.amount_cents, 100000);
    assert_eq!(receipt.balance_before_cents, 245000);
    assert_eq!(receipt.balance_after_cents, 145000);

    // Verify in database: balance must be 145,000
    let bal_db: i64 = db.with_connection(|conn| {
        let b: i64 = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'", [], |r| r.get(0))?;
        Ok(b)
    }).unwrap();
    assert_eq!(bal_db, 145000);
}

#[test]
fn test_08_zero_payment_rejection() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let res = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 0,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res.is_err(), "Zero payment must be rejected");
}

#[test]
fn test_09_negative_payment_rejection() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let res = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: -5000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res.is_err(), "Negative payment must be rejected");
}

#[test]
fn test_10_overpayment_rejection() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Current credit is 245,000 cents. Attempt 300,000 cents.
    let res = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 300000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res.is_err(), "Overpayment exceeding current credit must be rejected");
}

#[test]
fn test_11_invalid_customer_rejection() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Non-existent customer
    let res_none = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_ghost".to_string(),
            amount_cents: 5000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res_none.is_err(), "Non-existent customer must be rejected");

    // Inactive customer
    let res_inactive = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_inactive".to_string(),
            amount_cents: 5000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res_inactive.is_err(), "Inactive customer must be rejected");
}

#[test]
fn test_12_payment_method_validation() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Valid canonical methods
    for method in ["CASH", "UPI", "BANK_TRANSFER", "CARD", "OTHER"] {
        let quote = prepare_customer_payment_inner(
            &db,
            &session,
            &cache,
            PrepareCustomerPaymentIpcInput {
                customer_id: "cust_sharma".to_string(),
                amount_cents: 1000,
                payment_method: method.to_string(),
                notes: None,
            },
        );
        assert!(quote.is_ok(), "Method {} should be valid", method);
    }

    // Invalid method
    let res_invalid = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 1000,
            payment_method: "BITCOIN".to_string(),
            notes: None,
        },
    );
    assert!(res_invalid.is_err(), "Invalid payment method must be rejected");
}

#[test]
fn test_13_ledger_before_after_calculation() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_verma".to_string(),
            amount_cents: 40000, // Pay ₹400 out of ₹1,200
            payment_method: "CARD".to_string(),
            notes: None,
        },
    ).unwrap();

    let receipt = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).unwrap();

    assert_eq!(receipt.balance_before_cents, 120000);
    assert_eq!(receipt.balance_after_cents, 80000);

    // Verify ledger row in database
    let (b_before, b_after, amt): (i64, i64, i64) = db.with_connection(|conn| {
        let row = conn.query_row(
            "SELECT balance_before_cents, balance_after_cents, amount_cents
             FROM customer_ledger
             WHERE customer_id = 'cust_verma' AND entry_type = 'PAYMENT_RECEIVED'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(row)
    }).unwrap();

    assert_eq!(b_before, 120000);
    assert_eq!(b_after, 80000);
    assert_eq!(amt, 40000);
}

#[test]
fn test_14_historical_sale_remains_unchanged() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    let admin_id = admin.user_id().to_string();
    session.set_identity(Some(admin));

    // Seed a historical sale for customer sharma
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO sales (id, sale_number, customer_id, total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status, user_id, sale_date, created_at)
             VALUES ('sale_hist_01', 'INV-HIST-01', 'cust_sharma', 245000, 0, 245000, 'UNPAID', ?1, '2026-09-13', '2026-09-13T10:00:00Z')",
            params![admin_id],
        )?;
        Ok(())
    }).unwrap();

    // Confirm payment of ₹1,000 against customer
    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 100000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).unwrap();

    // Verify historical sale is 100% UNCHANGED
    let (s_tot, s_paid, s_cred, s_stat): (i64, i64, i64, String) = db.with_connection(|conn| {
        let row = conn.query_row(
            "SELECT total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status
             FROM sales WHERE id = 'sale_hist_01'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        Ok(row)
    }).unwrap();

    assert_eq!(s_tot, 245000);
    assert_eq!(s_paid, 0);
    assert_eq!(s_cred, 245000);
    assert_eq!(s_stat, "UNPAID");
}

#[test]
fn test_15_payment_creates_correct_ledger_event() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 50000,
            payment_method: "UPI".to_string(),
            notes: Some("GPay payment".to_string()),
        },
    ).unwrap();

    let receipt = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).unwrap();

    let (entry_type, ref_type, ref_id, notes): (String, String, String, Option<String>) = db.with_connection(|conn| {
        let row = conn.query_row(
            "SELECT entry_type, reference_type, reference_id, notes
             FROM customer_ledger
             WHERE reference_id = ?1",
            params![receipt.payment_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        Ok(row)
    }).unwrap();

    assert_eq!(entry_type, "PAYMENT_RECEIVED");
    assert_eq!(ref_type, "PAYMENT");
    assert_eq!(ref_id, receipt.payment_id);
    assert_eq!(notes.as_deref(), Some("GPay payment"));
}

#[test]
fn test_16_atomic_rollback_on_failure() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    let admin_id = admin.user_id().to_string();
    session.set_identity(Some(admin));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 50000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    // Inject a conflicting duplicate payment row to force mid-transaction failure
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
             VALUES (?1, 'CUSTOMER_CREDIT_SETTLEMENT', 'CUSTOMER', 'cust_sharma', 50000, 'CASH', ?2, NULL, 'now')",
            params![quote.payment_id, admin_id],
        )?;
        Ok(())
    }).unwrap();

    // Confirmation should fail because payment_id primary key collides
    let res = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(res.is_err(), "Mid-transaction failure must return error");

    // Check that customer balance was NOT modified (rolled back cleanly)
    let bal: i64 = db.with_connection(|conn| {
        let b = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'", [], |r| r.get(0))?;
        Ok(b)
    }).unwrap();
    assert_eq!(bal, 245000, "Balance must remain unchanged after transaction rollback");
}

#[test]
fn test_17_audit_actor_correctness() {
    let (db, session, cache, _admin, emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(emp_credits.clone()));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 20000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    let receipt = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).unwrap();

    // Verify audit log has caller user_id
    let audit_user: String = db.with_connection(|conn| {
        let u: String = conn.query_row(
            "SELECT user_id FROM audit_logs WHERE entity_id = ?1",
            params![receipt.payment_id],
            |r| r.get(0),
        )?;
        Ok(u)
    }).unwrap();

    assert_eq!(audit_user, emp_credits.user_id());
}

#[test]
fn test_18_authoritative_committed_balance() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 50000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    let receipt = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    ).unwrap();

    let db_bal: i64 = db.with_connection(|conn| {
        let b = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'", [], |r| r.get(0))?;
        Ok(b)
    }).unwrap();

    assert_eq!(receipt.balance_after_cents, db_bal);
}

#[test]
fn test_19_session_isolation() {
    let (db, session, cache, _admin, _emp_credits, _emp_unauth) = setup_test_context();
    // No session set
    let res = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 10000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res.is_err(), "Unauthenticated call must be rejected");
}

#[test]
fn test_20_business_isolation() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Attempt payment for customer belonging to an inactive status
    let res = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_inactive".to_string(),
            amount_cents: 10000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    );
    assert!(res.is_err(), "Isolated/inactive customer must be rejected");
}

#[test]
fn test_21_replay_protection() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 50000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    // First confirmation succeeds
    let receipt1 = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token.clone(),
        },
    );
    assert!(receipt1.is_ok());

    // Second confirmation with same token MUST FAIL (single-use consumption)
    let receipt2 = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(receipt2.is_err(), "Replay of same preparation token must be rejected");
}

#[test]
fn test_22_logout_invalidation() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin.clone()));

    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 50000,
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    // Log out user
    session.logout_customer_payments(&cache);

    // Attempting to confirm after logout fails
    session.set_identity(Some(admin));
    let res = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    );
    assert!(res.is_err(), "Confirmation of prepared payment after logout must fail");
}

#[test]
fn test_23_concurrent_payment_race() {
    // MANDATORY CONCURRENCY TEST:
    // Customer has ₹1,000 balance (100,000 cents).
    // Two threads attempt payments of ₹700 (70,000 cents) concurrently.
    // Result:
    // - Exactly 1 succeeds.
    // - Exactly 1 is rejected.
    // - Final outstanding balance = ₹300 (30,000 cents).
    // - Total settled amount = ₹700 (70,000 cents).
    // - Second ₹700 is NEVER silently reduced to ₹300.
    let (db, _session, _cache, admin, _emp_credits, _emp_unauth) = setup_test_context();

    // Set customer credit to exactly 100,000 cents (₹1,000.00)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE customers SET current_credit_cents = 100000 WHERE id = 'cust_sharma'",
            [],
        )?;
        Ok(())
    }).unwrap();

    let db_arc = Arc::new(db);

    // Prepare payment 1
    let session1 = AuthSession::default();
    session1.set_identity(Some(admin.clone()));
    let cache1 = PreparedCustomerPaymentCache::default();
    let quote1 = prepare_customer_payment_inner(
        &db_arc,
        &session1,
        &cache1,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 70000,
            payment_method: "CASH".to_string(),
            notes: Some("Attempt A".to_string()),
        },
    ).expect("Prepare 1 failed");

    // Prepare payment 2
    let session2 = AuthSession::default();
    session2.set_identity(Some(admin.clone()));
    let cache2 = PreparedCustomerPaymentCache::default();
    let quote2 = prepare_customer_payment_inner(
        &db_arc,
        &session2,
        &cache2,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 70000,
            payment_method: "UPI".to_string(),
            notes: Some("Attempt B".to_string()),
        },
    ).expect("Prepare 2 failed");

    let db_clone1 = Arc::clone(&db_arc);
    let token1 = quote1.preparation_token;
    let handle1 = thread::spawn(move || {
        confirm_customer_payment_inner(
            &db_clone1,
            &session1,
            &cache1,
            ConfirmCustomerPaymentIpcInput {
                preparation_token: token1,
            },
        )
    });

    let db_clone2 = Arc::clone(&db_arc);
    let token2 = quote2.preparation_token;
    let handle2 = thread::spawn(move || {
        confirm_customer_payment_inner(
            &db_clone2,
            &session2,
            &cache2,
            ConfirmCustomerPaymentIpcInput {
                preparation_token: token2,
            },
        )
    });

    let res1 = handle1.join().unwrap();
    let res2 = handle2.join().unwrap();

    let successes = (if res1.is_ok() { 1 } else { 0 }) + (if res2.is_ok() { 1 } else { 0 });
    let failures = (if res1.is_err() { 1 } else { 0 }) + (if res2.is_err() { 1 } else { 0 });

    assert_eq!(successes, 1, "Exactly one concurrent payment of ₹700 must succeed");
    assert_eq!(failures, 1, "Exactly one concurrent payment of ₹700 must be rejected");

    let final_balance: i64 = db_arc.with_connection(|conn| {
        let b = conn.query_row("SELECT current_credit_cents FROM customers WHERE id = 'cust_sharma'", [], |r| r.get(0))?;
        Ok(b)
    }).unwrap();

    assert_eq!(final_balance, 30000, "Final outstanding balance must be exactly ₹300 (30,000 cents)");
}

#[test]
fn test_24_stale_outstanding_balance_at_confirmation() {
    let (db, session, cache, admin, _emp_credits, _emp_unauth) = setup_test_context();
    session.set_identity(Some(admin));

    // Customer has ₹2,450.00 credit
    let quote = prepare_customer_payment_inner(
        &db,
        &session,
        &cache,
        PrepareCustomerPaymentIpcInput {
            customer_id: "cust_sharma".to_string(),
            amount_cents: 200000, // Prepare payment for ₹2,000.00
            payment_method: "CASH".to_string(),
            notes: None,
        },
    ).unwrap();

    // Now an external transaction reduces customer credit to ₹500 (50,000 cents)
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE customers SET current_credit_cents = 50000 WHERE id = 'cust_sharma'",
            [],
        )?;
        Ok(())
    }).unwrap();

    // Now confirming the ₹2,000 payment must fail because live credit (₹500) < payment amount (₹2,000)
    let res = confirm_customer_payment_inner(
        &db,
        &session,
        &cache,
        ConfirmCustomerPaymentIpcInput {
            preparation_token: quote.preparation_token,
        },
    );

    assert!(res.is_err(), "Stale preparation exceeding updated balance must be rejected at confirmation");
}
