use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------------------------------------
// Explicit Merchant Confirmation Pattern
// ------------------------------------------------------------------------------------------------

/// A validated, authoritatively calculated command ready for merchant confirmation.
/// Cannot be executed until explicitly confirmed.
#[derive(Debug, Clone)]
pub struct Prepared<T> {
    pub payload: T,
    pub prepared_at: String,
}

impl<T> Prepared<T> {
    pub fn new(payload: T) -> Self {
        Self {
            payload,
            prepared_at: format!("{:?}", std::time::SystemTime::now()),
        }
    }

    /// Explicit confirmation boundary: Transforms the prepared command into a Confirmed command.
    /// TransactionEngine will ONLY accept Confirmed commands.
    /// The confirmation actor MUST originate from an AuthenticatedIdentity.
    pub fn confirm(self, identity: &crate::auth::AuthenticatedIdentity) -> Confirmed<T> {
        Confirmed {
            payload: self.payload,
            confirmed_by_user_id: identity.user_id().to_string(),
            confirmed_at: format!("{:?}", std::time::SystemTime::now()),
        }
    }
}

/// An explicitly confirmed command, authorized by a merchant/user.
/// This is the ONLY input accepted by the TransactionEngine.
#[derive(Debug, Clone)]
pub struct Confirmed<T> {
    pub payload: T,
    confirmed_by_user_id: String,
    confirmed_at: String,
}

impl<T> Confirmed<T> {
    pub fn confirmed_by_user_id(&self) -> &str {
        &self.confirmed_by_user_id
    }

    pub fn confirmed_at(&self) -> &str {
        &self.confirmed_at
    }

    pub(crate) fn new(payload: T, confirmed_by_user_id: String, confirmed_at: String) -> Self {
        Self {
            payload,
            confirmed_by_user_id,
            confirmed_at,
        }
    }
}

// ------------------------------------------------------------------------------------------------
// 1. SALE COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleItemRequest {
    pub product_id: String,
    pub quantity: i64, // millie-units (scale 1000)
    pub unit_price_cents: i64, // historical selling price
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmSaleRequest {
    pub sale_id: String,
    pub sale_number: String,
    pub customer_id: Option<String>,
    pub items: Vec<SaleItemRequest>,
    pub paid_amount_cents: i64,
    pub payment_method: Option<String>,
    pub user_id: String,
    pub sale_date: String,
}

#[derive(Debug, Clone)]
pub struct AuthoritativeSaleItem {
    pub product_id: String,
    pub quantity: i64,
    pub unit_price_cents: i64, // authoritative selling price
    pub cost_price_cents: i64, // historical cost price snapshot
    pub line_total_cents: i64, // calculated via round-half-up
}

#[derive(Debug, Clone)]
pub struct PreparedSale {
    pub sale_id: String,
    pub sale_number: String,
    pub customer_id: Option<String>,
    pub items: Vec<AuthoritativeSaleItem>,
    pub total_amount_cents: i64, // authoritatively summed
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub payment_status: String, // PAID | PARTIAL | UNPAID
    pub payment_method: Option<String>,
    pub user_id: String,
    pub sale_date: String,
}

// ------------------------------------------------------------------------------------------------
// 2. PURCHASE COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseItemRequest {
    pub product_id: String,
    pub quantity: i64, // millie-units (scale 1000)
    pub unit_cost_cents: i64, // actual buying cost (mandatory)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmPurchaseRequest {
    pub purchase_id: String,
    pub purchase_number: String,
    pub supplier_id: String,
    pub items: Vec<PurchaseItemRequest>,
    pub paid_amount_cents: i64,
    pub payment_method: Option<String>,
    pub user_id: String,
    pub purchase_date: String,
}

