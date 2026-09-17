use crate::auth::{AuthenticatedIdentity, AuthorizationService, PermissionKey};
use crate::db::math::calculate_line_total;
use crate::db::DatabaseManager;
use crate::engine::{
    BusinessEngine, ConfirmPurchaseRequest, ConfirmSaleRequest, ConvertOrderToSaleRequest,
    EngineError, Prepared, PreparedCustomerPayment, PreparedOrderConversion, PreparedPurchase,
    PreparedSale, PreparedStockCorrection, PurchaseItemRequest, RecordCustomerPaymentRequest,
    RecordStockCorrectionRequest, SaleItemRequest, TransactionEngine,
};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// ================================================================================================
// 1. SESSION & CACHE INFRASTRUCTURE
// ================================================================================================

/// Thread-safe active authentication session for the local Tauri application.
/// Derived strictly on the backend — never accepted from client payload.
pub struct AuthSession {
    pub current_user: Mutex<Option<AuthenticatedIdentity>>,
}

impl Default for AuthSession {
    fn default() -> Self {
        Self {
            current_user: Mutex::new(None),
        }
    }
}

impl AuthSession {
    pub fn get_identity(&self) -> Result<AuthenticatedIdentity, String> {
        let guard = self.current_user.lock().map_err(|e| e.to_string())?;
        guard
            .clone()
            .ok_or_else(|| "Unauthenticated: No active user session".to_string())
    }

    pub fn set_identity(&self, identity: Option<AuthenticatedIdentity>) {
        if let Ok(mut guard) = self.current_user.lock() {
            *guard = identity;
        }
    }

    /// Explicitly terminates active session and purges any prepared purchases owned by this user.
    pub fn logout(&self, cache: &PreparedPurchaseCache) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                cache.clear_for_user(user.user_id());
            }
        }
    }

    /// Explicitly terminates active session and purges any prepared sales owned by this user.
    pub fn logout_sales(&self, cache: &PreparedSaleCache) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                cache.clear_for_user(user.user_id());
            }
        }
    }

    /// Explicitly terminates active session and purges any prepared customer payments owned by this user.
    pub fn logout_customer_payments(&self, cache: &PreparedCustomerPaymentCache) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                cache.clear_for_user(user.user_id());
            }
        }
    }

    /// Explicitly terminates active session and purges any prepared order conversions owned by this user.
    pub fn logout_order_conversions(&self, cache: &PreparedOrderConversionCache) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                cache.clear_for_user(user.user_id());
            }
        }
    }

    /// Explicitly terminates active session and purges any prepared returns owned by this user.
    pub fn logout_returns(&self, cache: &PreparedReturnCache) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                cache.clear_for_user(user.user_id());
            }
        }
    }

    /// Explicitly terminates active session and purges any prepared stock corrections owned by this user.
    pub fn logout_stock_corrections(&self, cache: &PreparedStockCorrectionCache) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                cache.clear_for_user(user.user_id());
            }
        }
    }

    /// Explicitly terminates active session and purges all prepared purchases, sales, customer payments, order conversions, returns, and stock corrections owned by this user.
    pub fn logout_all(
        &self,
        purchase_cache: &PreparedPurchaseCache,
        sale_cache: &PreparedSaleCache,
        payment_cache: &PreparedCustomerPaymentCache,
        order_conversion_cache: &PreparedOrderConversionCache,
        return_cache: &PreparedReturnCache,
        stock_correction_cache: &PreparedStockCorrectionCache,
    ) {
        if let Ok(mut guard) = self.current_user.lock() {
            if let Some(user) = guard.take() {
                purchase_cache.clear_for_user(user.user_id());
                sale_cache.clear_for_user(user.user_id());
                payment_cache.clear_for_user(user.user_id());
                order_conversion_cache.clear_for_user(user.user_id());
                return_cache.clear_for_user(user.user_id());
                stock_correction_cache.clear_for_user(user.user_id());
            }
        }
    }
}

/// A prepared purchase held in memory awaiting explicit merchant confirmation.
/// Bound strictly to the session user that created it.
#[derive(Clone)]
pub struct CachedPreparedPurchase {
    pub owner_user_id: String,
    pub prepared: Prepared<PreparedPurchase>,
    pub supplier_id: String,
    pub product_ids: Vec<String>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
}

/// Server-side temporary cache for prepared purchases.
/// Bound strictly to session ownership: keyed by (owner_user_id, preparation_token).
/// Single-use lifecycle: consumed on confirmation; evicted on logout.
pub struct PreparedPurchaseCache {
    pub entries: Mutex<HashMap<(String, String), CachedPreparedPurchase>>,
}

impl Default for PreparedPurchaseCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl PreparedPurchaseCache {
    pub fn clear_for_user(&self, user_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(uid, _), _| uid != user_id);
        }
    }
}

/// A prepared sale held in memory awaiting explicit merchant confirmation.
/// Bound strictly to the session user that created it.
#[derive(Clone)]
pub struct CachedPreparedSale {
    pub owner_user_id: String,
    pub prepared: Prepared<PreparedSale>,
    pub customer_id: Option<String>,
    pub product_quantities: Vec<(String, i64)>, // (product_id, quantity) for stock verification
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub settlement_mode: String, // "PAID" | "CREDIT"
    pub payment_method: Option<String>,
}

/// Server-side temporary cache for prepared sales.
/// Bound strictly to session ownership: keyed by (owner_user_id, preparation_token).
/// Single-use lifecycle: consumed on confirmation; evicted on logout.
pub struct PreparedSaleCache {
    pub entries: Mutex<HashMap<(String, String), CachedPreparedSale>>,
}

impl Default for PreparedSaleCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl PreparedSaleCache {
    pub fn clear_for_user(&self, user_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(uid, _), _| uid != user_id);
        }
    }
}

/// A prepared customer payment held in memory awaiting explicit merchant confirmation.
/// Bound strictly to the session user that created it.
#[derive(Clone)]
pub struct CachedPreparedCustomerPayment {
    pub owner_user_id: String,
    pub prepared: Prepared<PreparedCustomerPayment>,
    pub customer_id: String,
    pub customer_name: String,
    pub customer_phone: Option<String>,
    pub amount_cents: i64,
    pub payment_method: String,
    pub notes: Option<String>,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
}

/// Server-side temporary cache for prepared customer payments.
/// Bound strictly to session ownership: keyed by (owner_user_id, preparation_token).
/// Single-use lifecycle: consumed on confirmation; evicted on logout.
pub struct PreparedCustomerPaymentCache {
    pub entries: Mutex<HashMap<(String, String), CachedPreparedCustomerPayment>>,
}

impl Default for PreparedCustomerPaymentCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl PreparedCustomerPaymentCache {
    pub fn clear_for_user(&self, user_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(uid, _), _| uid != user_id);
        }
    }
}

/// A prepared order conversion held in memory awaiting explicit merchant confirmation.
/// Bound strictly to the session user that created it.
#[derive(Clone)]
pub struct CachedPreparedOrderConversion {
    pub owner_user_id: String,
    pub prepared: Prepared<PreparedOrderConversion>,
    pub order_id: String,
    pub order_number: String,
    pub customer_id: Option<String>,
    pub product_quantities: Vec<(String, i64)>, // (product_id, quantity) for stock verification
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub settlement_mode: String, // "PAID" | "CREDIT"
    pub payment_method: Option<String>,
}

/// Server-side temporary cache for prepared order conversions.
/// Bound strictly to session ownership: keyed by (owner_user_id, preparation_token).
/// Single-use lifecycle: consumed on confirmation; evicted on logout.
pub struct PreparedOrderConversionCache {
    pub entries: Mutex<HashMap<(String, String), CachedPreparedOrderConversion>>,
}

impl Default for PreparedOrderConversionCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl PreparedOrderConversionCache {
    pub fn clear_for_user(&self, user_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(uid, _), _| uid != user_id);
        }
    }
}

/// A prepared return held in memory awaiting explicit merchant confirmation.
/// Supports both CUSTOMER_RETURN and SUPPLIER_RETURN.
#[derive(Clone)]
pub enum CachedPreparedReturnPayload {
    Customer {
        customer_id: Option<String>,
        reference_sale_id: Option<String>,
        items: Vec<(String, i64)>, // (product_id, quantity)
        reason: String,
        refund_payment_method: Option<String>,
    },
    Supplier {
        supplier_id: Option<String>,
        reference_purchase_id: Option<String>,
        items: Vec<(String, i64)>, // (product_id, quantity)
        reason: String,
    },
}

#[derive(Clone)]
pub struct CachedPreparedReturn {
    pub owner_user_id: String,
    pub return_id: String,
    pub return_type: String, // "CUSTOMER_RETURN" | "SUPPLIER_RETURN"
    pub payload: CachedPreparedReturnPayload,
}

/// Server-side temporary cache for prepared returns.
/// Bound strictly to session ownership: keyed by (owner_user_id, preparation_token).
/// Single-use lifecycle: consumed on confirmation; evicted on logout.
pub struct PreparedReturnCache {
    pub entries: Mutex<HashMap<(String, String), CachedPreparedReturn>>,
}

impl Default for PreparedReturnCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl PreparedReturnCache {
    pub fn clear_for_user(&self, user_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(uid, _), _| uid != user_id);
        }
    }
}

/// A prepared stock correction held in memory awaiting explicit merchant confirmation.
/// Bound strictly to the session user (admin) that created it.
#[derive(Clone)]
pub struct CachedPreparedStockCorrection {
    pub owner_user_id: String,
    pub correction_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity_change: i64, // signed delta in millie-units
    pub quantity_before: i64,
    pub quantity_after: i64,
    pub reason: String,
    pub note: String,
    pub admin_user_id: String,
    pub admin_username: String,
}

/// Server-side temporary cache for prepared stock corrections.
/// Bound strictly to session ownership: keyed by (owner_user_id, preparation_token).
/// Single-use lifecycle: consumed on confirmation; evicted on logout.
pub struct PreparedStockCorrectionCache {
    pub entries: Mutex<HashMap<(String, String), CachedPreparedStockCorrection>>,
}

impl Default for PreparedStockCorrectionCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl PreparedStockCorrectionCache {
    pub fn clear_for_user(&self, user_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(uid, _), _| uid != user_id);
        }
    }
}



/// Generates a cryptographically secure, unguessable, 256-bit random preparation token.
fn generate_preparation_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let hex = bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    format!("prep_{}", hex)
}

/// Computes (year, month, day) from system UTC time without external dependencies.
fn format_order_date() -> (u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut days = (secs / 86400) as i64;
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m, d)
}

/// Generates a human-readable, collision-safe order number strictly on the backend.
/// Uses date + 24-bit CSPRNG entropy with an in-transaction retry loop (up to 5 attempts).
pub fn generate_order_number(conn: &rusqlite::Connection) -> Result<String, EngineError> {
    for _ in 0..5 {
        let (y, m, d) = format_order_date();
        let mut rand_bytes = [0u8; 3];
        OsRng.fill_bytes(&mut rand_bytes);
        let hex_suffix = format!("{:02X}{:02X}{:02X}", rand_bytes[0], rand_bytes[1], rand_bytes[2]);
        let candidate = format!("ORD-{:04}{:02}{:02}-{}", y, m, d, hex_suffix);

        let exists: bool = match conn.query_row(
            "SELECT 1 FROM customer_orders WHERE order_number = ?1",
            params![candidate],
            |_| Ok(()),
        ) {
            Ok(_) => true,
            Err(rusqlite::Error::QueryReturnedNoRows) => false,
            Err(e) => return Err(EngineError::DatabaseError(e.to_string())),
        };

        if !exists {
            return Ok(candidate);
        }
    }
    Err(EngineError::DatabaseError(
        "Failed to generate unique order number after retries".to_string(),
    ))
}

/// Generates a human-readable, collision-safe return number strictly on the backend.
/// Uses date + 24-bit CSPRNG entropy with an in-transaction retry loop (up to 5 attempts).
/// Prefix is "RET-" for customer returns and "SR-" for supplier returns.
pub fn generate_return_number(conn: &rusqlite::Connection, return_type: &str) -> Result<String, EngineError> {
    let prefix = match return_type {
        "SUPPLIER_RETURN" => "SR",
        _ => "RET",
    };
    for _ in 0..5 {
        let (y, m, d) = format_order_date();
        let mut rand_bytes = [0u8; 3];
        OsRng.fill_bytes(&mut rand_bytes);
        let hex_suffix = format!("{:02X}{:02X}{:02X}", rand_bytes[0], rand_bytes[1], rand_bytes[2]);
        let candidate = format!("{}-{:04}{:02}{:02}-{}", prefix, y, m, d, hex_suffix);

        let exists: bool = match conn.query_row(
            "SELECT 1 FROM returns WHERE return_number = ?1",
            params![candidate],
            |_| Ok(()),
        ) {
            Ok(_) => true,
            Err(rusqlite::Error::QueryReturnedNoRows) => false,
            Err(e) => return Err(EngineError::DatabaseError(e.to_string())),
        };

        if !exists {
            return Ok(candidate);
        }
    }
    Err(EngineError::DatabaseError(
        "Failed to generate unique return number after retries".to_string(),
    ))
}


