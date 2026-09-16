use rusqlite::{params, Connection};

use crate::auth::{AuthenticatedIdentity, Role};
use crate::engine::{
    BusinessEngine, ConfirmPurchaseRequest, ConfirmSaleRequest, ConvertOrderToSaleRequest,
    EngineError, ProcessCustomerReturnRequest, ProcessSupplierReturnRequest,
    PurchaseItemRequest, RecordPaymentRequest, RecordStockCorrectionRequest, ReturnItemRequest,
    SaleItemRequest, TransactionEngine,
};

/// Re-export EngineError as BusinessError for backward compatibility.
pub type BusinessError = EngineError;

/// Re-export item inputs for backward compatibility.
pub type SaleItemInput = SaleItemRequest;
pub type PurchaseItemInput = PurchaseItemRequest;
pub type ReturnItemInput = ReturnItemRequest;

/// Internal helper for adapter functions to resolve an active user into an AuthenticatedIdentity.
fn resolve_identity(conn: &Connection, user_id: &str) -> Result<AuthenticatedIdentity, BusinessError> {
    let (username, role_str): (String, String) = conn
        .query_row(
            "SELECT username, role FROM users WHERE id = ?1 AND is_active = 1",
            params![user_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| EngineError::EntityNotFound(format!("Active user {}", user_id)))?;

    let role = Role::from_str(&role_str).ok_or_else(|| {
        EngineError::DatabaseError(format!("Invalid role '{}' in users table", role_str))
    })?;

    Ok(AuthenticatedIdentity::new(user_id.to_string(), username, role))
}

// ------------------------------------------------------------------------------------------------
// 1. CONFIRM SALE (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn confirm_sale(
    conn: &mut Connection,
    sale_id: &str,
    sale_number: &str,
    customer_id: Option<&str>,
    items: &[SaleItemInput],
    paid_amount_cents: i64,
    payment_method: Option<&str>,
    user_id: &str,
    sale_date: &str,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, user_id)?;

    let req = ConfirmSaleRequest {
        sale_id: sale_id.to_string(),
        sale_number: sale_number.to_string(),
        customer_id: customer_id.map(|s| s.to_string()),
        items: items.to_vec(),
        paid_amount_cents,
        payment_method: payment_method.map(|s| s.to_string()),
        user_id: user_id.to_string(),
        sale_date: sale_date.to_string(),
    };

    // Flow: BusinessEngine (validate) -> Confirm -> TransactionEngine (execute atomically)
    let prepared = BusinessEngine::prepare_sale(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_sale(conn, confirmed)
}

// ------------------------------------------------------------------------------------------------
// 2. CONFIRM PURCHASE (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn confirm_purchase(
    conn: &mut Connection,
    purchase_id: &str,
    purchase_number: &str,
    supplier_id: &str,
    items: &[PurchaseItemInput],
    paid_amount_cents: i64,
    payment_method: Option<&str>,
    user_id: &str,
    purchase_date: &str,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, user_id)?;

    let req = ConfirmPurchaseRequest {
        purchase_id: purchase_id.to_string(),
        purchase_number: purchase_number.to_string(),
        supplier_id: supplier_id.to_string(),
        items: items.to_vec(),
        paid_amount_cents,
        payment_method: payment_method.map(|s| s.to_string()),
        user_id: user_id.to_string(),
        purchase_date: purchase_date.to_string(),
    };

    let prepared = BusinessEngine::prepare_purchase(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_purchase(conn, confirmed)
}

// ------------------------------------------------------------------------------------------------
// 3. CONVERT ORDER TO SALE (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn convert_order_to_sale(
    conn: &mut Connection,
    order_id: &str,
    sale_id: &str,
    sale_number: &str,
    paid_amount_cents: i64,
    payment_method: Option<&str>,
    user_id: &str,
    sale_date: &str,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, user_id)?;

    let req = ConvertOrderToSaleRequest {
        order_id: order_id.to_string(),
        sale_id: sale_id.to_string(),
        sale_number: sale_number.to_string(),
        paid_amount_cents,
        payment_method: payment_method.map(|s| s.to_string()),
        user_id: user_id.to_string(),
        sale_date: sale_date.to_string(),
    };

    let prepared = BusinessEngine::prepare_order_conversion(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_order_conversion(conn, confirmed)
}