#[derive(Debug, Clone)]
pub struct AuthoritativePurchaseItem {
    pub product_id: String,
    pub quantity: i64,
    pub unit_cost_cents: i64,
    pub line_total_cents: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedPurchase {
    pub purchase_id: String,
    pub purchase_number: String,
    pub supplier_id: String,
    pub items: Vec<AuthoritativePurchaseItem>,
    pub total_amount_cents: i64,
    pub paid_amount_cents: i64,
    pub credit_amount_cents: i64,
    pub payment_method: Option<String>,
    pub user_id: String,
    pub purchase_date: String,
}

// ------------------------------------------------------------------------------------------------
// 3. CUSTOMER PAYMENT COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCustomerPaymentRequest {
    pub payment_id: String,
    pub customer_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub user_id: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreparedCustomerPayment {
    pub payment_id: String,
    pub customer_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub user_id: String,
    pub notes: Option<String>,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
}

// ------------------------------------------------------------------------------------------------
// 4. SUPPLIER PAYMENT COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordSupplierPaymentRequest {
    pub payment_id: String,
    pub supplier_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub user_id: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreparedSupplierPayment {
    pub payment_id: String,
    pub supplier_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub user_id: String,
    pub notes: Option<String>,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
}

// ------------------------------------------------------------------------------------------------
// 5. CUSTOMER RETURN COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnItemRequest {
    pub product_id: String,
    pub quantity: i64, // millie-units (scale 1000)
    pub unit_price_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessCustomerReturnRequest {
    pub return_id: String,
    pub return_number: String,
    pub reference_sale_id: Option<String>,
    pub customer_id: String,
    pub items: Vec<ReturnItemRequest>,
    pub reason: String,
    pub admin_user_id: String,
}

#[derive(Debug, Clone)]
pub struct AuthoritativeReturnItem {
    pub product_id: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub line_total_cents: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedCustomerReturn {
    pub return_id: String,
    pub return_number: String,
    pub reference_sale_id: Option<String>,
    pub customer_id: String,
    pub items: Vec<AuthoritativeReturnItem>,
    pub total_amount_cents: i64,
    pub debt_reduction_cents: i64,
    pub cash_refund_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reason: String,
    pub admin_user_id: String,
}

// ------------------------------------------------------------------------------------------------
// 6. SUPPLIER RETURN COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSupplierReturnRequest {
    pub return_id: String,
    pub return_number: String,
    pub reference_purchase_id: Option<String>,
    pub supplier_id: String,
    pub items: Vec<ReturnItemRequest>,
    pub reason: String,
    pub admin_user_id: String,
}

#[derive(Debug, Clone)]
pub struct PreparedSupplierReturn {
    pub return_id: String,
    pub return_number: String,
    pub reference_purchase_id: Option<String>,
    pub supplier_id: String,
    pub items: Vec<AuthoritativeReturnItem>,
    pub total_amount_cents: i64,
    pub balance_before_cents: i64,
    pub balance_after_cents: i64,
    pub reason: String,
    pub admin_user_id: String,
}

// ------------------------------------------------------------------------------------------------
// 7. STOCK CORRECTION COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordStockCorrectionRequest {
    pub correction_id: String,
    pub product_id: String,
    pub quantity_change: i64, // signed delta millie-units
    pub reason: String, // DAMAGED | EXPIRED | LOST | MISCOUNT
    pub note: String,
    pub admin_user_id: String,
}

#[derive(Debug, Clone)]
pub struct PreparedStockCorrection {
    pub correction_id: String,
    pub product_id: String,
    pub quantity_change: i64,
    pub quantity_before: i64,
    pub quantity_after: i64,
    pub reason: String,
    pub note: String,
    pub admin_user_id: String,
}

// ------------------------------------------------------------------------------------------------
// 8. ORDER TO SALE CONVERSION COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertOrderToSaleRequest {
    pub order_id: String,
    pub sale_id: String,
    pub sale_number: String,
    pub paid_amount_cents: i64,
    pub payment_method: Option<String>,
    pub user_id: String,
    pub sale_date: String,
}

#[derive(Debug, Clone)]
pub struct PreparedOrderConversion {
    pub order_id: String,
    pub prepared_sale: PreparedSale,
}

// ------------------------------------------------------------------------------------------------
// 9. EXPENSE COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordExpenseRequest {
    pub expense_id: String,
    pub expense_name: String,
    pub amount_cents: i64,
    pub category: String,
    pub expense_date: String,
    pub notes: Option<String>,
    pub user_id: String,
}

#[derive(Debug, Clone)]
pub struct PreparedExpense {
    pub expense_id: String,
    pub expense_name: String,
    pub amount_cents: i64,
    pub category: String,
    pub expense_date: String,
    pub notes: Option<String>,
    pub user_id: String,
}

