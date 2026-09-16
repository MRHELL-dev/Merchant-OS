use rusqlite::{params, Connection};

use super::authorization::AuthorizationService;
use super::error::AuthError;
use super::identity::{AuthenticatedIdentity, Role};
use super::password::{hash_password, hash_security_answer, verify_password, verify_security_answer};

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static AUDIT_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_audit_id(prefix: &str) -> String {
    let count = AUDIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}_{}_{}", prefix, nanos, count)
}

/// Authoritative Authentication Service for Merchant OS.
///
/// Crucial Security Invariants:
/// - Single local source of truth: authoritative SQLite database.
/// - Passwords and security answers hashed using Argon2id.
/// - Generic authentication failure to prevent username enumeration.
/// - Audit logs record authoritative actor ID and NEVER contain credentials or secret hashes.
pub struct AuthService;

impl AuthService {
    // --------------------------------------------------------------------------------------------
    // 1. INITIAL ADMIN SETUP
    // --------------------------------------------------------------------------------------------
    pub fn create_initial_admin(
        conn: &mut Connection,
        username: &str,
        password: &str,
        confirm_password: &str,
        security_question: &str,
        security_answer: &str,
    ) -> Result<AuthenticatedIdentity, AuthError> {
        let trimmed_user = username.trim();
        if trimmed_user.is_empty() {
            return Err(AuthError::InvalidCredentials);
        }

        if password != confirm_password {
            return Err(AuthError::PasswordMismatch);
        }

        let trimmed_q = security_question.trim();
        if trimmed_q.is_empty() {
            return Err(AuthError::WeakPassword(
                "Security question cannot be empty".to_string(),
            ));
        }

        // Check if an Admin account already exists
        let admin_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'ADMIN'",
            [],
            |row| row.get(0),
        )?;
        if admin_count > 0 {
            return Err(AuthError::AdminAlreadyExists);
        }

