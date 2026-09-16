use crate::ai::error::AIError;
use crate::ai::intents::*;
use rusqlite::Connection;

pub struct DeterministicIntentValidator;

impl DeterministicIntentValidator {
    /// Validates and enriches a structured intent against the live catalog using read-only parameterized queries.
    pub fn validate_and_enrich(
        conn: &Connection,
        intent: StructuredIntent,
    ) -> Result<StructuredIntent, AIError> {
        match intent {
            StructuredIntent::CreateSale {
                mut items,
                customer_reference,
                customer_id,
                payment_method,
                settlement_mode,
            } => {
                if items.is_empty() {
                    return Err(AIError::ValidationFailure("Sale must contain at least one item".to_string()));
                }

                for item in &mut items {
                    if item.quantity_millie <= 0 || item.quantity_millie > 1_000_000_000 {
                        return Err(AIError::ValidationFailure(format!(
                            "Invalid quantity for item '{}': quantity must be greater than zero and within bounds",
                            item.product_reference
                        )));
                    }

                    // Resolve product reference using parameterized query
                    let (prod_id, prod_name, price_cents) = Self::resolve_single_product(conn, &item.product_reference)?;
                    item.product_id = Some(prod_id);
                    item.product_name = Some(prod_name);
                    item.unit_price_cents = Some(price_cents);
                }

                // Resolve customer if provided
                let resolved_cust_id = if let Some(ref cust_ref) = customer_reference {
                    if !cust_ref.trim().is_empty() {
                        Some(Self::resolve_single_customer(conn, cust_ref)?)
                    } else {
                        None
                    }
                } else {
                    customer_id
                };

                // Validate settlement rules
                if settlement_mode == "CREDIT" && resolved_cust_id.is_none() {
                    return Err(AIError::ValidationFailure(
                        "Credit sale requires a registered customer identity".to_string(),
                    ));
                }

                Ok(StructuredIntent::CreateSale {
                    items,
                    customer_reference,
                    customer_id: resolved_cust_id,
                    payment_method,
                    settlement_mode,
                })
            }

            StructuredIntent::CreatePurchase {
                mut items,
                supplier_reference,
                supplier_id: _,
                payment_method,
            } => {
                if items.is_empty() {
                    return Err(AIError::ValidationFailure("Purchase must contain at least one item".to_string()));
                }

                for item in &mut items {
                    if item.quantity_millie <= 0 || item.quantity_millie > 1_000_000_000 {
                        return Err(AIError::ValidationFailure(format!(
                            "Invalid quantity for item '{}': must be greater than zero",
                            item.product_reference
                        )));
                    }

                    let (prod_id, prod_name, cost_cents) = Self::resolve_single_product_cost(conn, &item.product_reference)?;
                    item.product_id = Some(prod_id);
                    item.product_name = Some(prod_name);
                    item.unit_cost_cents = Some(cost_cents);
                }

                let resolved_supp_id = Self::resolve_single_supplier(conn, &supplier_reference)?;

                Ok(StructuredIntent::CreatePurchase {
                    items,
                    supplier_reference,
                    supplier_id: Some(resolved_supp_id),
                    payment_method,
                })
            }

            StructuredIntent::CreateCustomerOrder {
                mut items,
                customer_reference,
                customer_id,
                notes,
            } => {
                if items.is_empty() {
                    return Err(AIError::ValidationFailure("Customer order must contain at least one item".to_string()));
                }

                for item in &mut items {
                    if item.quantity_millie <= 0 || item.quantity_millie > 1_000_000_000 {
                        return Err(AIError::ValidationFailure("Order item quantity must be greater than zero".to_string()));
                    }

                    let (prod_id, prod_name, _) = Self::resolve_single_product(conn, &item.product_reference)?;
                    item.product_id = Some(prod_id);
                    item.product_name = Some(prod_name);
                }

                let resolved_cust_id = if let Some(ref cust_ref) = customer_reference {
                    if !cust_ref.trim().is_empty() {
                        Some(Self::resolve_single_customer(conn, cust_ref)?)
                    } else {
                        None
                    }
                } else {
                    customer_id
                };

                Ok(StructuredIntent::CreateCustomerOrder {
                    items,
                    customer_reference,
                    customer_id: resolved_cust_id,
                    notes,
                })
            }

            StructuredIntent::CheckStock {
                product_reference,
                product_id: _,
            } => {
                let (prod_id, _name, _) = Self::resolve_single_product(conn, &product_reference)?;
                Ok(StructuredIntent::CheckStock {
                    product_reference,
                    product_id: Some(prod_id),
                })
            }

            StructuredIntent::CheckCustomerCredit {
                customer_reference,
                customer_id: _,
            } => {
                if customer_reference == "ALL" {
                    return Ok(StructuredIntent::CheckCustomerCredit {
                        customer_reference,
                        customer_id: None,
                    });
                }
                let cust_id = Self::resolve_single_customer(conn, &customer_reference)?;
                Ok(StructuredIntent::CheckCustomerCredit {
                    customer_reference,
                    customer_id: Some(cust_id),
                })
            }

            StructuredIntent::RecordCustomerPayment {
                customer_reference,
                customer_id: _,
                amount_cents,
                payment_method,
            } => {
                if amount_cents <= 0 {
                    return Err(AIError::ValidationFailure("Payment amount must be greater than zero".to_string()));
                }
                let cust_id = Self::resolve_single_customer(conn, &customer_reference)?;
                Ok(StructuredIntent::RecordCustomerPayment {
                    customer_reference,
                    customer_id: Some(cust_id),
                    amount_cents,
                    payment_method,
                })
            }

            StructuredIntent::CheckSupplierBalance {
                supplier_reference,
                supplier_id: _,
            } => {
                let supp_id = Self::resolve_single_supplier(conn, &supplier_reference)?;
                Ok(StructuredIntent::CheckSupplierBalance {
                    supplier_reference,
                    supplier_id: Some(supp_id),
                })
            }

            other => Ok(other),
        }
    }

