use desktop_lib::auth::{AuthService, Role};
use desktop_lib::commands::{
    create_initial_admin_inner, get_auth_state_inner, get_business_profile_inner, login_inner,
    save_business_profile_inner, AuthSession, CreateInitialAdminInput, LoginInput,
    SaveBusinessProfileInput,
};
use desktop_lib::db::DatabaseManager;

use std::sync::atomic::{AtomicU64, Ordering};
static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn setup_test_db() -> (DatabaseManager, std::path::PathBuf) {
    let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_dir = std::env::temp_dir().join(format!("merchant_b16_test_{}_{}", std::process::id(), count));
    let _ = std::fs::create_dir_all(&temp_dir);
    let db_path = temp_dir.join("test_b16.db");
    let db = DatabaseManager::open(&db_path).expect("Failed to open fresh test db");
    (db, temp_dir)
}

// ================================================================================================
// BUILD 16 TESTS: BUSINESS PROFILE ONBOARDING & BOUNDARIES
// ================================================================================================

#[test]
fn test_01_fresh_database_has_no_admin_and_no_business_profile() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    let auth_state = get_auth_state_inner(&db, &session).expect("Failed to get auth state");
    assert_eq!(auth_state.status, "FIRST_RUN_ADMIN_SETUP");
    assert!(auth_state.user.is_none());
    assert!(auth_state.business.is_none(), "Fresh DB must have no business profile");

    // Direct SQLite check: businesses table must be empty
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM businesses", [], |r| r.get(0))?;
        assert_eq!(count, 0, "businesses table must be completely empty on fresh DB");
        Ok(())
    })
    .expect("DB query failed");
}

#[test]
fn test_02_unauthenticated_access_to_profile_operations_rejected() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    // get_business_profile without session must fail
    let get_res = get_business_profile_inner(&db, &session);
    assert!(get_res.is_err(), "Unauthenticated get_business_profile must be rejected");

    // save_business_profile without session must fail
    let save_res = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "My Kirana Store".to_string(),
            phone: "+919876543210".to_string(),
            address: "Main Market, Delhi".to_string(),
        },
    );
    assert!(save_res.is_err(), "Unauthenticated save_business_profile must be rejected");
}

#[test]
fn test_03_admin_creation_establishes_session_requiring_business_onboarding() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    // 1. Create Initial Admin
    let user_dto = create_initial_admin_inner(
        &db,
        &session,
        CreateInitialAdminInput {
            username: "admin_owner".to_string(),
            password: "SecurePassword123!".to_string(),
            confirm_password: "SecurePassword123!".to_string(),
        },
    )
    .expect("Failed to create admin");

    assert_eq!(user_dto.username, "admin_owner");
    assert_eq!(user_dto.role, "ADMIN");

    // 2. Auth state is AUTHENTICATED, but business is None
    let auth_state = get_auth_state_inner(&db, &session).expect("Failed to get auth state");
    assert_eq!(auth_state.status, "AUTHENTICATED");
    assert!(auth_state.user.is_some());
    assert!(
        auth_state.business.is_none(),
        "Business profile must be None prior to onboarding"
    );

    // 3. get_business_profile returns None
    let profile = get_business_profile_inner(&db, &session).expect("Failed to query profile");
    assert!(profile.is_none(), "Profile query must return None before save");
}

#[test]
fn test_04_employee_forbidden_from_saving_business_profile() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    // Create admin first
    create_initial_admin_inner(
        &db,
        &session,
        CreateInitialAdminInput {
            username: "admin_owner".to_string(),
            password: "AdminPassword123!".to_string(),
            confirm_password: "AdminPassword123!".to_string(),
        },
    )
    .expect("Create admin");

    let admin_identity = session.get_identity().expect("Admin identity");

    // Create an employee
    let emp_id = db
        .with_connection(|conn| {
            Ok(AuthService::create_employee(conn, &admin_identity, "cashier_raj", "CashierPass123!")?)
        })
        .expect("Create employee");
    assert!(!emp_id.is_empty());

    // Switch session to employee
    let emp_identity = db
        .with_connection(|conn| Ok(AuthService::authenticate(conn, "cashier_raj", "CashierPass123!")?))
        .expect("Authenticate employee");
    assert_eq!(emp_identity.role(), Role::Employee);
    session.set_identity(Some(emp_identity));

    // Employee attempt to save business profile must be rejected
    let err = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Hacked Store Name".to_string(),
            phone: "+910000000000".to_string(),
            address: "Unauthorized Address".to_string(),
        },
    )
    .expect_err("Employee must not be authorized to save business profile");

    assert!(
        err.contains("Only an Administrator"),
        "Error message must indicate admin requirement, got: {}",
        err
    );
}