// ------------------------------------------------------------------------------------------------
// 10. POLYMORPHIC PAYMENT REFERENCE COMMANDS
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordPaymentRequest {
    pub payment_id: String,
    pub payment_type: String,
    pub related_entity_type: String,
    pub related_entity_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub user_id: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreparedPayment {
    pub payment_id: String,
    pub payment_type: String,
    pub related_entity_type: String,
    pub related_entity_id: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub user_id: String,
    pub notes: Option<String>,
}

// ------------------------------------------------------------------------------------------------
// 11. PRODUCT & INVENTORY COMMANDS (BUILD 05)
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductType {
    Packaged,
    Loose,
}

impl ProductType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProductType::Packaged => "PACKAGED",
            ProductType::Loose => "LOOSE",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "PACKAGED" => Some(ProductType::Packaged),
            "LOOSE" => Some(ProductType::Loose),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StockStatus {
    LowStock,
    Normal,
}

impl StockStatus {
    pub fn calculate(current_quantity: i64, min_stock_level: i64) -> Self {
        if current_quantity <= min_stock_level {
            StockStatus::LowStock
        } else {
            StockStatus::Normal
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StockStatus::LowStock => "LOW_STOCK",
            StockStatus::Normal => "NORMAL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarcodeType {
    Manufacturer,
    Internal,
    WeighingScale,
}

impl BarcodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BarcodeType::Manufacturer => "MANUFACTURER",
            BarcodeType::Internal => "INTERNAL",
            BarcodeType::WeighingScale => "WEIGHING_SCALE",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "MANUFACTURER" => Some(BarcodeType::Manufacturer),
            "INTERNAL" => Some(BarcodeType::Internal),
            "WEIGHING_SCALE" => Some(BarcodeType::WeighingScale),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProductRequest {
    pub product_id: Option<String>,
    pub business_id: String,
    pub name: String,
    pub product_type: String, // "PACKAGED" | "LOOSE"
    pub unit: String,
    pub barcode: Option<String>,
    pub barcode_type: Option<String>,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
    pub initial_stock: i64, // millie-units (scale: 1000)
    pub min_stock_level: i64, // millie-units (scale: 1000)
}

#[derive(Debug, Clone)]
pub struct PreparedCreateProduct {
    pub product_id: String,
    pub business_id: String,
    pub name: String,
    pub product_type: ProductType,
    pub unit: String,
    pub barcode: Option<String>,
    pub barcode_type: BarcodeType,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
    pub initial_stock: i64,
    pub min_stock_level: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProductBatchRequest {
    pub business_id: String,
    pub products: Vec<CreateProductRequest>,
}

#[derive(Debug, Clone)]
pub struct PreparedCreateProductBatch {
    pub business_id: String,
    pub products: Vec<PreparedCreateProduct>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProductRequest {
    pub product_id: String,
    pub business_id: String,
    pub name: Option<String>,
    pub unit: Option<String>,
    pub barcode: Option<Option<String>>, // Some(Some(b)) to set, Some(None) to remove
    pub barcode_type: Option<String>,
    pub cost_price_cents: Option<i64>,
    pub selling_price_cents: Option<i64>,
    pub min_stock_level: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PreparedUpdateProduct {
    pub product_id: String,
    pub business_id: String,
    pub name: Option<String>,
    pub unit: Option<String>,
    pub barcode: Option<Option<String>>,
    pub barcode_type: Option<BarcodeType>,
    pub cost_price_cents: Option<i64>,
    pub selling_price_cents: Option<i64>,
    pub min_stock_level: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemapBarcodeRequest {
    pub business_id: String,
    pub barcode: String,
    pub new_product_id: String,
}

#[derive(Debug, Clone)]
pub struct PreparedRemapBarcode {
    pub business_id: String,
    pub barcode: String,
    pub old_product_id: String,
    pub new_product_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductResult {
    pub id: String,
    pub business_id: String,
    pub name: String,
    pub product_type: String,
    pub unit: String,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
    pub min_stock_level: i64,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryResult {
    pub product_id: String,
    pub current_quantity: i64,
    pub min_stock_level: i64,
    pub stock_status: String,
    pub last_updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductStockStatusResult {
    pub product_id: String,
    pub product_name: String,
    pub current_quantity: i64,
    pub min_stock_level: i64,
    pub stock_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeighingScaleMetadata {
    pub raw_payload: String,
    pub embedded_weight_millie_units: Option<i64>,
    pub embedded_price_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BarcodeResolutionResult {
    pub barcode: String,
    pub barcode_type: String,
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub unit: String,
    pub cost_price_cents: i64,
    pub selling_price_cents: i64,
    pub current_quantity: i64,
    pub weighing_metadata: Option<WeighingScaleMetadata>,
}