// ------------------------------------------------------------------------------------------------
// 4. CUSTOMER RETURN (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn process_customer_return(
    conn: &mut Connection,
    return_id: &str,
    return_number: &str,
    reference_sale_id: Option<&str>,
    customer_id: &str,
    items: &[ReturnItemInput],
    reason: &str,
    admin_user_id: &str,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, admin_user_id)?;

    let req = ProcessCustomerReturnRequest {
        return_id: return_id.to_string(),
        return_number: return_number.to_string(),
        reference_sale_id: reference_sale_id.map(|s| s.to_string()),
        customer_id: customer_id.to_string(),
        items: items.to_vec(),
        reason: reason.to_string(),
        admin_user_id: admin_user_id.to_string(),
    };

    let prepared = BusinessEngine::prepare_customer_return(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_customer_return(conn, confirmed)
}

// ------------------------------------------------------------------------------------------------
// 5. SUPPLIER RETURN (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn process_supplier_return(
    conn: &mut Connection,
    return_id: &str,
    return_number: &str,
    reference_purchase_id: Option<&str>,
    supplier_id: &str,
    items: &[ReturnItemInput],
    reason: &str,
    admin_user_id: &str,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, admin_user_id)?;

    let req = ProcessSupplierReturnRequest {
        return_id: return_id.to_string(),
        return_number: return_number.to_string(),
        reference_purchase_id: reference_purchase_id.map(|s| s.to_string()),
        supplier_id: supplier_id.to_string(),
        items: items.to_vec(),
        reason: reason.to_string(),
        admin_user_id: admin_user_id.to_string(),
    };

    let prepared = BusinessEngine::prepare_supplier_return(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_supplier_return(conn, confirmed)
}

// ------------------------------------------------------------------------------------------------
// 6. STOCK CORRECTION (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn record_stock_correction(
    conn: &mut Connection,
    correction_id: &str,
    product_id: &str,
    quantity_change: i64,
    reason: &str,
    note: &str,
    admin_user_id: &str,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, admin_user_id)?;

    let req = RecordStockCorrectionRequest {
        correction_id: correction_id.to_string(),
        product_id: product_id.to_string(),
        quantity_change,
        reason: reason.to_string(),
        note: note.to_string(),
        admin_user_id: admin_user_id.to_string(),
    };

    let prepared = BusinessEngine::prepare_stock_correction(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_stock_correction(conn, confirmed)
}

// ------------------------------------------------------------------------------------------------
// 7. RECORD PAYMENT (ADAPTER)
// ------------------------------------------------------------------------------------------------
pub fn record_payment(
    conn: &mut Connection,
    payment_id: &str,
    payment_type: &str,
    related_entity_type: &str,
    related_entity_id: &str,
    amount_cents: i64,
    payment_method: &str,
    user_id: &str,
    notes: Option<&str>,
) -> Result<(), BusinessError> {
    let identity = resolve_identity(conn, user_id)?;

    let req = RecordPaymentRequest {
        payment_id: payment_id.to_string(),
        payment_type: payment_type.to_string(),
        related_entity_type: related_entity_type.to_string(),
        related_entity_id: related_entity_id.to_string(),
        amount_cents,
        payment_method: payment_method.to_string(),
        user_id: user_id.to_string(),
        notes: notes.map(|s| s.to_string()),
    };

    let prepared = BusinessEngine::prepare_payment(conn, &identity, req)?;
    let confirmed = prepared.confirm(&identity);
    TransactionEngine::execute_payment(conn, confirmed)
}
