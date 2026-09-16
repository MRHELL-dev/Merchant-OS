use rusqlite::Error as SqliteError;
use std::fmt;

/// Errors emitted by the Business Engine and Transaction Engine.
#[derive(Debug, PartialEq, Eq)]
pub enum EngineError {
    AdminAuthorizationRequired(String),
    EntityNotFound(String),
    InsufficientStock {
        product_id: String,
        available: i64,
        requested: i64,
    },
    InvalidQuantity {
        product_id: String,
        quantity: i64,
        message: String,
    },
    InvalidAmount {
        field: String,
        amount: i64,
        message: String,
    },
    OverpaymentNotAllowed {
        owed: i64,
        attempted: i64,
    },
    InvalidPaymentReference {
        entity_type: String,
        entity_id: String,
    },
    OrderNotDraft(String),
    OrderAlreadyConverted(String),
    InvalidCorrectionReason(String),
    PermissionDenied {
        user_id: String,
        feature: String,
    },
    InvalidProductName(String),
    InvalidProductType(String),
    InvalidUnit(String),
    NegativeStock(i64),
    NegativeMinimumStock(i64),
    UnitChangeForbidden(String),
    DuplicateBarcodeMapping {
        barcode: String,
        existing_product_id: String,
    },
    CrossBusinessAccessDenied {
        attempted_business: String,
        resource_business: String,
    },
    DirectStockModificationRejected,
    MathError(String),
    DatabaseError(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::AdminAuthorizationRequired(msg) => {
                write!(f, "Admin authorization required: {}", msg)
            }
            EngineError::PermissionDenied { user_id, feature } => write!(
                f,
                "Permission denied: user {} lacks permission for feature '{}'",
                user_id, feature
            ),
            EngineError::EntityNotFound(msg) => write!(f, "Entity not found: {}", msg),
            EngineError::InvalidProductName(msg) => write!(f, "Invalid product name: {}", msg),
            EngineError::InvalidProductType(msg) => write!(f, "Invalid product type: {}", msg),
            EngineError::InvalidUnit(msg) => write!(f, "Invalid unit: {}", msg),
            EngineError::NegativeStock(qty) => write!(f, "Initial stock cannot be negative: {}", qty),
            EngineError::NegativeMinimumStock(qty) => {
                write!(f, "Minimum stock level cannot be negative: {}", qty)
            }
            EngineError::UnitChangeForbidden(msg) => write!(f, "Unit change rejected: {}", msg),
            EngineError::DuplicateBarcodeMapping {
                barcode,
                existing_product_id,
            } => write!(
                f,
                "Duplicate barcode mapping: barcode '{}' is already assigned to product '{}'",
                barcode, existing_product_id
            ),
            EngineError::CrossBusinessAccessDenied {
                attempted_business,
                resource_business,
            } => write!(
                f,
                "Cross-business access denied: attempted business '{}' does not match resource business '{}'",
                attempted_business, resource_business
            ),
            EngineError::DirectStockModificationRejected => write!(
                f,
                "Direct stock modification rejected: stock can only mutate via controlled business events"
            ),
            EngineError::InsufficientStock {
                product_id,
                available,
                requested,
            } => write!(
                f,
                "Insufficient stock for product {}: available {}, requested {}",
                product_id, available, requested
            ),
            EngineError::InvalidQuantity {
                product_id,
                quantity,
                message,
            } => write!(
                f,
                "Invalid quantity {} for product {}: {}",
                quantity, product_id, message
            ),
            EngineError::InvalidAmount {
                field,
                amount,
                message,
            } => write!(f, "Invalid amount {} for {}: {}", amount, field, message),
            EngineError::OverpaymentNotAllowed { owed, attempted } => write!(
                f,
                "Overpayment not allowed: outstanding balance is {}, attempted payment is {}",
                owed, attempted
            ),
            EngineError::InvalidPaymentReference {
                entity_type,
                entity_id,
            } => write!(
                f,
                "Invalid payment reference: related_entity_type '{}' with id '{}' does not exist",
                entity_type, entity_id
            ),
            EngineError::OrderNotDraft(msg) => {
                write!(f, "Order cannot be converted: {}", msg)
            }
            EngineError::OrderAlreadyConverted(msg) => {
                write!(f, "Order already converted: {}", msg)
            }
            EngineError::InvalidCorrectionReason(reason) => {
                write!(f, "Invalid stock correction reason: {}", reason)
            }
            EngineError::MathError(msg) => write!(f, "Math calculation error: {}", msg),
            EngineError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<SqliteError> for EngineError {
    fn from(err: SqliteError) -> Self {
        EngineError::DatabaseError(err.to_string())
    }
}

impl From<crate::auth::AuthError> for EngineError {
    fn from(err: crate::auth::AuthError) -> Self {
        match err {
            crate::auth::AuthError::AdminAuthorizationRequired(msg) => {
                EngineError::AdminAuthorizationRequired(msg)
            }
            crate::auth::AuthError::PermissionDenied { user_id, feature } => {
                EngineError::PermissionDenied { user_id, feature }
            }
            crate::auth::AuthError::DatabaseError(msg) => EngineError::DatabaseError(msg),
            other => EngineError::DatabaseError(other.to_string()),
        }
    }
}