#[test]
fn test_05_required_field_validation_rejects_empty_inputs() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    create_initial_admin_inner(
        &db,
        &session,
        CreateInitialAdminInput {
            username: "admin_owner".to_string(),
            password: "AdminPassword123!".to_string(),
            confirm_password: "AdminPassword123!".to_string(),
        },
    )
    .expect("Create admin");

    // 1. Empty Name
    let err_name = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "   ".to_string(),
            phone: "+919876543210".to_string(),
            address: "Valid Address".to_string(),
        },
    )
    .expect_err("Empty name must be rejected");
    assert!(err_name.contains("Name cannot be empty"));

    // 2. Empty Phone
    let err_phone = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Valid Store Name".to_string(),
            phone: "   ".to_string(),
            address: "Valid Address".to_string(),
        },
    )
    .expect_err("Empty phone must be rejected");
    assert!(err_phone.contains("Phone Number cannot be empty"));

    // 3. Empty Address
    let err_addr = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Valid Store Name".to_string(),
            phone: "+919876543210".to_string(),
            address: "   ".to_string(),
        },
    )
    .expect_err("Empty address must be rejected");
    assert!(err_addr.contains("Address cannot be empty"));
}

#[test]
fn test_06_successful_onboarding_saves_profile_and_transitions_state() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    create_initial_admin_inner(
        &db,
        &session,
        CreateInitialAdminInput {
            username: "admin_owner".to_string(),
            password: "AdminPassword123!".to_string(),
            confirm_password: "AdminPassword123!".to_string(),
        },
    )
    .expect("Create admin");

    // Save profile
    let profile = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Bharat Kirana & General Store".to_string(),
            phone: "+919876543210".to_string(),
            address: "Shop 4, Market Complex, Delhi".to_string(),
        },
    )
    .expect("Save business profile");

    assert_eq!(profile.name, "Bharat Kirana & General Store");
    assert_eq!(profile.phone, "+919876543210");
    assert_eq!(profile.address, "Shop 4, Market Complex, Delhi");
    assert_eq!(profile.id, "biz_default");

    // Auth state now includes business
    let auth_state = get_auth_state_inner(&db, &session).expect("Get auth state");
    assert_eq!(auth_state.status, "AUTHENTICATED");
    assert!(auth_state.business.is_some());
    let biz = auth_state.business.unwrap();
    assert_eq!(biz.name, "Bharat Kirana & General Store");

    // Verify directly in SQLite
    db.with_connection(|conn| {
        let (name, phone, address): (String, String, String) = conn.query_row(
            "SELECT name, phone, address FROM businesses WHERE id = ?1",
            rusqlite::params![profile.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(name, "Bharat Kirana & General Store");
        assert_eq!(phone, "+919876543210");
        assert_eq!(address, "Shop 4, Market Complex, Delhi");

        let meta: String = conn.query_row(
            "SELECT value FROM system_metadata WHERE key = 'business_profile_onboarded'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(meta, "true");
        Ok(())
    })
    .expect("DB verification failed");
}

#[test]
fn test_07_persistence_across_restart_and_login_transition_to_dashboard() {
    let (_initial_db, temp_dir) = setup_test_db();
    let db_path = temp_dir.join("test_b16.db");

    // Phase 1: First Run Setup
    {
        let db = DatabaseManager::open(&db_path).expect("Open db");
        let session = AuthSession::default();

        create_initial_admin_inner(
            &db,
            &session,
            CreateInitialAdminInput {
                username: "super_merchant".to_string(),
                password: "MerchantPassword2026!".to_string(),
                confirm_password: "MerchantPassword2026!".to_string(),
            },
        )
        .expect("Create admin");

        save_business_profile_inner(
            &db,
            &session,
            SaveBusinessProfileInput {
                name: "Super Mart Superstore".to_string(),
                phone: "+919123456780".to_string(),
                address: "Connaught Place, Central Delhi".to_string(),
            },
        )
        .expect("Save profile");
    }

    // Phase 2: Application Restart (new process, fresh DB manager, empty session)
    {
        let restart_db = DatabaseManager::open(&db_path).expect("Reopen db");
        let restart_session = AuthSession::default();

        // 1. Prior to login: UNAUTHENTICATED, but existing business profile is detected
        let pre_login_state =
            get_auth_state_inner(&restart_db, &restart_session).expect("Auth state");
        assert_eq!(pre_login_state.status, "UNAUTHENTICATED");
        assert!(pre_login_state.user.is_none());
        assert!(
            pre_login_state.business.is_some(),
            "Existing business profile must be present across restart"
        );
        assert_eq!(
            pre_login_state.business.unwrap().name,
            "Super Mart Superstore"
        );

        // 2. Login as Admin
        let user = login_inner(
            &restart_db,
            &restart_session,
            LoginInput {
                username: "super_merchant".to_string(),
                password: "MerchantPassword2026!".to_string(),
            },
        )
        .expect("Login");
        assert_eq!(user.username, "super_merchant");

        // 3. Post-login: AUTHENTICATED with existing valid Business Profile -> Dashboard directly
        let post_login_state =
            get_auth_state_inner(&restart_db, &restart_session).expect("Post login auth state");
        assert_eq!(post_login_state.status, "AUTHENTICATED");
        assert!(post_login_state.user.is_some());
        assert!(post_login_state.business.is_some());
        assert_eq!(
            post_login_state.business.unwrap().name,
            "Super Mart Superstore"
        );

        // 4. get_business_profile returns the profile
        let profile =
            get_business_profile_inner(&restart_db, &restart_session).expect("Get profile");
        assert!(profile.is_some());
        assert_eq!(profile.unwrap().address, "Connaught Place, Central Delhi");
    }
}

#[test]
fn test_08_updating_existing_business_profile_modifies_record_without_duplicates() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    create_initial_admin_inner(
        &db,
        &session,
        CreateInitialAdminInput {
            username: "admin_owner".to_string(),
            password: "AdminPassword123!".to_string(),
            confirm_password: "AdminPassword123!".to_string(),
        },
    )
    .expect("Create admin");

    // Initial save
    let initial = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Original Store Name".to_string(),
            phone: "+919811122233".to_string(),
            address: "Original Address".to_string(),
        },
    )
    .expect("Initial save");

    assert_eq!(initial.name, "Original Store Name");

    // Second save (updating profile from settings / dashboard)
    let updated = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Updated Store Name".to_string(),
            phone: "+919844455566".to_string(),
            address: "New Renovated Address".to_string(),
        },
    )
    .expect("Update save");

    assert_eq!(updated.id, initial.id, "Must update existing business ID without creating duplicate");
    assert_eq!(updated.name, "Updated Store Name");
    assert_eq!(updated.phone, "+919844455566");
    assert_eq!(updated.address, "New Renovated Address");

    // Verify row count remains exactly 1
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM businesses", [], |r| r.get(0))?;
        assert_eq!(count, 1, "Must not create duplicate business records");
        Ok(())
    })
    .expect("Count verification failed");
}

