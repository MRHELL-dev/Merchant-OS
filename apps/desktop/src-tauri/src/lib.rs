pub mod ai;
pub mod auth;
pub mod backup;
pub mod commands;
pub mod db;
pub mod engine;

use commands::{
    AuthSession, PreparedCustomerPaymentCache, PreparedOrderConversionCache, PreparedPurchaseCache,
    PreparedReturnCache, PreparedSaleCache, PreparedStockCorrectionCache,
};
use db::DatabaseManager;
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Resolves the local SQLite database path for Merchant OS.
///
/// Resolution Priority:
/// 1. `MERCHANT_OS_DB_PATH` environment variable, if set and non-empty (for testing/validation/custom storage).
/// 2. DEBUG build: `<app_data_dir>/dev_merchant_os.db`
/// 3. RELEASE build: `<app_data_dir>/merchant_os.db`
pub fn resolve_db_path(app_data_dir: Option<PathBuf>) -> PathBuf {
    if let Ok(custom_path) = std::env::var("MERCHANT_OS_DB_PATH") {
        let trimmed = custom_path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    let base_dir = app_data_dir.unwrap_or_else(|| PathBuf::from("."));

    #[cfg(debug_assertions)]
    {
        base_dir.join("dev_merchant_os.db")
    }

    #[cfg(not(debug_assertions))]
    {
        base_dir.join("merchant_os.db")
    }
}

/// Initializes the local database manager.
///
/// In RELEASE mode (`is_release = true`):
/// If opening or migrating the persistent database fails, logs the exact error
/// and returns an error (fail-fast, preventing silent data loss in volatile memory).
///
/// In DEBUG mode (`is_release = false`):
/// If opening fails, logs the error and falls back to an in-memory SQLite database.
pub fn open_app_database_with_mode(
    db_path: &Path,
    is_release: bool,
) -> Result<DatabaseManager, String> {
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    match DatabaseManager::open(db_path) {
        Ok(manager) => {
            println!("[Merchant OS] Database initialized at: {:?}", db_path);
            Ok(manager)
        }
        Err(e) => {
            eprintln!(
                "[Merchant OS] FATAL: Failed to initialize database at {:?}: {}",
                db_path, e
            );
            if is_release {
                Err(format!(
                    "FATAL: Failed to open Merchant OS database at {:?}: {}. Application aborted to prevent data loss.",
                    db_path, e
                ))
            } else {
                eprintln!(
                    "[Merchant OS] DEBUG mode: Falling back to in-memory SQLite database."
                );
                DatabaseManager::open_in_memory().map_err(|mem_err| {
                    format!("Failed to initialize even in-memory SQLite database: {}", mem_err)
                })
            }
        }
    }
}

pub fn open_app_database(db_path: &Path) -> Result<DatabaseManager, String> {
    open_app_database_with_mode(db_path, !cfg!(debug_assertions))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Determine local persistent app database path
            let db_path = resolve_db_path(app.path().app_data_dir().ok());

            let db_manager = match open_app_database(&db_path) {
                Ok(manager) => manager,
                Err(err_msg) => {
                    panic!("{}", err_msg);
                }
            };

            let auth_session = AuthSession::default();

            let purchase_cache = PreparedPurchaseCache::default();
            let sale_cache = PreparedSaleCache::default();
            let customer_payment_cache = PreparedCustomerPaymentCache::default();
            let order_conversion_cache = PreparedOrderConversionCache::default();
            let return_cache = PreparedReturnCache::default();
            let stock_correction_cache = PreparedStockCorrectionCache::default();

            app.manage(db_manager);
            app.manage(auth_session);
            app.manage(purchase_cache);
            app.manage(sale_cache);
            app.manage(customer_payment_cache);
            app.manage(order_conversion_cache);
            app.manage(return_cache);
            app.manage(stock_correction_cache);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // BUILD 15 First-Run Authentication & Admin Onboarding Commands
            commands::get_auth_state,
            commands::create_initial_admin,
            commands::login,
            commands::logout,
            commands::get_system_status,
            commands::get_purchase_form_data,
            commands::create_supplier_for_purchase,
            commands::resolve_barcode_for_purchase,
            commands::prepare_purchase,
            commands::confirm_purchase,
            // BUILD 07 Sales / POS Commands
            commands::get_sales_form_data,
            commands::create_customer_for_sale,
            commands::resolve_barcode_for_sale,
            commands::prepare_sale,
            commands::confirm_sale,
            // BUILD 08 Customer Credits & Payments Commands
            commands::get_customer_credits_summary,
            commands::get_customer_ledger_history,
            commands::prepare_customer_payment,
            commands::confirm_customer_payment,
            // BUILD 09 Customer Orders Commands
            commands::get_customer_orders_form_data,
            commands::get_customer_orders_summary,
            commands::get_customer_order_detail,
            commands::create_customer_order,
            commands::update_customer_order,
            commands::cancel_customer_order,
            commands::prepare_order_conversion,
            commands::confirm_order_conversion,
            // BUILD 10 Returns & Stock Reversal Commands
            commands::get_returns_form_data,
            commands::get_returns_summary,
            commands::get_return_detail,
            commands::prepare_customer_return,
            commands::confirm_customer_return,
            commands::prepare_supplier_return,
            commands::confirm_supplier_return,
            // BUILD 11 Stock Corrections Commands
            commands::get_stock_corrections_form_data,
            commands::get_stock_corrections_summary,
            commands::prepare_stock_correction,
            commands::confirm_stock_correction,
            // BUILD 12 Offline Sync & Backup Commands
            commands::get_backup_status,
            commands::toggle_auto_backup,
            commands::create_manual_backup,
            commands::trigger_auto_backup,
            commands::validate_backup,
            commands::restore_backup,
            // BUILD 13 AI & Voice Intelligence Commands
            commands::query_ai,
            commands::get_ai_status,
            commands::get_demand_recommendations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_db_path_with_env_override() {
        let override_path = "C:\\MerchantOS_Validation\\test_merchant_os.db";
        std::env::set_var("MERCHANT_OS_DB_PATH", override_path);
        let resolved = resolve_db_path(Some(PathBuf::from("C:\\AppData\\Roaming\\com.merchantos.desktop")));
        assert_eq!(resolved, PathBuf::from(override_path));
        std::env::remove_var("MERCHANT_OS_DB_PATH");
    }

    #[test]
    fn test_resolve_db_path_ignores_empty_env_override() {
        std::env::set_var("MERCHANT_OS_DB_PATH", "   ");
        let base = PathBuf::from("C:\\AppData\\Roaming\\com.merchantos.desktop");
        let resolved = resolve_db_path(Some(base.clone()));
        #[cfg(debug_assertions)]
        assert_eq!(resolved, base.join("dev_merchant_os.db"));
        #[cfg(not(debug_assertions))]
        assert_eq!(resolved, base.join("merchant_os.db"));
        std::env::remove_var("MERCHANT_OS_DB_PATH");
    }

    #[test]
    fn test_resolve_db_path_default_without_env() {
        std::env::remove_var("MERCHANT_OS_DB_PATH");
        let base = PathBuf::from("C:\\AppData\\Roaming\\com.merchantos.desktop");
        let resolved = resolve_db_path(Some(base.clone()));
        #[cfg(debug_assertions)]
        assert_eq!(resolved, base.join("dev_merchant_os.db"));
        #[cfg(not(debug_assertions))]
        assert_eq!(resolved, base.join("merchant_os.db"));
    }

    #[test]
    fn test_release_failure_mode_does_not_fallback_to_in_memory() {
        let invalid_path = std::env::temp_dir();
        let result = open_app_database_with_mode(&invalid_path, true);
        assert!(result.is_err(), "Release mode must return Err and fail fast on database failure");
        let err_msg = result.err().unwrap();
        assert!(err_msg.contains("FATAL: Failed to open Merchant OS database"));
        assert!(err_msg.contains("Application aborted to prevent data loss"));
    }

    #[test]
    fn test_debug_failure_mode_falls_back_to_in_memory() {
        let invalid_path = std::env::temp_dir();
        let result = open_app_database_with_mode(&invalid_path, false);
        assert!(result.is_ok(), "Debug mode should fall back to in-memory SQLite if required");
        let manager = result.ok().unwrap();
        assert_eq!(manager.db_path(), None, "In-memory database must have db_path == None");
    }
}