    /// Parameterized lookup resolving a product reference to (id, name, selling_price_cents).
    /// Detects ambiguity if multiple products match.
    pub fn resolve_single_product(conn: &Connection, reference: &str) -> Result<(String, String, i64), AIError> {
        let pattern = format!("%{}%", reference.trim());
        let mut stmt = conn.prepare(
            "SELECT id, name, selling_price_cents FROM products WHERE (id = ?1 OR name LIKE ?2) AND is_active = 1 LIMIT 10",
        )?;
        let rows: Vec<(String, String, i64)> = stmt
            .query_map(rusqlite::params![reference.trim(), pattern], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Err(AIError::ValidationFailure(format!(
                "No active product found matching reference: '{}'",
                reference
            )));
        }

        if rows.len() > 1 {
            // Check for exact case-insensitive name or ID match
            let exact = rows.iter().find(|(_, name, _)| name.eq_ignore_ascii_case(reference.trim()) || reference.trim() == name);
            if let Some((id, name, price)) = exact {
                return Ok((id.clone(), name.clone(), *price));
            }
            let candidates: Vec<String> = rows.into_iter().map(|(_, n, _)| n).collect();
            return Err(AIError::AmbiguousIntent(candidates));
        }

        let first = rows.into_iter().next().unwrap();
        Ok(first)
    }

    /// Parameterized lookup resolving product reference to (id, name, cost_price_cents).
    pub fn resolve_single_product_cost(conn: &Connection, reference: &str) -> Result<(String, String, i64), AIError> {
        let pattern = format!("%{}%", reference.trim());
        let mut stmt = conn.prepare(
            "SELECT id, name, cost_price_cents FROM products WHERE (id = ?1 OR name LIKE ?2) AND is_active = 1 LIMIT 10",
        )?;
        let rows: Vec<(String, String, i64)> = stmt
            .query_map(rusqlite::params![reference.trim(), pattern], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Err(AIError::ValidationFailure(format!(
                "No active product found matching reference: '{}'",
                reference
            )));
        }

        if rows.len() > 1 {
            let exact = rows.iter().find(|(_, name, _)| name.eq_ignore_ascii_case(reference.trim()));
            if let Some((id, name, cost)) = exact {
                return Ok((id.clone(), name.clone(), *cost));
            }
            let candidates: Vec<String> = rows.into_iter().map(|(_, n, _)| n).collect();
            return Err(AIError::AmbiguousIntent(candidates));
        }

        let first = rows.into_iter().next().unwrap();
        Ok(first)
    }

    /// Parameterized lookup resolving a customer reference to customer id.
    pub fn resolve_single_customer(conn: &Connection, reference: &str) -> Result<String, AIError> {
        let pattern = format!("%{}%", reference.trim());
        let mut stmt = conn.prepare(
            "SELECT id, name FROM customers WHERE (id = ?1 OR name LIKE ?2 OR phone = ?1) AND is_active = 1 LIMIT 10",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map(rusqlite::params![reference.trim(), pattern], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Err(AIError::ValidationFailure(format!(
                "No active customer found matching: '{}'",
                reference
            )));
        }

        if rows.len() > 1 {
            let exact = rows.iter().find(|(_, name)| name.eq_ignore_ascii_case(reference.trim()));
            if let Some((id, _)) = exact {
                return Ok(id.clone());
            }
            let candidates: Vec<String> = rows.into_iter().map(|(_, n)| n).collect();
            return Err(AIError::AmbiguousIntent(candidates));
        }

        Ok(rows[0].0.clone())
    }

    /// Parameterized lookup resolving a supplier reference to supplier id.
    pub fn resolve_single_supplier(conn: &Connection, reference: &str) -> Result<String, AIError> {
        let pattern = format!("%{}%", reference.trim());
        let mut stmt = conn.prepare(
            "SELECT id, name FROM suppliers WHERE (id = ?1 OR name LIKE ?2 OR phone = ?1) AND is_active = 1 LIMIT 10",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map(rusqlite::params![reference.trim(), pattern], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Err(AIError::ValidationFailure(format!(
                "No active supplier found matching: '{}'",
                reference
            )));
        }

        if rows.len() > 1 {
            let exact = rows.iter().find(|(_, name)| name.eq_ignore_ascii_case(reference.trim()));
            if let Some((id, _)) = exact {
                return Ok(id.clone());
            }
            let candidates: Vec<String> = rows.into_iter().map(|(_, n)| n).collect();
            return Err(AIError::AmbiguousIntent(candidates));
        }

        Ok(rows[0].0.clone())
    }
}
