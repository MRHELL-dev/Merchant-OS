use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaleIntentItem {
    pub product_reference: String,
    pub product_id: Option<String>,
    pub product_name: Option<String>,
    pub quantity_display: String,
    pub quantity_millie: i64,
    pub unit_price_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseIntentItem {
    pub product_reference: String,
    pub product_id: Option<String>,
    pub product_name: Option<String>,
    pub quantity_display: String,
    pub quantity_millie: i64,
    pub unit_cost_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrderIntentItem {
    pub product_reference: String,
    pub product_id: Option<String>,
    pub product_name: Option<String>,
    pub quantity_display: String,
    pub quantity_millie: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "intentType", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StructuredIntent {
    #[serde(rename = "CREATE_SALE", rename_all = "camelCase")]
    CreateSale {
        items: Vec<SaleIntentItem>,
        customer_reference: Option<String>,
        customer_id: Option<String>,
        payment_method: String,
        settlement_mode: String,
    },
    #[serde(rename = "CREATE_PURCHASE", rename_all = "camelCase")]
    CreatePurchase {
        items: Vec<PurchaseIntentItem>,
        supplier_reference: String,
        supplier_id: Option<String>,
        payment_method: String,
    },
    #[serde(rename = "CREATE_CUSTOMER_ORDER", rename_all = "camelCase")]
    CreateCustomerOrder {
        items: Vec<OrderIntentItem>,
        customer_reference: Option<String>,
        customer_id: Option<String>,
        notes: Option<String>,
    },
    #[serde(rename = "CHECK_STOCK", rename_all = "camelCase")]
    CheckStock {
        product_reference: String,
        product_id: Option<String>,
    },
    #[serde(rename = "CHECK_CUSTOMER_CREDIT", rename_all = "camelCase")]
    CheckCustomerCredit {
        customer_reference: String,
        customer_id: Option<String>,
    },
    #[serde(rename = "RECORD_CUSTOMER_PAYMENT", rename_all = "camelCase")]
    RecordCustomerPayment {
        customer_reference: String,
        customer_id: Option<String>,
        amount_cents: i64,
        payment_method: String,
    },
    #[serde(rename = "CHECK_SUPPLIER_BALANCE", rename_all = "camelCase")]
    CheckSupplierBalance {
        supplier_reference: String,
        supplier_id: Option<String>,
    },
    #[serde(rename = "CHECK_SALES", rename_all = "camelCase")]
    CheckSales {
        period: Option<String>,
    },
    #[serde(rename = "CHECK_DEMAND", rename_all = "camelCase")]
    CheckDemand {
        category_reference: Option<String>,
    },
    #[serde(rename = "CHECK_LOW_STOCK", rename_all = "camelCase")]
    CheckLowStock,
    #[serde(rename = "PREPARE_REORDER", rename_all = "camelCase")]
    PrepareReorder {
        product_reference: Option<String>,
        product_id: Option<String>,
    },
    #[serde(rename = "TRANSLATE", rename_all = "camelCase")]
    Translate {
        text: String,
        target_language: String,
    },
    #[serde(rename = "GENERAL_QUERY", rename_all = "camelCase")]
    GeneralQuery {
        query: String,
    },
}

impl StructuredIntent {
    pub fn intent_type_str(&self) -> &'static str {
        match self {
            StructuredIntent::CreateSale { .. } => "CREATE_SALE",
            StructuredIntent::CreatePurchase { .. } => "CREATE_PURCHASE",
            StructuredIntent::CreateCustomerOrder { .. } => "CREATE_CUSTOMER_ORDER",
            StructuredIntent::CheckStock { .. } => "CHECK_STOCK",
            StructuredIntent::CheckCustomerCredit { .. } => "CHECK_CUSTOMER_CREDIT",
            StructuredIntent::RecordCustomerPayment { .. } => "RECORD_CUSTOMER_PAYMENT",
            StructuredIntent::CheckSupplierBalance { .. } => "CHECK_SUPPLIER_BALANCE",
            StructuredIntent::CheckSales { .. } => "CHECK_SALES",
            StructuredIntent::CheckDemand { .. } => "CHECK_DEMAND",
            StructuredIntent::CheckLowStock => "CHECK_LOW_STOCK",
            StructuredIntent::PrepareReorder { .. } => "PREPARE_REORDER",
            StructuredIntent::Translate { .. } => "TRANSLATE",
            StructuredIntent::GeneralQuery { .. } => "GENERAL_QUERY",
        }
    }
}
