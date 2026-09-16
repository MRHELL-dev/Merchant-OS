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
use std::path::PathBuf;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Determine local persistent app database path
            let db_path = match app.path().app_data_dir() {
                Ok(path) => path.join("merchant_os.db"),
                Err(_) => PathBuf::from("merchant_os.db"),
            };

            let db_manager = match DatabaseManager::open(&db_path) {
                Ok(manager) => {
                    println!("[Merchant OS] Database initialized at: {:?}", db_path);
                    manager
                }
                Err(e) => {
                    eprintln!(
                        "[Merchant OS] Failed to initialize file database ({}), falling back to in-memory: {:?}",
                        e, db_path
                    );
                    DatabaseManager::open_in_memory()
                        .expect("Failed to initialize even in-memory SQLite database")
                }
            };

            let auth_session = AuthSession::default();
            // Automatically initialize local active admin session if one exists in database
            let _ = db_manager.with_connection(|conn| {
                if let Ok(admin) = conn.query_row(
                    "SELECT id, username, role FROM users WHERE role = 'ADMIN' AND is_active = 1 LIMIT 1",
                    [],
                    |r| Ok(crate::auth::AuthenticatedIdentity::new(r.get(0)?, r.get(1)?, crate::auth::Role::Admin)),
                ) {
                    auth_session.set_identity(Some(admin));
                }
                Ok(())
            });

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
