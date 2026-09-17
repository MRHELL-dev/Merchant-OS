use desktop_lib::auth::Role;
use desktop_lib::commands::{
    create_initial_admin_inner, get_auth_state_inner, login_inner, logout_inner, AuthSession,
    CreateInitialAdminInput, LoginInput,
};
use desktop_lib::db::DatabaseManager;

use std::sync::atomic::{AtomicU64, Ordering};
static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn setup_test_db() -> (DatabaseManager, std::path::PathBuf) {
    let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_dir = std::env::temp_dir().join(format!("merchant_b15_test_{}_{}", std::process::id(), count));
    let _ = std::fs::create_dir_all(&temp_dir);
    let db_path = temp_dir.join("test.db");
    let db = DatabaseManager::open(&db_path).expect("Failed to open fresh test db");
    (db, temp_dir)
}

// ================================================================================================
// BUILD 15 TESTS: FIRST-RUN AUTHENTICATION & ADMIN ONBOARDING
// ================================================================================================

#[test]
fn test_a_fresh_database_reports_first_run_admin_setup() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    let auth_state = get_auth_state_inner(&db, &session).expect("Failed to get auth state");
    assert_eq!(auth_state.status, "FIRST_RUN_ADMIN_SETUP");
    assert!(auth_state.user.is_none());
}

#[test]
fn test_b_c_d_i_valid_credentials_create_first_admin_and_establish_session() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();
    let raw_password = "SecureAdminPassword!2026";

    let input = CreateInitialAdminInput {
        username: "super_admin".to_string(),
        password: raw_password.to_string(),
        confirm_password: raw_password.to_string(),
    };

    // TEST B: Create first Admin
    let user_dto =
        create_initial_admin_inner(&db, &session, input).expect("Failed to create initial admin");

    // TEST C: Role must be ADMIN
    assert_eq!(user_dto.username, "super_admin");
    assert_eq!(user_dto.role, "ADMIN");

    // TEST I: Active authenticated session established
    let identity = session.get_identity().expect("Session must be active");
    assert_eq!(identity.username(), "super_admin");
    assert_eq!(identity.role(), Role::Admin);

    // TEST D: Password is not plaintext; Argon2id hash verified in SQLite
    db.with_connection(|conn| {
        let stored_hash: String = conn.query_row(
            "SELECT password_hash FROM users WHERE username = 'super_admin'",
            [],
            |r| r.get(0),
        )?;
        assert!(
            stored_hash.starts_with("$argon2id$"),
            "Hash must start with $argon2id$"
        );
        assert_ne!(
            stored_hash, raw_password,
            "Password must never be stored as plaintext"
        );

        let count_raw: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE password_hash = ?1",
            rusqlite::params![raw_password],
            |r| r.get(0),
        )?;
        assert_eq!(count_raw, 0, "Plaintext password must not exist in database");
        Ok(())
    })
    .expect("DB check failed");

    // Auth state must now be AUTHENTICATED
    let auth_state = get_auth_state_inner(&db, &session).expect("Failed to get auth state");
    assert_eq!(auth_state.status, "AUTHENTICATED");
    assert!(auth_state.user.is_some());
    assert_eq!(auth_state.user.unwrap().username, "super_admin");
}

#[test]
fn test_e_empty_username_rejected() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    let input = CreateInitialAdminInput {
        username: "   ".to_string(),
        password: "ValidPassword123".to_string(),
        confirm_password: "ValidPassword123".to_string(),
    };

    let result = create_initial_admin_inner(&db, &session, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("cannot be empty"));
    assert!(session.get_identity().is_err());
}

#[test]
fn test_f_empty_password_rejected() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    let input = CreateInitialAdminInput {
        username: "admin".to_string(),
        password: "".to_string(),
        confirm_password: "".to_string(),
    };

    let result = create_initial_admin_inner(&db, &session, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("cannot be empty"));
    assert!(session.get_identity().is_err());
}

