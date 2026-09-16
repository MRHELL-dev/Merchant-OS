use rusqlite::{params, Connection};

use super::commands::*;
use super::error::EngineError;
use crate::auth::{AuthenticatedIdentity, AuthorizationService, PermissionKey};
use crate::db::math::calculate_line_total;

/// The Business Engine enforces all business rules, authority checks, and authoritative calculations.
///
/// Crucial Architecture Invariant:
/// - BusinessEngine performs READ-ONLY queries against SQLite.
/// - BusinessEngine NEVER mutates database state.
/// - Returns a `Prepared<T>` command which MUST be explicitly confirmed before transaction execution.
pub struct BusinessEngine;

impl BusinessEngine {
    // --------------------------------------------------------------------------------------------
    // 1. PREPARE SALE
    // --------------------------------------------------------------------------------------------
    pub fn prepare_sale(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: ConfirmSaleRequest,
    ) -> Result<Prepared<PreparedSale>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Sales.as_str())?;

        if request.items.is_empty() {
            return Err(EngineError::InvalidQuantity {
                product_id: "".to_string(),
                quantity: 0,
                message: "Sale must contain at least one item".to_string(),
            });
        }

        if request.paid_amount_cents < 0 {
            return Err(EngineError::InvalidAmount {
                field: "paid_amount_cents".to_string(),
                amount: request.paid_amount_cents,
                message: "Paid amount cannot be negative".to_string(),
            });
        }

        // 1. Verify customer if provided
        if let Some(ref c_id) = request.customer_id {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM customers WHERE id = ?1",
                params![c_id],
                |row| row.get(0),
            )?;
            if count == 0 {
                return Err(EngineError::EntityNotFound(format!("Customer {}", c_id)));
            }
        }

        // 2. Validate items, stock, and authoritatively calculate line totals
        let mut authoritative_items = Vec::new();
        let mut total_amount_cents: i64 = 0;

        for item in request.items {
            if item.quantity <= 0 {
                return Err(EngineError::InvalidQuantity {
                    product_id: item.product_id,
                    quantity: item.quantity,
                    message: "Item quantity must be strictly positive".to_string(),
                });
            }

            // Verify product exists and read canonical cost price
            let cost_price_cents: i64 = conn.query_row(
                "SELECT cost_price_cents FROM products WHERE id = ?1",
                params![item.product_id],
                |row| row.get(0),
            ).map_err(|_| EngineError::EntityNotFound(format!("Product {}", item.product_id)))?;

            let unit_price_cents = item.unit_price_cents;
            if unit_price_cents < 0 {
                return Err(EngineError::InvalidAmount {
                    field: format!("unit_price_cents for product {}", item.product_id),
                    amount: unit_price_cents,
                    message: "Selling price cannot be negative".to_string(),
                });
            }

            // Check stock availability
            let available: i64 = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![item.product_id],
                |row| row.get(0),
            ).unwrap_or(0);

            if available < item.quantity {
                return Err(EngineError::InsufficientStock {
                    product_id: item.product_id,
                    available,
                    requested: item.quantity,
                });
            }

            // Authoritative line total calculation via deterministic round-half-up
            let line_total_cents = calculate_line_total(item.quantity, unit_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;

            total_amount_cents = total_amount_cents
                .checked_add(line_total_cents)
                .ok_or_else(|| EngineError::MathError("Total sale amount overflow".to_string()))?;

            authoritative_items.push(AuthoritativeSaleItem {
                product_id: item.product_id,
                quantity: item.quantity,
                unit_price_cents,
                cost_price_cents,
                line_total_cents,
            });
        }

        // 3. Authoritatively compute credit and payment status
        let credit_amount_cents = total_amount_cents.saturating_sub(request.paid_amount_cents);

        // Credit sale requires a customer
        if credit_amount_cents > 0 && request.customer_id.is_none() {
            return Err(EngineError::InvalidAmount {
                field: "credit_amount_cents".to_string(),
                amount: credit_amount_cents,
                message: "Credit sale requires a registered customer identity".to_string(),
            });
        }

        let payment_status = if credit_amount_cents == 0 {
            "PAID".to_string()
        } else if request.paid_amount_cents > 0 {
            "PARTIAL".to_string()
        } else {
            "UNPAID".to_string()
        };

        Ok(Prepared::new(PreparedSale {
            sale_id: request.sale_id,
            sale_number: request.sale_number,
            customer_id: request.customer_id,
            items: authoritative_items,
            total_amount_cents,
            paid_amount_cents: request.paid_amount_cents,
            credit_amount_cents,
            payment_status,
            payment_method: request.payment_method,
            user_id: request.user_id,
            sale_date: request.sale_date,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 2. PREPARE PURCHASE
    // --------------------------------------------------------------------------------------------
    pub fn prepare_purchase(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: ConfirmPurchaseRequest,
    ) -> Result<Prepared<PreparedPurchase>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Purchases.as_str())?;

        if request.items.is_empty() {
            return Err(EngineError::InvalidQuantity {
                product_id: "".to_string(),
                quantity: 0,
                message: "Purchase must contain at least one item".to_string(),
            });
        }

        if request.paid_amount_cents < 0 {
            return Err(EngineError::InvalidAmount {
                field: "paid_amount_cents".to_string(),
                amount: request.paid_amount_cents,
                message: "Paid amount cannot be negative".to_string(),
            });
        }

        // Verify supplier exists
        let s_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM suppliers WHERE id = ?1",
            params![request.supplier_id],
            |row| row.get(0),
        )?;
        if s_count == 0 {
            return Err(EngineError::EntityNotFound(format!("Supplier {}", request.supplier_id)));
        }

        // Validate items and authoritatively calculate line totals
        let mut authoritative_items = Vec::new();
        let mut total_amount_cents: i64 = 0;

        for item in request.items {
            if item.quantity <= 0 {
                return Err(EngineError::InvalidQuantity {
                    product_id: item.product_id,
                    quantity: item.quantity,
                    message: "Purchase quantity must be strictly positive".to_string(),
                });
            }

            if item.unit_cost_cents <= 0 {
                return Err(EngineError::InvalidAmount {
                    field: format!("unit_cost_cents for product {}", item.product_id),
                    amount: item.unit_cost_cents,
                    message: "Buying price must be strictly positive".to_string(),
                });
            }

            // Verify product exists
            let p_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM products WHERE id = ?1",
                params![item.product_id],
                |row| row.get(0),
            )?;
            if p_count == 0 {
                return Err(EngineError::EntityNotFound(format!("Product {}", item.product_id)));
            }

            let line_total_cents = calculate_line_total(item.quantity, item.unit_cost_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;

            total_amount_cents = total_amount_cents
                .checked_add(line_total_cents)
                .ok_or_else(|| EngineError::MathError("Total purchase amount overflow".to_string()))?;

            authoritative_items.push(AuthoritativePurchaseItem {
                product_id: item.product_id,
                quantity: item.quantity,
                unit_cost_cents: item.unit_cost_cents,
                line_total_cents,
            });
        }

        let credit_amount_cents = total_amount_cents.saturating_sub(request.paid_amount_cents);

        Ok(Prepared::new(PreparedPurchase {
            purchase_id: request.purchase_id,
            purchase_number: request.purchase_number,
            supplier_id: request.supplier_id,
            items: authoritative_items,
            total_amount_cents,
            paid_amount_cents: request.paid_amount_cents,
            credit_amount_cents,
            payment_method: request.payment_method,
            user_id: request.user_id,
            purchase_date: request.purchase_date,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 3. PREPARE CUSTOMER PAYMENT
    // --------------------------------------------------------------------------------------------
    pub fn prepare_customer_payment(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: RecordCustomerPaymentRequest,
    ) -> Result<Prepared<PreparedCustomerPayment>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::CustomerCredits.as_str())?;

        if request.amount_cents <= 0 {
            return Err(EngineError::InvalidAmount {
                field: "amount_cents".to_string(),
                amount: request.amount_cents,
                message: "Customer payment amount must be positive".to_string(),
            });
        }

        let valid_methods = ["CASH", "UPI", "BANK_TRANSFER", "CARD", "OTHER"];
        let norm_method = request.payment_method.trim().to_uppercase();
        if !valid_methods.contains(&norm_method.as_str()) {
            return Err(EngineError::DatabaseError(format!(
                "Invalid payment method '{}'. Valid methods: {:?}",
                request.payment_method, valid_methods
            )));
        }

        // Verify customer exists, is active, and read outstanding credit
        let (current_credit_cents, is_active): (i64, i64) = conn.query_row(
            "SELECT current_credit_cents, is_active FROM customers WHERE id = ?1",
            params![request.customer_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|_| EngineError::EntityNotFound(format!("Customer {}", request.customer_id)))?;

        if is_active != 1 {
            return Err(EngineError::DatabaseError(format!(
                "Customer {} is inactive",
                request.customer_id
            )));
        }

        if request.amount_cents > current_credit_cents {
            return Err(EngineError::OverpaymentNotAllowed {
                owed: current_credit_cents,
                attempted: request.amount_cents,
            });
        }

        let balance_after_cents = current_credit_cents - request.amount_cents;

        Ok(Prepared::new(PreparedCustomerPayment {
            payment_id: request.payment_id,
            customer_id: request.customer_id,
            amount_cents: request.amount_cents,
            payment_method: norm_method,
            user_id: request.user_id,
            notes: request.notes,
            balance_before_cents: current_credit_cents,
            balance_after_cents,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 4. PREPARE SUPPLIER PAYMENT
    // --------------------------------------------------------------------------------------------
    pub fn prepare_supplier_payment(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: RecordSupplierPaymentRequest,
    ) -> Result<Prepared<PreparedSupplierPayment>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Suppliers.as_str())?;

        if request.amount_cents <= 0 {
            return Err(EngineError::InvalidAmount {
                field: "amount_cents".to_string(),
                amount: request.amount_cents,
                message: "Supplier payment amount must be positive".to_string(),
            });
        }

        // Verify supplier exists and read outstanding balance
        let current_outstanding_cents: i64 = conn.query_row(
            "SELECT current_outstanding_cents FROM suppliers WHERE id = ?1",
            params![request.supplier_id],
            |row| row.get(0),
        ).map_err(|_| EngineError::EntityNotFound(format!("Supplier {}", request.supplier_id)))?;

        if request.amount_cents > current_outstanding_cents {
            return Err(EngineError::OverpaymentNotAllowed {
                owed: current_outstanding_cents,
                attempted: request.amount_cents,
            });
        }

        let balance_after_cents = current_outstanding_cents - request.amount_cents;

        Ok(Prepared::new(PreparedSupplierPayment {
            payment_id: request.payment_id,
            supplier_id: request.supplier_id,
            amount_cents: request.amount_cents,
            payment_method: request.payment_method,
            user_id: request.user_id,
            notes: request.notes,
            balance_before_cents: current_outstanding_cents,
            balance_after_cents,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 5. PREPARE CUSTOMER RETURN
    // --------------------------------------------------------------------------------------------
    pub fn prepare_customer_return(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: ProcessCustomerReturnRequest,
    ) -> Result<Prepared<PreparedCustomerReturn>, EngineError> {
        // 1. Admin authority check
        AuthorizationService::require_admin(identity, "approve customer return")?;

        if request.items.is_empty() {
            return Err(EngineError::InvalidQuantity {
                product_id: "".to_string(),
                quantity: 0,
                message: "Return must contain at least one item".to_string(),
            });
        }

        // 2. Read customer credit balance
        let current_debt: i64 = conn.query_row(
            "SELECT current_credit_cents FROM customers WHERE id = ?1",
            params![request.customer_id],
            |row| row.get(0),
        ).map_err(|_| EngineError::EntityNotFound(format!("Customer {}", request.customer_id)))?;

        // 3. Authoritatively calculate total return value
        let mut authoritative_items = Vec::new();
        let mut total_amount_cents: i64 = 0;

        for item in request.items {
            if item.quantity <= 0 {
                return Err(EngineError::InvalidQuantity {
                    product_id: item.product_id,
                    quantity: item.quantity,
                    message: "Returned quantity must be strictly positive".to_string(),
                });
            }

            // Verify product exists
            let p_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM products WHERE id = ?1",
                params![item.product_id],
                |row| row.get(0),
            )?;
            if p_count == 0 {
                return Err(EngineError::EntityNotFound(format!("Product {}", item.product_id)));
            }

            let line_total_cents = calculate_line_total(item.quantity, item.unit_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;

            total_amount_cents = total_amount_cents
                .checked_add(line_total_cents)
                .ok_or_else(|| EngineError::MathError("Total return amount overflow".to_string()))?;

            authoritative_items.push(AuthoritativeReturnItem {
                product_id: item.product_id,
                quantity: item.quantity,
                unit_price_cents: item.unit_price_cents,
                line_total_cents,
            });
        }

        // 4. Authoritative debt-reduction vs cash refund split
        let debt_reduction_cents = std::cmp::min(total_amount_cents, current_debt);
        let cash_refund_cents = total_amount_cents - debt_reduction_cents;
        let balance_after_cents = current_debt - debt_reduction_cents;

        Ok(Prepared::new(PreparedCustomerReturn {
            return_id: request.return_id,
            return_number: request.return_number,
            reference_sale_id: request.reference_sale_id,
            customer_id: request.customer_id,
            items: authoritative_items,
            total_amount_cents,
            debt_reduction_cents,
            cash_refund_cents,
            balance_before_cents: current_debt,
            balance_after_cents,
            reason: request.reason,
            admin_user_id: identity.user_id().to_string(),
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 6. PREPARE SUPPLIER RETURN
    // --------------------------------------------------------------------------------------------
    pub fn prepare_supplier_return(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: ProcessSupplierReturnRequest,
    ) -> Result<Prepared<PreparedSupplierReturn>, EngineError> {
        // 1. Admin authority check
        AuthorizationService::require_admin(identity, "approve supplier return")?;

        if request.items.is_empty() {
            return Err(EngineError::InvalidQuantity {
                product_id: "".to_string(),
                quantity: 0,
                message: "Supplier return must contain at least one item".to_string(),
            });
        }

        // 2. Read current supplier payable balance
        let current_payable: i64 = conn.query_row(
            "SELECT current_outstanding_cents FROM suppliers WHERE id = ?1",
            params![request.supplier_id],
            |row| row.get(0),
        ).map_err(|_| EngineError::EntityNotFound(format!("Supplier {}", request.supplier_id)))?;

        // 3. Authoritatively calculate return value and verify available stock
        let mut authoritative_items = Vec::new();
        let mut total_amount_cents: i64 = 0;

        for item in request.items {
            if item.quantity <= 0 {
                return Err(EngineError::InvalidQuantity {
                    product_id: item.product_id,
                    quantity: item.quantity,
                    message: "Returned quantity must be strictly positive".to_string(),
                });
            }

            // Verify stock is available for return (cannot return stock we don't have)
            let available: i64 = conn.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![item.product_id],
                |row| row.get(0),
            ).unwrap_or(0);

            if available < item.quantity {
                return Err(EngineError::InsufficientStock {
                    product_id: item.product_id,
                    available,
                    requested: item.quantity,
                });
            }

            let line_total_cents = calculate_line_total(item.quantity, item.unit_price_cents)
                .map_err(|e| EngineError::MathError(e.to_string()))?;

            total_amount_cents = total_amount_cents
                .checked_add(line_total_cents)
                .ok_or_else(|| EngineError::MathError("Total return amount overflow".to_string()))?;

            authoritative_items.push(AuthoritativeReturnItem {
                product_id: item.product_id,
                quantity: item.quantity,
                unit_price_cents: item.unit_price_cents,
                line_total_cents,
            });
        }

        let balance_after_cents = current_payable - total_amount_cents;

        Ok(Prepared::new(PreparedSupplierReturn {
            return_id: request.return_id,
            return_number: request.return_number,
            reference_purchase_id: request.reference_purchase_id,
            supplier_id: request.supplier_id,
            items: authoritative_items,
            total_amount_cents,
            balance_before_cents: current_payable,
            balance_after_cents,
            reason: request.reason,
            admin_user_id: identity.user_id().to_string(),
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 7. PREPARE STOCK CORRECTION
    // --------------------------------------------------------------------------------------------
    pub fn prepare_stock_correction(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: RecordStockCorrectionRequest,
    ) -> Result<Prepared<PreparedStockCorrection>, EngineError> {
        // 1. Admin authority check
        AuthorizationService::require_admin(identity, "stock correction")?;

        if request.quantity_change == 0 {
            return Err(EngineError::InvalidQuantity {
                product_id: request.product_id,
                quantity: 0,
                message: "Stock correction delta cannot be zero".to_string(),
            });
        }

        // 2. Reason validation
        match request.reason.as_str() {
            "DAMAGED" | "EXPIRED" | "LOST" | "MISCOUNT" => {}
            _ => return Err(EngineError::InvalidCorrectionReason(request.reason)),
        }

        // 3. Read current quantity and verify resulting stock >= 0
        let qty_before: i64 = conn.query_row(
            "SELECT current_quantity FROM inventory WHERE product_id = ?1",
            params![request.product_id],
            |row| row.get(0),
        ).map_err(|_| EngineError::EntityNotFound(format!("Inventory for product {}", request.product_id)))?;

        let qty_after = qty_before + request.quantity_change;
        if qty_after < 0 {
            return Err(EngineError::InsufficientStock {
                product_id: request.product_id,
                available: qty_before,
                requested: request.quantity_change.abs(),
            });
        }

        Ok(Prepared::new(PreparedStockCorrection {
            correction_id: request.correction_id,
            product_id: request.product_id,
            quantity_change: request.quantity_change,
            quantity_before: qty_before,
            quantity_after: qty_after,
            reason: request.reason,
            note: request.note,
            admin_user_id: identity.user_id().to_string(),
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 8. PREPARE ORDER CONVERSION
    // --------------------------------------------------------------------------------------------
    pub fn prepare_order_conversion(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: ConvertOrderToSaleRequest,
    ) -> Result<Prepared<PreparedOrderConversion>, EngineError> {
        // 1. Permission check
        AuthorizationService::authorize(conn, identity, PermissionKey::CustomerOrders.as_str())?;

        // 2. Verify order status
        let (status, customer_id): (String, Option<String>) = conn.query_row(
            "SELECT status, customer_id FROM customer_orders WHERE id = ?1",
            params![request.order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|_| EngineError::EntityNotFound(format!("Order {}", request.order_id)))?;

        if status == "CONVERTED" {
            return Err(EngineError::OrderAlreadyConverted(format!("Order {} is already converted", request.order_id)));
        }

        if status != "DRAFT" {
            return Err(EngineError::OrderNotDraft(format!("Order {} is in status '{}', expected 'DRAFT'", request.order_id, status)));
        }

        // 3. Fetch items from customer_order_items and resolve current authoritative catalog pricing
        let mut items_stmt = conn.prepare("SELECT product_id, quantity FROM customer_order_items WHERE order_id = ?1")?;
        let rows = items_stmt.query_map(params![request.order_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut items = Vec::new();
        for item_res in rows {
            let (product_id, quantity) = item_res?;
            if quantity <= 0 {
                return Err(EngineError::InvalidQuantity {
                    product_id: product_id.clone(),
                    quantity,
                    message: "Order item quantity must be strictly positive".to_string(),
                });
            }

            // Price authority: Fetch authoritative current selling price from products catalog
            let (current_selling_price, is_active): (i64, i64) = conn
                .query_row(
                    "SELECT selling_price_cents, is_active FROM products WHERE id = ?1",
                    params![product_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Product {}", product_id)))?;

            if is_active != 1 {
                return Err(EngineError::DatabaseError(format!("Product {} is inactive", product_id)));
            }

            items.push(SaleItemRequest {
                product_id,
                quantity,
                unit_price_cents: current_selling_price,
            });
        }

        if items.is_empty() {
            return Err(EngineError::OrderNotDraft("Cannot convert empty order with no items".to_string()));
        }

        // 4. Delegate to prepare_sale for full inventory and financial validation
        let sale_request = ConfirmSaleRequest {
            sale_id: request.sale_id,
            sale_number: request.sale_number,
            customer_id,
            items,
            paid_amount_cents: request.paid_amount_cents,
            payment_method: request.payment_method,
            user_id: request.user_id,
            sale_date: request.sale_date,
        };

        let prepared_sale = Self::prepare_sale(conn, identity, sale_request)?;

        Ok(Prepared::new(PreparedOrderConversion {
            order_id: request.order_id,
            prepared_sale: prepared_sale.payload,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 9. PREPARE EXPENSE
    // --------------------------------------------------------------------------------------------
    pub fn prepare_expense(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: RecordExpenseRequest,
    ) -> Result<Prepared<PreparedExpense>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Expenses.as_str())?;

        if request.amount_cents <= 0 {
            return Err(EngineError::InvalidAmount {
                field: "amount_cents".to_string(),
                amount: request.amount_cents,
                message: "Expense amount must be strictly positive".to_string(),
            });
        }

        // Verify user exists
        let u_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE id = ?1",
            params![request.user_id],
            |row| row.get(0),
        )?;
        if u_count == 0 {
            return Err(EngineError::EntityNotFound(format!("User {}", request.user_id)));
        }

        Ok(Prepared::new(PreparedExpense {
            expense_id: request.expense_id,
            expense_name: request.expense_name,
            amount_cents: request.amount_cents,
            category: request.category,
            expense_date: request.expense_date,
            notes: request.notes,
            user_id: request.user_id,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 10. PREPARE PAYMENT (WITH STRICT POLYMORPHIC REFERENCE VALIDATION)
    // --------------------------------------------------------------------------------------------
    pub fn prepare_payment(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: RecordPaymentRequest,
    ) -> Result<Prepared<PreparedPayment>, EngineError> {
        let feature = match request.related_entity_type.as_str() {
            "SALE" | "CUSTOMER" => PermissionKey::Sales.as_str(),
            "PURCHASE" | "SUPPLIER" => PermissionKey::Purchases.as_str(),
            _ => PermissionKey::Dashboard.as_str(),
        };
        AuthorizationService::authorize(conn, identity, feature)?;

        if request.amount_cents <= 0 {
            return Err(EngineError::InvalidAmount {
                field: "amount_cents".to_string(),
                amount: request.amount_cents,
                message: "Payment amount must be strictly positive".to_string(),
            });
        }

        let table_name = match request.related_entity_type.as_str() {
            "SALE" => "sales",
            "CUSTOMER" => "customers",
            "PURCHASE" => "purchases",
            "SUPPLIER" => "suppliers",
            "RETURN" => "returns",
            _ => return Err(EngineError::InvalidPaymentReference {
                entity_type: request.related_entity_type,
                entity_id: request.related_entity_id,
            }),
        };

        let query = format!("SELECT COUNT(*) FROM {} WHERE id = ?1", table_name);
        let count: i64 = conn.query_row(&query, params![request.related_entity_id], |row| row.get(0))?;

        if count == 0 {
            return Err(EngineError::InvalidPaymentReference {
                entity_type: request.related_entity_type,
                entity_id: request.related_entity_id,
            });
        }

        Ok(Prepared::new(PreparedPayment {
            payment_id: request.payment_id,
            payment_type: request.payment_type,
            related_entity_type: request.related_entity_type,
            related_entity_id: request.related_entity_id,
            amount_cents: request.amount_cents,
            payment_method: request.payment_method,
            user_id: request.user_id,
            notes: request.notes,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 11. PREPARE CREATE PRODUCT (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn prepare_create_product(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: CreateProductRequest,
    ) -> Result<Prepared<PreparedCreateProduct>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        // 1. Verify business existence
        let b_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM businesses WHERE id = ?1",
            params![request.business_id],
            |r| r.get(0),
        )?;
        if b_count == 0 {
            return Err(EngineError::EntityNotFound(format!("Business {}", request.business_id)));
        }

        // 2. Validate product name
        let trimmed_name = request.name.trim().to_string();
        if trimmed_name.is_empty() {
            return Err(EngineError::InvalidProductName("Product name cannot be empty".to_string()));
        }
        if trimmed_name.len() > 255 {
            return Err(EngineError::InvalidProductName("Product name exceeds 255 characters".to_string()));
        }

        // 3. Validate product type
        let p_type = ProductType::from_str(&request.product_type).ok_or_else(|| {
            EngineError::InvalidProductType(format!(
                "Unsupported product type '{}', expected 'PACKAGED' or 'LOOSE'",
                request.product_type
            ))
        })?;

        // 4. Validate unit
        let trimmed_unit = request.unit.trim().to_string();
        if trimmed_unit.is_empty() {
            return Err(EngineError::InvalidUnit("Product unit cannot be empty".to_string()));
        }
        if trimmed_unit.len() > 30 {
            return Err(EngineError::InvalidUnit("Product unit exceeds 30 characters".to_string()));
        }

        // 5. Validate pricing
        if request.cost_price_cents < 0 {
            return Err(EngineError::InvalidAmount {
                field: "cost_price_cents".to_string(),
                amount: request.cost_price_cents,
                message: "Cost price cannot be negative".to_string(),
            });
        }
        if request.selling_price_cents < 0 {
            return Err(EngineError::InvalidAmount {
                field: "selling_price_cents".to_string(),
                amount: request.selling_price_cents,
                message: "Selling price cannot be negative".to_string(),
            });
        }

        // 6. Validate initial stock and minimum stock
        if request.initial_stock < 0 {
            return Err(EngineError::NegativeStock(request.initial_stock));
        }
        if request.min_stock_level < 0 {
            return Err(EngineError::NegativeMinimumStock(request.min_stock_level));
        }

        // 7. Validate barcode mapping
        let (norm_barcode, b_type) = match request.barcode.as_ref().map(|b| b.trim()).filter(|b| !b.is_empty()) {
            Some(b) => {
                let existing: Result<String, _> = conn.query_row(
                    "SELECT product_id FROM barcode_mappings WHERE barcode = ?1",
                    params![b],
                    |r| r.get(0),
                );
                if let Ok(existing_prod) = existing {
                    return Err(EngineError::DuplicateBarcodeMapping {
                        barcode: b.to_string(),
                        existing_product_id: existing_prod,
                    });
                }
                let bt = request
                    .barcode_type
                    .as_deref()
                    .and_then(BarcodeType::from_str)
                    .unwrap_or(BarcodeType::Manufacturer);
                (Some(b.to_string()), bt)
            }
            None => (None, BarcodeType::Manufacturer),
        };

        let product_id = request.product_id.unwrap_or_else(|| next_entity_id("prod"));

        Ok(Prepared::new(PreparedCreateProduct {
            product_id,
            business_id: request.business_id,
            name: trimmed_name,
            product_type: p_type,
            unit: trimmed_unit,
            barcode: norm_barcode,
            barcode_type: b_type,
            cost_price_cents: request.cost_price_cents,
            selling_price_cents: request.selling_price_cents,
            initial_stock: request.initial_stock,
            min_stock_level: request.min_stock_level,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 12. PREPARE CREATE PRODUCT BATCH (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn prepare_create_product_batch(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: CreateProductBatchRequest,
    ) -> Result<Prepared<PreparedCreateProductBatch>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        if request.products.is_empty() {
            return Err(EngineError::InvalidQuantity {
                product_id: "".to_string(),
                quantity: 0,
                message: "Batch must contain at least one product".to_string(),
            });
        }

        // Validate business existence
        let b_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM businesses WHERE id = ?1",
            params![request.business_id],
            |r| r.get(0),
        )?;
        if b_count == 0 {
            return Err(EngineError::EntityNotFound(format!("Business {}", request.business_id)));
        }

        let mut prepared_list = Vec::new();
        let mut seen_barcodes = std::collections::HashSet::new();

        for mut item in request.products {
            // Inherit batch business_id
            item.business_id = request.business_id.clone();

            // Check duplicate barcode within batch
            if let Some(ref b) = item.barcode {
                let trimmed = b.trim();
                if !trimmed.is_empty() {
                    if !seen_barcodes.insert(trimmed.to_string()) {
                        return Err(EngineError::DuplicateBarcodeMapping {
                            barcode: trimmed.to_string(),
                            existing_product_id: "batch_duplicate".to_string(),
                        });
                    }
                }
            }

            let prepared_item = Self::prepare_create_product(conn, identity, item)?;
            prepared_list.push(prepared_item.payload);
        }

        Ok(Prepared::new(PreparedCreateProductBatch {
            business_id: request.business_id,
            products: prepared_list,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 13. PREPARE UPDATE PRODUCT (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn prepare_update_product(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: UpdateProductRequest,
    ) -> Result<Prepared<PreparedUpdateProduct>, EngineError> {
        // Base inventory permission required for editing any product metadata
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        // If prices are being updated, PRICES permission is additionally required
        if request.cost_price_cents.is_some() || request.selling_price_cents.is_some() {
            AuthorizationService::authorize(conn, identity, PermissionKey::Prices.as_str())?;
        }

        // 1. Fetch current product and verify business ownership
        let (prod_business_id, _name, current_unit, _cost_cents, _sell_cents, _min_stock): (
            Option<String>,
            String,
            String,
            i64,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT business_id, name, unit, cost_price_cents, selling_price_cents, min_stock_level
                 FROM products WHERE id = ?1",
                params![request.product_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Product {}", request.product_id)))?;

        let actual_biz = prod_business_id.unwrap_or_else(|| "biz_default".to_string());
        if actual_biz != request.business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: request.business_id,
                resource_business: actual_biz,
            });
        }

        // 2. Validate name if updated
        let norm_name = if let Some(ref n) = request.name {
            let t = n.trim();
            if t.is_empty() {
                return Err(EngineError::InvalidProductName("Product name cannot be empty".to_string()));
            }
            if t.len() > 255 {
                return Err(EngineError::InvalidProductName("Product name exceeds 255 characters".to_string()));
            }
            Some(t.to_string())
        } else {
            None
        };

        // 3. Validate unit immutability rule
        let norm_unit = if let Some(ref u) = request.unit {
            let t = u.trim();
            if t.is_empty() {
                return Err(EngineError::InvalidUnit("Product unit cannot be empty".to_string()));
            }
            if t.len() > 30 {
                return Err(EngineError::InvalidUnit("Product unit exceeds 30 characters".to_string()));
            }

            if t != current_unit {
                // Check if inventory has been initialized or stock history exists
                let current_qty: i64 = conn
                    .query_row(
                        "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                        params![request.product_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let m_count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM stock_movements WHERE product_id = ?1",
                        params![request.product_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);

                if current_qty > 0 || m_count > 0 {
                    return Err(EngineError::UnitChangeForbidden(format!(
                        "Unit cannot be changed from '{}' to '{}' after inventory has been initialized or stock history exists (current_quantity: {}, movements: {})",
                        current_unit, t, current_qty, m_count
                    )));
                }
            }
            Some(t.to_string())
        } else {
            None
        };

        // 4. Validate prices if updated
        if let Some(c) = request.cost_price_cents {
            if c < 0 {
                return Err(EngineError::InvalidAmount {
                    field: "cost_price_cents".to_string(),
                    amount: c,
                    message: "Cost price cannot be negative".to_string(),
                });
            }
        }
        if let Some(s) = request.selling_price_cents {
            if s < 0 {
                return Err(EngineError::InvalidAmount {
                    field: "selling_price_cents".to_string(),
                    amount: s,
                    message: "Selling price cannot be negative".to_string(),
                });
            }
        }

        // 5. Validate minimum stock if updated
        if let Some(m) = request.min_stock_level {
            if m < 0 {
                return Err(EngineError::NegativeMinimumStock(m));
            }
        }

        // 6. Validate barcode changes
        let (norm_barcode, norm_barcode_type) = match request.barcode {
            Some(Some(ref b)) => {
                let t = b.trim();
                if t.is_empty() {
                    (Some(None), None)
                } else {
                    let existing: Result<String, _> = conn.query_row(
                        "SELECT product_id FROM barcode_mappings WHERE barcode = ?1",
                        params![t],
                        |r| r.get(0),
                    );
                    if let Ok(other_id) = existing {
                        if other_id != request.product_id {
                            return Err(EngineError::DuplicateBarcodeMapping {
                                barcode: t.to_string(),
                                existing_product_id: other_id,
                            });
                        }
                    }
                    let bt = request
                        .barcode_type
                        .as_deref()
                        .and_then(BarcodeType::from_str)
                        .unwrap_or(BarcodeType::Manufacturer);
                    (Some(Some(t.to_string())), Some(bt))
                }
            }
            Some(None) => (Some(None), None),
            None => (None, None),
        };

        Ok(Prepared::new(PreparedUpdateProduct {
            product_id: request.product_id,
            business_id: request.business_id,
            name: norm_name,
            unit: norm_unit,
            barcode: norm_barcode,
            barcode_type: norm_barcode_type,
            cost_price_cents: request.cost_price_cents,
            selling_price_cents: request.selling_price_cents,
            min_stock_level: request.min_stock_level,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 14. PREPARE REMAP BARCODE (ADMIN ONLY) (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn prepare_remap_barcode(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        request: RemapBarcodeRequest,
    ) -> Result<Prepared<PreparedRemapBarcode>, EngineError> {
        AuthorizationService::require_admin(identity, "REMAP_BARCODE")?;

        let norm_barcode = request.barcode.trim();
        if norm_barcode.is_empty() {
            return Err(EngineError::EntityNotFound("Barcode cannot be empty".to_string()));
        }

        // Find existing mapping and verify business ownership
        let (old_product_id, old_business_id): (String, Option<String>) = conn
            .query_row(
                "SELECT b.product_id, p.business_id
                 FROM barcode_mappings b
                 JOIN products p ON p.id = b.product_id
                 WHERE b.barcode = ?1",
                params![norm_barcode],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Barcode mapping for '{}'", norm_barcode)))?;

        let actual_old_biz = old_business_id.unwrap_or_else(|| "biz_default".to_string());
        if actual_old_biz != request.business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: request.business_id.clone(),
                resource_business: actual_old_biz,
            });
        }

        // Verify target new product exists and belongs to same business
        let new_business_id: Option<String> = conn
            .query_row(
                "SELECT business_id FROM products WHERE id = ?1",
                params![request.new_product_id],
                |r| r.get(0),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Product {}", request.new_product_id)))?;

        let actual_new_biz = new_business_id.unwrap_or_else(|| "biz_default".to_string());
        if actual_new_biz != request.business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: request.business_id.clone(),
                resource_business: actual_new_biz,
            });
        }

        Ok(Prepared::new(PreparedRemapBarcode {
            business_id: request.business_id,
            barcode: norm_barcode.to_string(),
            old_product_id,
            new_product_id: request.new_product_id,
        }))
    }

    // --------------------------------------------------------------------------------------------
    // 15. READ-ONLY QUERY FUNCTIONS (BUILD 05)
    // --------------------------------------------------------------------------------------------

    pub fn get_product(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        business_id: &str,
        product_id: &str,
    ) -> Result<ProductResult, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        let row = conn.query_row(
            "SELECT id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at
             FROM products WHERE id = ?1",
            params![product_id],
            |r| {
                Ok(ProductResult {
                    id: r.get(0)?,
                    business_id: r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "biz_default".to_string()),
                    name: r.get(2)?,
                    product_type: r.get(3)?,
                    unit: r.get(4)?,
                    cost_price_cents: r.get(5)?,
                    selling_price_cents: r.get(6)?,
                    min_stock_level: r.get(7)?,
                    is_active: r.get(8)?,
                    created_at: r.get(9)?,
                    updated_at: r.get(10)?,
                })
            },
        ).map_err(|_| EngineError::EntityNotFound(format!("Product {}", product_id)))?;

        if row.business_id != business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: business_id.to_string(),
                resource_business: row.business_id,
            });
        }

        Ok(row)
    }

    pub fn list_products(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        business_id: &str,
    ) -> Result<Vec<ProductResult>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        let mut stmt = conn.prepare(
            "SELECT id, business_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at
             FROM products
             WHERE business_id = ?1 OR (?1 = 'biz_default' AND business_id IS NULL)
             ORDER BY name ASC",
        )?;

        let rows = stmt.query_map(params![business_id], |r| {
            Ok(ProductResult {
                id: r.get(0)?,
                business_id: r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "biz_default".to_string()),
                name: r.get(2)?,
                product_type: r.get(3)?,
                unit: r.get(4)?,
                cost_price_cents: r.get(5)?,
                selling_price_cents: r.get(6)?,
                min_stock_level: r.get(7)?,
                is_active: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn get_inventory(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        business_id: &str,
        product_id: &str,
    ) -> Result<InventoryResult, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        let (actual_biz, min_stock): (Option<String>, i64) = conn
            .query_row(
                "SELECT business_id, min_stock_level FROM products WHERE id = ?1",
                params![product_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Product {}", product_id)))?;

        let biz = actual_biz.unwrap_or_else(|| "biz_default".to_string());
        if biz != business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: business_id.to_string(),
                resource_business: biz,
            });
        }

        let (current_quantity, last_updated_at): (i64, String) = conn
            .query_row(
                "SELECT current_quantity, last_updated_at FROM inventory WHERE product_id = ?1",
                params![product_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Inventory for product {}", product_id)))?;

        let status = StockStatus::calculate(current_quantity, min_stock);

        Ok(InventoryResult {
            product_id: product_id.to_string(),
            current_quantity,
            min_stock_level: min_stock,
            stock_status: status.as_str().to_string(),
            last_updated_at,
        })
    }

    pub fn get_product_stock_status(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        business_id: &str,
        product_id: &str,
    ) -> Result<ProductStockStatusResult, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        let (actual_biz, name, min_stock): (Option<String>, String, i64) = conn
            .query_row(
                "SELECT business_id, name, min_stock_level FROM products WHERE id = ?1",
                params![product_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Product {}", product_id)))?;

        let biz = actual_biz.unwrap_or_else(|| "biz_default".to_string());
        if biz != business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: business_id.to_string(),
                resource_business: biz,
            });
        }

        let current_quantity: i64 = conn
            .query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![product_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let status = StockStatus::calculate(current_quantity, min_stock);

        Ok(ProductStockStatusResult {
            product_id: product_id.to_string(),
            product_name: name,
            current_quantity,
            min_stock_level: min_stock,
            stock_status: status.as_str().to_string(),
        })
    }

    pub fn list_low_stock_products(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        business_id: &str,
    ) -> Result<Vec<ProductStockStatusResult>, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, COALESCE(i.current_quantity, 0), p.min_stock_level
             FROM products p
             LEFT JOIN inventory i ON i.product_id = p.id
             WHERE (p.business_id = ?1 OR (?1 = 'biz_default' AND p.business_id IS NULL))
               AND COALESCE(i.current_quantity, 0) <= p.min_stock_level
             ORDER BY p.name ASC",
        )?;

        let rows = stmt.query_map(params![business_id], |r| {
            let qty: i64 = r.get(2)?;
            let min_stock: i64 = r.get(3)?;
            let status = StockStatus::calculate(qty, min_stock);
            Ok(ProductStockStatusResult {
                product_id: r.get(0)?,
                product_name: r.get(1)?,
                current_quantity: qty,
                min_stock_level: min_stock,
                stock_status: status.as_str().to_string(),
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn resolve_barcode(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        business_id: &str,
        barcode: &str,
    ) -> Result<BarcodeResolutionResult, EngineError> {
        AuthorizationService::authorize(conn, identity, PermissionKey::Inventory.as_str())?;

        let norm_barcode = barcode.trim();
        if norm_barcode.is_empty() {
            return Err(EngineError::EntityNotFound("Barcode cannot be empty".to_string()));
        }

        let row = conn.query_row(
            "SELECT b.barcode_type, p.id, p.business_id, p.name, p.product_type, p.unit,
                    p.cost_price_cents, p.selling_price_cents, COALESCE(i.current_quantity, 0)
             FROM barcode_mappings b
             JOIN products p ON p.id = b.product_id
             LEFT JOIN inventory i ON i.product_id = p.id
             WHERE b.barcode = ?1",
            params![norm_barcode],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?.unwrap_or_else(|| "biz_default".to_string()),
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            },
        ).map_err(|_| EngineError::EntityNotFound(format!("Barcode mapping for '{}'", norm_barcode)))?;

        let (b_type, prod_id, prod_biz, name, p_type, unit, cost_cents, sell_cents, cur_qty) = row;

        if prod_biz != business_id {
            return Err(EngineError::CrossBusinessAccessDenied {
                attempted_business: business_id.to_string(),
                resource_business: prod_biz,
            });
        }

        // Extensible weighing scale metadata foundation
        let weighing_metadata = if b_type == "WEIGHING_SCALE" {
            Some(WeighingScaleMetadata {
                raw_payload: norm_barcode.to_string(),
                embedded_weight_millie_units: None,
                embedded_price_cents: None,
            })
        } else {
            None
        };

        Ok(BarcodeResolutionResult {
            barcode: norm_barcode.to_string(),
            barcode_type: b_type,
            product_id: prod_id,
            product_name: name,
            product_type: p_type,
            unit,
            cost_price_cents: cost_cents,
            selling_price_cents: sell_cents,
            current_quantity: cur_qty,
            weighing_metadata,
        })
    }
}

fn next_entity_id(prefix: &str) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}_{:x}_{:x}", prefix, nanos, count)
}