// ================================================================================================
// 2. DATA TRANSFER OBJECTS (SAFE FRONTEND CONTRACTS)
// ================================================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub status: String,
    pub database: String,
    pub sqlite_version: String,
    pub internet: String,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SupplierSummary {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub current_outstanding_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProductForPurchase {
    pub id: String,
    pub name: String,
    pub product_type: String,
    pub unit: String,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
    pub current_quantity: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseFormData {
    pub suppliers: Vec<SupplierSummary>,
    pub products: Vec<ProductForPurchase>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateSupplierForPurchaseRequest {
    pub name: String,
    pub phone: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseItemIpcInput {
    pub product_id: String,
    pub quantity: i64, // millie-units (scale 1000)
    pub unit_cost_cents: i64, // actual buying price in cents
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparePurchaseIpcInput {
    pub supplier_id: String,
    pub items: Vec<PurchaseItemIpcInput>,
    pub paid_amount_cents: i64,
    pub payment_method: Option<String>,
    pub purchase_date: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPurchaseItemQuote {
    pub product_id: String,
    pub product_name: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_cost_cents: i64,
    pub line_total_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPurchaseQuote {
    pub preparation_token: String,
    pub purchase_id: String,
    pub purchase_number: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub items: Vec<PreparedPurchaseItemQuote>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub payment_status: String,
    pub payment_method: Option<String>,
    pub purchase_date: String,
    pub prepared_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPurchaseIpcInput {
    pub preparation_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseReceiptItem {
    pub product_id: String,
    pub product_name: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_cost_cents: i64,
    pub total_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseReceipt {
    pub purchase_id: String,
    pub purchase_number: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub supplier_phone: Option<String>,
    pub items: Vec<PurchaseReceiptItem>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub payment_status: String,
    pub payment_method: Option<String>,
    pub purchase_date: String,
    pub created_at: String,
}

// ------------------------------------------------------------------------------------------------
// Sales DTOs
// ------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerSummary {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub current_balance_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProductForSale {
    pub id: String,
    pub name: String,
    pub product_type: String,
    pub unit: String,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
    pub current_quantity: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SalesFormData {
    pub customers: Vec<CustomerSummary>,
    pub products: Vec<ProductForSale>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateCustomerForSaleRequest {
    pub name: String,
    pub phone: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleItemIpcInput {
    pub product_id: String,
    pub quantity: i64, // milli-units (scale 1000)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSaleIpcInput {
    pub customer_id: Option<String>,
    pub items: Vec<SaleItemIpcInput>,
    pub settlement_mode: String, // "PAID" | "CREDIT"
    pub payment_method: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSaleItemQuote {
    pub product_id: String,
    pub product_name: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub line_total_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSaleQuote {
    pub preparation_token: String,
    pub sale_id: String,
    pub sale_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub items: Vec<PreparedSaleItemQuote>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub settlement_mode: String,
    pub payment_method: Option<String>,
    pub prepared_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmSaleIpcInput {
    pub preparation_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleReceiptItem {
    pub product_id: String,
    pub product_name: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub total_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleReceipt {
    pub sale_id: String,
    pub sale_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub items: Vec<SaleReceiptItem>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub settlement_mode: String,
    pub payment_method: Option<String>,
    pub sale_date: String,
    pub created_at: String,
}

// ------------------------------------------------------------------------------------------------
// BUILD 08 Customer Credits & Payment DTOs
// ------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerCreditItem {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub current_credit_cents: i64,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerCreditsSummary {
    pub customers: Vec<CustomerCreditItem>,
    pub total_outstanding_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerLedgerItem {
    pub id: String,
    pub customer_id: String,
    pub entry_type: String,
    pub amount_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reference_type: String,
    pub reference_id: String,
    pub notes: Option<String>,
    pub user_id: String,
    pub user_name: Option<String>,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerLedgerHistory {
    pub customer: CustomerCreditItem,
    pub entries: Vec<CustomerLedgerItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCustomerPaymentIpcInput {
    pub customer_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCustomerPaymentQuote {
    pub preparation_token: String,
    pub payment_id: String,
    pub customer_id: String,
    pub customer_name: String,
    pub customer_phone: Option<String>,
    pub amount_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub payment_method: String,
    pub notes: Option<String>,
    pub prepared_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmCustomerPaymentIpcInput {
    pub preparation_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerPaymentReceipt {
    pub payment_id: String,
    pub customer_id: String,
    pub customer_name: String,
    pub customer_phone: Option<String>,
    pub amount_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub payment_method: String,
    pub notes: Option<String>,
    pub payment_date: String,
    pub created_at: String,
}

// --- BUILD 09: Customer Orders Contracts ---

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerOrderItemInput {
    pub product_id: String,
    pub quantity: i64, // milli-units (scale 1000)
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateCustomerOrderIpcInput {
    pub customer_id: Option<String>,
    pub items: Vec<CustomerOrderItemInput>,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCustomerOrderIpcInput {
    pub order_id: String,
    pub customer_id: Option<String>,
    pub items: Vec<CustomerOrderItemInput>,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerOrderItemDetail {
    pub id: String,
    pub order_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub line_total_cents: i64,
    pub available_stock: i64,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerOrderDetail {
    pub id: String,
    pub order_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub status: String,
    pub total_amount_cents: i64,
    pub converted_sale_id: Option<String>,
    pub converted_sale_number: Option<String>,
    pub notes: Option<String>,
    pub user_id: String,
    pub user_name: Option<String>,
    pub items: Vec<CustomerOrderItemDetail>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerOrderSummaryItem {
    pub id: String,
    pub order_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub status: String,
    pub item_count: i64,
    pub total_amount_cents: i64,
    pub converted_sale_id: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerOrdersFormData {
    pub customers: Vec<CustomerSummary>,
    pub products: Vec<ProductForSale>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareOrderConversionIpcInput {
    pub order_id: String,
    pub settlement_mode: String,
    pub payment_method: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedOrderConversionQuote {
    pub preparation_token: String,
    pub order_id: String,
    pub order_number: String,
    pub sale_id: String,
    pub sale_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub items: Vec<PreparedSaleItemQuote>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub settlement_mode: String,
    pub payment_method: Option<String>,
    pub prepared_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmOrderConversionIpcInput {
    pub preparation_token: String,
}

// ------------------------------------------------------------------------------------------------
// Returns DTOs (BUILD 10)
// ------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReturnItemInput {
    pub product_id: String,
    pub quantity: i64, // milli-units (scale 1000)
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCustomerReturnIpcInput {
    pub customer_id: Option<String>,
    pub reference_sale_id: Option<String>,
    pub items: Vec<ReturnItemInput>,
    pub reason: String,
    pub refund_payment_method: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCustomerReturnItemQuote {
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub line_total_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCustomerReturnQuote {
    pub preparation_token: String,
    pub return_id: String,
    pub return_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub reference_sale_id: Option<String>,
    pub items: Vec<PreparedCustomerReturnItemQuote>,
    pub total_amount_cents: i64,
    pub debt_reduction_cents: i64,
    pub refund_amount_cents: i64,
    pub refund_payment_method: Option<String>,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reason: String,
    pub prepared_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmCustomerReturnIpcInput {
    pub preparation_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomerReturnReceipt {
    pub return_id: String,
    pub return_number: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub items: Vec<PreparedCustomerReturnItemQuote>,
    pub total_amount_cents: i64,
    pub debt_reduction_cents: i64,
    pub refund_amount_cents: i64,
    pub refund_payment_method: Option<String>,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reason: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSupplierReturnIpcInput {
    pub supplier_id: Option<String>,
    pub reference_purchase_id: Option<String>,
    pub items: Vec<ReturnItemInput>,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSupplierReturnItemQuote {
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_cost_cents: i64,
    pub line_total_cents: i64,
    pub available_stock: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSupplierReturnQuote {
    pub preparation_token: String,
    pub return_id: String,
    pub return_number: String,
    pub supplier_id: Option<String>,
    pub supplier_name: Option<String>,
    pub reference_purchase_id: Option<String>,
    pub items: Vec<PreparedSupplierReturnItemQuote>,
    pub total_amount_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reason: String,
    pub prepared_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmSupplierReturnIpcInput {
    pub preparation_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SupplierReturnReceipt {
    pub return_id: String,
    pub return_number: String,
    pub supplier_id: Option<String>,
    pub supplier_name: Option<String>,
    pub items: Vec<PreparedSupplierReturnItemQuote>,
    pub total_amount_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reason: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReturnSummaryItem {
    pub id: String,
    pub return_number: String,
    pub return_type: String,
    pub counterparty_name: Option<String>,
    pub item_count: i64,
    pub total_amount_cents: i64,
    pub reason: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReturnItemDetail {
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub total_cents: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReturnDetail {
    pub id: String,
    pub return_number: String,
    pub return_type: String,
    pub reference_id: Option<String>,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub supplier_id: Option<String>,
    pub supplier_name: Option<String>,
    pub total_amount_cents: i64,
    pub reason: String,
    pub admin_user_id: String,
    pub admin_user_name: Option<String>,
    pub items: Vec<ReturnItemDetail>,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReturnsFormData {
    pub customers: Vec<CustomerSummary>,
    pub suppliers: Vec<SupplierSummary>,
    pub products: Vec<ProductForSale>,
}


// ================================================================================================
// 3. CORE LOGIC (DIRECTLY TESTABLE WITHOUT TAURI RUNTIME)
// ================================================================================================

pub fn get_system_status_inner(db: &DatabaseManager) -> Result<SystemStatus, String> {
    let is_connected = db.verify_connection().map_err(|e| e.to_string())?;
    let sqlite_version = db.get_sqlite_version().map_err(|e| e.to_string())?;

    let db_status = if is_connected {
        "SQLite — Connected".to_string()
    } else {
        "SQLite — Disconnected".to_string()
    };

    Ok(SystemStatus {
        status: "LOCAL SYSTEM ONLINE".to_string(),
        database: db_status,
        sqlite_version,
        internet: "Not Required".to_string(),
        timestamp: format!("{:?}", SystemTime::now()),
    })
}

pub fn get_purchase_form_data_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<PurchaseFormData, String> {
    let identity = session.get_identity()?;

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Purchases.as_str())?;

        // 1. Fetch active suppliers
        let mut supp_stmt = conn.prepare(
            "SELECT id, name, phone, address, current_outstanding_cents
             FROM suppliers
             WHERE is_active = 1
             ORDER BY name ASC",
        )?;

        let suppliers = supp_stmt
            .query_map([], |row| {
                Ok(SupplierSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    phone: row.get(2)?,
                    address: row.get(3)?,
                    current_outstanding_cents: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // 2. Fetch active products with current stock from inventory
        let mut prod_stmt = conn.prepare(
            "SELECT p.id, p.name, p.product_type, p.unit, p.cost_price_cents, p.selling_price_cents,
                    COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.is_active = 1
             ORDER BY p.name ASC",
        )?;

        let products = prod_stmt
            .query_map([], |row| {
                Ok(ProductForPurchase {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    product_type: row.get(2)?,
                    unit: row.get(3)?,
                    cost_price_cents: row.get(4)?,
                    selling_price_cents: row.get(5)?,
                    current_quantity: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PurchaseFormData { suppliers, products })
    })
    .map_err(|e| e.to_string())
}

pub fn create_supplier_for_purchase_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    request: CreateSupplierForPurchaseRequest,
) -> Result<SupplierSummary, String> {
    let identity = session.get_identity()?;
    let trimmed_name = request.name.trim();
    if trimmed_name.is_empty() {
        return Err("Supplier name cannot be empty".to_string());
    }

    db.with_connection(|conn| {
        // Enforce supplier creation rule:
        // ADMIN -> allowed
        // EMPLOYEE with PURCHASES permission -> allowed
        // EMPLOYEE with SUPPLIERS only -> denied
        // EMPLOYEE with neither -> denied
        if !identity.is_admin() {
            let has_purchases: bool = conn
                .query_row(
                    "SELECT is_enabled FROM permissions WHERE user_id = ?1 AND feature_key = 'PURCHASES'",
                    params![identity.user_id()],
                    |r| r.get::<_, i64>(0).map(|v| v == 1),
                )
                .unwrap_or(false);

            if !has_purchases {
                return Err(EngineError::PermissionDenied {
                    user_id: identity.user_id().to_string(),
                    feature: "PURCHASES (create supplier)".to_string(),
                });
            }
        }

        let supplier_id = format!(
            "supp_{}_{}",
            trimmed_name.to_lowercase().replace(' ', "_"),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
        );
        let now = format!("{:?}", SystemTime::now());

        conn.execute(
            "INSERT INTO suppliers (id, name, phone, address, current_outstanding_cents, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, 1, ?5, ?5)",
            params![supplier_id, trimmed_name, request.phone, None::<String>, now],
        )?;

        Ok(SupplierSummary {
            id: supplier_id,
            name: trimmed_name.to_string(),
            phone: request.phone,
            address: None,
            current_outstanding_cents: 0,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn resolve_barcode_for_purchase_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    barcode: String,
) -> Result<ProductForPurchase, String> {
    let identity = session.get_identity()?;

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Purchases.as_str())?;

        // 1. Check barcode mappings
        let product_id: String = conn
            .query_row(
                "SELECT product_id FROM barcode_mappings WHERE barcode = ?1 LIMIT 1",
                params![barcode],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Barcode '{}' not found in catalog", barcode)))?;

        // 2. Read product details
        let prod = conn.query_row(
            "SELECT p.id, p.name, p.product_type, p.unit, p.cost_price_cents, p.selling_price_cents,
                    COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.id = ?1 AND p.is_active = 1",
            params![product_id],
            |row| {
                Ok(ProductForPurchase {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    product_type: row.get(2)?,
                    unit: row.get(3)?,
                    cost_price_cents: row.get(4)?,
                    selling_price_cents: row.get(5)?,
                    current_quantity: row.get(6)?,
                })
            },
        )?;

        Ok(prod)
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_purchase_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedPurchaseCache,
    input: PreparePurchaseIpcInput,
) -> Result<PreparedPurchaseQuote, String> {
    let identity = session.get_identity()?;

    if input.items.is_empty() {
        return Err("Purchase must contain at least one item".to_string());
    }

    if input.paid_amount_cents < 0 {
        return Err("Paid amount cannot be negative".to_string());
    }

    // Payment method rules:
    // CREDIT purchase (paid == 0) -> no payment method permitted
    // PAID / PARTIAL purchase (paid > 0) -> payment method required & must be valid
    if input.paid_amount_cents == 0 {
        if input.payment_method.is_some() && !input.payment_method.as_ref().unwrap().trim().is_empty() {
            return Err("Payment method cannot be specified when paid amount is zero (CREDIT purchase)".to_string());
        }
    } else {
        let pm = input
            .payment_method
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Payment method is required when paying an amount".to_string())?;

        let valid_methods = ["CASH", "UPI", "BANK_TRANSFER", "CARD", "OTHER"];
        if !valid_methods.contains(&pm.to_uppercase().as_str()) {
            return Err(format!("Invalid payment method '{}'. Valid methods: {:?}", pm, valid_methods));
        }
    }

    let purchase_id = format!(
        "pur_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let purchase_number = format!(
        "PO-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let purchase_date = input
        .purchase_date
        .unwrap_or_else(|| format!("{:?}", SystemTime::now()));

    let engine_req = ConfirmPurchaseRequest {
        purchase_id: purchase_id.clone(),
        purchase_number: purchase_number.clone(),
        supplier_id: input.supplier_id.clone(),
        items: input
            .items
            .iter()
            .map(|i| PurchaseItemRequest {
                product_id: i.product_id.clone(),
                quantity: i.quantity,
                unit_cost_cents: i.unit_cost_cents,
            })
            .collect(),
        paid_amount_cents: input.paid_amount_cents,
        payment_method: if input.paid_amount_cents > 0 { input.payment_method.clone() } else { None },
        user_id: identity.user_id().to_string(),
        purchase_date: purchase_date.clone(),
    };

    // Run authoritative calculation through BusinessEngine (READ-ONLY, zero SQLite mutations)
    let (prepared_quote, cached_item) = db
        .with_connection(|conn| {
            let prepared = BusinessEngine::prepare_purchase(conn, &identity, engine_req)?;

            // Overpayment protection
            if prepared.payload.paid_amount_cents > prepared.payload.total_amount_cents {
                return Err(EngineError::InvalidAmount {
                    field: "paid_amount_cents".to_string(),
                    amount: prepared.payload.paid_amount_cents,
                    message: format!(
                        "Overpayment rejected: paid amount ({} cents) exceeds total purchase amount ({} cents)",
                        prepared.payload.paid_amount_cents, prepared.payload.total_amount_cents
                    ),
                });
            }

            // Retrieve supplier name & verify active status
            let (supplier_name, supp_active): (String, i64) = conn
                .query_row(
                    "SELECT name, is_active FROM suppliers WHERE id = ?1",
                    params![input.supplier_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Supplier {}", input.supplier_id)))?;

            if supp_active != 1 {
                return Err(EngineError::DatabaseError(format!(
                    "Supplier {} is inactive",
                    input.supplier_id
                )));
            }

            // Enrich item quotes with product names & units and verify active products
            let mut enriched_items = Vec::new();
            let mut product_ids = Vec::new();
            let mut business_id: Option<String> = None;

            for item in &prepared.payload.items {
                product_ids.push(item.product_id.clone());
                let (p_name, p_unit, p_active, p_biz): (String, String, i64, Option<String>) = conn
                    .query_row(
                        "SELECT name, unit, is_active, business_id FROM products WHERE id = ?1",
                        params![item.product_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Product {}", item.product_id)))?;

                if p_active != 1 {
                    return Err(EngineError::DatabaseError(format!(
                        "Product {} is inactive",
                        item.product_id
                    )));
                }

                if let Some(biz) = p_biz {
                    if let Some(ref cur_biz) = business_id {
                        if cur_biz != &biz {
                            return Err(EngineError::CrossBusinessAccessDenied {
                                attempted_business: cur_biz.clone(),
                                resource_business: biz,
                            });
                        }
                    } else {
                        business_id = Some(biz);
                    }
                }

                enriched_items.push(PreparedPurchaseItemQuote {
                    product_id: item.product_id.clone(),
                    product_name: p_name,
                    unit: p_unit,
                    quantity: item.quantity,
                    unit_cost_cents: item.unit_cost_cents,
                    line_total_cents: item.line_total_cents,
                });
            }

            let prep_token = generate_preparation_token();

            let payment_status = if prepared.payload.credit_amount_cents == 0 {
                "PAID".to_string()
            } else if prepared.payload.paid_amount_cents > 0 {
                "PARTIAL".to_string()
            } else {
                "CREDIT".to_string()
            };

            let quote = PreparedPurchaseQuote {
                preparation_token: prep_token.clone(),
                purchase_id: prepared.payload.purchase_id.clone(),
                purchase_number: prepared.payload.purchase_number.clone(),
                supplier_id: prepared.payload.supplier_id.clone(),
                supplier_name,
                items: enriched_items,
                total_amount_cents: prepared.payload.total_amount_cents,
                paid_amount_cents: prepared.payload.paid_amount_cents,
                credit_amount_cents: prepared.payload.credit_amount_cents,
                payment_status,
                payment_method: prepared.payload.payment_method.clone(),
                purchase_date: prepared.payload.purchase_date.clone(),
                prepared_at: prepared.prepared_at.clone(),
            };

            let cached = CachedPreparedPurchase {
                owner_user_id: identity.user_id().to_string(),
                prepared,
                supplier_id: input.supplier_id,
                product_ids,
                total_amount_cents: quote.total_amount_cents,
                paid_amount_cents: quote.paid_amount_cents,
            };

            Ok((quote, (prep_token, cached)))
        })
        .map_err(|e| e.to_string())?;

    // Store in session-bound temporary cache keyed by (user_id, token)
    let (prep_token, cached_entry) = cached_item;
    let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
    cache_guard.insert((identity.user_id().to_string(), prep_token), cached_entry);

    Ok(prepared_quote)
}

pub fn confirm_purchase_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedPurchaseCache,
    input: ConfirmPurchaseIpcInput,
) -> Result<PurchaseReceipt, String> {
    let identity = session.get_identity()?;

    // 1. Race-Safe Single-Use Consumption:
    // Atomically take the prepared purchase from memory using session-bound key (user_id, token).
    // If two concurrent requests arrive with the same token, exactly one takes the entry;
    // the other fails immediately with StaleOrInvalidPreparation before touching SQLite.
    let cached = {
        let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
        cache_guard
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| {
                "StaleOrInvalidPreparation: Prepared purchase not found, already confirmed, or owned by another session".to_string()
            })?
    };

    // 2. Comprehensive Stale Preparation Revalidation against current SQLite state
    db.with_connection(|conn| {
        // A. Re-verify caller still possesses PURCHASES authority
        AuthorizationService::authorize(conn, &identity, PermissionKey::Purchases.as_str())?;

        // B. Re-verify supplier still exists and is active
        let supp_active: i64 = conn
            .query_row(
                "SELECT is_active FROM suppliers WHERE id = ?1",
                params![cached.supplier_id],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Supplier {}", cached.supplier_id)))?;

        if supp_active != 1 {
            return Err(EngineError::DatabaseError(format!(
                "Supplier {} is no longer active",
                cached.supplier_id
            )));
        }

        // C. Re-verify all products still exist, are active, and enforce business isolation
        let mut business_id: Option<String> = None;
        for p_id in &cached.product_ids {
            let (prod_active, prod_biz): (i64, Option<String>) = conn
                .query_row(
                    "SELECT is_active, business_id FROM products WHERE id = ?1",
                    params![p_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if prod_active != 1 {
                return Err(EngineError::DatabaseError(format!(
                    "Product {} is no longer active",
                    p_id
                )));
            }

            if let Some(biz) = prod_biz {
                if let Some(ref current_biz) = business_id {
                    if current_biz != &biz {
                        return Err(EngineError::CrossBusinessAccessDenied {
                            attempted_business: current_biz.clone(),
                            resource_business: biz,
                        });
                    }
                } else {
                    business_id = Some(biz);
                }
            }
        }

        // D. Re-verify items: quantities and buying prices strictly positive
        if cached.prepared.payload.items.is_empty() {
            return Err(EngineError::InvalidQuantity {
                product_id: "".to_string(),
                quantity: 0,
                message: "Purchase must contain at least one item".to_string(),
            });
        }
        for it in &cached.prepared.payload.items {
            if it.quantity <= 0 {
                return Err(EngineError::InvalidQuantity {
                    product_id: it.product_id.clone(),
                    quantity: it.quantity,
                    message: "Purchase quantity must be strictly positive".to_string(),
                });
            }
            if it.unit_cost_cents <= 0 {
                return Err(EngineError::InvalidAmount {
                    field: format!("unit_cost_cents for product {}", it.product_id),
                    amount: it.unit_cost_cents,
                    message: "Buying price must be strictly positive".to_string(),
                });
            }
        }

        // E. Re-verify amounts and payment rules
        if cached.paid_amount_cents < 0 {
            return Err(EngineError::InvalidAmount {
                field: "paid_amount_cents".to_string(),
                amount: cached.paid_amount_cents,
                message: "Paid amount cannot be negative".to_string(),
            });
        }
        if cached.paid_amount_cents > cached.total_amount_cents {
            return Err(EngineError::InvalidAmount {
                field: "paid_amount_cents".to_string(),
                amount: cached.paid_amount_cents,
                message: format!(
                    "Overpayment rejected: paid amount ({} cents) exceeds total purchase amount ({} cents)",
                    cached.paid_amount_cents, cached.total_amount_cents
                ),
            });
        }

        if cached.paid_amount_cents == 0 {
            if cached.prepared.payload.payment_method.is_some() {
                return Err(EngineError::DatabaseError(
                    "Payment method cannot be specified when paid amount is zero (CREDIT purchase)".to_string(),
                ));
            }
            if cached.prepared.payload.credit_amount_cents != cached.total_amount_cents {
                return Err(EngineError::DatabaseError(
                    "Credit amount must equal total amount when paid amount is zero".to_string(),
                ));
            }
        } else {
            if cached.prepared.payload.payment_method.is_none() {
                return Err(EngineError::DatabaseError(
                    "Payment method is required when paying an amount".to_string(),
                ));
            }
            let expected_credit = cached.total_amount_cents - cached.paid_amount_cents;
            if cached.prepared.payload.credit_amount_cents != expected_credit {
                return Err(EngineError::DatabaseError(
                    "Credit amount must equal total minus paid amount".to_string(),
                ));
            }
        }

        let target_purchase_id = cached.prepared.payload.purchase_id.clone();
        // 3. Explicit Confirmation Barrier: Confirm the prepared command with the authenticated identity
        let confirmed = cached.prepared.confirm(&identity);

        // 5. Execute Atomic Transaction in TransactionEngine
        TransactionEngine::execute_purchase(conn, confirmed)?;

        // 6. Build Authoritative PurchaseReceipt strictly from committed SQLite record
        let purchase_row = conn.query_row(
            "SELECT p.id, p.purchase_number, p.supplier_id, s.name, s.phone,
                    p.total_amount_cents, p.paid_amount_cents, p.credit_amount_cents,
                    p.payment_method, p.purchase_date, p.created_at
             FROM purchases p
             JOIN suppliers s ON p.supplier_id = s.id
             WHERE p.id = ?1",
            params![target_purchase_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, String>(10)?,
                ))
            },
        )?;

        let mut item_stmt = conn.prepare(
            "SELECT pi.product_id, p.name, p.unit, pi.quantity, pi.unit_cost_cents, pi.total_cents
             FROM purchase_items pi
             JOIN products p ON pi.product_id = p.id
             WHERE pi.purchase_id = ?1
             ORDER BY pi.id ASC",
        )?;

        let receipt_items = item_stmt
            .query_map(params![purchase_row.0], |r| {
                Ok(PurchaseReceiptItem {
                    product_id: r.get(0)?,
                    product_name: r.get(1)?,
                    unit: r.get(2)?,
                    quantity: r.get(3)?,
                    unit_cost_cents: r.get(4)?,
                    total_cents: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let payment_status = if purchase_row.7 == 0 {
            "PAID".to_string()
        } else if purchase_row.6 > 0 {
            "PARTIAL".to_string()
        } else {
            "CREDIT".to_string()
        };

        Ok(PurchaseReceipt {
            purchase_id: purchase_row.0,
            purchase_number: purchase_row.1,
            supplier_id: purchase_row.2,
            supplier_name: purchase_row.3,
            supplier_phone: purchase_row.4,
            items: receipt_items,
            total_amount_cents: purchase_row.5,
            paid_amount_cents: purchase_row.6,
            credit_amount_cents: purchase_row.7,
            payment_status,
            payment_method: purchase_row.8,
            purchase_date: purchase_row.9,
            created_at: purchase_row.10,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn get_sales_form_data_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<SalesFormData, String> {
    let identity = session.get_identity()?;

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Sales.as_str())?;

        // 1. Fetch active customers
        let mut cust_stmt = conn.prepare(
            "SELECT id, name, phone, current_credit_cents
             FROM customers
             WHERE is_active = 1
             ORDER BY name ASC",
        )?;

        let customers = cust_stmt
            .query_map([], |row| {
                Ok(CustomerSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    phone: row.get(2)?,
                    current_balance_cents: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // 2. Fetch active products with current stock from inventory
        let mut prod_stmt = conn.prepare(
            "SELECT p.id, p.name, p.product_type, p.unit, p.cost_price_cents, p.selling_price_cents,
                    COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.is_active = 1
             ORDER BY p.name ASC",
        )?;

        let products = prod_stmt
            .query_map([], |row| {
                Ok(ProductForSale {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    product_type: row.get(2)?,
                    unit: row.get(3)?,
                    cost_price_cents: row.get(4)?,
                    selling_price_cents: row.get(5)?,
                    current_quantity: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SalesFormData { customers, products })
    })
    .map_err(|e| e.to_string())
}

pub fn create_customer_for_sale_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    request: CreateCustomerForSaleRequest,
) -> Result<CustomerSummary, String> {
    let identity = session.get_identity()?;
    let trimmed_name = request.name.trim();
    if trimmed_name.is_empty() {
        return Err("Customer name cannot be empty".to_string());
    }

    db.with_connection(|conn| {
        if !identity.is_admin() {
            let has_sales: bool = conn
                .query_row(
                    "SELECT is_enabled FROM permissions WHERE user_id = ?1 AND feature_key = 'SALES'",
                    params![identity.user_id()],
                    |r| r.get::<_, i64>(0).map(|v| v == 1),
                )
                .unwrap_or(false);

            if !has_sales {
                return Err(EngineError::PermissionDenied {
                    user_id: identity.user_id().to_string(),
                    feature: "SALES (create customer)".to_string(),
                });
            }
        }

        let customer_id = format!(
            "cust_{}_{}",
            trimmed_name.to_lowercase().replace(' ', "_"),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
        );
        let now = format!("{:?}", SystemTime::now());

        conn.execute(
            "INSERT INTO customers (id, name, phone, address, current_credit_cents, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, 1, ?5, ?5)",
            params![customer_id, trimmed_name, request.phone, None::<String>, now],
        )?;

        Ok(CustomerSummary {
            id: customer_id,
            name: trimmed_name.to_string(),
            phone: request.phone,
            current_balance_cents: 0,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn resolve_barcode_for_sale_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    barcode: String,
) -> Result<ProductForSale, String> {
    let identity = session.get_identity()?;

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Sales.as_str())?;

        // 1. Check barcode mappings
        let product_id: String = conn
            .query_row(
                "SELECT product_id FROM barcode_mappings WHERE barcode = ?1 LIMIT 1",
                params![barcode],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Barcode '{}' not found in catalog", barcode)))?;

        // 2. Read product details
        let prod = conn.query_row(
            "SELECT p.id, p.name, p.product_type, p.unit, p.cost_price_cents, p.selling_price_cents,
                    COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.id = ?1 AND p.is_active = 1",
            params![product_id],
            |row| {
                Ok(ProductForSale {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    product_type: row.get(2)?,
                    unit: row.get(3)?,
                    cost_price_cents: row.get(4)?,
                    selling_price_cents: row.get(5)?,
                    current_quantity: row.get(6)?,
                })
            },
        )?;

        Ok(prod)
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_sale_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedSaleCache,
    input: PrepareSaleIpcInput,
) -> Result<PreparedSaleQuote, String> {
    let identity = session.get_identity()?;

    if input.items.is_empty() {
        return Err("Sale must contain at least one item".to_string());
    }

    let mode = input.settlement_mode.trim().to_uppercase();
    if mode != "PAID" && mode != "CREDIT" {
        return Err(format!(
            "Invalid settlement mode '{}'. BUILD 07 only supports 'PAID' or 'CREDIT'",
            input.settlement_mode
        ));
    }

    let customer_id = match &input.customer_id {
        Some(id) if !id.trim().is_empty() => Some(id.trim().to_string()),
        _ => None,
    };

    let payment_method = if mode == "CREDIT" {
        if customer_id.is_none() {
            return Err("Credit sale requires a registered customer identity".to_string());
        }
        if let Some(ref pm) = input.payment_method {
            if !pm.trim().is_empty() {
                return Err("Payment method cannot be specified for CREDIT sale".to_string());
            }
        }
        None
    } else {
        // PAID
        let pm = input
            .payment_method
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Payment method is required for PAID sale".to_string())?;

        let valid_methods = ["CASH", "UPI", "BANK_TRANSFER", "CARD", "OTHER"];
        if !valid_methods.contains(&pm.to_uppercase().as_str()) {
            return Err(format!(
                "Invalid payment method '{}'. Valid methods: {:?}",
                pm, valid_methods
            ));
        }
        Some(pm.to_uppercase())
    };

    let (prepared_quote, cached_item) = db
        .with_connection(|conn| {
            AuthorizationService::authorize(conn, &identity, PermissionKey::Sales.as_str())?;

            let mut customer_name: Option<String> = None;
            if let Some(ref c_id) = customer_id {
                let (name, is_active): (String, i64) = conn
                    .query_row(
                        "SELECT name, is_active FROM customers WHERE id = ?1",
                        params![c_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

                if is_active != 1 {
                    return Err(EngineError::DatabaseError(format!("Customer {} is inactive", c_id)));
                }
                customer_name = Some(name);
            }

            let mut sale_item_requests = Vec::new();
            let mut enriched_items = Vec::new();
            let mut product_quantities = Vec::new();
            let mut business_id: Option<String> = None;
            let mut calculated_total_cents: i64 = 0;

            for item in &input.items {
                if item.quantity <= 0 {
                    return Err(EngineError::InvalidQuantity {
                        product_id: item.product_id.clone(),
                        quantity: item.quantity,
                        message: "Sale quantity must be strictly positive".to_string(),
                    });
                }

                let (p_name, p_unit, p_selling_price, p_active, p_biz): (
                    String,
                    String,
                    i64,
                    i64,
                    Option<String>,
                ) = conn
                    .query_row(
                        "SELECT name, unit, selling_price_cents, is_active, business_id FROM products WHERE id = ?1",
                        params![item.product_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Product {}", item.product_id)))?;

                if p_active != 1 {
                    return Err(EngineError::DatabaseError(format!(
                        "Product {} is inactive",
                        item.product_id
                    )));
                }

                if let Some(biz) = p_biz {
                    if let Some(ref cur_biz) = business_id {
                        if cur_biz != &biz {
                            return Err(EngineError::CrossBusinessAccessDenied {
                                attempted_business: cur_biz.clone(),
                                resource_business: biz,
                            });
                        }
                    } else {
                        business_id = Some(biz);
                    }
                }

                // Check stock availability (Pre-check)
                let available: i64 = conn
                    .query_row(
                        "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                        params![item.product_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);

                if available < item.quantity {
                    return Err(EngineError::InsufficientStock {
                        product_id: item.product_id.clone(),
                        available,
                        requested: item.quantity,
                    });
                }

                let line_total = calculate_line_total(item.quantity, p_selling_price)
                    .map_err(|e| EngineError::MathError(e.to_string()))?;

                calculated_total_cents = calculated_total_cents
                    .checked_add(line_total)
                    .ok_or_else(|| EngineError::MathError("Total sale amount overflow".to_string()))?;

                sale_item_requests.push(SaleItemRequest {
                    product_id: item.product_id.clone(),
                    quantity: item.quantity,
                    unit_price_cents: p_selling_price,
                });

                enriched_items.push(PreparedSaleItemQuote {
                    product_id: item.product_id.clone(),
                    product_name: p_name,
                    unit: p_unit,
                    quantity: item.quantity,
                    unit_price_cents: p_selling_price,
                    line_total_cents: line_total,
                });

                product_quantities.push((item.product_id.clone(), item.quantity));
            }

            let (paid_amount_cents, _credit_amount_cents) = if mode == "PAID" {
                (calculated_total_cents, 0)
            } else {
                (0, calculated_total_cents)
            };

            let sale_id = format!(
                "sale_{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let sale_number = format!(
                "INV-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            );
            let sale_date = format!("{:?}", SystemTime::now());

            let engine_req = ConfirmSaleRequest {
                sale_id: sale_id.clone(),
                sale_number: sale_number.clone(),
                customer_id: customer_id.clone(),
                items: sale_item_requests,
                paid_amount_cents,
                payment_method: payment_method.clone(),
                user_id: identity.user_id().to_string(),
                sale_date,
            };

            // Run authoritative preparation in BusinessEngine (zero SQLite mutations)
            let prepared = BusinessEngine::prepare_sale(conn, &identity, engine_req)?;

            let prep_token = generate_preparation_token();

            let quote = PreparedSaleQuote {
                preparation_token: prep_token.clone(),
                sale_id: prepared.payload.sale_id.clone(),
                sale_number: prepared.payload.sale_number.clone(),
                customer_id: prepared.payload.customer_id.clone(),
                customer_name,
                items: enriched_items,
                total_amount_cents: prepared.payload.total_amount_cents,
                paid_amount_cents: prepared.payload.paid_amount_cents,
                credit_amount_cents: prepared.payload.credit_amount_cents,
                settlement_mode: mode.clone(),
                payment_method: prepared.payload.payment_method.clone(),
                prepared_at: prepared.prepared_at.clone(),
            };

            let cached = CachedPreparedSale {
                owner_user_id: identity.user_id().to_string(),
                prepared,
                customer_id: quote.customer_id.clone(),
                product_quantities,
                total_amount_cents: quote.total_amount_cents,
                paid_amount_cents: quote.paid_amount_cents,
                credit_amount_cents: quote.credit_amount_cents,
                settlement_mode: mode,
                payment_method: quote.payment_method.clone(),
            };

            Ok((quote, (prep_token, cached)))
        })
        .map_err(|e| e.to_string())?;

    let (prep_token, cached_entry) = cached_item;
    let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
    cache_guard.insert((identity.user_id().to_string(), prep_token), cached_entry);

    Ok(prepared_quote)
}

pub fn confirm_sale_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedSaleCache,
    input: ConfirmSaleIpcInput,
) -> Result<SaleReceipt, String> {
    let identity = session.get_identity()?;

    // 1. Race-Safe Single-Use Consumption:
    // Atomically remove prepared sale from memory using session-bound key (user_id, token).
    let cached = {
        let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
        cache_guard
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| {
                "StaleOrInvalidPreparation: Prepared sale not found, already confirmed, or owned by another session".to_string()
            })?
    };

    // 2. Comprehensive Stale Preparation Revalidation against current SQLite state
    db.with_connection(|conn| {
        // A. Re-verify caller still possesses SALES authority
        AuthorizationService::authorize(conn, &identity, PermissionKey::Sales.as_str())?;

        // B. Re-verify customer if present, or if CREDIT mode, ensure customer exists and is active
        if cached.settlement_mode == "CREDIT" {
            let c_id = cached.customer_id.as_deref().ok_or_else(|| {
                EngineError::DatabaseError("Credit sale requires a registered customer".to_string())
            })?;
            let is_active: i64 = conn
                .query_row(
                    "SELECT is_active FROM customers WHERE id = ?1",
                    params![c_id],
                    |r| r.get(0),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is no longer active", c_id)));
            }

            if cached.paid_amount_cents != 0 || cached.credit_amount_cents != cached.total_amount_cents {
                return Err(EngineError::DatabaseError("Invalid settlement amounts for CREDIT sale".to_string()));
            }
            if cached.payment_method.is_some() {
                return Err(EngineError::DatabaseError("Payment method must be absent for CREDIT sale".to_string()));
            }
        } else if cached.settlement_mode == "PAID" {
            if let Some(ref c_id) = cached.customer_id {
                let is_active: i64 = conn
                    .query_row(
                        "SELECT is_active FROM customers WHERE id = ?1",
                        params![c_id],
                        |r| r.get(0),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

                if is_active != 1 {
                    return Err(EngineError::DatabaseError(format!("Customer {} is no longer active", c_id)));
                }
            }
            if cached.paid_amount_cents != cached.total_amount_cents || cached.credit_amount_cents != 0 {
                return Err(EngineError::DatabaseError("Invalid settlement amounts for PAID sale".to_string()));
            }
            if cached.payment_method.is_none() {
                return Err(EngineError::DatabaseError("Payment method required for PAID sale".to_string()));
            }
        } else {
            return Err(EngineError::DatabaseError(format!("Invalid settlement mode {}", cached.settlement_mode)));
        }

        // C. Re-verify all products still exist, are active, enforce business isolation, and pre-check stock
        let mut business_id: Option<String> = None;
        for (p_id, qty) in &cached.product_quantities {
            let (prod_active, prod_biz): (i64, Option<String>) = conn
                .query_row(
                    "SELECT is_active, business_id FROM products WHERE id = ?1",
                    params![p_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if prod_active != 1 {
                return Err(EngineError::DatabaseError(format!(
                    "Product {} is no longer active",
                    p_id
                )));
            }

            if let Some(biz) = prod_biz {
                if let Some(ref current_biz) = business_id {
                    if current_biz != &biz {
                        return Err(EngineError::CrossBusinessAccessDenied {
                            attempted_business: current_biz.clone(),
                            resource_business: biz,
                        });
                    }
                } else {
                    business_id = Some(biz);
                }
            }

            let available: i64 = conn
                .query_row(
                    "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                    params![p_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            if available < *qty {
                return Err(EngineError::InsufficientStock {
                    product_id: p_id.clone(),
                    available,
                    requested: *qty,
                });
            }
        }

        let target_sale_id = cached.prepared.payload.sale_id.clone();

        // D. Explicit Confirmation Barrier: Confirm the prepared command with the authenticated identity
        let confirmed = cached.prepared.confirm(&identity);

        // E. Execute Atomic Transaction in TransactionEngine (includes atomic conditional stock decrement)
        TransactionEngine::execute_sale(conn, confirmed)?;

        // F. Build Authoritative SaleReceipt strictly from committed SQLite records
        let sale_row = conn.query_row(
            "SELECT s.id, s.sale_number, s.customer_id, c.name, c.phone,
                    s.total_amount_cents, s.paid_amount_cents, s.credit_amount_cents,
                    s.sale_date, s.created_at
             FROM sales s
             LEFT JOIN customers c ON s.customer_id = c.id
             WHERE s.id = ?1",
            params![target_sale_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                ))
            },
        )?;

        let payment_method: Option<String> = conn
            .query_row(
                "SELECT payment_method FROM payments WHERE related_entity_type = 'SALE' AND related_entity_id = ?1 LIMIT 1",
                params![target_sale_id],
                |r| r.get(0),
            )
            .ok();

        let mut item_stmt = conn.prepare(
            "SELECT si.product_id, p.name, p.unit, si.quantity, si.unit_price_cents, si.total_cents
             FROM sale_items si
             JOIN products p ON si.product_id = p.id
             WHERE si.sale_id = ?1
             ORDER BY si.id ASC",
        )?;

        let receipt_items = item_stmt
            .query_map(params![sale_row.0], |r| {
                Ok(SaleReceiptItem {
                    product_id: r.get(0)?,
                    product_name: r.get(1)?,
                    unit: r.get(2)?,
                    quantity: r.get(3)?,
                    unit_price_cents: r.get(4)?,
                    total_cents: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let settlement_mode = if sale_row.7 == 0 {
            "PAID".to_string()
        } else {
            "CREDIT".to_string()
        };

        Ok(SaleReceipt {
            sale_id: sale_row.0,
            sale_number: sale_row.1,
            customer_id: sale_row.2,
            customer_name: sale_row.3,
            customer_phone: sale_row.4,
            items: receipt_items,
            total_amount_cents: sale_row.5,
            paid_amount_cents: sale_row.6,
            credit_amount_cents: sale_row.7,
            settlement_mode,
            payment_method,
            sale_date: sale_row.8,
            created_at: sale_row.9,
        })
    })
    .map_err(|e| e.to_string())
}

// ================================================================================================
// 4. TAURI IPC COMMAND WRAPPERS
// ================================================================================================

#[tauri::command]
pub fn get_system_status(db: tauri::State<DatabaseManager>) -> Result<SystemStatus, String> {
    get_system_status_inner(&db)
}

#[tauri::command]
pub fn get_purchase_form_data(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<PurchaseFormData, String> {
    get_purchase_form_data_inner(&db, &session)
}

#[tauri::command]
pub fn create_supplier_for_purchase(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    request: CreateSupplierForPurchaseRequest,
) -> Result<SupplierSummary, String> {
    create_supplier_for_purchase_inner(&db, &session, request)
}

#[tauri::command]
pub fn resolve_barcode_for_purchase(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    barcode: String,
) -> Result<ProductForPurchase, String> {
    resolve_barcode_for_purchase_inner(&db, &session, barcode)
}

#[tauri::command]
pub fn prepare_purchase(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedPurchaseCache>,
    input: PreparePurchaseIpcInput,
) -> Result<PreparedPurchaseQuote, String> {
    prepare_purchase_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_purchase(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedPurchaseCache>,
    input: ConfirmPurchaseIpcInput,
) -> Result<PurchaseReceipt, String> {
    confirm_purchase_inner(&db, &session, &cache, input)
}

// BUILD 07: Sales / POS Commands

#[tauri::command]
pub fn get_sales_form_data(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<SalesFormData, String> {
    get_sales_form_data_inner(&db, &session)
}

#[tauri::command]
pub fn create_customer_for_sale(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    request: CreateCustomerForSaleRequest,
) -> Result<CustomerSummary, String> {
    create_customer_for_sale_inner(&db, &session, request)
}

#[tauri::command]
pub fn resolve_barcode_for_sale(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    barcode: String,
) -> Result<ProductForSale, String> {
    resolve_barcode_for_sale_inner(&db, &session, barcode)
}

#[tauri::command]
pub fn prepare_sale(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedSaleCache>,
    input: PrepareSaleIpcInput,
) -> Result<PreparedSaleQuote, String> {
    prepare_sale_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_sale(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedSaleCache>,
    input: ConfirmSaleIpcInput,
) -> Result<SaleReceipt, String> {
    confirm_sale_inner(&db, &session, &cache, input)
}

// ================================================================================================
// BUILD 08 CUSTOMER CREDITS & PAYMENTS (INNER & TAURI COMMANDS)
// ================================================================================================

pub fn get_customer_credits_summary_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<CustomerCreditsSummary, String> {
    let identity = session.get_identity()?;

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerCredits.as_str())?;

        let mut stmt = conn.prepare(
            "SELECT id, name, phone, address, current_credit_cents, is_active, created_at, updated_at
             FROM customers
             WHERE is_active = 1
             ORDER BY name ASC, id ASC",
        )?;

        let customers = stmt
            .query_map([], |row| {
                Ok(CustomerCreditItem {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    phone: row.get(2)?,
                    address: row.get(3)?,
                    current_credit_cents: row.get(4)?,
                    is_active: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let total_outstanding_cents = customers.iter().map(|c| c.current_credit_cents).sum();

        Ok(CustomerCreditsSummary {
            customers,
            total_outstanding_cents,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn get_customer_ledger_history_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    customer_id: String,
) -> Result<CustomerLedgerHistory, String> {
    let identity = session.get_identity()?;

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerCredits.as_str())?;

        let customer = conn
            .query_row(
                "SELECT id, name, phone, address, current_credit_cents, is_active, created_at, updated_at
                 FROM customers
                 WHERE id = ?1",
                params![customer_id],
                |row| {
                    Ok(CustomerCreditItem {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        phone: row.get(2)?,
                        address: row.get(3)?,
                        current_credit_cents: row.get(4)?,
                        is_active: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", customer_id)))?;

        let mut stmt = conn.prepare(
            "SELECT l.id, l.customer_id, l.entry_type, l.amount_cents, l.balance_before_cents,
                    l.balance_after_cents, l.reference_type, l.reference_id, l.notes,
                    l.user_id, u.username, l.created_at
             FROM customer_ledger l
             LEFT JOIN users u ON l.user_id = u.id
             WHERE l.customer_id = ?1
             ORDER BY l.created_at DESC, l.id DESC",
        )?;

        let entries = stmt
            .query_map(params![customer_id], |row| {
                Ok(CustomerLedgerItem {
                    id: row.get(0)?,
                    customer_id: row.get(1)?,
                    entry_type: row.get(2)?,
                    amount_cents: row.get(3)?,
                    balance_before_cents: row.get(4)?,
                    balance_after_cents: row.get(5)?,
                    reference_type: row.get(6)?,
                    reference_id: row.get(7)?,
                    notes: row.get(8)?,
                    user_id: row.get(9)?,
                    user_name: row.get(10)?,
                    created_at: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CustomerLedgerHistory { customer, entries })
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_customer_payment_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedCustomerPaymentCache,
    input: PrepareCustomerPaymentIpcInput,
) -> Result<PreparedCustomerPaymentQuote, String> {
    let identity = session.get_identity()?;

    if input.amount_cents <= 0 {
        return Err("Customer payment amount must be strictly positive".to_string());
    }

    let norm_method = input.payment_method.trim().to_uppercase();
    let valid_methods = ["CASH", "UPI", "BANK_TRANSFER", "CARD", "OTHER"];
    if !valid_methods.contains(&norm_method.as_str()) {
        return Err(format!(
            "Invalid payment method '{}'. Valid methods: {:?}",
            input.payment_method, valid_methods
        ));
    }

    let (quote, cached_item) = db
        .with_connection(|conn| {
            AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerCredits.as_str())?;

            let (name, phone, is_active, current_credit): (String, Option<String>, i64, i64) = conn
                .query_row(
                    "SELECT name, phone, is_active, current_credit_cents FROM customers WHERE id = ?1",
                    params![input.customer_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", input.customer_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is inactive", input.customer_id)));
            }

            if input.amount_cents > current_credit {
                return Err(EngineError::OverpaymentNotAllowed {
                    owed: current_credit,
                    attempted: input.amount_cents,
                });
            }

            let payment_id = format!(
                "pmt_{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );

            let engine_req = RecordCustomerPaymentRequest {
                payment_id: payment_id.clone(),
                customer_id: input.customer_id.clone(),
                amount_cents: input.amount_cents,
                payment_method: norm_method.clone(),
                user_id: identity.user_id().to_string(),
                notes: input.notes.clone(),
            };

            let prepared = BusinessEngine::prepare_customer_payment(conn, &identity, engine_req)?;
            let prep_token = generate_preparation_token();

            let quote = PreparedCustomerPaymentQuote {
                preparation_token: prep_token.clone(),
                payment_id: prepared.payload.payment_id.clone(),
                customer_id: prepared.payload.customer_id.clone(),
                customer_name: name.clone(),
                customer_phone: phone.clone(),
                amount_cents: prepared.payload.amount_cents,
                balance_before_cents: prepared.payload.balance_before_cents,
                balance_after_cents: prepared.payload.balance_after_cents,
                payment_method: norm_method.clone(),
                notes: input.notes.clone(),
                prepared_at: prepared.prepared_at.clone(),
            };

            let cached = CachedPreparedCustomerPayment {
                owner_user_id: identity.user_id().to_string(),
                prepared,
                customer_id: quote.customer_id.clone(),
                customer_name: name,
                customer_phone: phone,
                amount_cents: quote.amount_cents,
                payment_method: norm_method,
                notes: quote.notes.clone(),
                balance_before_cents: quote.balance_before_cents,
                balance_after_cents: quote.balance_after_cents,
            };

            Ok((quote, (prep_token, cached)))
        })
        .map_err(|e| e.to_string())?;

    let (prep_token, cached_entry) = cached_item;
    let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
    cache_guard.insert((identity.user_id().to_string(), prep_token), cached_entry);

    Ok(quote)
}

pub fn confirm_customer_payment_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedCustomerPaymentCache,
    input: ConfirmCustomerPaymentIpcInput,
) -> Result<CustomerPaymentReceipt, String> {
    let identity = session.get_identity()?;

    // 1. Single-use consumption
    let cached = {
        let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
        cache_guard
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| {
                "StaleOrInvalidPreparation: Prepared customer payment not found, already confirmed, or owned by another session".to_string()
            })?
    };

    // 2. Revalidation before transaction
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerCredits.as_str())?;

        let (is_active, current_credit): (i64, i64) = conn
            .query_row(
                "SELECT is_active, current_credit_cents FROM customers WHERE id = ?1",
                params![cached.customer_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", cached.customer_id)))?;

        if is_active != 1 {
            return Err(EngineError::DatabaseError(format!("Customer {} is no longer active", cached.customer_id)));
        }

        if cached.amount_cents > current_credit {
            return Err(EngineError::OverpaymentNotAllowed {
                owed: current_credit,
                attempted: cached.amount_cents,
            });
        }

        let target_payment_id = cached.prepared.payload.payment_id.clone();
        let confirmed = cached.prepared.confirm(&identity);

        TransactionEngine::execute_customer_payment(conn, confirmed)?;

        // 3. Build authoritative receipt from committed state
        let receipt = conn.query_row(
            "SELECT p.id, p.related_entity_id, c.name, c.phone, p.amount_cents,
                    c.current_credit_cents, p.payment_method, p.notes, p.created_at
             FROM payments p
             JOIN customers c ON p.related_entity_id = c.id
             WHERE p.id = ?1",
            params![target_payment_id],
            |r| {
                let p_id: String = r.get(0)?;
                let c_id: String = r.get(1)?;
                let c_name: String = r.get(2)?;
                let c_phone: Option<String> = r.get(3)?;
                let amt: i64 = r.get(4)?;
                let bal_after: i64 = r.get(5)?;
                let method: String = r.get(6)?;
                let notes: Option<String> = r.get(7)?;
                let created_at: String = r.get(8)?;

                let bal_before = bal_after + amt;

                Ok(CustomerPaymentReceipt {
                    payment_id: p_id,
                    customer_id: c_id,
                    customer_name: c_name,
                    customer_phone: c_phone,
                    amount_cents: amt,
                    balance_before_cents: bal_before,
                    balance_after_cents: bal_after,
                    payment_method: method,
                    notes,
                    payment_date: created_at.clone(),
                    created_at,
                })
            },
        )?;

        Ok(receipt)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_customer_credits_summary(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<CustomerCreditsSummary, String> {
    get_customer_credits_summary_inner(&db, &session)
}

#[tauri::command]
pub fn get_customer_ledger_history(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    customer_id: String,
) -> Result<CustomerLedgerHistory, String> {
    get_customer_ledger_history_inner(&db, &session, customer_id)
}

#[tauri::command]
pub fn prepare_customer_payment(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedCustomerPaymentCache>,
    input: PrepareCustomerPaymentIpcInput,
) -> Result<PreparedCustomerPaymentQuote, String> {
    prepare_customer_payment_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_customer_payment(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedCustomerPaymentCache>,
    input: ConfirmCustomerPaymentIpcInput,
) -> Result<CustomerPaymentReceipt, String> {
    confirm_customer_payment_inner(&db, &session, &cache, input)
}

// ================================================================================================
// BUILD 09: CUSTOMER ORDERS COMMANDS
// ================================================================================================

pub fn get_customer_orders_form_data_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<CustomerOrdersFormData, String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        let mut cust_stmt = conn.prepare(
            "SELECT id, name, phone, current_balance_cents
             FROM customers
             WHERE is_active = 1
             ORDER BY name ASC",
        )?;
        let cust_rows = cust_stmt.query_map([], |r| {
            Ok(CustomerSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                phone: r.get(2)?,
                current_balance_cents: r.get(3)?,
            })
        })?;
        let mut customers = Vec::new();
        for c in cust_rows {
            customers.push(c?);
        }

        let mut prod_stmt = conn.prepare(
            "SELECT p.id, p.name, p.product_type, p.unit, p.cost_price_cents, p.selling_price_cents,
                    COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.is_active = 1
             ORDER BY p.name ASC",
        )?;
        let prod_rows = prod_stmt.query_map([], |r| {
            Ok(ProductForSale {
                id: r.get(0)?,
                name: r.get(1)?,
                product_type: r.get(2)?,
                unit: r.get(3)?,
                cost_price_cents: r.get(4)?,
                selling_price_cents: r.get(5)?,
                current_quantity: r.get(6)?,
            })
        })?;
        let mut products = Vec::new();
        for p in prod_rows {
            products.push(p?);
        }

        Ok(CustomerOrdersFormData {
            customers,
            products,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn get_customer_orders_summary_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    status_filter: Option<String>,
) -> Result<Vec<CustomerOrderSummaryItem>, String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        let filter_val = status_filter
            .as_ref()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty() && s != "ALL");

        let mut stmt = conn.prepare(
            "SELECT o.id, o.order_number, o.customer_id, c.name, c.phone, o.status,
                    (SELECT COUNT(*) FROM customer_order_items WHERE order_id = o.id) as item_count,
                    (SELECT COALESCE(SUM(quantity * unit_price_cents / 1000), 0) FROM customer_order_items WHERE order_id = o.id) as total_cents,
                    o.converted_sale_id, o.notes, o.created_at, o.updated_at
             FROM customer_orders o
             LEFT JOIN customers c ON o.customer_id = c.id
             WHERE (?1 IS NULL OR o.status = ?1)
             ORDER BY o.created_at DESC, o.id DESC",
        )?;

        let rows = stmt.query_map(params![filter_val], |r| {
            Ok(CustomerOrderSummaryItem {
                id: r.get(0)?,
                order_number: r.get(1)?,
                customer_id: r.get(2)?,
                customer_name: r.get(3)?,
                customer_phone: r.get(4)?,
                status: r.get(5)?,
                item_count: r.get(6)?,
                total_amount_cents: r.get(7)?,
                converted_sale_id: r.get(8)?,
                notes: r.get(9)?,
                created_at: r.get(10)?,
                updated_at: r.get(11)?,
            })
        })?;

        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        Ok(list)
    })
    .map_err(|e| e.to_string())
}

pub fn get_customer_order_detail_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    order_id: String,
) -> Result<CustomerOrderDetail, String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        let (
            id,
            order_number,
            customer_id,
            customer_name,
            customer_phone,
            status,
            converted_sale_id,
            converted_sale_number,
            notes,
            user_id,
            user_name,
            created_at,
            updated_at,
        ): (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT o.id, o.order_number, o.customer_id, c.name, c.phone, o.status,
                        o.converted_sale_id, s.sale_number, o.notes, o.user_id, u.username,
                        o.created_at, o.updated_at
                 FROM customer_orders o
                 LEFT JOIN customers c ON o.customer_id = c.id
                 LEFT JOIN users u ON o.user_id = u.id
                 LEFT JOIN sales s ON o.converted_sale_id = s.id
                 WHERE o.id = ?1",
                params![order_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                        r.get(11)?,
                        r.get(12)?,
                    ))
                },
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Order {}", order_id)))?;

        let mut items_stmt = conn.prepare(
            "SELECT oi.id, oi.order_id, oi.product_id, p.name, p.product_type, p.unit,
                    oi.quantity, oi.unit_price_cents,
                    COALESCE(i.current_quantity, 0), oi.notes
             FROM customer_order_items oi
             JOIN products p ON oi.product_id = p.id
             LEFT JOIN inventory i ON oi.product_id = i.product_id
             WHERE oi.order_id = ?1
             ORDER BY oi.id ASC",
        )?;

        let item_rows = items_stmt.query_map(params![order_id], |r| {
            let quantity: i64 = r.get(6)?;
            let unit_price_cents: i64 = r.get(7)?;
            let line_total_cents = calculate_line_total(quantity, unit_price_cents)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            Ok(CustomerOrderItemDetail {
                id: r.get(0)?,
                order_id: r.get(1)?,
                product_id: r.get(2)?,
                product_name: r.get(3)?,
                product_type: r.get(4)?,
                unit: r.get(5)?,
                quantity,
                unit_price_cents,
                line_total_cents,
                available_stock: r.get(8)?,
                notes: r.get(9)?,
            })
        })?;

        let mut items = Vec::new();
        let mut total_amount_cents: i64 = 0;
        for item in item_rows {
            let detail = item?;
            total_amount_cents = total_amount_cents
                .checked_add(detail.line_total_cents)
                .ok_or_else(|| EngineError::MathError("Total amount overflow".to_string()))?;
            items.push(detail);
        }

        Ok(CustomerOrderDetail {
            id,
            order_number,
            customer_id,
            customer_name,
            customer_phone,
            status,
            total_amount_cents,
            converted_sale_id,
            converted_sale_number,
            notes,
            user_id,
            user_name,
            items,
            created_at,
            updated_at,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn create_customer_order_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    input: CreateCustomerOrderIpcInput,
) -> Result<CustomerOrderDetail, String> {
    let identity = session.get_identity()?;

    if input.items.is_empty() {
        return Err("Cannot create an order with zero items".to_string());
    }

    let mut consolidated: Vec<CustomerOrderItemInput> = Vec::new();
    for item in input.items {
        if item.quantity <= 0 {
            return Err("All order item quantities must be strictly positive".to_string());
        }
        if let Some(existing) = consolidated.iter_mut().find(|x| x.product_id == item.product_id) {
            existing.quantity = existing
                .quantity
                .checked_add(item.quantity)
                .ok_or_else(|| "Quantity overflow on product consolidation".to_string())?;
        } else {
            consolidated.push(item);
        }
    }

    let customer_id = match input.customer_id {
        Some(id) if !id.trim().is_empty() => Some(id.trim().to_string()),
        _ => None,
    };

    let order_id = format!(
        "ord_{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    );

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        // 1. Verify customer if provided
        if let Some(ref c_id) = customer_id {
            let is_active: i64 = conn
                .query_row(
                    "SELECT is_active FROM customers WHERE id = ?1",
                    params![c_id],
                    |r| r.get(0),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is inactive", c_id)));
            }
        }

        // 2. Verify all products and resolve catalog prices
        let mut enriched = Vec::new();
        for item in &consolidated {
            let (is_active, selling_price_cents): (i64, i64) = conn
                .query_row(
                    "SELECT is_active, selling_price_cents FROM products WHERE id = ?1",
                    params![item.product_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", item.product_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", item.product_id)));
            }

            enriched.push((item.product_id.clone(), item.quantity, selling_price_cents, item.notes.clone()));
        }

        // 3. In SQLite transaction, generate collision-safe order number and insert
        let tx = conn.transaction()?;
        let order_number = generate_order_number(&tx)?;
        let now = format!("{:?}", SystemTime::now());

        tx.execute(
            "INSERT INTO customer_orders (id, order_number, customer_id, status, converted_sale_id, notes, user_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'DRAFT', NULL, ?4, ?5, ?6, ?6)",
            params![
                order_id,
                order_number,
                customer_id,
                input.notes,
                identity.user_id(),
                now,
            ],
        )?;

        for (idx, (p_id, qty, price, item_notes)) in enriched.iter().enumerate() {
            let item_id = format!("item_{}_{}", order_id, idx);
            tx.execute(
                "INSERT INTO customer_order_items (id, order_id, product_id, quantity, unit_price_cents, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![item_id, order_id, p_id, qty, price, item_notes],
            )?;
        }

        // Audit log
        let audit_id = format!("audit_{}_{}", order_id, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'ORDER_CREATED', 'customer_orders', ?3, ?4, ?5)",
            params![
                audit_id,
                identity.user_id(),
                order_id,
                format!("Created draft customer order {}", order_number),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    get_customer_order_detail_inner(db, session, order_id)
}

pub fn update_customer_order_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    input: UpdateCustomerOrderIpcInput,
) -> Result<CustomerOrderDetail, String> {
    let identity = session.get_identity()?;

    if input.items.is_empty() {
        return Err("Cannot update an order to have zero items".to_string());
    }

    let mut consolidated: Vec<CustomerOrderItemInput> = Vec::new();
    for item in input.items {
        if item.quantity <= 0 {
            return Err("All order item quantities must be strictly positive".to_string());
        }
        if let Some(existing) = consolidated.iter_mut().find(|x| x.product_id == item.product_id) {
            existing.quantity = existing
                .quantity
                .checked_add(item.quantity)
                .ok_or_else(|| "Quantity overflow on product consolidation".to_string())?;
        } else {
            consolidated.push(item);
        }
    }

    let customer_id = match input.customer_id {
        Some(id) if !id.trim().is_empty() => Some(id.trim().to_string()),
        _ => None,
    };

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        let status: String = conn
            .query_row(
                "SELECT status FROM customer_orders WHERE id = ?1",
                params![input.order_id],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Order {}", input.order_id)))?;

        if status == "CONVERTED" {
            return Err(EngineError::OrderAlreadyConverted(format!("Order {} is already converted", input.order_id)));
        }
        if status != "DRAFT" {
            return Err(EngineError::OrderNotDraft(format!("Order {} status is '{}', cannot edit non-draft", input.order_id, status)));
        }

        if let Some(ref c_id) = customer_id {
            let is_active: i64 = conn
                .query_row(
                    "SELECT is_active FROM customers WHERE id = ?1",
                    params![c_id],
                    |r| r.get(0),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is inactive", c_id)));
            }
        }

        let mut enriched = Vec::new();
        for item in &consolidated {
            let (is_active, selling_price_cents): (i64, i64) = conn
                .query_row(
                    "SELECT is_active, selling_price_cents FROM products WHERE id = ?1",
                    params![item.product_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", item.product_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", item.product_id)));
            }

            enriched.push((item.product_id.clone(), item.quantity, selling_price_cents, item.notes.clone()));
        }

        let tx = conn.transaction()?;
        let now = format!("{:?}", SystemTime::now());

        let rows_affected = tx.execute(
            "UPDATE customer_orders SET customer_id = ?1, notes = ?2, updated_at = ?3 WHERE id = ?4 AND status = 'DRAFT'",
            params![customer_id, input.notes, now, input.order_id],
        )?;

        if rows_affected == 0 {
            return Err(EngineError::OrderNotDraft(format!("Order {} is no longer in DRAFT status", input.order_id)));
        }

        tx.execute(
            "DELETE FROM customer_order_items WHERE order_id = ?1",
            params![input.order_id],
        )?;

        for (idx, (p_id, qty, price, item_notes)) in enriched.iter().enumerate() {
            let item_id = format!("item_{}_{}", input.order_id, idx);
            tx.execute(
                "INSERT INTO customer_order_items (id, order_id, product_id, quantity, unit_price_cents, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![item_id, input.order_id, p_id, qty, price, item_notes],
            )?;
        }

        let audit_id = format!("audit_{}_{}", input.order_id, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'ORDER_UPDATED', 'customer_orders', ?3, ?4, ?5)",
            params![
                audit_id,
                identity.user_id(),
                input.order_id,
                format!("Updated draft customer order {}", input.order_id),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    get_customer_order_detail_inner(db, session, input.order_id)
}

pub fn cancel_customer_order_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    order_id: String,
) -> Result<(), String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        let status: String = conn
            .query_row(
                "SELECT status FROM customer_orders WHERE id = ?1",
                params![order_id],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Order {}", order_id)))?;

        if status == "CONVERTED" {
            return Err(EngineError::OrderAlreadyConverted(format!("Order {} is already converted", order_id)));
        }
        if status != "DRAFT" {
            return Err(EngineError::OrderNotDraft(format!("Order {} status is '{}', cannot cancel non-draft", order_id, status)));
        }

        let tx = conn.transaction()?;
        let now = format!("{:?}", SystemTime::now());

        let rows_affected = tx.execute(
            "UPDATE customer_orders SET status = 'CANCELLED', updated_at = ?1 WHERE id = ?2 AND status = 'DRAFT'",
            params![now, order_id],
        )?;

        if rows_affected == 0 {
            return Err(EngineError::OrderNotDraft(format!("Order {} is no longer in DRAFT status", order_id)));
        }

        let audit_id = format!("audit_{}_{}", order_id, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'ORDER_CANCELLED', 'customer_orders', ?3, ?4, ?5)",
            params![
                audit_id,
                identity.user_id(),
                order_id,
                format!("Cancelled draft customer order {}", order_id),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_order_conversion_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedOrderConversionCache,
    input: PrepareOrderConversionIpcInput,
) -> Result<PreparedOrderConversionQuote, String> {
    let identity = session.get_identity()?;

    let mode = input.settlement_mode.trim().to_uppercase();
    if mode != "PAID" && mode != "CREDIT" {
        return Err(format!(
            "Invalid settlement mode '{}'. BUILD 09 conversion only supports 'PAID' or 'CREDIT'",
            input.settlement_mode
        ));
    }

    let (prepared_quote, cached_item) = db
        .with_connection(|conn| {
            AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

            let (order_num, customer_id, status): (String, Option<String>, String) = conn
                .query_row(
                    "SELECT order_number, customer_id, status FROM customer_orders WHERE id = ?1",
                    params![input.order_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Order {}", input.order_id)))?;

            if status == "CONVERTED" {
                return Err(EngineError::OrderAlreadyConverted(format!("Order {} is already converted", input.order_id)));
            }
            if status != "DRAFT" {
                return Err(EngineError::OrderNotDraft(format!("Order {} is in status '{}', expected 'DRAFT'", input.order_id, status)));
            }

            let payment_method = if mode == "CREDIT" {
                if customer_id.is_none() {
                    return Err(EngineError::DatabaseError("Credit conversion requires a registered customer identity on the order".to_string()));
                }
                if let Some(ref pm) = input.payment_method {
                    if !pm.trim().is_empty() {
                        return Err(EngineError::DatabaseError("Payment method cannot be specified for CREDIT conversion".to_string()));
                    }
                }
                None
            } else {
                let pm = input
                    .payment_method
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| EngineError::DatabaseError("Payment method is required for PAID conversion".to_string()))?;

                let valid_methods = ["CASH", "UPI", "BANK_TRANSFER", "CARD", "OTHER"];
                if !valid_methods.contains(&pm.to_uppercase().as_str()) {
                    return Err(EngineError::DatabaseError(format!(
                        "Invalid payment method '{}'. Valid methods: {:?}",
                        pm, valid_methods
                    )));
                }
                Some(pm.to_uppercase())
            };

            let mut customer_name: Option<String> = None;
            let mut customer_phone: Option<String> = None;
            if let Some(ref c_id) = customer_id {
                let (name, phone, is_active): (String, Option<String>, i64) = conn
                    .query_row(
                        "SELECT name, phone, is_active FROM customers WHERE id = ?1",
                        params![c_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

                if is_active != 1 {
                    return Err(EngineError::DatabaseError(format!("Customer {} is inactive", c_id)));
                }
                customer_name = Some(name);
                customer_phone = phone;
            }

            let mut items_stmt = conn.prepare("SELECT product_id, quantity FROM customer_order_items WHERE order_id = ?1")?;
            let item_rows = items_stmt.query_map(params![input.order_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;

            let mut items = Vec::new();
            let mut enriched_items = Vec::new();
            let mut product_quantities = Vec::new();
            let mut calculated_total_cents: i64 = 0;

            for item_res in item_rows {
                let (product_id, quantity) = item_res?;
                if quantity <= 0 {
                    return Err(EngineError::InvalidQuantity {
                        product_id: product_id.clone(),
                        quantity,
                        message: "Order item quantity must be strictly positive".to_string(),
                    });
                }

                let (p_name, p_unit, p_selling_price, p_active): (String, String, i64, i64) = conn
                    .query_row(
                        "SELECT name, unit, selling_price_cents, is_active FROM products WHERE id = ?1",
                        params![product_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Product {}", product_id)))?;

                if p_active != 1 {
                    return Err(EngineError::DatabaseError(format!("Product {} is inactive", product_id)));
                }

                let available: i64 = conn
                    .query_row(
                        "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                        params![product_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);

                if available < quantity {
                    return Err(EngineError::InsufficientStock {
                        product_id: product_id.clone(),
                        available,
                        requested: quantity,
                    });
                }

                let line_total = calculate_line_total(quantity, p_selling_price)
                    .map_err(|e| EngineError::MathError(e.to_string()))?;

                calculated_total_cents = calculated_total_cents
                    .checked_add(line_total)
                    .ok_or_else(|| EngineError::MathError("Total sale amount overflow".to_string()))?;

                items.push(SaleItemRequest {
                    product_id: product_id.clone(),
                    quantity,
                    unit_price_cents: p_selling_price,
                });

                enriched_items.push(PreparedSaleItemQuote {
                    product_id: product_id.clone(),
                    product_name: p_name,
                    unit: p_unit,
                    quantity,
                    unit_price_cents: p_selling_price,
                    line_total_cents: line_total,
                });

                product_quantities.push((product_id, quantity));
            }

            if items.is_empty() {
                return Err(EngineError::OrderNotDraft("Cannot convert empty order with no items".to_string()));
            }

            let (paid_amount_cents, credit_amount_cents) = if mode == "PAID" {
                (calculated_total_cents, 0)
            } else {
                (0, calculated_total_cents)
            };

            let sale_id = format!(
                "sale_{}",
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            );
            let sale_number = format!(
                "INV-{}",
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
            );
            let sale_date = format!("{:?}", SystemTime::now());

            let conv_req = ConvertOrderToSaleRequest {
                order_id: input.order_id.clone(),
                sale_id: sale_id.clone(),
                sale_number: sale_number.clone(),
                paid_amount_cents,
                payment_method: payment_method.clone(),
                user_id: identity.user_id().to_string(),
                sale_date,
            };

            let prepared = BusinessEngine::prepare_order_conversion(conn, &identity, conv_req)?;

            let prep_token = generate_preparation_token();

            let quote = PreparedOrderConversionQuote {
                preparation_token: prep_token.clone(),
                order_id: input.order_id.clone(),
                order_number: order_num.clone(),
                sale_id: sale_id.clone(),
                sale_number: sale_number.clone(),
                customer_id: customer_id.clone(),
                customer_name,
                customer_phone,
                items: enriched_items,
                total_amount_cents: calculated_total_cents,
                paid_amount_cents,
                credit_amount_cents,
                settlement_mode: mode.clone(),
                payment_method: payment_method.clone(),
                prepared_at: prepared.prepared_at.clone(),
            };

            let cached = CachedPreparedOrderConversion {
                owner_user_id: identity.user_id().to_string(),
                prepared,
                order_id: input.order_id.clone(),
                order_number: order_num,
                customer_id,
                product_quantities,
                total_amount_cents: calculated_total_cents,
                paid_amount_cents,
                credit_amount_cents,
                settlement_mode: mode,
                payment_method,
            };

            Ok((quote, (prep_token, cached)))
        })
        .map_err(|e| e.to_string())?;

    let (prep_token, cached) = cached_item;
    if let Ok(mut entries) = cache.entries.lock() {
        entries.insert((identity.user_id().to_string(), prep_token), cached);
    }

    Ok(prepared_quote)
}

pub fn confirm_order_conversion_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedOrderConversionCache,
    input: ConfirmOrderConversionIpcInput,
) -> Result<SaleReceipt, String> {
    let identity = session.get_identity()?;

    let cached = {
        let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
        cache_guard
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| {
                "StaleOrInvalidPreparation: Prepared order conversion not found, already confirmed, or owned by another session".to_string()
            })?
    };

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::CustomerOrders.as_str())?;

        let order_status: String = conn
            .query_row(
                "SELECT status FROM customer_orders WHERE id = ?1",
                params![cached.order_id],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Order {}", cached.order_id)))?;

        if order_status == "CONVERTED" {
            return Err(EngineError::OrderAlreadyConverted(format!("Order {} is already converted", cached.order_id)));
        }
        if order_status != "DRAFT" {
            return Err(EngineError::OrderNotDraft(format!("Order {} status is '{}', expected 'DRAFT'", cached.order_id, order_status)));
        }

        if cached.settlement_mode == "CREDIT" {
            let c_id = cached.customer_id.as_deref().ok_or_else(|| {
                EngineError::DatabaseError("Credit conversion requires a registered customer".to_string())
            })?;
            let is_active: i64 = conn
                .query_row(
                    "SELECT is_active FROM customers WHERE id = ?1",
                    params![c_id],
                    |r| r.get(0),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is no longer active", c_id)));
            }
        } else if cached.settlement_mode == "PAID" {
            if let Some(ref c_id) = cached.customer_id {
                let is_active: i64 = conn
                    .query_row(
                        "SELECT is_active FROM customers WHERE id = ?1",
                        params![c_id],
                        |r| r.get(0),
                    )
                    .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

                if is_active != 1 {
                    return Err(EngineError::DatabaseError(format!("Customer {} is no longer active", c_id)));
                }
            }
        }

        for (p_id, qty) in &cached.product_quantities {
            let is_active: i64 = conn
                .query_row(
                    "SELECT is_active FROM products WHERE id = ?1",
                    params![p_id],
                    |r| r.get(0),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is no longer active", p_id)));
            }

            let available: i64 = conn
                .query_row(
                    "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                    params![p_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            if available < *qty {
                return Err(EngineError::InsufficientStock {
                    product_id: p_id.clone(),
                    available,
                    requested: *qty,
                });
            }
        }

        let target_sale_id = cached.prepared.payload.prepared_sale.sale_id.clone();

        let confirmed = cached.prepared.confirm(&identity);

        TransactionEngine::execute_order_conversion(conn, confirmed)?;

        let sale_row = conn.query_row(
            "SELECT s.id, s.sale_number, s.customer_id, c.name, c.phone,
                    s.total_amount_cents, s.paid_amount_cents, s.credit_amount_cents,
                    s.sale_date, s.created_at
             FROM sales s
             LEFT JOIN customers c ON s.customer_id = c.id
             WHERE s.id = ?1",
            params![target_sale_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                ))
            },
        )?;

        let payment_method: Option<String> = conn
            .query_row(
                "SELECT payment_method FROM payments WHERE related_entity_type = 'SALE' AND related_entity_id = ?1 LIMIT 1",
                params![target_sale_id],
                |r| r.get(0),
            )
            .ok();

        let mut item_stmt = conn.prepare(
            "SELECT si.product_id, p.name, p.unit, si.quantity, si.unit_price_cents, si.total_cents
             FROM sale_items si
             JOIN products p ON si.product_id = p.id
             WHERE si.sale_id = ?1
             ORDER BY si.id ASC",
        )?;

        let receipt_items = item_stmt
            .query_map(params![sale_row.0], |r| {
                Ok(SaleReceiptItem {
                    product_id: r.get(0)?,
                    product_name: r.get(1)?,
                    unit: r.get(2)?,
                    quantity: r.get(3)?,
                    unit_price_cents: r.get(4)?,
                    total_cents: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let receipt = SaleReceipt {
            sale_id: sale_row.0,
            sale_number: sale_row.1,
            customer_id: sale_row.2,
            customer_name: sale_row.3,
            customer_phone: sale_row.4,
            items: receipt_items,
            total_amount_cents: sale_row.5,
            paid_amount_cents: sale_row.6,
            credit_amount_cents: sale_row.7,
            settlement_mode: cached.settlement_mode.clone(),
            payment_method,
            sale_date: sale_row.8,
            created_at: sale_row.9,
        };

        Ok(receipt)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_customer_orders_form_data(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<CustomerOrdersFormData, String> {
    get_customer_orders_form_data_inner(&db, &session)
}

#[tauri::command]
pub fn get_customer_orders_summary(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    status_filter: Option<String>,
) -> Result<Vec<CustomerOrderSummaryItem>, String> {
    get_customer_orders_summary_inner(&db, &session, status_filter)
}

#[tauri::command]
pub fn get_customer_order_detail(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    order_id: String,
) -> Result<CustomerOrderDetail, String> {
    get_customer_order_detail_inner(&db, &session, order_id)
}

#[tauri::command]
pub fn create_customer_order(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: CreateCustomerOrderIpcInput,
) -> Result<CustomerOrderDetail, String> {
    create_customer_order_inner(&db, &session, input)
}

#[tauri::command]
pub fn update_customer_order(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: UpdateCustomerOrderIpcInput,
) -> Result<CustomerOrderDetail, String> {
    update_customer_order_inner(&db, &session, input)
}

#[tauri::command]
pub fn cancel_customer_order(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    order_id: String,
) -> Result<(), String> {
    cancel_customer_order_inner(&db, &session, order_id)
}

#[tauri::command]
pub fn prepare_order_conversion(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedOrderConversionCache>,
    input: PrepareOrderConversionIpcInput,
) -> Result<PreparedOrderConversionQuote, String> {
    prepare_order_conversion_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_order_conversion(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedOrderConversionCache>,
    input: ConfirmOrderConversionIpcInput,
) -> Result<SaleReceipt, String> {
    confirm_order_conversion_inner(&db, &session, &cache, input)
}

// ================================================================================================
// BUILD 10 — RETURNS & STOCK REVERSAL INNER LOGIC & TAURI COMMANDS
// ================================================================================================

pub fn get_returns_form_data_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<ReturnsFormData, String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        let mut cust_stmt = conn.prepare(
            "SELECT id, name, phone, current_credit_cents
             FROM customers
             WHERE is_active = 1
             ORDER BY name ASC",
        )?;
        let customers = cust_stmt
            .query_map([], |r| {
                Ok(CustomerSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    phone: r.get(2)?,
                    current_balance_cents: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut sup_stmt = conn.prepare(
            "SELECT id, name, phone, address, current_outstanding_cents
             FROM suppliers
             WHERE is_active = 1
             ORDER BY name ASC",
        )?;
        let suppliers = sup_stmt
            .query_map([], |r| {
                Ok(SupplierSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    phone: r.get(2)?,
                    address: r.get(3)?,
                    current_outstanding_cents: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut prod_stmt = conn.prepare(
            "SELECT p.id, p.name, p.product_type, p.unit, p.cost_price_cents, p.selling_price_cents,
                    COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.is_active = 1
             ORDER BY p.name ASC",
        )?;
        let products = prod_stmt
            .query_map([], |r| {
                Ok(ProductForSale {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    product_type: r.get(2)?,
                    unit: r.get(3)?,
                    cost_price_cents: r.get(4)?,
                    selling_price_cents: r.get(5)?,
                    current_quantity: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(ReturnsFormData {
            customers,
            suppliers,
            products,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn get_returns_summary_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    filter_type: Option<String>,
) -> Result<Vec<ReturnSummaryItem>, String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        let filter_val = filter_type
            .as_ref()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty() && s != "ALL");

        let mut stmt = conn.prepare(
            "SELECT r.id, r.return_number, r.return_type,
                    CASE WHEN r.return_type = 'CUSTOMER_RETURN' THEN c.name ELSE s.name END as counterparty_name,
                    (SELECT COUNT(*) FROM return_items WHERE return_id = r.id) as item_count,
                    r.total_amount_cents, r.reason, r.created_at
             FROM returns r
             LEFT JOIN customers c ON r.customer_id = c.id
             LEFT JOIN suppliers s ON r.supplier_id = s.id
             WHERE (?1 IS NULL OR r.return_type = ?1)
             ORDER BY r.created_at DESC, r.id DESC",
        )?;

        let rows = stmt.query_map(params![filter_val], |r| {
            Ok(ReturnSummaryItem {
                id: r.get(0)?,
                return_number: r.get(1)?,
                return_type: r.get(2)?,
                counterparty_name: r.get(3)?,
                item_count: r.get(4)?,
                total_amount_cents: r.get(5)?,
                reason: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?;

        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        Ok(list)
    })
    .map_err(|e| e.to_string())
}

pub fn get_return_detail_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    return_id: String,
) -> Result<ReturnDetail, String> {
    let identity = session.get_identity()?;
    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        let (
            id,
            return_number,
            return_type,
            reference_id,
            customer_id,
            customer_name,
            supplier_id,
            supplier_name,
            total_amount_cents,
            reason,
            admin_user_id,
            admin_user_name,
            created_at,
        ): (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            String,
            String,
            Option<String>,
            String,
        ) = conn
            .query_row(
                "SELECT r.id, r.return_number, r.return_type, r.reference_id,
                        r.customer_id, c.name, r.supplier_id, s.name,
                        r.total_amount_cents, r.reason, r.admin_user_id, u.username, r.created_at
                 FROM returns r
                 LEFT JOIN customers c ON r.customer_id = c.id
                 LEFT JOIN suppliers s ON r.supplier_id = s.id
                 LEFT JOIN users u ON r.admin_user_id = u.id
                 WHERE r.id = ?1",
                params![return_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                        r.get(11)?,
                        r.get(12)?,
                    ))
                },
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Return {}", return_id)))?;

        let mut item_stmt = conn.prepare(
            "SELECT ri.product_id, p.name, p.product_type, p.unit, ri.quantity, ri.unit_price_cents, ri.total_cents
             FROM return_items ri
             JOIN products p ON ri.product_id = p.id
             WHERE ri.return_id = ?1
             ORDER BY ri.id ASC",
        )?;

        let items = item_stmt
            .query_map(params![return_id], |r| {
                Ok(ReturnItemDetail {
                    product_id: r.get(0)?,
                    product_name: r.get(1)?,
                    product_type: r.get(2)?,
                    unit: r.get(3)?,
                    quantity: r.get(4)?,
                    unit_price_cents: r.get(5)?,
                    total_cents: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(ReturnDetail {
            id,
            return_number,
            return_type,
            reference_id,
            customer_id,
            customer_name,
            supplier_id,
            supplier_name,
            total_amount_cents,
            reason,
            admin_user_id,
            admin_user_name,
            items,
            created_at,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_customer_return_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedReturnCache,
    input: PrepareCustomerReturnIpcInput,
) -> Result<PreparedCustomerReturnQuote, String> {
    let identity = session.get_identity()?;

    if input.items.is_empty() {
        return Err("Cannot prepare a return with zero items".to_string());
    }

    for item in &input.items {
        if item.quantity <= 0 {
            return Err("Item quantity must be strictly greater than 0".to_string());
        }
    }

    if input.reason.trim().is_empty() {
        return Err("Return reason is required".to_string());
    }

    let customer_id = input
        .customer_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let reference_sale_id = input
        .reference_sale_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Consolidate duplicate products by summing quantities
    let mut consolidated_map: HashMap<String, i64> = HashMap::new();
    let mut product_order = Vec::new();
    for item in &input.items {
        let p_id = item.product_id.trim().to_string();
        if !consolidated_map.contains_key(&p_id) {
            product_order.push(p_id.clone());
        }
        *consolidated_map.entry(p_id).or_insert(0) += item.quantity;
    }
    let consolidated: Vec<(String, i64)> = product_order
        .into_iter()
        .map(|pid| {
            let qty = consolidated_map[&pid];
            (pid, qty)
        })
        .collect();

    let prep_token = generate_preparation_token();
    let return_id = format!(
        "ret_{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    );
    let preview_number = format!("RET-PREVIEW-{}", &prep_token[5..11]);

    let quote = db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        // 1. Verify customer if provided
        let (customer_name, customer_phone, customer_debt) = if let Some(ref c_id) = customer_id {
            let (is_active, name, phone, debt): (i64, String, Option<String>, i64) = conn
                .query_row(
                    "SELECT is_active, name, phone, current_credit_cents FROM customers WHERE id = ?1",
                    params![c_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is inactive", c_id)));
            }
            (Some(name), phone, debt)
        } else {
            (None, None, 0)
        };

        // 2. Validate products and calculate quote using catalog selling prices
        let mut quote_items = Vec::new();
        let mut total_amount_cents: i64 = 0;

        for (p_id, qty) in &consolidated {
            let (is_active, name, p_type, unit, selling_price_cents): (i64, String, String, String, i64) = conn
                .query_row(
                    "SELECT is_active, name, product_type, unit, selling_price_cents FROM products WHERE id = ?1",
                    params![p_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", p_id)));
            }

            let line_total = calculate_line_total(*qty, selling_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;
            total_amount_cents += line_total;

            quote_items.push(PreparedCustomerReturnItemQuote {
                product_id: p_id.clone(),
                product_name: name,
                product_type: p_type,
                unit,
                quantity: *qty,
                unit_price_cents: selling_price_cents,
                line_total_cents: line_total,
            });
        }

        // 3. Financial calculations: Khata debt offset vs cash/digital refund
        let (debt_reduction_cents, refund_amount_cents, balance_before, balance_after) = if customer_id.is_some() {
            let debt_offset = if customer_debt > 0 {
                std::cmp::min(total_amount_cents, customer_debt)
            } else {
                0
            };
            let refund = total_amount_cents - debt_offset;
            let bal_after = customer_debt - debt_offset;
            (debt_offset, refund, customer_debt, bal_after)
        } else {
            (0, total_amount_cents, 0, 0)
        };

        // 4. Validate refund payment method
        let refund_method = input
            .refund_payment_method
            .as_ref()
            .map(|s| s.trim().to_uppercase())
            .or_else(|| {
                if refund_amount_cents > 0 {
                    Some("CASH".to_string())
                } else {
                    None
                }
            });

        if let Some(ref m) = refund_method {
            match m.as_str() {
                "CASH" | "UPI" | "CARD" | "BANK_TRANSFER" => {}
                _ => return Err(EngineError::DatabaseError(format!("Invalid refund payment method: {}", m))),
            }
        }

        let now = format!("{:?}", SystemTime::now());

        Ok(PreparedCustomerReturnQuote {
            preparation_token: prep_token.clone(),
            return_id: return_id.clone(),
            return_number: preview_number,
            customer_id: customer_id.clone(),
            customer_name,
            customer_phone,
            reference_sale_id: reference_sale_id.clone(),
            items: quote_items,
            total_amount_cents,
            debt_reduction_cents,
            refund_amount_cents,
            refund_payment_method: refund_method.clone(),
            balance_before_cents: balance_before,
            balance_after_cents: balance_after,
            reason: input.reason.trim().to_string(),
            prepared_at: now,
        })
    })
    .map_err(|e| e.to_string())?;

    // Store in session cache
    let cached = CachedPreparedReturn {
        owner_user_id: identity.user_id().to_string(),
        return_id,
        return_type: "CUSTOMER_RETURN".to_string(),
        payload: CachedPreparedReturnPayload::Customer {
            customer_id,
            reference_sale_id,
            items: consolidated,
            reason: input.reason.trim().to_string(),
            refund_payment_method: quote.refund_payment_method.clone(),
        },
    };

    if let Ok(mut entries) = cache.entries.lock() {
        entries.insert((identity.user_id().to_string(), prep_token), cached);
    }

    Ok(quote)
}

pub fn confirm_customer_return_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedReturnCache,
    input: ConfirmCustomerReturnIpcInput,
) -> Result<CustomerReturnReceipt, String> {
    let identity = session.get_identity()?;

    let cached = {
        let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
        cache_guard
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| {
                "StaleOrInvalidPreparation: Prepared customer return not found, already confirmed, or owned by another session".to_string()
            })?
    };

    let (customer_id, reference_sale_id, items, reason, refund_payment_method) = match cached.payload {
        CachedPreparedReturnPayload::Customer {
            customer_id,
            reference_sale_id,
            items,
            reason,
            refund_payment_method,
        } => (customer_id, reference_sale_id, items, reason, refund_payment_method),
        _ => return Err("Invalid return type in cache for customer return".to_string()),
    };

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        let tx = conn.transaction()?;
        let now = format!("{:?}", SystemTime::now());

        // GUARDRAIL 3 — LIVE REVALIDATION: customer existence and LIVE debt balance
        let (customer_name, live_customer_debt) = if let Some(ref c_id) = customer_id {
            let (is_active, name, debt): (i64, String, i64) = tx
                .query_row(
                    "SELECT is_active, name, current_credit_cents FROM customers WHERE id = ?1",
                    params![c_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", c_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Customer {} is inactive", c_id)));
            }
            (Some(name), debt)
        } else {
            (None, 0)
        };

        // GUARDRAIL 3 — LIVE REVALIDATION: product existence, active status, and LIVE catalog selling price
        let mut enriched_items = Vec::new();
        let mut live_total_amount_cents: i64 = 0;

        for (p_id, qty) in &items {
            let (is_active, name, p_type, unit, selling_price_cents): (i64, String, String, String, i64) = tx
                .query_row(
                    "SELECT is_active, name, product_type, unit, selling_price_cents FROM products WHERE id = ?1",
                    params![p_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", p_id)));
            }

            let line_total = calculate_line_total(*qty, selling_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;
            live_total_amount_cents += line_total;

            enriched_items.push((
                p_id.clone(),
                name,
                p_type,
                unit,
                *qty,
                selling_price_cents,
                line_total,
            ));
        }

        // GUARDRAIL 3 — LIVE Financial revalidation: authoritative debt offset vs refund amount
        let (live_debt_reduction_cents, live_refund_amount_cents, bal_before, bal_after) = if customer_id.is_some() {
            let debt_offset = if live_customer_debt > 0 {
                std::cmp::min(live_total_amount_cents, live_customer_debt)
            } else {
                0
            };
            let refund = live_total_amount_cents - debt_offset;
            let after = live_customer_debt - debt_offset;
            (debt_offset, refund, live_customer_debt, after)
        } else {
            (0, live_total_amount_cents, 0, 0)
        };

        // Live validation of refund payment method
        let effective_refund_method = if live_refund_amount_cents > 0 {
            let m = refund_payment_method
                .as_deref()
                .unwrap_or("CASH");
            match m {
                "CASH" | "UPI" | "CARD" | "BANK_TRANSFER" => Some(m.to_string()),
                _ => {
                    return Err(EngineError::DatabaseError(
                        "Valid refund payment method (CASH, UPI, CARD, BANK_TRANSFER) is required when refund payout is owed"
                            .to_string(),
                    ))
                }
            }
        } else {
            None
        };

        // GUARDRAIL 4 — Collision-safe return number generation strictly on the backend
        let return_number = generate_return_number(&tx, "CUSTOMER_RETURN")?;

        // 1. Insert authoritative return record
        tx.execute(
            "INSERT INTO returns (id, return_number, return_type, reference_id, customer_id, supplier_id, total_amount_cents, reason, admin_user_id, created_at)
             VALUES (?1, ?2, 'CUSTOMER_RETURN', ?3, ?4, NULL, ?5, ?6, ?7, ?8)",
            params![
                cached.return_id,
                return_number,
                reference_sale_id,
                customer_id,
                live_total_amount_cents,
                reason,
                identity.user_id(),
                now,
            ],
        )?;

        // 2. Insert return items and atomically reverse stock (increment inventory)
        for (idx, (p_id, _name, _p_type, _unit, qty, unit_price, total_cents)) in enriched_items.iter().enumerate() {
            let item_id = format!("{}_item_{}", cached.return_id, idx);
            tx.execute(
                "INSERT INTO return_items (id, return_id, product_id, quantity, unit_price_cents, total_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![item_id, cached.return_id, p_id, qty, unit_price, total_cents],
            )?;

            // Ensure inventory row exists
            tx.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES (?1, ?2, 0, ?3)
                 ON CONFLICT(product_id) DO NOTHING",
                params![format!("inv_{}", p_id), p_id, now],
            )?;

            let qty_before: i64 = tx.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![p_id],
                |r| r.get(0),
            )?;
            let qty_after = qty_before + qty;

            tx.execute(
                "UPDATE inventory SET current_quantity = ?1, last_updated_at = ?2 WHERE product_id = ?3",
                params![qty_after, now, p_id],
            )?;

            let movement_id = format!("{}_mov_{}", cached.return_id, idx);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'CUSTOMER_RETURN', 'RETURN', ?6, ?7, ?8)",
                params![movement_id, p_id, qty, qty_before, qty_after, cached.return_id, identity.user_id(), now],
            )?;
        }

        // 3. Atomically update customer credit if debt reduction occurred
        if live_debt_reduction_cents > 0 {
            let c_id = customer_id.as_ref().unwrap();
            let rows = tx.execute(
                "UPDATE customers
                 SET current_credit_cents = current_credit_cents - ?1,
                     updated_at = ?2
                 WHERE id = ?3
                   AND current_credit_cents >= ?1",
                params![live_debt_reduction_cents, now, c_id],
            )?;

            if rows == 0 {
                return Err(EngineError::DatabaseError(
                    "Customer debt balance changed concurrently during return confirmation".to_string(),
                ));
            }

            let ledger_id = format!("{}_cleg", cached.return_id);
            tx.execute(
                "INSERT INTO customer_ledger (id, customer_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
                 VALUES (?1, ?2, 'RETURN_CREDIT', ?3, ?4, ?5, 'RETURN', ?6, ?7, ?8, ?9)",
                params![
                    ledger_id,
                    c_id,
                    live_debt_reduction_cents,
                    bal_before,
                    bal_after,
                    cached.return_id,
                    format!("Debt offset from return {}", return_number),
                    identity.user_id(),
                    now,
                ],
            )?;
        }

        // 4. Record refund payout in payments table if refund amount > 0
        if live_refund_amount_cents > 0 {
            let payment_id = format!(
                "pay_{}_{}",
                cached.return_id,
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            );
            tx.execute(
                "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
                 VALUES (?1, 'CUSTOMER_RETURN_REFUND', 'RETURN', ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    payment_id,
                    cached.return_id,
                    live_refund_amount_cents,
                    effective_refund_method.as_ref().unwrap(),
                    identity.user_id(),
                    format!("Refund payout for return {}", return_number),
                    now,
                ],
            )?;
        }

        // 5. Append audit log
        let audit_id = format!(
            "audit_{}_{}",
            cached.return_id,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        );
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'CUSTOMER_RETURN', 'returns', ?3, ?4, ?5)",
            params![
                audit_id,
                identity.user_id(),
                cached.return_id,
                format!("Confirmed customer return {} total {}", return_number, live_total_amount_cents),
                now,
            ],
        )?;

        tx.commit()?;

        let receipt_items = enriched_items
            .into_iter()
            .map(|(p_id, name, p_type, unit, qty, price, total)| PreparedCustomerReturnItemQuote {
                product_id: p_id,
                product_name: name,
                product_type: p_type,
                unit,
                quantity: qty,
                unit_price_cents: price,
                line_total_cents: total,
            })
            .collect();

        Ok(CustomerReturnReceipt {
            return_id: cached.return_id,
            return_number,
            customer_id,
            customer_name,
            items: receipt_items,
            total_amount_cents: live_total_amount_cents,
            debt_reduction_cents: live_debt_reduction_cents,
            refund_amount_cents: live_refund_amount_cents,
            refund_payment_method: effective_refund_method,
            balance_before_cents: bal_before,
            balance_after_cents: bal_after,
            reason,
            created_at: now,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_supplier_return_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedReturnCache,
    input: PrepareSupplierReturnIpcInput,
) -> Result<PreparedSupplierReturnQuote, String> {
    let identity = session.get_identity()?;

    if input.items.is_empty() {
        return Err("Cannot prepare a return with zero items".to_string());
    }

    for item in &input.items {
        if item.quantity <= 0 {
            return Err("Item quantity must be strictly greater than 0".to_string());
        }
    }

    if input.reason.trim().is_empty() {
        return Err("Return reason is required".to_string());
    }

    let supplier_id = input
        .supplier_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let reference_purchase_id = input
        .reference_purchase_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut consolidated_map: HashMap<String, i64> = HashMap::new();
    let mut product_order = Vec::new();
    for item in &input.items {
        let p_id = item.product_id.trim().to_string();
        if !consolidated_map.contains_key(&p_id) {
            product_order.push(p_id.clone());
        }
        *consolidated_map.entry(p_id).or_insert(0) += item.quantity;
    }
    let consolidated: Vec<(String, i64)> = product_order
        .into_iter()
        .map(|pid| {
            let qty = consolidated_map[&pid];
            (pid, qty)
        })
        .collect();

    let prep_token = generate_preparation_token();
    let return_id = format!(
        "ret_{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    );
    let preview_number = format!("SR-PREVIEW-{}", &prep_token[5..11]);

    let quote = db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        // 1. Verify supplier if provided
        let (supplier_name, supplier_balance) = if let Some(ref s_id) = supplier_id {
            let (is_active, name, bal): (i64, String, i64) = conn
                .query_row(
                    "SELECT is_active, name, current_outstanding_cents FROM suppliers WHERE id = ?1",
                    params![s_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Supplier {}", s_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Supplier {} is inactive", s_id)));
            }
            (Some(name), bal)
        } else {
            (None, 0)
        };

        // 2. Validate products and calculate quote using catalog cost prices and check stock
        let mut quote_items = Vec::new();
        let mut total_amount_cents: i64 = 0;

        for (p_id, qty) in &consolidated {
            let (is_active, name, p_type, unit, cost_price_cents): (i64, String, String, String, i64) = conn
                .query_row(
                    "SELECT is_active, name, product_type, unit, cost_price_cents FROM products WHERE id = ?1",
                    params![p_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", p_id)));
            }

            let available_stock: i64 = conn
                .query_row(
                    "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                    params![p_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let line_total = calculate_line_total(*qty, cost_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;
            total_amount_cents += line_total;

            quote_items.push(PreparedSupplierReturnItemQuote {
                product_id: p_id.clone(),
                product_name: name,
                product_type: p_type,
                unit,
                quantity: *qty,
                unit_cost_cents: cost_price_cents,
                line_total_cents: line_total,
                available_stock,
            });
        }

        // GUARDRAIL 2: Signed supplier balance arithmetic (no zero clamping)
        let (balance_before, balance_after) = if supplier_id.is_some() {
            let bal_after = supplier_balance - total_amount_cents;
            (supplier_balance, bal_after)
        } else {
            (0, 0)
        };

        let now = format!("{:?}", SystemTime::now());

        Ok(PreparedSupplierReturnQuote {
            preparation_token: prep_token.clone(),
            return_id: return_id.clone(),
            return_number: preview_number,
            supplier_id: supplier_id.clone(),
            supplier_name,
            reference_purchase_id: reference_purchase_id.clone(),
            items: quote_items,
            total_amount_cents,
            balance_before_cents: balance_before,
            balance_after_cents: balance_after,
            reason: input.reason.trim().to_string(),
            prepared_at: now,
        })
    })
    .map_err(|e| e.to_string())?;

    let cached = CachedPreparedReturn {
        owner_user_id: identity.user_id().to_string(),
        return_id,
        return_type: "SUPPLIER_RETURN".to_string(),
        payload: CachedPreparedReturnPayload::Supplier {
            supplier_id,
            reference_purchase_id,
            items: consolidated,
            reason: input.reason.trim().to_string(),
        },
    };

    if let Ok(mut entries) = cache.entries.lock() {
        entries.insert((identity.user_id().to_string(), prep_token), cached);
    }

    Ok(quote)
}

pub fn confirm_supplier_return_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedReturnCache,
    input: ConfirmSupplierReturnIpcInput,
) -> Result<SupplierReturnReceipt, String> {
    let identity = session.get_identity()?;

    let cached = {
        let mut cache_guard = cache.entries.lock().map_err(|e| e.to_string())?;
        cache_guard
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| {
                "StaleOrInvalidPreparation: Prepared supplier return not found, already confirmed, or owned by another session".to_string()
            })?
    };

    let (supplier_id, reference_purchase_id, items, reason) = match cached.payload {
        CachedPreparedReturnPayload::Supplier {
            supplier_id,
            reference_purchase_id,
            items,
            reason,
        } => (supplier_id, reference_purchase_id, items, reason),
        _ => return Err("Invalid return type in cache for supplier return".to_string()),
    };

    db.with_connection(|conn| {
        AuthorizationService::authorize(conn, &identity, PermissionKey::Returns.as_str())?;

        let tx = conn.transaction()?;
        let now = format!("{:?}", SystemTime::now());

        // GUARDRAIL 3 — LIVE REVALIDATION: supplier existence and LIVE balance
        let (supplier_name, live_supplier_balance) = if let Some(ref s_id) = supplier_id {
            let (is_active, name, bal): (i64, String, i64) = tx
                .query_row(
                    "SELECT is_active, name, current_outstanding_cents FROM suppliers WHERE id = ?1",
                    params![s_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Supplier {}", s_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Supplier {} is inactive", s_id)));
            }
            (Some(name), bal)
        } else {
            (None, 0)
        };

        // GUARDRAIL 3 — LIVE REVALIDATION: product existence, active status, and LIVE catalog cost price
        let mut enriched_items = Vec::new();
        let mut live_total_amount_cents: i64 = 0;

        for (p_id, qty) in &items {
            let (is_active, name, p_type, unit, cost_price_cents): (i64, String, String, String, i64) = tx
                .query_row(
                    "SELECT is_active, name, product_type, unit, cost_price_cents FROM products WHERE id = ?1",
                    params![p_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", p_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", p_id)));
            }

            let line_total = calculate_line_total(*qty, cost_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;
            live_total_amount_cents += line_total;

            enriched_items.push((
                p_id.clone(),
                name,
                p_type,
                unit,
                *qty,
                cost_price_cents,
                line_total,
            ));
        }

        // GUARDRAIL 4 — Collision-safe return number generation strictly on the backend
        let return_number = generate_return_number(&tx, "SUPPLIER_RETURN")?;

        // 1. Insert authoritative return record
        tx.execute(
            "INSERT INTO returns (id, return_number, return_type, reference_id, customer_id, supplier_id, total_amount_cents, reason, admin_user_id, created_at)
             VALUES (?1, ?2, 'SUPPLIER_RETURN', ?3, NULL, ?4, ?5, ?6, ?7, ?8)",
            params![
                cached.return_id,
                return_number,
                reference_purchase_id,
                supplier_id,
                live_total_amount_cents,
                reason,
                identity.user_id(),
                now,
            ],
        )?;

        // 2. Insert return items and atomically decrement inventory with Guardrail 1 authoritative stock protection
        for (idx, (p_id, _name, _p_type, _unit, qty, cost_price, line_total)) in enriched_items.iter().enumerate() {
            let item_id = format!("{}_item_{}", cached.return_id, idx);
            tx.execute(
                "INSERT INTO return_items (id, return_id, product_id, quantity, unit_price_cents, total_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![item_id, cached.return_id, p_id, qty, cost_price, line_total],
            )?;

            let qty_before: i64 = tx
                .query_row(
                    "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                    params![p_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            // GUARDRAIL 1 — SQLite Concurrency & Atomic Conditional Decrement
            let rows_affected = tx.execute(
                "UPDATE inventory
                 SET current_quantity = current_quantity - ?1,
                     last_updated_at = ?2
                 WHERE product_id = ?3
                   AND current_quantity >= ?1",
                params![qty, now, p_id],
            )?;

            if rows_affected == 0 {
                // If 0 rows affected, stock is insufficient; abort transaction with InsufficientStock
                return Err(EngineError::InsufficientStock {
                    product_id: p_id.clone(),
                    available: qty_before,
                    requested: *qty,
                });
            }

            let qty_after = qty_before - qty;

            let movement_id = format!("{}_mov_{}", cached.return_id, idx);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, -?3, ?4, ?5, 'SUPPLIER_RETURN', 'RETURN', ?6, ?7, ?8)",
                params![movement_id, p_id, qty, qty_before, qty_after, cached.return_id, identity.user_id(), now],
            )?;
        }

        // 3. GUARDRAIL 2: Supplier Signed-Balance Update & Ledger Entry
        let (bal_before, bal_after) = if let Some(ref s_id) = supplier_id {
            let before = live_supplier_balance;
            let after = before - live_total_amount_cents; // Signed! Can be negative!

            tx.execute(
                "UPDATE suppliers
                 SET current_outstanding_cents = ?1,
                     updated_at = ?2
                 WHERE id = ?3",
                params![after, now, s_id],
            )?;

            let ledger_id = format!("{}_sleg", cached.return_id);
            tx.execute(
                "INSERT INTO supplier_ledger (id, supplier_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
                 VALUES (?1, ?2, 'RETURN_DEBIT', ?3, ?4, ?5, 'RETURN', ?6, ?7, ?8, ?9)",
                params![
                    ledger_id,
                    s_id,
                    live_total_amount_cents,
                    before,
                    after,
                    cached.return_id,
                    format!("Stock return debit {}", return_number),
                    identity.user_id(),
                    now,
                ],
            )?;

            (before, after)
        } else {
            (0, 0)
        };

        // 4. Audit log
        let audit_id = format!(
            "audit_{}_{}",
            cached.return_id,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        );
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'SUPPLIER_RETURN', 'returns', ?3, ?4, ?5)",
            params![
                audit_id,
                identity.user_id(),
                cached.return_id,
                format!("Confirmed supplier return {} total {}", return_number, live_total_amount_cents),
                now,
            ],
        )?;

        tx.commit()?;

        let receipt_items = enriched_items
            .into_iter()
            .map(|(p_id, name, p_type, unit, qty, cost_price, line_total)| PreparedSupplierReturnItemQuote {
                product_id: p_id,
                product_name: name,
                product_type: p_type,
                unit,
                quantity: qty,
                unit_cost_cents: cost_price,
                line_total_cents: line_total,
                available_stock: 0,
            })
            .collect();

        Ok(SupplierReturnReceipt {
            return_id: cached.return_id,
            return_number,
            supplier_id,
            supplier_name,
            items: receipt_items,
            total_amount_cents: live_total_amount_cents,
            balance_before_cents: bal_before,
            balance_after_cents: bal_after,
            reason,
            created_at: now,
        })
    })
    .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------------------------------------
// Tauri IPC Command Handlers for Returns (BUILD 10)
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn get_returns_form_data(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<ReturnsFormData, String> {
    get_returns_form_data_inner(&db, &session)
}

#[tauri::command]
pub fn get_returns_summary(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    filter_type: Option<String>,
) -> Result<Vec<ReturnSummaryItem>, String> {
    get_returns_summary_inner(&db, &session, filter_type)
}

#[tauri::command]
pub fn get_return_detail(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    return_id: String,
) -> Result<ReturnDetail, String> {
    get_return_detail_inner(&db, &session, return_id)
}

#[tauri::command]
pub fn prepare_customer_return(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedReturnCache>,
    input: PrepareCustomerReturnIpcInput,
) -> Result<PreparedCustomerReturnQuote, String> {
    prepare_customer_return_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_customer_return(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedReturnCache>,
    input: ConfirmCustomerReturnIpcInput,
) -> Result<CustomerReturnReceipt, String> {
    confirm_customer_return_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn prepare_supplier_return(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedReturnCache>,
    input: PrepareSupplierReturnIpcInput,
) -> Result<PreparedSupplierReturnQuote, String> {
    prepare_supplier_return_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_supplier_return(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedReturnCache>,
    input: ConfirmSupplierReturnIpcInput,
) -> Result<SupplierReturnReceipt, String> {
    confirm_supplier_return_inner(&db, &session, &cache, input)
}

// ================================================================================================
// BUILD 11: STOCK CORRECTIONS & PHYSICAL INVENTORY ADJUSTMENT DTOs & HANDLERS
// ================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductForCorrectionDto {
    pub id: String,
    pub name: String,
    pub product_type: String,
    pub unit: String,
    pub current_quantity: i64,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCorrectionsFormDataDto {
    pub products: Vec<ProductForCorrectionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareStockCorrectionPayload {
    pub product_id: String,
    pub quantity_change: i64, // signed delta in millie-units (scale 1000)
    pub reason: String,       // "DAMAGED" | "EXPIRED" | "LOST" | "MISCOUNT"
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedStockCorrectionQuoteDto {
    pub preparation_token: String,
    pub correction_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity_change: i64,
    pub quantity_before: i64,
    pub quantity_after: i64,
    pub reason: String,
    pub note: String,
    pub admin_user_id: String,
    pub admin_username: String,
    pub prepared_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmStockCorrectionPayload {
    pub preparation_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCorrectionReceiptDto {
    pub correction_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub quantity_change: i64,
    pub quantity_before: i64,
    pub quantity_after: i64,
    pub reason: String,
    pub note: String,
    pub admin_user_id: String,
    pub admin_username: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCorrectionHistoryItemDto {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub unit: String,
    pub quantity_change: i64,
    pub quantity_before: i64,
    pub quantity_after: i64,
    pub reason: String,
    pub note: String,
    pub admin_user_id: String,
    pub admin_username: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCorrectionsSummaryDto {
    pub corrections: Vec<StockCorrectionHistoryItemDto>,
    pub total_corrections_count: usize,
}

pub fn get_stock_corrections_form_data_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<StockCorrectionsFormDataDto, String> {
    let _identity = session.get_identity()?;
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, COALESCE(p.product_type, 'PACKAGED'), p.unit, COALESCE(i.current_quantity, 0), p.cost_price_cents, p.selling_price_cents
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.is_active = 1
             ORDER BY p.name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProductForCorrectionDto {
                id: row.get(0)?,
                name: row.get(1)?,
                product_type: row.get(2)?,
                unit: row.get(3)?,
                current_quantity: row.get(4)?,
                cost_price_cents: row.get(5)?,
                selling_price_cents: row.get(6)?,
            })
        })?;

        let mut products = Vec::new();
        for r in rows {
            products.push(r?);
        }

        Ok(StockCorrectionsFormDataDto { products })
    })
    .map_err(|e| e.to_string())
}

pub fn get_stock_corrections_summary_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<StockCorrectionsSummaryDto, String> {
    let _identity = session.get_identity()?;
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT sc.id, sc.product_id, p.name, p.unit, sc.quantity_change,
                    COALESCE(sm.quantity_before, 0), COALESCE(sm.quantity_after, 0),
                    sc.reason, sc.note, sc.admin_user_id, COALESCE(u.username, 'Admin'), sc.created_at
             FROM stock_corrections sc
             JOIN products p ON sc.product_id = p.id
             LEFT JOIN stock_movements sm ON sm.reference_type = 'STOCK_CORRECTIONS' AND sm.reference_id = sc.id
             LEFT JOIN users u ON sc.admin_user_id = u.id
             ORDER BY sc.created_at DESC
             LIMIT 100",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(StockCorrectionHistoryItemDto {
                id: row.get(0)?,
                product_id: row.get(1)?,
                product_name: row.get(2)?,
                unit: row.get(3)?,
                quantity_change: row.get(4)?,
                quantity_before: row.get(5)?,
                quantity_after: row.get(6)?,
                reason: row.get(7)?,
                note: row.get(8)?,
                admin_user_id: row.get(9)?,
                admin_username: row.get(10)?,
                created_at: row.get(11)?,
            })
        })?;

        let mut corrections = Vec::new();
        for r in rows {
            corrections.push(r?);
        }
        let total_corrections_count = corrections.len();

        Ok(StockCorrectionsSummaryDto {
            corrections,
            total_corrections_count,
        })
    })
    .map_err(|e| e.to_string())
}

pub fn prepare_stock_correction_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedStockCorrectionCache,
    input: PrepareStockCorrectionPayload,
) -> Result<PreparedStockCorrectionQuoteDto, String> {
    let identity = session.get_identity()?;

    // 1. Authorize: Admin role strictly required for physical stock correction
    if identity.role() != crate::auth::Role::Admin {
        return Err("Unauthorized: Only administrators may perform physical stock corrections".to_string());
    }

    if input.quantity_change == 0 {
        return Err("Validation Error: Stock correction delta cannot be zero".to_string());
    }

    let note_trimmed = input.note.trim().to_string();
    if note_trimmed.is_empty() {
        return Err("Validation Error: An explanatory note is required for stock corrections".to_string());
    }

    match input.reason.as_str() {
        "DAMAGED" | "EXPIRED" | "LOST" | "MISCOUNT" => {}
        _ => return Err(format!("Validation Error: Invalid correction reason '{}'", input.reason)),
    }

    let (prepared, product_name, product_type, unit) = db.with_connection(|conn| {
        // Verify product exists and is active
        let (prod_name, prod_type, p_unit, is_active): (String, String, String, i64) = conn.query_row(
            "SELECT name, COALESCE(product_type, 'PACKAGED'), unit, is_active FROM products WHERE id = ?1",
            params![input.product_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|_| EngineError::EntityNotFound(format!("Product {}", input.product_id)))?;

        if is_active != 1 {
            return Err(EngineError::DatabaseError(format!("Product '{}' is inactive and cannot receive stock corrections", prod_name)));
        }

        let correction_id = format!(
            "corr_{}_{}",
            input.product_id,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        );

        let req = RecordStockCorrectionRequest {
            correction_id,
            product_id: input.product_id.clone(),
            quantity_change: input.quantity_change,
            reason: input.reason.clone(),
            note: note_trimmed.clone(),
            admin_user_id: identity.user_id().to_string(),
        };

        let prep = BusinessEngine::prepare_stock_correction(conn, &identity, req)?;

        Ok((prep, prod_name, prod_type, p_unit))
    }).map_err(|e| e.to_string())?;

    let token = generate_preparation_token();
    let prepared_at = format!("{:?}", SystemTime::now());

    let cached = CachedPreparedStockCorrection {
        owner_user_id: identity.user_id().to_string(),
        correction_id: prepared.payload.correction_id.clone(),
        product_id: input.product_id.clone(),
        product_name: product_name.clone(),
        product_type: product_type.clone(),
        unit: unit.clone(),
        quantity_change: prepared.payload.quantity_change,
        quantity_before: prepared.payload.quantity_before,
        quantity_after: prepared.payload.quantity_after,
        reason: prepared.payload.reason.clone(),
        note: prepared.payload.note.clone(),
        admin_user_id: identity.user_id().to_string(),
        admin_username: identity.username().to_string(),
    };

    {
        let mut entries = cache.entries.lock().map_err(|e| e.to_string())?;
        entries.insert((identity.user_id().to_string(), token.clone()), cached);
    }

    Ok(PreparedStockCorrectionQuoteDto {
        preparation_token: token,
        correction_id: prepared.payload.correction_id,
        product_id: input.product_id,
        product_name,
        product_type,
        unit,
        quantity_change: prepared.payload.quantity_change,
        quantity_before: prepared.payload.quantity_before,
        quantity_after: prepared.payload.quantity_after,
        reason: prepared.payload.reason,
        note: prepared.payload.note,
        admin_user_id: identity.user_id().to_string(),
        admin_username: identity.username().to_string(),
        prepared_at,
    })
}

pub fn confirm_stock_correction_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    cache: &PreparedStockCorrectionCache,
    input: ConfirmStockCorrectionPayload,
) -> Result<StockCorrectionReceiptDto, String> {
    let identity = session.get_identity()?;

    if identity.role() != crate::auth::Role::Admin {
        return Err("Unauthorized: Only administrators may confirm stock corrections".to_string());
    }

    let cached = {
        let mut entries = cache.entries.lock().map_err(|e| e.to_string())?;
        entries
            .remove(&(identity.user_id().to_string(), input.preparation_token.clone()))
            .ok_or_else(|| "Invalid, expired, or already-used preparation token".to_string())?
    };

    db.with_connection(|conn| {
        // Live revalidation of product and current inventory
        let (p_name, p_type, unit, is_active): (String, String, String, i64) = conn.query_row(
            "SELECT name, COALESCE(product_type, 'PACKAGED'), unit, is_active FROM products WHERE id = ?1",
            params![cached.product_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|_| EngineError::EntityNotFound(format!("Product {}", cached.product_id)))?;

        if is_active != 1 {
            return Err(EngineError::DatabaseError(format!("Product '{}' is inactive and cannot receive stock corrections", p_name)));
        }

        let live_qty_before: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = ?1",
            params![cached.product_id],
            |row| row.get(0),
        ).map_err(|_| EngineError::EntityNotFound(format!("Inventory record not found for product {}", cached.product_id)))?;

        let live_qty_after = live_qty_before + cached.quantity_change;
        if live_qty_after < 0 {
            return Err(EngineError::InsufficientStock {
                product_id: cached.product_id.clone(),
                available: live_qty_before,
                requested: cached.quantity_change.abs(),
            });
        }

        let prep = Prepared::new(PreparedStockCorrection {
            correction_id: cached.correction_id.clone(),
            product_id: cached.product_id.clone(),
            quantity_change: cached.quantity_change,
            quantity_before: live_qty_before,
            quantity_after: live_qty_after,
            reason: cached.reason.clone(),
            note: cached.note.clone(),
            admin_user_id: identity.user_id().to_string(),
        });
        let confirmed = prep.confirm(&identity);
        let confirmed_at = confirmed.confirmed_at().to_string();

        TransactionEngine::execute_stock_correction(conn, confirmed)?;

        Ok(StockCorrectionReceiptDto {
            correction_id: cached.correction_id,
            product_id: cached.product_id,
            product_name: p_name,
            product_type: p_type,
            unit,
            quantity_change: cached.quantity_change,
            quantity_before: live_qty_before,
            quantity_after: live_qty_after,
            reason: cached.reason,
            note: cached.note,
            admin_user_id: identity.user_id().to_string(),
            admin_username: identity.username().to_string(),
            created_at: confirmed_at,
        })
    })
    .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------------------------------------
// Tauri IPC Command Handlers for Stock Corrections (BUILD 11)
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn get_stock_corrections_form_data(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<StockCorrectionsFormDataDto, String> {
    get_stock_corrections_form_data_inner(&db, &session)
}

#[tauri::command]
pub fn get_stock_corrections_summary(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<StockCorrectionsSummaryDto, String> {
    get_stock_corrections_summary_inner(&db, &session)
}

#[tauri::command]
pub fn prepare_stock_correction(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedStockCorrectionCache>,
    input: PrepareStockCorrectionPayload,
) -> Result<PreparedStockCorrectionQuoteDto, String> {
    prepare_stock_correction_inner(&db, &session, &cache, input)
}

#[tauri::command]
pub fn confirm_stock_correction(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    cache: tauri::State<PreparedStockCorrectionCache>,
    input: ConfirmStockCorrectionPayload,
) -> Result<StockCorrectionReceiptDto, String> {
    confirm_stock_correction_inner(&db, &session, &cache, input)
}

// ==============================================================================================
// BUILD 12: OFFLINE SYNC & BACKUP COMMANDS
// ==============================================================================================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatusDto {
    pub settings: crate::backup::BackupSettings,
    pub backups: Vec<crate::backup::BackupMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleAutoBackupPayload {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateManualBackupPayload {
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerAutoBackupPayload {
    #[serde(alias = "is_internet_available")]
    pub is_internet_available: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateBackupPayload {
    #[serde(alias = "file_name")]
    pub file_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreBackupPayload {
    #[serde(alias = "file_name")]
    pub file_name: String,
}


#[tauri::command]
pub fn get_backup_status(
    db: tauri::State<DatabaseManager>,
    _session: tauri::State<AuthSession>,
) -> Result<BackupStatusDto, String> {
    let backup_dir = crate::backup::BackupService::get_default_backup_dir(&db);
    let settings = crate::backup::BackupService::get_settings(&db, &backup_dir)
        .map_err(|e| e.to_string())?;
    let backups = crate::backup::BackupService::list_backups(&backup_dir)
        .map_err(|e| e.to_string())?;
    Ok(BackupStatusDto { settings, backups })
}

#[tauri::command]
pub fn toggle_auto_backup(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: ToggleAutoBackupPayload,
) -> Result<crate::backup::BackupSettings, String> {
    let identity = session.get_identity()?;
    if !identity.is_admin() {
        return Err("Only an Admin can change automatic backup settings".to_string());
    }
    let backup_dir = crate::backup::BackupService::get_default_backup_dir(&db);
    crate::backup::BackupService::update_auto_backup_setting(&db, input.enabled, &backup_dir)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_manual_backup(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: CreateManualBackupPayload,
) -> Result<crate::backup::BackupMetadata, String> {
    let identity = session.get_identity()?;
    let backup_dir = crate::backup::BackupService::get_default_backup_dir(&db);
    crate::backup::BackupService::create_backup(
        &db,
        &identity,
        "MANUAL",
        input.note.as_deref(),
        Some(&backup_dir),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn trigger_auto_backup(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: TriggerAutoBackupPayload,
) -> Result<Option<crate::backup::BackupMetadata>, String> {
    let identity = session.get_identity()?;
    let backup_dir = crate::backup::BackupService::get_default_backup_dir(&db);
    let internet = input.is_internet_available.unwrap_or(true);
    crate::backup::BackupService::trigger_auto_backup(&db, &identity, internet, Some(&backup_dir))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn validate_backup(
    db: tauri::State<DatabaseManager>,
    _session: tauri::State<AuthSession>,
    input: ValidateBackupPayload,
) -> Result<crate::backup::BackupValidationReport, String> {
    let backup_dir = crate::backup::BackupService::get_default_backup_dir(&db);
    let target_file = backup_dir.join(&input.file_name);
    crate::backup::BackupService::validate_backup_file(&target_file)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_backup(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: RestoreBackupPayload,
) -> Result<crate::backup::RestoreReport, String> {
    let identity = session.get_identity()?;
    let backup_dir = crate::backup::BackupService::get_default_backup_dir(&db);
    let target_file = backup_dir.join(&input.file_name);
    crate::backup::BackupService::restore_backup_file(&db, &identity, &target_file)
        .map_err(|e| e.to_string())
}

// ==============================================================================================
// BUILD 13: AI & VOICE INTELLIGENCE COMMANDS
// ==============================================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIQueryPayload {
    pub query: String,
    pub language: Option<String>,
    pub is_voice_transcript: Option<bool>,
}

#[tauri::command]
pub fn query_ai(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: AIQueryPayload,
) -> Result<crate::ai::AIResponseDto, String> {
    let identity = session.get_identity()?;
    let provider = crate::ai::DeterministicLocalInterpreter::new();
    let is_voice = input.is_voice_transcript.unwrap_or(false);

    let response = db
        .with_connection(|conn| {
            Ok(crate::ai::AIInterpreter::process_query(
                conn,
                &provider,
                &input.query,
                is_voice,
            ))
        })
        .map_err(|e: crate::db::operations::BusinessError| e.to_string())?;

    // Audit log AI query event (observational, strictly separated from transaction events)
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let _ = db.with_connection(|conn| {
        let audit_id = format!("aud_ai_query_{}", token);
        let now = format!("{:?}", std::time::SystemTime::now());
        let details = serde_json::json!({
            "query": input.query,
            "mode": format!("{:?}", response.mode),
            "intentType": response.intent_type,
            "isVoice": is_voice,
        })
        .to_string();
        conn.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'AI_INTERPRETATION', 'AI', ?3, ?4, ?5)",
            rusqlite::params![audit_id, identity.user_id(), response.intent_type, details, now],
        )?;
        Ok(())
    });

    Ok(response)
}

#[tauri::command]
pub fn get_ai_status(
    _session: tauri::State<AuthSession>,
) -> Result<crate::ai::AIStatusDto, String> {
    Ok(crate::ai::AIStatusDto::default_local())
}

#[tauri::command]
pub fn get_demand_recommendations(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<Vec<crate::ai::DemandRecommendationDto>, String> {
    let identity = session.get_identity()?;
    let recs = db
        .with_connection(|conn| {
            crate::ai::DemandIntelligenceService::generate_recommendations(conn)
                .map_err(|e| crate::db::operations::BusinessError::DatabaseError(e.to_string()))
        })
        .map_err(|e| e.to_string())?;

    // Audit recommendation retrieval
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let _ = db.with_connection(|conn| {
        let audit_id = format!("aud_ai_rec_{}", token);
        let now = format!("{:?}", std::time::SystemTime::now());
        let details = serde_json::json!({ "count": recs.len() }).to_string();
        conn.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'AI_RECOMMENDATION', 'AI', 'DEMAND', ?3, ?4)",
            rusqlite::params![audit_id, identity.user_id(), details, now],
        )?;
        Ok(())
    });

    Ok(recs)
}

// ================================================================================================
// BUILD 15: FIRST-RUN AUTHENTICATION & ADMIN ONBOARDING
// ================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInitialAdminInput {
    pub username: String,
    pub password: String,
    pub confirm_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedUserDto {
    pub user_id: String,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStateDto {
    pub status: String,
    pub user: Option<AuthenticatedUserDto>,
}

pub fn get_auth_state_inner(
    db: &DatabaseManager,
    session: &AuthSession,
) -> Result<AuthStateDto, String> {
    db.with_connection(|conn| {
        let admin_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'ADMIN'",
            [],
            |r| r.get(0),
        )?;

        if admin_count == 0 {
            Ok(AuthStateDto {
                status: "FIRST_RUN_ADMIN_SETUP".to_string(),
                user: None,
            })
        } else if let Ok(identity) = session.get_identity() {
            Ok(AuthStateDto {
                status: "AUTHENTICATED".to_string(),
                user: Some(AuthenticatedUserDto {
                    user_id: identity.user_id().to_string(),
                    username: identity.username().to_string(),
                    role: identity.role().as_str().to_string(),
                }),
            })
        } else {
            Ok(AuthStateDto {
                status: "UNAUTHENTICATED".to_string(),
                user: None,
            })
        }
    })
    .map_err(|e| e.to_string())
}

pub fn create_initial_admin_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    input: CreateInitialAdminInput,
) -> Result<AuthenticatedUserDto, String> {
    let trimmed_user = input.username.trim();
    if trimmed_user.is_empty() {
        return Err("Admin username cannot be empty".to_string());
    }
    if input.password.is_empty() {
        return Err("Admin password cannot be empty".to_string());
    }
    if input.password != input.confirm_password {
        return Err("Passwords do not match".to_string());
    }

    db.with_connection(|conn| {
        // Authoritative backend check: reject if an Admin already exists
        let admin_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'ADMIN'",
            [],
            |r| r.get(0),
        )?;
        if admin_count > 0 {
            return Err(crate::db::operations::BusinessError::DatabaseError(
                "An Admin account already exists. Initial setup is complete.".to_string(),
            ));
        }

        let identity = crate::auth::AuthService::create_initial_admin(
            conn,
            trimmed_user,
            &input.password,
            &input.confirm_password,
            "Initial Admin Setup",
            "Merchant OS Local Store",
        )
        .map_err(|e| match e {
            crate::auth::AuthError::PasswordMismatch => {
                crate::db::operations::BusinessError::DatabaseError(
                    "Passwords do not match".to_string(),
                )
            }
            crate::auth::AuthError::AdminAlreadyExists => {
                crate::db::operations::BusinessError::DatabaseError(
                    "An Admin account already exists".to_string(),
                )
            }
            crate::auth::AuthError::WeakPassword(msg) => {
                crate::db::operations::BusinessError::DatabaseError(msg)
            }
            other => crate::db::operations::BusinessError::DatabaseError(other.to_string()),
        })?;

        // Immediately establish authenticated session for the new initial admin
        session.set_identity(Some(identity.clone()));

        Ok(AuthenticatedUserDto {
            user_id: identity.user_id().to_string(),
            username: identity.username().to_string(),
            role: identity.role().as_str().to_string(),
        })
    })
    .map_err(|e| match e {
        crate::db::operations::BusinessError::DatabaseError(msg) => msg,
        other => other.to_string(),
    })
}

pub fn login_inner(
    db: &DatabaseManager,
    session: &AuthSession,
    input: LoginInput,
) -> Result<AuthenticatedUserDto, String> {
    let trimmed_user = input.username.trim();
    if trimmed_user.is_empty() {
        return Err("Username cannot be empty".to_string());
    }
    if input.password.is_empty() {
        return Err("Password cannot be empty".to_string());
    }

    db.with_connection(|conn| {
        let identity = crate::auth::AuthService::authenticate(conn, trimmed_user, &input.password)
            .map_err(|e| match e {
                crate::auth::AuthError::InvalidCredentials => {
                    crate::db::operations::BusinessError::DatabaseError(
                        "Invalid username or password".to_string(),
                    )
                }
                crate::auth::AuthError::UserInactive => {
                    crate::db::operations::BusinessError::DatabaseError(
                        "This user account is inactive".to_string(),
                    )
                }
                other => crate::db::operations::BusinessError::DatabaseError(other.to_string()),
            })?;

        session.set_identity(Some(identity.clone()));

        Ok(AuthenticatedUserDto {
            user_id: identity.user_id().to_string(),
            username: identity.username().to_string(),
            role: identity.role().as_str().to_string(),
        })
    })
    .map_err(|e| match e {
        crate::db::operations::BusinessError::DatabaseError(msg) => msg,
        other => other.to_string(),
    })
}

pub fn logout_inner(session: &AuthSession) -> Result<(), String> {
    session.set_identity(None);
    Ok(())
}

#[tauri::command]
pub fn get_auth_state(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
) -> Result<AuthStateDto, String> {
    get_auth_state_inner(&db, &session)
}

#[tauri::command]
pub fn create_initial_admin(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: CreateInitialAdminInput,
) -> Result<AuthenticatedUserDto, String> {
    create_initial_admin_inner(&db, &session, input)
}

#[tauri::command]
pub fn login(
    db: tauri::State<DatabaseManager>,
    session: tauri::State<AuthSession>,
    input: LoginInput,
) -> Result<AuthenticatedUserDto, String> {
    login_inner(&db, &session, input)
}

#[tauri::command]
pub fn logout(session: tauri::State<AuthSession>) -> Result<(), String> {
    logout_inner(&session)
}






