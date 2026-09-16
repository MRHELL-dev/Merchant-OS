use rusqlite::{params, Connection};

use super::commands::*;
use super::error::EngineError;

/// The Transaction Engine is the sole authoritative executor of consequential business mutations.
///
/// Crucial Architecture Invariants:
/// - Only accepts explicitly `Confirmed<T>` commands.
/// - Executes inside atomic SQLite transactions (`tx = conn.transaction()?`).
/// - Commits all mutations, stock movements, ledgers, and audit logs together.
/// - Any failure triggers an automatic, 100% rollback of all changes.
pub struct TransactionEngine;

impl TransactionEngine {
    // --------------------------------------------------------------------------------------------
    // 1. EXECUTE CONFIRMED SALE
    // --------------------------------------------------------------------------------------------
    pub fn execute_sale(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedSale>,
    ) -> Result<(), EngineError> {
        let tx = conn.transaction()?;
        Self::execute_sale_in_tx(&tx, &confirmed)?;
        tx.commit()?;
        Ok(())
    }

    /// Internal helper that writes all sale tables within an existing SQLite transaction.
    fn execute_sale_in_tx(
        tx: &rusqlite::Transaction,
        confirmed: &Confirmed<PreparedSale>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into sales
        tx.execute(
            "INSERT INTO sales (id, sale_number, customer_id, total_amount_cents, paid_amount_cents, credit_amount_cents, payment_status, user_id, sale_date, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                cmd.sale_id,
                cmd.sale_number,
                cmd.customer_id,
                cmd.total_amount_cents,
                cmd.paid_amount_cents,
                cmd.credit_amount_cents,
                cmd.payment_status,
                confirmed_by,
                cmd.sale_date,
                now,
            ],
        )?;

        // 2. Insert sale items & update stock movements
        for (idx, item) in cmd.items.iter().enumerate() {
            let item_id = format!("{}_item_{}", cmd.sale_id, idx);
            tx.execute(
                "INSERT INTO sale_items (id, sale_id, product_id, quantity, unit_price_cents, cost_price_cents, total_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    item_id,
                    cmd.sale_id,
                    item.product_id,
                    item.quantity,
                    item.unit_price_cents,
                    item.cost_price_cents,
                    item.line_total_cents,
                ],
            )?;

            // 2b. Concurrency-Safe Atomic Conditional Stock Decrement (Zero-Oversell Protection)
            let qty_before: i64 = tx
                .query_row(
                    "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                    params![item.product_id],
                    |row| row.get(0),
                )
                .map_err(|_| EngineError::EntityNotFound(format!("Inventory for product {}", item.product_id)))?;

            let rows_affected = tx.execute(
                "UPDATE inventory
                 SET current_quantity = current_quantity - ?1,
                     last_updated_at = ?2
                 WHERE product_id = ?3
                   AND current_quantity >= ?1",
                params![item.quantity, now, item.product_id],
            )?;

            if rows_affected == 0 {
                return Err(EngineError::InsufficientStock {
                    product_id: item.product_id.clone(),
                    available: qty_before,
                    requested: item.quantity,
                });
            }
            let qty_after = qty_before - item.quantity;