        // Check if username is already taken by any user
        let user_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE username = ?1",
            params![trimmed_user],
            |row| row.get(0),
        )?;
        if user_count > 0 {
            return Err(AuthError::AdminAlreadyExists);
        }

        // Cryptographically hash password and security answer with Argon2id
        let password_hash = hash_password(password)?;
        let answer_hash = hash_security_answer(security_answer)?;

        let now = format!("{:?}", std::time::SystemTime::now());
        let admin_id = format!("usr_admin_{}", trimmed_user);

        // Atomic transaction for initial admin setup
        let tx = conn.transaction()?;

        // 1. Insert admin user
        tx.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'ADMIN', 1, ?4, ?5)",
            params![admin_id, trimmed_user, password_hash, now, now],
        )?;

        // 2. Persist security question and hashed answer in system_metadata
        tx.execute(
            "INSERT INTO system_metadata (key, value, updated_at)
             VALUES ('admin_security_question', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = ?2",
            params![trimmed_q, now],
        )?;

        tx.execute(
            "INSERT INTO system_metadata (key, value, updated_at)
             VALUES ('admin_security_answer_hash', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = ?2",
            params![answer_hash, now],
        )?;

        // 3. Audit log (strictly without credentials)
        let audit_id = next_audit_id(&format!("audit_admin_init_{}", admin_id));
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'ADMIN_CREATED', 'users', ?2, 'Initial system Admin established', ?3)",
            params![audit_id, admin_id, now],
        )?;

        tx.commit()?;

        Ok(AuthenticatedIdentity::new(
            admin_id,
            trimmed_user.to_string(),
            Role::Admin,
        ))
    }

    // --------------------------------------------------------------------------------------------
    // 2. AUTHENTICATION (LOGIN)
    // --------------------------------------------------------------------------------------------
    pub fn authenticate(
        conn: &Connection,
        username: &str,
        password: &str,
    ) -> Result<AuthenticatedIdentity, AuthError> {
        let trimmed_user = username.trim();

        // Query user record by username
        let user_opt: Option<(String, String, String, i64)> = conn
            .query_row(
                "SELECT id, password_hash, role, is_active FROM users WHERE username = ?1",
                params![trimmed_user],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();

        let (user_id, password_hash, role_str, is_active) = match user_opt {
            Some(row) => row,
            None => {
                // Unknown username: return generic failure to prevent enumeration
                return Err(AuthError::InvalidCredentials);
            }
        };

        // Check if user is inactive
        if is_active == 0 {
            return Err(AuthError::UserInactive);
        }

        // Verify password against stored Argon2id hash
        let is_valid = verify_password(password, &password_hash)?;
        if !is_valid {
            return Err(AuthError::InvalidCredentials);
        }

        let role = Role::from_str(&role_str).ok_or_else(|| {
            AuthError::DatabaseError(format!("Invalid role '{}' in users table", role_str))
        })?;

        // Audit login success (non-failing)
        let now = format!("{:?}", std::time::SystemTime::now());
        let audit_id = next_audit_id(&format!("audit_login_{}", user_id));
        let _ = conn.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'USER_LOGIN_SUCCESS', 'users', ?2, 'Local authentication verified', ?3)",
            params![audit_id, user_id, now],
        );

        Ok(AuthenticatedIdentity::new(
            user_id,
            trimmed_user.to_string(),
            role,
        ))
    }

    // --------------------------------------------------------------------------------------------
    // 3. EMPLOYEE CREATION (ADMIN-ONLY)
    // --------------------------------------------------------------------------------------------
    pub fn create_employee(
        conn: &mut Connection,
        admin_identity: &AuthenticatedIdentity,
        username: &str,
        password: &str,
    ) -> Result<String, AuthError> {
        // Enforce Admin authority
        AuthorizationService::require_admin(admin_identity, "create employee")?;

        let trimmed_user = username.trim();
        if trimmed_user.is_empty() {
            return Err(AuthError::InvalidCredentials);
        }

        // Enforce unique username
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE username = ?1",
            params![trimmed_user],
            |row| row.get(0),
        )?;
        if count > 0 {
            return Err(AuthError::DatabaseError(format!(
                "Username '{}' already exists",
                trimmed_user
            )));
        }

        let password_hash = hash_password(password)?;
        let employee_id = format!("usr_emp_{}", trimmed_user);
        let now = format!("{:?}", std::time::SystemTime::now());

        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO users (id, username, password_hash, role, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'EMPLOYEE', 1, ?4, ?5)",
            params![employee_id, trimmed_user, password_hash, now, now],
        )?;

        let audit_id = next_audit_id(&format!("audit_emp_create_{}", employee_id));
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'EMPLOYEE_CREATED', 'users', ?3, ?4, ?5)",
            params![
                audit_id,
                admin_identity.user_id(),
                employee_id,
                format!("Employee account created for user '{}'", trimmed_user),
                now,
            ],
        )?;

        tx.commit()?;

        Ok(employee_id)
    }

    // --------------------------------------------------------------------------------------------
    // 4. PERMISSION MANAGEMENT (ADMIN-ONLY)
    // --------------------------------------------------------------------------------------------
    pub fn set_employee_permission(
        conn: &mut Connection,
        admin_identity: &AuthenticatedIdentity,
        employee_user_id: &str,
        feature_key: &str,
        is_enabled: bool,
    ) -> Result<(), AuthError> {
        // Enforce Admin authority
        AuthorizationService::require_admin(admin_identity, "set permission")?;

        // Verify target employee exists and is an Employee
        let role_str: String = conn
            .query_row(
                "SELECT role FROM users WHERE id = ?1",
                params![employee_user_id],
                |row| row.get(0),
            )
            .map_err(|_| AuthError::DatabaseError(format!("User {} not found", employee_user_id)))?;

        if role_str != "EMPLOYEE" {
            return Err(AuthError::AdminAuthorizationRequired(
                "Permissions can only be assigned to Employees".to_string(),
            ));
        }

        let now = format!("{:?}", std::time::SystemTime::now());
        let perm_id = format!("perm_{}_{}", employee_user_id, feature_key);
        let enabled_int: i64 = if is_enabled { 1 } else { 0 };

        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO permissions (id, user_id, feature_key, is_enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(user_id, feature_key) DO UPDATE SET is_enabled = ?4, updated_at = ?5",
            params![perm_id, employee_user_id, feature_key, enabled_int, now],
        )?;

        let audit_id = next_audit_id(&format!("audit_perm_{}", perm_id));
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PERMISSION_CHANGED', 'permissions', ?3, ?4, ?5)",
            params![
                audit_id,
                admin_identity.user_id(),
                perm_id,
                format!("Feature '{}' set to enabled={}", feature_key, is_enabled),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 5. OFFLINE PASSWORD RESET (ADMIN-ONLY VIA HASHED SECURITY ANSWER)
    // --------------------------------------------------------------------------------------------
    pub fn reset_admin_password(
        conn: &mut Connection,
        username: &str,
        security_answer: &str,
        new_password: &str,
        confirm_new_password: &str,
    ) -> Result<(), AuthError> {
        let trimmed_user = username.trim();

        if new_password != confirm_new_password {
            return Err(AuthError::PasswordMismatch);
        }

        // Verify target user is an ADMIN
        let (admin_id, role_str): (String, String) = conn
            .query_row(
                "SELECT id, role FROM users WHERE username = ?1",
                params![trimmed_user],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| AuthError::InvalidCredentials)?;

        if role_str != "ADMIN" {
            // Only Admin has security question offline recovery
            return Err(AuthError::AdminAuthorizationRequired(
                "Only Admin accounts support security-question password reset".to_string(),
            ));
        }

        // Read stored security answer hash from system_metadata
        let stored_answer_hash: String = conn
            .query_row(
                "SELECT value FROM system_metadata WHERE key = 'admin_security_answer_hash'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| {
                AuthError::DatabaseError("Security question not configured for Admin".to_string())
            })?;

        // Verify security answer against Argon2id hash
        let is_valid = verify_security_answer(security_answer, &stored_answer_hash)?;
        if !is_valid {
            return Err(AuthError::IncorrectSecurityAnswer);
        }

        // Hash new password with Argon2id
        let new_password_hash = hash_password(new_password)?;
        let now = format!("{:?}", std::time::SystemTime::now());

        let tx = conn.transaction()?;

        tx.execute(
            "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_password_hash, now, admin_id],
        )?;

        // Audit log (strictly without credentials or hashes)
        let audit_id = next_audit_id(&format!("audit_pwd_reset_{}", admin_id));
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PASSWORD_RESET', 'users', ?2, 'Admin password successfully reset via security answer', ?3)",
            params![audit_id, admin_id, now],
        )?;

        tx.commit()?;
        Ok(())
    }
}