#[test]
fn test_g_password_mismatch_rejected() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    let input = CreateInitialAdminInput {
        username: "admin".to_string(),
        password: "PasswordA123".to_string(),
        confirm_password: "PasswordB456".to_string(),
    };

    let result = create_initial_admin_inner(&db, &session, input);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("do not match"));
    assert!(session.get_identity().is_err());
}

#[test]
fn test_h_second_attempt_to_create_initial_admin_is_rejected() {
    let (db, _dir) = setup_test_db();
    let session = AuthSession::default();

    let input1 = CreateInitialAdminInput {
        username: "admin_first".to_string(),
        password: "Password123".to_string(),
        confirm_password: "Password123".to_string(),
    };
    create_initial_admin_inner(&db, &session, input1).expect("First admin creation must succeed");

    let input2 = CreateInitialAdminInput {
        username: "admin_second".to_string(),
        password: "Password456".to_string(),
        confirm_password: "Password456".to_string(),
    };
    let result = create_initial_admin_inner(&db, &session, input2);
    assert!(result.is_err());
    let err_msg = result.err().unwrap();
    assert!(
        err_msg.contains("already exists"),
        "Expected admin already exists error, got: {}",
        err_msg
    );
}

#[test]
fn test_j_k_existing_admin_without_session_reports_unauthenticated_not_setup() {
    let (db, _dir) = setup_test_db();

    // 1. Create admin in DB
    let init_session = AuthSession::default();
    let input = CreateInitialAdminInput {
        username: "existing_admin".to_string(),
        password: "AdminPassword123".to_string(),
        confirm_password: "AdminPassword123".to_string(),
    };
    create_initial_admin_inner(&db, &init_session, input).expect("Setup must succeed");

    // 2. Simulate application restart with a fresh empty AuthSession
    let restart_session = AuthSession::default();
    assert!(restart_session.get_identity().is_err());

    // TEST K: Must report UNAUTHENTICATED (requiring login), NEVER FIRST_RUN_ADMIN_SETUP
    let auth_state = get_auth_state_inner(&db, &restart_session).expect("Get auth state");
    assert_eq!(
        auth_state.status, "UNAUTHENTICATED",
        "Must require login; no auto-login merely because Admin exists"
    );
    assert!(auth_state.user.is_none());
}

#[test]
fn test_l_m_n_login_and_logout_lifecycle() {
    let (db, _dir) = setup_test_db();
    let raw_password = "CorrectPassword123";

    // Create initial admin
    let setup_session = AuthSession::default();
    let input = CreateInitialAdminInput {
        username: "store_owner".to_string(),
        password: raw_password.to_string(),
        confirm_password: raw_password.to_string(),
    };
    create_initial_admin_inner(&db, &setup_session, input).expect("Setup must succeed");

    // Fresh session (simulating restart or logged out)
    let session = AuthSession::default();
    assert_eq!(
        get_auth_state_inner(&db, &session).unwrap().status,
        "UNAUTHENTICATED"
    );

    // TEST M: Login with wrong password rejected
    let bad_login = LoginInput {
        username: "store_owner".to_string(),
        password: "WrongPassword".to_string(),
    };
    let bad_res = login_inner(&db, &session, bad_login);
    assert!(bad_res.is_err());
    assert_eq!(
        get_auth_state_inner(&db, &session).unwrap().status,
        "UNAUTHENTICATED"
    );

    // TEST L: Login with correct password succeeds
    let good_login = LoginInput {
        username: "store_owner".to_string(),
        password: raw_password.to_string(),
    };
    let user_dto = login_inner(&db, &session, good_login).expect("Login must succeed");
    assert_eq!(user_dto.username, "store_owner");
    assert_eq!(user_dto.role, "ADMIN");
    assert_eq!(
        get_auth_state_inner(&db, &session).unwrap().status,
        "AUTHENTICATED"
    );

    // TEST N: Logout terminates session
    logout_inner(&session).expect("Logout must succeed");
    assert!(session.get_identity().is_err());
    assert_eq!(
        get_auth_state_inner(&db, &session).unwrap().status,
        "UNAUTHENTICATED"
    );
}