            let movement_id = format!("{}_mov_{}", cmd.sale_id, idx);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'SALE', 'SALES', ?6, ?7, ?8)",
                params![
                    movement_id,
                    item.product_id,
                    -item.quantity,
                    qty_before,
                    qty_after,
                    cmd.sale_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 3. Create payment record if money was actually paid
        if cmd.paid_amount_cents > 0 {
            let payment_id = format!("{}_pmt", cmd.sale_id);
            let method = cmd.payment_method.as_deref().unwrap_or("CASH");
            tx.execute(
                "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
                 VALUES (?1, 'CUSTOMER_SALE', 'SALE', ?2, ?3, ?4, ?5, 'Checkout payment', ?6)",
                params![payment_id, cmd.sale_id, cmd.paid_amount_cents, method, confirmed_by, now],
            )?;
        }

        // 4. Update customer credit & customer ledger if credit was extended
        if cmd.credit_amount_cents > 0 {
            let c_id = cmd.customer_id.as_deref().ok_or_else(|| {
                EngineError::DatabaseError("Credit sale requires a registered customer".to_string())
            })?;

            let bal_before: i64 = tx.query_row(
                "SELECT current_credit_cents FROM customers WHERE id = ?1",
                params![c_id],
                |row| row.get(0),
            )?;
            let bal_after = bal_before + cmd.credit_amount_cents;

            tx.execute(
                "UPDATE customers SET current_credit_cents = ?1, updated_at = ?2 WHERE id = ?3",
                params![bal_after, now, c_id],
            )?;

            let ledger_id = format!("{}_cleg", cmd.sale_id);
            tx.execute(
                "INSERT INTO customer_ledger (id, customer_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
                 VALUES (?1, ?2, 'SALE_CREDIT', ?3, ?4, ?5, 'SALE', ?6, 'Credit sale due', ?7, ?8)",
                params![
                    ledger_id,
                    c_id,
                    cmd.credit_amount_cents,
                    bal_before,
                    bal_after,
                    cmd.sale_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 5. Audit log
        let audit_id = format!("{}_audit", cmd.sale_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'SALE_CONFIRMED', 'sales', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.sale_id,
                format!(
                    "Total: {}, Paid: {}, Credit: {}, ConfirmedBy: {}",
                    cmd.total_amount_cents,
                    cmd.paid_amount_cents,
                    cmd.credit_amount_cents,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 2. EXECUTE CONFIRMED PURCHASE
    // --------------------------------------------------------------------------------------------
    pub fn execute_purchase(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedPurchase>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into purchases
        tx.execute(
            "INSERT INTO purchases (id, purchase_number, supplier_id, total_amount_cents, paid_amount_cents, credit_amount_cents, payment_method, user_id, purchase_date, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                cmd.purchase_id,
                cmd.purchase_number,
                cmd.supplier_id,
                cmd.total_amount_cents,
                cmd.paid_amount_cents,
                cmd.credit_amount_cents,
                cmd.payment_method,
                confirmed_by,
                cmd.purchase_date,
                now,
            ],
        )?;

        // 2. Insert purchase items, ensure inventory records, and record stock movements
        for (idx, item) in cmd.items.iter().enumerate() {
            let item_id = format!("{}_item_{}", cmd.purchase_id, idx);
            tx.execute(
                "INSERT INTO purchase_items (id, purchase_id, product_id, quantity, unit_cost_cents, total_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item_id,
                    cmd.purchase_id,
                    item.product_id,
                    item.quantity,
                    item.unit_cost_cents,
                    item.line_total_cents,
                ],
            )?;

            // Ensure inventory record exists
            tx.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES (?1, ?2, 0, ?3)
                 ON CONFLICT(product_id) DO NOTHING",
                params![format!("inv_{}", item.product_id), item.product_id, now],
            )?;

            let qty_before: i64 = tx.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![item.product_id],
                |row| row.get(0),
            )?;
            let qty_after = qty_before + item.quantity;

            tx.execute(
                "UPDATE inventory SET current_quantity = ?1, last_updated_at = ?2 WHERE product_id = ?3",
                params![qty_after, now, item.product_id],
            )?;

            let movement_id = format!("{}_mov_{}", cmd.purchase_id, idx);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'PURCHASE', 'PURCHASES', ?6, ?7, ?8)",
                params![
                    movement_id,
                    item.product_id,
                    item.quantity,
                    qty_before,
                    qty_after,
                    cmd.purchase_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 3. Update supplier payable & ledger if credit_amount > 0
        if cmd.credit_amount_cents > 0 {
            let bal_before: i64 = tx.query_row(
                "SELECT current_outstanding_cents FROM suppliers WHERE id = ?1",
                params![cmd.supplier_id],
                |row| row.get(0),
            )?;
            let bal_after = bal_before + cmd.credit_amount_cents;

            tx.execute(
                "UPDATE suppliers SET current_outstanding_cents = ?1, updated_at = ?2 WHERE id = ?3",
                params![bal_after, now, cmd.supplier_id],
            )?;

            let ledger_id = format!("{}_sleg", cmd.purchase_id);
            tx.execute(
                "INSERT INTO supplier_ledger (id, supplier_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
                 VALUES (?1, ?2, 'PURCHASE_CREDIT', ?3, ?4, ?5, 'PURCHASE', ?6, 'Purchase credit invoice', ?7, ?8)",
                params![
                    ledger_id,
                    cmd.supplier_id,
                    cmd.credit_amount_cents,
                    bal_before,
                    bal_after,
                    cmd.purchase_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 4. Record payment if paid_amount > 0
        if cmd.paid_amount_cents > 0 {
            let payment_id = format!("{}_pmt", cmd.purchase_id);
            let method = cmd.payment_method.as_deref().unwrap_or("CASH");
            tx.execute(
                "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
                 VALUES (?1, 'PURCHASE_PAYMENT', 'PURCHASE', ?2, ?3, ?4, ?5, 'Purchase invoice payment', ?6)",
                params![payment_id, cmd.purchase_id, cmd.paid_amount_cents, method, confirmed_by, now],
            )?;
        }

        // 5. Audit log
        let audit_id = format!("{}_audit", cmd.purchase_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PURCHASE_CONFIRMED', 'purchases', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.purchase_id,
                format!(
                    "Total: {}, Paid: {}, Due: {}, ConfirmedBy: {}",
                    cmd.total_amount_cents,
                    cmd.paid_amount_cents,
                    cmd.credit_amount_cents,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 3. EXECUTE CONFIRMED CUSTOMER PAYMENT
    // --------------------------------------------------------------------------------------------
    pub fn execute_customer_payment(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedCustomerPayment>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Live in-transaction snapshot: read active status and live balance
        let (current_credit_cents, is_active): (i64, i64) = tx
            .query_row(
                "SELECT current_credit_cents, is_active FROM customers WHERE id = ?1",
                params![cmd.customer_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| EngineError::EntityNotFound(format!("Customer {}", cmd.customer_id)))?;

        if is_active != 1 {
            return Err(EngineError::DatabaseError(format!(
                "Customer {} is inactive",
                cmd.customer_id
            )));
        }

        if cmd.amount_cents > current_credit_cents {
            return Err(EngineError::OverpaymentNotAllowed {
                owed: current_credit_cents,
                attempted: cmd.amount_cents,
            });
        }

        // 2. Authoritative Settlement Safety Check: Atomic Conditional Decrement
        // rows_affected == 1: successful settlement
        // rows_affected == 0: rejected settlement (interleaving concurrent update) -> rollback
        let rows_affected = tx.execute(
            "UPDATE customers
             SET current_credit_cents = current_credit_cents - ?1,
                 updated_at = ?2
             WHERE id = ?3
               AND current_credit_cents >= ?1",
            params![cmd.amount_cents, now, cmd.customer_id],
        )?;

        if rows_affected == 0 {
            return Err(EngineError::OverpaymentNotAllowed {
                owed: current_credit_cents,
                attempted: cmd.amount_cents,
            });
        }

        let balance_before = current_credit_cents;
        let balance_after = current_credit_cents - cmd.amount_cents;

        // 3. Append to customer ledger with authoritative snapshot balances
        let ledger_id = format!("{}_cleg", cmd.payment_id);
        tx.execute(
            "INSERT INTO customer_ledger (id, customer_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
             VALUES (?1, ?2, 'PAYMENT_RECEIVED', ?3, ?4, ?5, 'PAYMENT', ?6, ?7, ?8, ?9)",
            params![
                ledger_id,
                cmd.customer_id,
                cmd.amount_cents,
                balance_before,
                balance_after,
                cmd.payment_id,
                cmd.notes.as_deref().unwrap_or("Customer dues payment"),
                confirmed_by,
                now,
            ],
        )?;

        // 4. Insert payment record
        tx.execute(
            "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
             VALUES (?1, 'CUSTOMER_CREDIT_SETTLEMENT', 'CUSTOMER', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                cmd.payment_id,
                cmd.customer_id,
                cmd.amount_cents,
                cmd.payment_method,
                confirmed_by,
                cmd.notes,
                now,
            ],
        )?;

        // 5. Audit log
        let audit_id = format!("{}_audit", cmd.payment_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'CUSTOMER_PAYMENT', 'payments', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed_by,
                cmd.payment_id,
                format!(
                    "Customer: {}, Amount: {}, Before: {}, After: {}, ConfirmedBy: {}",
                    cmd.customer_id,
                    cmd.amount_cents,
                    balance_before,
                    balance_after,
                    confirmed_by
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 4. EXECUTE CONFIRMED SUPPLIER PAYMENT
    // --------------------------------------------------------------------------------------------
    pub fn execute_supplier_payment(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedSupplierPayment>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Update supplier payable balance
        tx.execute(
            "UPDATE suppliers SET current_outstanding_cents = ?1, updated_at = ?2 WHERE id = ?3",
            params![cmd.balance_after_cents, now, cmd.supplier_id],
        )?;

        // 2. Append to supplier ledger
        let ledger_id = format!("{}_sleg", cmd.payment_id);
        tx.execute(
            "INSERT INTO supplier_ledger (id, supplier_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
             VALUES (?1, ?2, 'PAYMENT_MADE', ?3, ?4, ?5, 'PAYMENT', ?6, ?7, ?8, ?9)",
            params![
                ledger_id,
                cmd.supplier_id,
                cmd.amount_cents,
                cmd.balance_before_cents,
                cmd.balance_after_cents,
                cmd.payment_id,
                cmd.notes.as_deref().unwrap_or("Supplier dues payment"),
                confirmed_by,
                now,
            ],
        )?;

        // 3. Insert payment record
        tx.execute(
            "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
             VALUES (?1, 'SUPPLIER_DUES_SETTLEMENT', 'SUPPLIER', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                cmd.payment_id,
                cmd.supplier_id,
                cmd.amount_cents,
                cmd.payment_method,
                confirmed_by,
                cmd.notes,
                now,
            ],
        )?;

        // 4. Audit log
        let audit_id = format!("{}_audit", cmd.payment_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'SUPPLIER_PAYMENT', 'payments', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.payment_id,
                format!(
                    "Supplier: {}, Amount: {}, Before: {}, After: {}, ConfirmedBy: {}",
                    cmd.supplier_id,
                    cmd.amount_cents,
                    cmd.balance_before_cents,
                    cmd.balance_after_cents,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 5. EXECUTE CONFIRMED CUSTOMER RETURN
    // --------------------------------------------------------------------------------------------
    pub fn execute_customer_return(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedCustomerReturn>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into returns table
        tx.execute(
            "INSERT INTO returns (id, return_number, return_type, reference_id, customer_id, supplier_id, total_amount_cents, reason, admin_user_id, created_at)
             VALUES (?1, ?2, 'CUSTOMER_RETURN', ?3, ?4, NULL, ?5, ?6, ?7, ?8)",
            params![
                cmd.return_id,
                cmd.return_number,
                cmd.reference_sale_id,
                cmd.customer_id,
                cmd.total_amount_cents,
                cmd.reason,
                confirmed_by,
                now,
            ],
        )?;

        // 2. Insert return items and increase stock
        for (idx, item) in cmd.items.iter().enumerate() {
            let item_id = format!("{}_item_{}", cmd.return_id, idx);
            tx.execute(
                "INSERT INTO return_items (id, return_id, product_id, quantity, unit_price_cents, total_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item_id,
                    cmd.return_id,
                    item.product_id,
                    item.quantity,
                    item.unit_price_cents,
                    item.line_total_cents,
                ],
            )?;

            let qty_before: i64 = tx.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![item.product_id],
                |row| row.get(0),
            )?;
            let qty_after = qty_before + item.quantity;

            tx.execute(
                "UPDATE inventory SET current_quantity = ?1, last_updated_at = ?2 WHERE product_id = ?3",
                params![qty_after, now, item.product_id],
            )?;

            let movement_id = format!("{}_mov_{}", cmd.return_id, idx);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'CUSTOMER_RETURN', 'RETURNS', ?6, ?7, ?8)",
                params![
                    movement_id,
                    item.product_id,
                    item.quantity,
                    qty_before,
                    qty_after,
                    cmd.return_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 3. Reduce customer credit debt if applicable
        if cmd.debt_reduction_cents > 0 {
            tx.execute(
                "UPDATE customers SET current_credit_cents = ?1, updated_at = ?2 WHERE id = ?3",
                params![cmd.balance_after_cents, now, cmd.customer_id],
            )?;

            let ledger_id = format!("{}_cleg", cmd.return_id);
            tx.execute(
                "INSERT INTO customer_ledger (id, customer_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
                 VALUES (?1, ?2, 'RETURN_CREDIT', ?3, ?4, ?5, 'RETURN', ?6, 'Return debt adjustment', ?7, ?8)",
                params![
                    ledger_id,
                    cmd.customer_id,
                    cmd.debt_reduction_cents,
                    cmd.balance_before_cents,
                    cmd.balance_after_cents,
                    cmd.return_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 4. Record refund payment if return value exceeded outstanding debt
        if cmd.cash_refund_cents > 0 {
            let payment_id = format!("{}_refpmt", cmd.return_id);
            tx.execute(
                "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
                 VALUES (?1, 'CUSTOMER_RETURN_REFUND', 'RETURN', ?2, ?3, 'CASH', ?4, 'Customer return cash payout', ?5)",
                params![
                    payment_id,
                    cmd.return_id,
                    cmd.cash_refund_cents,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 5. Audit log
        let audit_id = format!("{}_audit", cmd.return_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'CUSTOMER_RETURN', 'returns', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.return_id,
                format!(
                    "TotalReturn: {}, DebtReduced: {}, CashRefund: {}, ConfirmedBy: {}",
                    cmd.total_amount_cents,
                    cmd.debt_reduction_cents,
                    cmd.cash_refund_cents,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 6. EXECUTE CONFIRMED SUPPLIER RETURN
    // --------------------------------------------------------------------------------------------
    pub fn execute_supplier_return(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedSupplierReturn>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into returns table
        tx.execute(
            "INSERT INTO returns (id, return_number, return_type, reference_id, customer_id, supplier_id, total_amount_cents, reason, admin_user_id, created_at)
             VALUES (?1, ?2, 'SUPPLIER_RETURN', ?3, NULL, ?4, ?5, ?6, ?7, ?8)",
            params![
                cmd.return_id,
                cmd.return_number,
                cmd.reference_purchase_id,
                cmd.supplier_id,
                cmd.total_amount_cents,
                cmd.reason,
                confirmed_by,
                now,
            ],
        )?;

        // 2. Insert return items & decrease stock
        for (idx, item) in cmd.items.iter().enumerate() {
            let item_id = format!("{}_item_{}", cmd.return_id, idx);
            tx.execute(
                "INSERT INTO return_items (id, return_id, product_id, quantity, unit_price_cents, total_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item_id,
                    cmd.return_id,
                    item.product_id,
                    item.quantity,
                    item.unit_price_cents,
                    item.line_total_cents,
                ],
            )?;

            let qty_before: i64 = tx.query_row(
                "SELECT current_quantity FROM inventory WHERE product_id = ?1",
                params![item.product_id],
                |row| row.get(0),
            )?;
            let qty_after = qty_before - item.quantity;

            tx.execute(
                "UPDATE inventory SET current_quantity = ?1, last_updated_at = ?2 WHERE product_id = ?3",
                params![qty_after, now, item.product_id],
            )?;

            let movement_id = format!("{}_mov_{}", cmd.return_id, idx);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'SUPPLIER_RETURN', 'RETURNS', ?6, ?7, ?8)",
                params![
                    movement_id,
                    item.product_id,
                    -item.quantity,
                    qty_before,
                    qty_after,
                    cmd.return_id,
                    confirmed_by,
                    now,
                ],
            )?;
        }

        // 3. Reduce supplier payable & append to supplier ledger
        tx.execute(
            "UPDATE suppliers SET current_outstanding_cents = ?1, updated_at = ?2 WHERE id = ?3",
            params![cmd.balance_after_cents, now, cmd.supplier_id],
        )?;

        let ledger_id = format!("{}_sleg", cmd.return_id);
        tx.execute(
            "INSERT INTO supplier_ledger (id, supplier_id, entry_type, amount_cents, balance_before_cents, balance_after_cents, reference_type, reference_id, notes, user_id, created_at)
             VALUES (?1, ?2, 'RETURN_DEBIT', ?3, ?4, ?5, 'RETURN', ?6, 'Supplier return debit', ?7, ?8)",
            params![
                ledger_id,
                cmd.supplier_id,
                cmd.total_amount_cents,
                cmd.balance_before_cents,
                cmd.balance_after_cents,
                cmd.return_id,
                confirmed_by,
                now,
            ],
        )?;

        // 4. Audit log
        let audit_id = format!("{}_audit", cmd.return_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'SUPPLIER_RETURN', 'returns', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.return_id,
                format!(
                    "SupplierReturn: {}, PayableReduced: {}, ConfirmedBy: {}",
                    cmd.total_amount_cents,
                    cmd.total_amount_cents,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 7. EXECUTE CONFIRMED STOCK CORRECTION
    // --------------------------------------------------------------------------------------------
    pub fn execute_stock_correction(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedStockCorrection>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into stock_corrections
        tx.execute(
            "INSERT INTO stock_corrections (id, product_id, quantity_change, reason, note, admin_user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                cmd.correction_id,
                cmd.product_id,
                cmd.quantity_change,
                cmd.reason,
                cmd.note,
                confirmed_by,
                now,
            ],
        )?;

        // 2. Update inventory
        tx.execute(
            "UPDATE inventory SET current_quantity = ?1, last_updated_at = ?2 WHERE product_id = ?3",
            params![cmd.quantity_after, now, cmd.product_id],
        )?;

        // 3. Insert stock movement
        let movement_id = format!("{}_mov", cmd.correction_id);
        tx.execute(
            "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'CORRECTION', 'STOCK_CORRECTIONS', ?6, ?7, ?8)",
            params![
                movement_id,
                cmd.product_id,
                cmd.quantity_change,
                cmd.quantity_before,
                cmd.quantity_after,
                cmd.correction_id,
                confirmed_by,
                now,
            ],
        )?;

        // 4. Audit log
        let audit_id = format!("{}_audit", cmd.correction_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'STOCK_CORRECTION', 'stock_corrections', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.correction_id,
                format!(
                    "Reason: {}, Delta: {}, Before: {}, After: {}, ConfirmedBy: {}",
                    cmd.reason,
                    cmd.quantity_change,
                    cmd.quantity_before,
                    cmd.quantity_after,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 8. EXECUTE CONFIRMED ORDER CONVERSION (ATOMIC DRAFT -> SALE -> CONVERTED)
    // --------------------------------------------------------------------------------------------
    pub fn execute_order_conversion(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedOrderConversion>,
    ) -> Result<(), EngineError> {
        let confirmed_by = confirmed.confirmed_by_user_id().to_string();
        let confirmed_at = confirmed.confirmed_at().to_string();
        let cmd = confirmed.payload;
        let sale_id = cmd.prepared_sale.sale_id.clone();
        let order_id = cmd.order_id.clone();

        // Wrap the sale execution in Confirmed wrapper
        let confirmed_sale = Confirmed::new(
            cmd.prepared_sale,
            confirmed_by.clone(),
            confirmed_at,
        );

        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Concurrency-Safe Atomic Conditional Status Transition:
        // Ensures exactly one concurrent worker can transition the order from DRAFT to CONVERTED.
        // If already converted, rows_affected == 0 triggers an immediate rollback with zero side effects.
        let rows_affected = tx.execute(
            "UPDATE customer_orders SET status = 'CONVERTED', converted_sale_id = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'DRAFT'",
            params![sale_id, now, order_id],
        )?;

        if rows_affected == 0 {
            return Err(EngineError::OrderAlreadyConverted(format!(
                "Order {} was already converted or is no longer in DRAFT status",
                order_id
            )));
        }

        // 2. Atomically execute all sale writes within this transaction
        Self::execute_sale_in_tx(&tx, &confirmed_sale)?;

        // 3. Audit log
        let audit_id = format!("{}_ord_conv_audit", order_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'ORDER_CONVERTED', 'customer_orders', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed_by,
                order_id,
                format!("Order {} converted to confirmed sale {}", order_id, sale_id),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 9. EXECUTE CONFIRMED EXPENSE
    // --------------------------------------------------------------------------------------------
    pub fn execute_expense(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedExpense>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into expenses
        tx.execute(
            "INSERT INTO expenses (id, expense_name, amount_cents, category, expense_date, notes, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                cmd.expense_id,
                cmd.expense_name,
                cmd.amount_cents,
                cmd.category,
                cmd.expense_date,
                cmd.notes,
                confirmed_by,
                now,
            ],
        )?;

        // 2. Audit log
        let audit_id = format!("{}_audit", cmd.expense_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'EXPENSE_RECORDED', 'expenses', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.expense_id,
                format!(
                    "Title: {}, Amount: {}, Category: {}, ConfirmedBy: {}",
                    cmd.expense_name,
                    cmd.amount_cents,
                    cmd.category,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 10. EXECUTE CONFIRMED PAYMENT
    // --------------------------------------------------------------------------------------------
    pub fn execute_payment(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedPayment>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        tx.execute(
            "INSERT INTO payments (id, payment_type, related_entity_type, related_entity_id, amount_cents, payment_method, user_id, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                cmd.payment_id,
                cmd.payment_type,
                cmd.related_entity_type,
                cmd.related_entity_id,
                cmd.amount_cents,
                cmd.payment_method,
                confirmed_by,
                cmd.notes,
                now,
            ],
        )?;

        let audit_id = format!("{}_audit", cmd.payment_id);
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PAYMENT_RECORDED', 'payments', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed.confirmed_by_user_id(),
                cmd.payment_id,
                format!(
                    "Type: {}, Entity: {}:{}, Amount: {}, ConfirmedBy: {}",
                    cmd.payment_type,
                    cmd.related_entity_type,
                    cmd.related_entity_id,
                    cmd.amount_cents,
                    confirmed.confirmed_by_user_id()
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 11. EXECUTE CONFIRMED CREATE PRODUCT (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn execute_create_product(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedCreateProduct>,
    ) -> Result<ProductResult, EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        // 1. Insert into products
        tx.execute(
            "INSERT INTO products (id, business_id, category_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
            params![
                cmd.product_id,
                cmd.business_id,
                cmd.name,
                cmd.product_type.as_str(),
                cmd.unit,
                cmd.cost_price_cents,
                cmd.selling_price_cents,
                cmd.min_stock_level,
                now,
            ],
        )?;

        // 2. Insert authoritative inventory row
        let inv_id = format!("inv_{}", cmd.product_id);
        tx.execute(
            "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![inv_id, cmd.product_id, cmd.initial_stock, now],
        )?;

        // 3. If initial stock > 0, record INITIAL_STOCK movement
        if cmd.initial_stock > 0 {
            let mov_id = format!("mov_init_{}", cmd.product_id);
            tx.execute(
                "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                 VALUES (?1, ?2, ?3, 0, ?3, 'INITIAL_STOCK', 'PRODUCTS', ?2, ?4, ?5)",
                params![mov_id, cmd.product_id, cmd.initial_stock, confirmed_by, now],
            )?;
        }

        // 4. If barcode mapping provided, record barcode_mappings
        if let Some(ref b) = cmd.barcode {
            let bm_id = format!("bm_{}", cmd.product_id);
            tx.execute(
                "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, notes, created_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
                params![bm_id, b, cmd.product_id, cmd.barcode_type.as_str(), now],
            )?;
        }

        // 5. Audit log
        let audit_id = format!("aud_prod_{}_{}", cmd.product_id, next_audit_token());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PRODUCT_CREATED', 'products', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed_by,
                cmd.product_id,
                format!(
                    "Product: {}, Type: {}, Unit: {}, Cost: {}, Sell: {}, InitialStock: {}, MinStock: {}, ConfirmedBy: {}",
                    cmd.name,
                    cmd.product_type.as_str(),
                    cmd.unit,
                    cmd.cost_price_cents,
                    cmd.selling_price_cents,
                    cmd.initial_stock,
                    cmd.min_stock_level,
                    confirmed_by
                ),
                now,
            ],
        )?;

        tx.commit()?;

        Ok(ProductResult {
            id: cmd.product_id.clone(),
            business_id: cmd.business_id.clone(),
            name: cmd.name.clone(),
            product_type: cmd.product_type.as_str().to_string(),
            unit: cmd.unit.clone(),
            cost_price_cents: cmd.cost_price_cents,
            selling_price_cents: cmd.selling_price_cents,
            min_stock_level: cmd.min_stock_level,
            is_active: 1,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    // --------------------------------------------------------------------------------------------
    // 12. EXECUTE CONFIRMED CREATE PRODUCT BATCH (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn execute_create_product_batch(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedCreateProductBatch>,
    ) -> Result<Vec<ProductResult>, EngineError> {
        let confirmed_by = confirmed.confirmed_by_user_id().to_string();
        let cmd = confirmed.payload;
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        let mut results = Vec::new();

        for item in &cmd.products {
            // 1. Insert product
            tx.execute(
                "INSERT INTO products (id, business_id, category_id, name, product_type, unit, cost_price_cents, selling_price_cents, min_stock_level, is_active, created_at, updated_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
                params![
                    item.product_id,
                    item.business_id,
                    item.name,
                    item.product_type.as_str(),
                    item.unit,
                    item.cost_price_cents,
                    item.selling_price_cents,
                    item.min_stock_level,
                    now,
                ],
            )?;

            // 2. Insert inventory
            let inv_id = format!("inv_{}", item.product_id);
            tx.execute(
                "INSERT INTO inventory (id, product_id, current_quantity, last_updated_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![inv_id, item.product_id, item.initial_stock, now],
            )?;

            // 3. If initial stock > 0, insert stock movement
            if item.initial_stock > 0 {
                let mov_id = format!("mov_init_{}", item.product_id);
                tx.execute(
                    "INSERT INTO stock_movements (id, product_id, quantity_change, quantity_before, quantity_after, movement_type, reference_type, reference_id, user_id, created_at)
                     VALUES (?1, ?2, ?3, 0, ?3, 'INITIAL_STOCK', 'PRODUCTS', ?2, ?4, ?5)",
                    params![mov_id, item.product_id, item.initial_stock, confirmed_by, now],
                )?;
            }

            // 4. If barcode mapping provided, insert barcode_mappings
            if let Some(ref b) = item.barcode {
                let bm_id = format!("bm_{}", item.product_id);
                tx.execute(
                    "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, notes, created_at)
                     VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
                    params![bm_id, b, item.product_id, item.barcode_type.as_str(), now],
                )?;
            }

            // 5. Individual audit log
            let audit_id = format!("aud_prod_{}_{}", item.product_id, next_audit_token());
            tx.execute(
                "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
                 VALUES (?1, ?2, 'PRODUCT_CREATED', 'products', ?3, ?4, ?5)",
                params![
                    audit_id,
                    confirmed_by,
                    item.product_id,
                    format!("Product: {}, BatchMember: true", item.name),
                    now,
                ],
            )?;

            results.push(ProductResult {
                id: item.product_id.clone(),
                business_id: item.business_id.clone(),
                name: item.name.clone(),
                product_type: item.product_type.as_str().to_string(),
                unit: item.unit.clone(),
                cost_price_cents: item.cost_price_cents,
                selling_price_cents: item.selling_price_cents,
                min_stock_level: item.min_stock_level,
                is_active: 1,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }

        // Batch summary audit log
        let batch_audit_id = format!("aud_batch_{}", next_audit_token());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PRODUCT_BATCH_CREATED', 'products', ?3, ?4, ?5)",
            params![
                batch_audit_id,
                confirmed_by,
                cmd.business_id,
                format!("Batch created {} products in business {}", results.len(), cmd.business_id),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(results)
    }

    // --------------------------------------------------------------------------------------------
    // 13. EXECUTE CONFIRMED UPDATE PRODUCT (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn execute_update_product(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedUpdateProduct>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        if let Some(ref name) = cmd.name {
            tx.execute(
                "UPDATE products SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, cmd.product_id],
            )?;
        }

        if let Some(ref unit) = cmd.unit {
            tx.execute(
                "UPDATE products SET unit = ?1, updated_at = ?2 WHERE id = ?3",
                params![unit, now, cmd.product_id],
            )?;
        }

        if let Some(c) = cmd.cost_price_cents {
            tx.execute(
                "UPDATE products SET cost_price_cents = ?1, updated_at = ?2 WHERE id = ?3",
                params![c, now, cmd.product_id],
            )?;
        }

        if let Some(s) = cmd.selling_price_cents {
            tx.execute(
                "UPDATE products SET selling_price_cents = ?1, updated_at = ?2 WHERE id = ?3",
                params![s, now, cmd.product_id],
            )?;
        }

        if let Some(m) = cmd.min_stock_level {
            tx.execute(
                "UPDATE products SET min_stock_level = ?1, updated_at = ?2 WHERE id = ?3",
                params![m, now, cmd.product_id],
            )?;
        }

        // Handle barcode mapping updates
        if let Some(ref barcode_opt) = cmd.barcode {
            match barcode_opt {
                Some(ref new_barcode) => {
                    let b_type = cmd
                        .barcode_type
                        .map(|bt| bt.as_str())
                        .unwrap_or("MANUFACTURER");

                    // Check if a mapping for this product already exists
                    let existing_bm_id: Option<String> = tx
                        .query_row(
                            "SELECT id FROM barcode_mappings WHERE product_id = ?1",
                            params![cmd.product_id],
                            |r| r.get(0),
                        )
                        .ok();

                    if let Some(bm_id) = existing_bm_id {
                        tx.execute(
                            "UPDATE barcode_mappings SET barcode = ?1, barcode_type = ?2 WHERE id = ?3",
                            params![new_barcode, b_type, bm_id],
                        )?;
                    } else {
                        let new_bm_id = format!("bm_{}", cmd.product_id);
                        tx.execute(
                            "INSERT INTO barcode_mappings (id, barcode, product_id, barcode_type, notes, created_at)
                             VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
                            params![new_bm_id, new_barcode, cmd.product_id, b_type, now],
                        )?;
                    }
                }
                None => {
                    // Remove barcode mappings for this product
                    tx.execute(
                        "DELETE FROM barcode_mappings WHERE product_id = ?1",
                        params![cmd.product_id],
                    )?;
                }
            }
        }

        // Audit log
        let audit_id = format!("aud_prod_upd_{}_{}", cmd.product_id, next_audit_token());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'PRODUCT_UPDATED', 'products', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed_by,
                cmd.product_id,
                format!("Product {} updated by {}", cmd.product_id, confirmed_by),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------
    // 14. EXECUTE CONFIRMED REMAP BARCODE (BUILD 05)
    // --------------------------------------------------------------------------------------------
    pub fn execute_remap_barcode(
        conn: &mut Connection,
        confirmed: Confirmed<PreparedRemapBarcode>,
    ) -> Result<(), EngineError> {
        let cmd = &confirmed.payload;
        let confirmed_by = confirmed.confirmed_by_user_id();
        let tx = conn.transaction()?;
        let now = format!("{:?}", std::time::SystemTime::now());

        tx.execute(
            "UPDATE barcode_mappings SET product_id = ?1 WHERE barcode = ?2",
            params![cmd.new_product_id, cmd.barcode],
        )?;

        let audit_id = format!("aud_bmap_{}_{}", cmd.barcode, next_audit_token());
        tx.execute(
            "INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
             VALUES (?1, ?2, 'BARCODE_REMAPPED', 'barcode_mappings', ?3, ?4, ?5)",
            params![
                audit_id,
                confirmed_by,
                cmd.barcode,
                format!(
                    "Barcode '{}' remapped from product '{}' to '{}' by {}",
                    cmd.barcode, cmd.old_product_id, cmd.new_product_id, confirmed_by
                ),
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }
}

fn next_audit_token() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:x}_{:x}", nanos, count)
}

