use base64::Engine;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::auth::{AuthenticatedIdentity, AuthorizationService, PermissionKey};
use crate::db::operations::BusinessError;
use crate::db::DatabaseManager;

pub const CURRENT_BACKUP_FORMAT_VERSION: u32 = 1;
pub const CURRENT_APP_VERSION: &str = "0.1.0";
pub const AUTO_BACKUP_RETENTION_LIMIT: usize = 7;

static AUDIT_COUNTER: AtomicU64 = AtomicU64::new(1);
fn next_audit_token() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let seq = AUDIT_COUNTER.fetch_add(1, Ordering::SeqCst);
    now + seq
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    #[serde(alias = "backup_format_version")]
    pub backup_format_version: u32,
    #[serde(alias = "app_version")]
    pub app_version: String,
    #[serde(alias = "created_at")]
    pub created_at: String,
    #[serde(alias = "backup_type")]
    pub backup_type: String, // "MANUAL" | "AUTO"
    #[serde(alias = "business_id")]
    pub business_id: String,
    #[serde(alias = "business_name")]
    pub business_name: String,
    #[serde(alias = "checksum_sha256")]
    pub checksum_sha256: String,
    #[serde(alias = "table_counts")]
    pub table_counts: HashMap<String, i64>,
    #[serde(alias = "total_records")]
    pub total_records: i64,
    #[serde(alias = "payload_size_bytes")]
    pub payload_size_bytes: usize,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPackage {
    pub manifest: BackupManifest,
    /// Base64 encoding of the SQLite binary snapshot.
    /// Note: Base64 is an encoding layer for JSON transport, not an integrity mechanism.
    /// Integrity is verified by computing SHA-256 over the decoded raw SQLite bytes.
    pub sqlite_snapshot_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupMetadata {
    pub id: String,
    #[serde(alias = "backup_type")]
    pub backup_type: String,
    #[serde(alias = "backup_format_version")]
    pub backup_format_version: u32,
    #[serde(alias = "app_version")]
    pub app_version: String,
    #[serde(alias = "created_at")]
    pub created_at: String,
    #[serde(alias = "business_id")]
    pub business_id: String,
    #[serde(alias = "business_name")]
    pub business_name: String,
    #[serde(alias = "checksum_sha256")]
    pub checksum_sha256: String,
    #[serde(alias = "file_size_bytes")]
    pub file_size_bytes: u64,
    #[serde(alias = "payload_size_bytes")]
    pub payload_size_bytes: usize,
    #[serde(alias = "table_counts")]
    pub table_counts: HashMap<String, i64>,
    #[serde(alias = "total_records")]
    pub total_records: i64,
    #[serde(alias = "file_name")]
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    #[serde(alias = "auto_backup_enabled")]
    pub auto_backup_enabled: bool,
    #[serde(alias = "retention_limit")]
    pub retention_limit: usize,
    #[serde(alias = "last_backup_at")]
    pub last_backup_at: Option<String>,
    #[serde(alias = "last_auto_backup_at")]
    pub last_auto_backup_at: Option<String>,
    #[serde(alias = "is_internet_available")]
    pub is_internet_available: bool,
    #[serde(alias = "total_backups_count")]
    pub total_backups_count: usize,
    #[serde(alias = "auto_backups_count")]
    pub auto_backups_count: usize,
    #[serde(alias = "manual_backups_count")]
    pub manual_backups_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupValidationReport {
    #[serde(alias = "is_valid")]
    pub is_valid: bool,
    pub manifest: Option<BackupManifest>,
    #[serde(alias = "integrity_check_passed")]
    pub integrity_check_passed: bool,
    #[serde(alias = "foreign_key_check_passed")]
    pub foreign_key_check_passed: bool,
    #[serde(alias = "schema_tables_passed")]
    pub schema_tables_passed: bool,
    #[serde(alias = "business_invariants_passed")]
    pub business_invariants_passed: bool,
    #[serde(alias = "tables_found")]
    pub tables_found: Vec<String>,
    #[serde(alias = "compatibility_status")]
    pub compatibility_status: String, // "COMPATIBLE" | "INCOMPATIBLE" | "CORRUPTED"
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub success: bool,
    #[serde(alias = "backup_id")]
    pub backup_id: String,
    #[serde(alias = "restored_at")]
    pub restored_at: String,
    #[serde(alias = "table_counts")]
    pub table_counts: HashMap<String, i64>,
    #[serde(alias = "total_records")]
    pub total_records: i64,
    #[serde(alias = "verification_passed")]
    pub verification_passed: bool,
    #[serde(alias = "rolled_back")]
    pub rolled_back: bool,
    pub message: String,
}

pub struct BackupService;

impl BackupService {
    /// Resolves the default backup directory.
    pub fn get_default_backup_dir(db: &DatabaseManager) -> PathBuf {
        if let Some(path) = db.db_path() {
            if let Some(parent) = path.parent() {
                let dir = parent.join("backups");
                let _ = fs::create_dir_all(&dir);
                return dir;
            }
        }
        let dir = std::env::temp_dir().join("merchant_os_backups");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// Retrieves current backup settings stored in `system_metadata`.
    pub fn get_settings(db: &DatabaseManager, backup_dir: &Path) -> Result<BackupSettings, BusinessError> {
        let (auto_enabled, last_backup, last_auto_backup) = db.with_connection(|conn| {
            let auto_enabled: String = conn
                .query_row(
                    "SELECT value FROM system_metadata WHERE key = 'backup_auto_enabled'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| "false".to_string());

            let last_backup: Option<String> = conn
                .query_row(
                    "SELECT value FROM system_metadata WHERE key = 'backup_last_run_at'",
                    [],
                    |r| r.get(0),
                )
                .ok();

            let last_auto_backup: Option<String> = conn
                .query_row(
                    "SELECT value FROM system_metadata WHERE key = 'backup_last_auto_run_at'",
                    [],
                    |r| r.get(0),
                )
                .ok();

            Ok((auto_enabled == "true", last_backup, last_auto_backup))
        })?;

        let backups = Self::list_backups(backup_dir)?;
        let auto_count = backups.iter().filter(|b| b.backup_type == "AUTO").count();
        let manual_count = backups.iter().filter(|b| b.backup_type == "MANUAL").count();

        Ok(BackupSettings {
            auto_backup_enabled: auto_enabled,
            retention_limit: AUTO_BACKUP_RETENTION_LIMIT,
            last_backup_at: last_backup,
            last_auto_backup_at: last_auto_backup,
            is_internet_available: true, // Default query status; overridden by client telemetry
            total_backups_count: backups.len(),
            auto_backups_count: auto_count,
            manual_backups_count: manual_count,
        })
    }

    /// Updates the `backup_auto_enabled` setting in `system_metadata`.
    pub fn update_auto_backup_setting(
        db: &DatabaseManager,
        enabled: bool,
        backup_dir: &Path,
    ) -> Result<BackupSettings, BusinessError> {
        let now = format!("{:?}", std::time::SystemTime::now());
        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO system_metadata (key, value, updated_at) VALUES ('backup_auto_enabled', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = ?2",
                rusqlite::params![if enabled { "true" } else { "false" }, now],
            )?;
            Ok(())
        })?;

        Self::get_settings(db, backup_dir)
    }

    /// Creates a complete, consistent backup package with SHA-256 integrity and table counts.
    pub fn create_backup(
        db: &DatabaseManager,
        identity: &AuthenticatedIdentity,
        backup_type: &str,
        note: Option<&str>,
        target_dir: Option<&Path>,
    ) -> Result<BackupMetadata, BusinessError> {
        // Backend authorization check
        if backup_type == "MANUAL" {
            db.with_connection(|conn| {
                AuthorizationService::authorize(conn, identity, PermissionKey::BackupRestore.as_str())
                    .map_err(|e| BusinessError::AdminAuthorizationRequired(format!("Backup creation denied: {}", e)))
            })?;
        }

        let out_dir = match target_dir {
            Some(p) => p.to_path_buf(),
            None => Self::get_default_backup_dir(db),
        };
        let _ = fs::create_dir_all(&out_dir);

        let temp_snapshot_file = out_dir.join(format!("temp_snap_{}.db", next_audit_token()));

        // 1. Create consistent SQLite snapshot
        db.create_consistent_snapshot(&temp_snapshot_file)?;

        // 2. Read snapshot bytes and compute SHA-256
        let snapshot_bytes = fs::read(&temp_snapshot_file).map_err(|e| {
            let _ = fs::remove_file(&temp_snapshot_file);
            BusinessError::DatabaseError(format!("Failed to read snapshot file: {}", e))
        })?;
        let _ = fs::remove_file(&temp_snapshot_file);

        let mut hasher = Sha256::new();
        hasher.update(&snapshot_bytes);
        let checksum_sha256 = hex::encode(hasher.finalize());

        // 3. Gather table row counts and business info
        let (business_id, business_name, table_counts, total_records) = db.with_connection(|conn| {
            let (b_id, b_name): (String, String) = conn
                .query_row(
                    "SELECT id, name FROM businesses ORDER BY created_at ASC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or_else(|_| ("biz_default".to_string(), "Default Business".to_string()));

            let tables = crate::db::migration::get_all_tables(conn)?;
            let mut counts = HashMap::new();
            let mut total = 0i64;

            for table in tables {
                let count: i64 = conn
                    .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", table), [], |r| r.get(0))
                    .unwrap_or(0);
                counts.insert(table, count);
                total += count;
            }

            Ok((b_id, b_name, counts, total))
        })?;

        let now_iso = format!("{:?}", std::time::SystemTime::now());
        let manifest = BackupManifest {
            backup_format_version: CURRENT_BACKUP_FORMAT_VERSION,
            app_version: CURRENT_APP_VERSION.to_string(),
            created_at: now_iso.clone(),
            backup_type: backup_type.to_string(),
            business_id,
            business_name,
            checksum_sha256: checksum_sha256.clone(),
            table_counts: table_counts.clone(),
            total_records,
            payload_size_bytes: snapshot_bytes.len(),
            note: note.map(|s| s.to_string()),
        };

        // 4. Base64 encode for JSON package
        let b64_snapshot = base64::engine::general_purpose::STANDARD.encode(&snapshot_bytes);
        let package = BackupPackage {
            manifest: manifest.clone(),
            sqlite_snapshot_base64: b64_snapshot,
        };

        let file_token = next_audit_token();
        let file_name = format!(
            "backup_{}_{}_{}.mosbackup",
            backup_type.to_lowercase(),
            manifest.business_id,
            file_token
        );
        let final_path = out_dir.join(&file_name);

        let json_content = serde_json::to_string_pretty(&package)
            .map_err(|e| BusinessError::DatabaseError(format!("Serialization error: {}", e)))?;

        // Write atomically via temporary file
        let temp_write_path = out_dir.join(format!("{}.tmp", file_name));
        fs::write(&temp_write_path, json_content.as_bytes())
            .map_err(|e| BusinessError::DatabaseError(format!("Failed to write backup package: {}", e)))?;
        fs::rename(&temp_write_path, &final_path)
            .map_err(|e| BusinessError::DatabaseError(format!("Failed to finalize backup file: {}", e)))?;

        let file_size = fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);

        // 5. Update system metadata and audit log
        let now = format!("{:?}", std::time::SystemTime::now());
        let _ = db.with_connection(|conn| {
            if backup_type == "MANUAL" {
                conn.execute(
                    "INSERT INTO system_metadata (key, value, updated_at) VALUES ('backup_last_run_at', ?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = ?2",
                    rusqlite::params![now_iso, now],
                )?;

                let audit_id = format!("aud_backup_{}", file_token);
                let details = serde_json::json!({
                    "fileName": file_name,
                    "checksum": checksum_sha256,
                    "totalRecords": total_records,
                    "fileSizeBytes": file_size,
                })
                .to_string();

                conn.execute(
                    "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                     VALUES (?1, ?2, 'BACKUP_MANUAL_CREATE', 'BACKUP', ?3, ?4, ?5)",
                    rusqlite::params![audit_id, identity.user_id(), file_name, details, now],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO system_metadata (key, value, updated_at) VALUES ('backup_last_auto_run_at', ?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = ?2",
                    rusqlite::params![now_iso, now],
                )?;
            }
            Ok(())
        });

        Ok(BackupMetadata {
            id: file_token.to_string(),
            backup_type: backup_type.to_string(),
            backup_format_version: manifest.backup_format_version,
            app_version: manifest.app_version,
            created_at: manifest.created_at,
            business_id: manifest.business_id,
            business_name: manifest.business_name,
            checksum_sha256,
            file_size_bytes: file_size,
            payload_size_bytes: manifest.payload_size_bytes,
            table_counts,
            total_records,
            file_name,
        })
    }

    /// Validates a backup package completely in an isolated sandbox.
    pub fn validate_backup_file(path: &Path) -> Result<BackupValidationReport, BusinessError> {
        let mut errors = Vec::new();

        if !path.exists() {
            return Ok(BackupValidationReport {
                is_valid: false,
                manifest: None,
                integrity_check_passed: false,
                foreign_key_check_passed: false,
                schema_tables_passed: false,
                business_invariants_passed: false,
                tables_found: vec![],
                compatibility_status: "CORRUPTED".to_string(),
                errors: vec![format!("File does not exist: {:?}", path)],
            });
        }

        let content = fs::read_to_string(path).map_err(|e| {
            BusinessError::DatabaseError(format!("Failed to read backup file: {}", e))
        })?;

        let package: BackupPackage = match serde_json::from_str(&content) {
            Ok(pkg) => pkg,
            Err(e) => {
                return Ok(BackupValidationReport {
                    is_valid: false,
                    manifest: None,
                    integrity_check_passed: false,
                    foreign_key_check_passed: false,
                    schema_tables_passed: false,
                    business_invariants_passed: false,
                    tables_found: vec![],
                    compatibility_status: "CORRUPTED".to_string(),
                    errors: vec![format!("Invalid JSON package structure: {}", e)],
                });
            }
        };

        // Version compatibility
        if package.manifest.backup_format_version > CURRENT_BACKUP_FORMAT_VERSION {
            errors.push(format!(
                "Incompatible backup format version {}. Current supported version is {}.",
                package.manifest.backup_format_version, CURRENT_BACKUP_FORMAT_VERSION
            ));
            return Ok(BackupValidationReport {
                is_valid: false,
                manifest: Some(package.manifest),
                integrity_check_passed: false,
                foreign_key_check_passed: false,
                schema_tables_passed: false,
                business_invariants_passed: false,
                tables_found: vec![],
                compatibility_status: "INCOMPATIBLE".to_string(),
                errors,
            });
        }

        // Decode Base64
        let raw_bytes = match base64::engine::general_purpose::STANDARD.decode(&package.sqlite_snapshot_base64) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("Corrupted base64 payload: {}", e));
                return Ok(BackupValidationReport {
                    is_valid: false,
                    manifest: Some(package.manifest),
                    integrity_check_passed: false,
                    foreign_key_check_passed: false,
                    schema_tables_passed: false,
                    business_invariants_passed: false,
                    tables_found: vec![],
                    compatibility_status: "CORRUPTED".to_string(),
                    errors,
                });
            }
        };

        // Verify SHA-256 Checksum
        let mut hasher = Sha256::new();
        hasher.update(&raw_bytes);
        let computed_checksum = hex::encode(hasher.finalize());

        if computed_checksum != package.manifest.checksum_sha256 {
            errors.push(format!(
                "SHA-256 checksum mismatch! Expected: {}, Computed: {}",
                package.manifest.checksum_sha256, computed_checksum
            ));
            return Ok(BackupValidationReport {
                is_valid: false,
                manifest: Some(package.manifest),
                integrity_check_passed: false,
                foreign_key_check_passed: false,
                schema_tables_passed: false,
                business_invariants_passed: false,
                tables_found: vec![],
                compatibility_status: "CORRUPTED".to_string(),
                errors,
            });
        }

        // Write to isolated temporary database file for pragma checks
        let temp_dir = std::env::temp_dir();
        let temp_check_path = temp_dir.join(format!("val_sandbox_{}.db", next_audit_token()));
        if let Err(e) = fs::write(&temp_check_path, &raw_bytes) {
            errors.push(format!("Failed to write sandbox database: {}", e));
            return Ok(BackupValidationReport {
                is_valid: false,
                manifest: Some(package.manifest),
                integrity_check_passed: false,
                foreign_key_check_passed: false,
                schema_tables_passed: false,
                business_invariants_passed: false,
                tables_found: vec![],
                compatibility_status: "CORRUPTED".to_string(),
                errors,
            });
        }

        let isolated_conn = Connection::open(&temp_check_path);
        let isolated_conn = match isolated_conn {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&temp_check_path);
                errors.push(format!("SQLite failed to open candidate database: {}", e));
                return Ok(BackupValidationReport {
                    is_valid: false,
                    manifest: Some(package.manifest),
                    integrity_check_passed: false,
                    foreign_key_check_passed: false,
                    schema_tables_passed: false,
                    business_invariants_passed: false,
                    tables_found: vec![],
                    compatibility_status: "CORRUPTED".to_string(),
                    errors,
                });
            }
        };

        // SQLite PRAGMA integrity_check
        let integrity: String = isolated_conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap_or_else(|e| format!("error: {}", e));
        let integrity_passed = integrity == "ok";
        if !integrity_passed {
            errors.push(format!("SQLite integrity check failed: {}", integrity));
        }

        // SQLite PRAGMA foreign_key_check
        let fk_violations = isolated_conn
            .prepare("PRAGMA foreign_key_check")
            .and_then(|mut s| s.query_map([], |_| Ok(())).map(|rows| rows.count()))
            .unwrap_or(1);
        let fk_passed = fk_violations == 0;
        if !fk_passed {
            errors.push(format!("Foreign key check failed: {} violations detected", fk_violations));
        }

        // Check required tables
        let tables_found = crate::db::migration::get_all_tables(&isolated_conn).unwrap_or_default();
        let schema_passed = tables_found.contains(&"users".to_string())
            && tables_found.contains(&"businesses".to_string())
            && tables_found.contains(&"products".to_string());
        if !schema_passed {
            errors.push("Missing foundational business tables (users, businesses, products)".to_string());
        }

        // Invariant checks
        let admin_count: i64 = isolated_conn
            .query_row("SELECT COUNT(*) FROM users WHERE role = 'ADMIN' AND is_active = 1", [], |r| r.get(0))
            .unwrap_or(0);
        let invariants_passed = admin_count > 0;
        if !invariants_passed {
            errors.push("Restored database does not contain any active ADMIN user".to_string());
        }

        drop(isolated_conn);
        let _ = fs::remove_file(&temp_check_path);

        let is_valid = errors.is_empty();
        let status = if is_valid {
            "COMPATIBLE".to_string()
        } else {
            "CORRUPTED".to_string()
        };

        Ok(BackupValidationReport {
            is_valid,
            manifest: Some(package.manifest),
            integrity_check_passed: integrity_passed,
            foreign_key_check_passed: fk_passed,
            schema_tables_passed: schema_passed,
            business_invariants_passed: invariants_passed,
            tables_found,
            compatibility_status: status,
            errors,
        })
    }

    /// Restores a backup file into the active database with durable rollback protection.
    pub fn restore_backup_file(
        db: &DatabaseManager,
        identity: &AuthenticatedIdentity,
        file_path: &Path,
    ) -> Result<RestoreReport, BusinessError> {
        // Backend authorization enforcement: RESTORE IS STRICTLY ADMIN-ONLY
        if !identity.is_admin() {
            return Err(BusinessError::AdminAuthorizationRequired(
                "Restore operation requires Admin privileges".to_string(),
            ));
        }

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let token = next_audit_token();
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Audit log: RESTORE_ATTEMPT
        let _ = db.with_connection(|conn| {
            let audit_id = format!("aud_rst_att_{}", token);
            conn.execute(
                "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                 VALUES (?1, ?2, 'RESTORE_ATTEMPT', 'BACKUP', ?3, ?4, ?5)",
                rusqlite::params![
                    audit_id,
                    identity.user_id(),
                    file_name,
                    serde_json::json!({ "filePath": file_path.to_string_lossy() }).to_string(),
                    now
                ],
            )?;
            Ok(())
        });

        // 2. Pre-validate candidate completely in sandbox
        let validation = Self::validate_backup_file(file_path)?;
        if !validation.is_valid {
            let err_msg = validation.errors.join("; ");
            let _ = db.with_connection(|conn| {
                let audit_id = format!("aud_rst_fail_{}", token);
                conn.execute(
                    "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                     VALUES (?1, ?2, 'RESTORE_FAILURE', 'BACKUP', ?3, ?4, ?5)",
                    rusqlite::params![
                        audit_id,
                        identity.user_id(),
                        file_name,
                        serde_json::json!({ "reason": err_msg, "rolledBack": false }).to_string(),
                        format!("{:?}", std::time::SystemTime::now())
                    ],
                )?;
                Ok(())
            });
            return Err(BusinessError::DatabaseError(format!(
                "Pre-restore validation failed: {}",
                err_msg
            )));
        }

        // 3. Extract candidate snapshot bytes to temporary file for the database manager switch
        let content = fs::read_to_string(file_path).map_err(|e| {
            BusinessError::DatabaseError(format!("Failed to read backup package: {}", e))
        })?;
        let package: BackupPackage = serde_json::from_str(&content).map_err(|e| {
            BusinessError::DatabaseError(format!("Package JSON parse failed: {}", e))
        })?;
        let raw_bytes = base64::engine::general_purpose::STANDARD
            .decode(&package.sqlite_snapshot_base64)
            .map_err(|e| BusinessError::DatabaseError(format!("Base64 decode failed: {}", e)))?;

        let temp_dir = std::env::temp_dir();
        let candidate_path = temp_dir.join(format!("candidate_restore_{}.db", token));
        fs::write(&candidate_path, &raw_bytes).map_err(|e| {
            BusinessError::DatabaseError(format!("Failed to write candidate database: {}", e))
        })?;

        // 4. Perform atomic switch with durable rollback guarantee
        let restore_res = db.restore_from_snapshot_file(&candidate_path);
        let _ = fs::remove_file(&candidate_path);

        if let Err(e) = restore_res {
            let _ = db.with_connection(|conn| {
                let audit_id = format!("aud_rst_fail_{}", token);
                conn.execute(
                    "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                     VALUES (?1, ?2, 'RESTORE_FAILURE', 'BACKUP', ?3, ?4, ?5)",
                    rusqlite::params![
                        audit_id,
                        identity.user_id(),
                        file_name,
                        serde_json::json!({ "reason": e.to_string(), "rolledBack": true }).to_string(),
                        format!("{:?}", std::time::SystemTime::now())
                    ],
                )?;
                Ok(())
            });
            return Err(e);
        }

        // 5. Gather post-restore stats and record RESTORE_SUCCESS
        let (table_counts, total_records) = db.with_connection(|conn| {
            let tables = crate::db::migration::get_all_tables(conn)?;
            let mut counts = HashMap::new();
            let mut total = 0i64;
            for table in tables {
                let count: i64 = conn
                    .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", table), [], |r| r.get(0))
                    .unwrap_or(0);
                counts.insert(table, count);
                total += count;
            }

            // Ensure RESTORE_ATTEMPT and RESTORE_SUCCESS are recorded in the active restored database
            let attempt_id = format!("aud_rst_att_{}", token);
            conn.execute(
                "INSERT OR REPLACE INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                 VALUES (?1, ?2, 'RESTORE_ATTEMPT', 'BACKUP', ?3, ?4, ?5)",
                rusqlite::params![
                    attempt_id,
                    identity.user_id(),
                    file_name,
                    serde_json::json!({ "filePath": file_path.to_string_lossy() }).to_string(),
                    now
                ],
            )?;

            let audit_id = format!("aud_rst_ok_{}", token);
            let success_details = serde_json::json!({
                "fileName": file_name,
                "totalRecords": total,
                "manifestChecksum": package.manifest.checksum_sha256,
            })
            .to_string();

            conn.execute(
                "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                 VALUES (?1, ?2, 'RESTORE_SUCCESS', 'BACKUP', ?3, ?4, ?5)",
                rusqlite::params![
                    audit_id,
                    identity.user_id(),
                    file_name,
                    success_details,
                    format!("{:?}", std::time::SystemTime::now())
                ],
            )?;

            Ok((counts, total))
        })?;

        Ok(RestoreReport {
            success: true,
            backup_id: token.to_string(),
            restored_at: format!("{:?}", std::time::SystemTime::now()),
            table_counts,
            total_records,
            verification_passed: true,
            rolled_back: false,
            message: "Database restored and verified successfully.".to_string(),
        })
    }

    /// Enforces auto-backup retention policy (strictly keeping the newest 7 auto backups).
    /// Manual backups are never deleted during this cleanup.
    pub fn apply_auto_retention(backup_dir: &Path, max_keep: usize) -> Result<Option<String>, BusinessError> {
        let mut auto_files = Vec::new();

        if let Ok(entries) = fs::read_dir(backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("mosbackup") {
                    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    if file_name.starts_with("backup_auto_") {
                        if let Ok(meta) = entry.metadata() {
                            let created = meta.created().unwrap_or(meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
                            auto_files.push((path, created, file_name));
                        }
                    }
                }
            }
        }

        // Sort ascending by creation time (oldest first)
        auto_files.sort_by_key(|item| item.1);

        let mut removed_name = None;
        if auto_files.len() > max_keep {
            let excess = auto_files.len() - max_keep;
            for i in 0..excess {
                let (path, _, name) = &auto_files[i];
                let _ = fs::remove_file(path);
                if removed_name.is_none() {
                    removed_name = Some(name.clone());
                }
            }
        }

        Ok(removed_name)
    }

    /// Lists all backups in the specified directory, sorted descending by creation time.
    pub fn list_backups(backup_dir: &Path) -> Result<Vec<BackupMetadata>, BusinessError> {
        let mut list = Vec::new();

        if let Ok(entries) = fs::read_dir(backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("mosbackup") {
                    let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();

                    // Read JSON manifest
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(pkg) = serde_json::from_str::<BackupPackage>(&content) {
                            let m = pkg.manifest;
                            let id = file_name.trim_end_matches(".mosbackup").to_string();
                            list.push(BackupMetadata {
                                id,
                                backup_type: m.backup_type,
                                backup_format_version: m.backup_format_version,
                                app_version: m.app_version,
                                created_at: m.created_at,
                                business_id: m.business_id,
                                business_name: m.business_name,
                                checksum_sha256: m.checksum_sha256,
                                file_size_bytes: file_size,
                                payload_size_bytes: m.payload_size_bytes,
                                table_counts: m.table_counts,
                                total_records: m.total_records,
                                file_name,
                            });
                        }
                    }
                }
            }
        }

        // Sort descending (newest first)
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(list)
    }

    /// Triggers automatic backup if eligible (auto-backup enabled AND internet available).
    pub fn trigger_auto_backup(
        db: &DatabaseManager,
        identity: &AuthenticatedIdentity,
        internet_available: bool,
        target_dir: Option<&Path>,
    ) -> Result<Option<BackupMetadata>, BusinessError> {
        let backup_dir = match target_dir {
            Some(p) => p.to_path_buf(),
            None => Self::get_default_backup_dir(db),
        };

        let settings = Self::get_settings(db, &backup_dir)?;
        if !settings.auto_backup_enabled {
            return Ok(None);
        }

        // Network is a scheduler/policy condition (Guardrail 4)
        if !internet_available {
            return Ok(None);
        }

        // Create automatic backup
        let meta = Self::create_backup(db, identity, "AUTO", None, Some(&backup_dir))?;

        // Apply 7-backup retention limit
        let _ = Self::apply_auto_retention(&backup_dir, AUTO_BACKUP_RETENTION_LIMIT);

        Ok(Some(meta))
    }
}