#[test]
fn test_09_gui_lifecycle_with_merchant_os_db_path() {
    let temp_dir = std::env::temp_dir().join(format!("merchant_b16_gui_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let temp_db_path = temp_dir.join("gui_test_merchant_os.db");

    // Set MERCHANT_OS_DB_PATH to isolate database
    std::env::set_var("MERCHANT_OS_DB_PATH", temp_db_path.to_str().unwrap());

    // 1. App Startup with fresh DB via resolve_db_path
    let resolved_path = desktop_lib::resolve_db_path(None);
    assert_eq!(resolved_path, temp_db_path);

    let db = desktop_lib::open_app_database(&resolved_path).expect("Open app database");
    let session = AuthSession::default();

    // Step A: Initial state -> Admin Setup
    let initial_auth = get_auth_state_inner(&db, &session).expect("Initial auth state");
    assert_eq!(initial_auth.status, "FIRST_RUN_ADMIN_SETUP");
    assert!(initial_auth.user.is_none());
    assert!(initial_auth.business.is_none());

    // Step B: Admin Setup -> Create Initial Admin
    let admin_user = create_initial_admin_inner(
        &db,
        &session,
        CreateInitialAdminInput {
            username: "store_admin".to_string(),
            password: "AdminPassword123!".to_string(),
            confirm_password: "AdminPassword123!".to_string(),
        },
    )
    .expect("Admin setup");
    assert_eq!(admin_user.username, "store_admin");

    // Step C: Business Profile prompt (Authenticated session without business profile)
    let post_admin_auth = get_auth_state_inner(&db, &session).expect("Post-admin auth state");
    assert_eq!(post_admin_auth.status, "AUTHENTICATED");
    assert!(post_admin_auth.user.is_some());
    assert!(post_admin_auth.business.is_none(), "Requires Business Profile onboarding");

    // Step D: Save Business Profile
    let initial_profile = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Kirana Central Superstore".to_string(),
            phone: "+919876543210".to_string(),
            address: "Plot 10, Sector 12, Dwarka, Delhi".to_string(),
        },
    )
    .expect("Save business profile");
    assert_eq!(initial_profile.name, "Kirana Central Superstore");

    // Step E: Transition to Dashboard -> Shop name visible in header
    let dashboard_auth = get_auth_state_inner(&db, &session).expect("Dashboard auth state");
    assert_eq!(dashboard_auth.status, "AUTHENTICATED");
    assert!(dashboard_auth.business.is_some());
    assert_eq!(dashboard_auth.business.as_ref().unwrap().name, "Kirana Central Superstore");

    // Step F: Edit Profile Modal -> Save updated profile
    let updated_profile = save_business_profile_inner(
        &db,
        &session,
        SaveBusinessProfileInput {
            name: "Kirana Central Mega Mart".to_string(),
            phone: "+919811122233".to_string(),
            address: "Plot 10 & 11, Sector 12, Dwarka, Delhi".to_string(),
        },
    )
    .expect("Update business profile");
    assert_eq!(updated_profile.id, initial_profile.id);
    assert_eq!(updated_profile.name, "Kirana Central Mega Mart");

    // Step G: Updated shop name visible
    let updated_auth = get_auth_state_inner(&db, &session).expect("Updated auth state");
    assert_eq!(updated_auth.business.as_ref().unwrap().name, "Kirana Central Mega Mart");

    // Step H: Logout
    desktop_lib::commands::logout_inner(&session).expect("Logout");
    let logged_out_auth = get_auth_state_inner(&db, &session).expect("Logged out auth state");
    assert_eq!(logged_out_auth.status, "UNAUTHENTICATED");
    assert!(logged_out_auth.user.is_none());
    assert!(logged_out_auth.business.is_some()); // Authoritative store exists in DB

    // Step I: Login
    let logged_in_user = login_inner(
        &db,
        &session,
        LoginInput {
            username: "store_admin".to_string(),
            password: "AdminPassword123!".to_string(),
        },
    )
    .expect("Login");
    assert_eq!(logged_in_user.username, "store_admin");

    // Step J: Dashboard with updated shop name directly (no duplicate onboarding prompt)
    let final_auth = get_auth_state_inner(&db, &session).expect("Final auth state");
    assert_eq!(final_auth.status, "AUTHENTICATED");
    assert!(final_auth.user.is_some());
    assert!(final_auth.business.is_some());
    assert_eq!(final_auth.business.as_ref().unwrap().name, "Kirana Central Mega Mart");

    // Clean up env
    std::env::remove_var("MERCHANT_OS_DB_PATH");
    let _ = std::fs::remove_dir_all(&temp_dir);
}
