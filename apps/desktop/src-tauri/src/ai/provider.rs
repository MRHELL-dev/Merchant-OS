use crate::ai::error::AIError;
use crate::ai::intents::*;
use crate::ai::translation::MultilingualNormalizer;

/// Abstract interface for AI providers.
/// Enables pluggable local or remote AI models without altering business logic.
pub trait AIProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    fn is_offline_capable(&self) -> bool;
    fn interpret(&self, query: &str) -> Result<StructuredIntent, AIError>;
}

/// Built-in deterministic rule- and lexicon-based local interpreter.
/// Operates 100% sovereignly offline with zero network dependencies.
pub struct DeterministicLocalInterpreter;

impl Default for DeterministicLocalInterpreter {
    fn default() -> Self {
        Self
    }
}

impl DeterministicLocalInterpreter {
    pub fn new() -> Self {
        Self
    }

    /// Helper to extract leading or embedded numeric quantities and units.
    fn extract_quantity(tokens: &[&str]) -> Option<(String, i64, usize)> {
        for (idx, token) in tokens.iter().enumerate() {
            // Check if numeric (integer or decimal)
            if let Ok(num) = token.parse::<f64>() {
                if num > 0.0 && num <= 1_000_000.0 {
                    let millie = (num * 1000.0).round() as i64;
                    return Some((token.to_string(), millie, idx));
                }
            }
        }
        None
    }

    /// Helper to detect payment method
    fn detect_payment_method(text: &str) -> (&'static str, &'static str) {
        let lower = text.to_lowercase();
        if lower.contains("upi") || lower.contains("gpay") || lower.contains("phonepe") || lower.contains("paytm") {
            ("UPI", "PAID")
        } else if lower.contains("card") || lower.contains("debit") || lower.contains("credit card") {
            ("CARD", "PAID")
        } else if lower.contains("credit") || lower.contains("udhari") || lower.contains("baki") || lower.contains("on credit") {
            ("CREDIT", "CREDIT")
        } else {
            // Default to Cash Paid
            ("CASH", "PAID")
        }
    }
}

impl AIProvider for DeterministicLocalInterpreter {
    fn name(&self) -> &'static str {
        "Deterministic Local Interpreter (V1 Offline)"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn is_offline_capable(&self) -> bool {
        true
    }

    fn interpret(&self, query: &str) -> Result<StructuredIntent, AIError> {
        let normalized = MultilingualNormalizer::normalize(query);
        let tokens: Vec<&str> = normalized.split_whitespace().collect();

        if tokens.is_empty() {
            return Err(AIError::ValidationFailure("Empty query".to_string()));
        }

        let lower = normalized.to_lowercase();

        // 1. Check Low Stock / Demand
        if lower.contains("low stock") || lower.contains("running low") || lower.contains("khatam hone") || lower.contains("out of stock") {
            return Ok(StructuredIntent::CheckLowStock);
        }

        if lower.contains("demand") || lower.contains("sales trend") || lower.contains("most sold") {
            return Ok(StructuredIntent::CheckDemand { category_reference: None });
        }

        if lower.contains("reorder") || lower.contains("order suggestion") {
            return Ok(StructuredIntent::PrepareReorder {
                product_reference: None,
                product_id: None,
            });
        }

        // 2. Sales queries ("how much did i sell", "sales today")
        if (lower.contains("sales") || lower.contains("sold")) && (lower.contains("today") || lower.contains("total") || lower.contains("report")) {
            let period = if lower.contains("week") {
                Some("THIS_WEEK".to_string())
            } else if lower.contains("month") {
                Some("THIS_MONTH".to_string())
            } else {
                Some("TODAY".to_string())
            };
            return Ok(StructuredIntent::CheckSales { period });
        }

        // 3. Customer Credit Queries ("who owes me", "credit of", "udhari")
        if (lower.contains("who owes") || lower.contains("total credit") || lower.contains("pending credit")) && !lower.contains("sold") && !lower.contains("sell") {
            return Ok(StructuredIntent::CheckCustomerCredit {
                customer_reference: "ALL".to_string(),
                customer_id: None,
            });
        }

        if (lower.contains("credit of") || lower.contains("udhari of")) && !lower.contains("sell") {
            let parts: Vec<&str> = lower.split("of").collect();
            if parts.len() >= 2 {
                let cust_ref = parts[1].trim().to_string();
                if !cust_ref.is_empty() {
                    return Ok(StructuredIntent::CheckCustomerCredit {
                        customer_reference: cust_ref,
                        customer_id: None,
                    });
                }
            }
        }

        // 4. Check Stock Queries ("how much rice", "check stock of", "stock kitna")
        if lower.contains("check stock") || lower.contains("how much") || lower.contains("stock of") || lower.starts_with("stock ") {
            // Extract product name
            let mut prod_ref = lower
                .replace("check stock of", "")
                .replace("check stock", "")
                .replace("how much", "")
                .replace("stock of", "")
                .replace("stock", "")
                .replace("do i have", "")
                .replace("is left", "")
                .replace("available", "");

            // Trim leading "of " if remaining
            prod_ref = prod_ref.trim().to_string();
            if prod_ref.starts_with("of ") {
                prod_ref = prod_ref[3..].trim().to_string();
            }

            // Strip trailing punctuation like ?, ., !, etc.
            let prod_ref = prod_ref
                .trim_matches(|c: char| c.is_ascii_punctuation())
                .trim()
                .to_string();

            if prod_ref.is_empty() {
                return Err(AIError::AmbiguousIntent(vec![
                    "Please specify which product stock to inspect (e.g. 'How much Basmati Rice is left?')".to_string(),
                ]));
            }

            return Ok(StructuredIntent::CheckStock {
                product_reference: prod_ref,
                product_id: None,
            });
        }

        // 5. Customer Orders ("order for", "customer order", "book order", "draft order")
        if (lower.contains("customer order") || lower.contains("book order") || lower.contains("draft order") || lower.starts_with("order ")) && !lower.contains("reorder") {
            if let Some((qty_disp, qty_millie, qty_idx)) = Self::extract_quantity(&tokens) {
                // Collect product tokens excluding the quantity and action words
                let mut prod_tokens = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    if i == qty_idx {
                        continue;
                    }
                    if *t == "order" || *t == "customer" || *t == "for" || *t == "packets" || *t == "packet" || *t == "kg" || *t == "g" || *t == "pcs" || *t == "of" {
                        continue;
                    }
                    prod_tokens.push(*t);
                }
                let prod_ref = prod_tokens.join(" ");
                if prod_ref.is_empty() {
                    return Err(AIError::AmbiguousIntent(vec![
                        "Order missing product reference. Example: 'Customer order 5 packets Basmati Rice'".to_string(),
                    ]));
                }
                return Ok(StructuredIntent::CreateCustomerOrder {
                    items: vec![OrderIntentItem {
                        product_reference: prod_ref,
                        product_id: None,
                        product_name: None,
                        quantity_display: qty_disp,
                        quantity_millie: qty_millie,
                    }],
                    customer_reference: None,
                    customer_id: None,
                    notes: Some("Created via AI assistant".to_string()),
                });
            }
        }

        // 6. Create Sale ("sell", "sold", "becha")
        if lower.starts_with("sell") || lower.starts_with("sold") || lower.contains(" sell ") || lower.contains(" sold ") {
            if let Some((qty_disp, qty_millie, qty_idx)) = Self::extract_quantity(&tokens) {
                let (payment_method, settlement_mode) = Self::detect_payment_method(&lower);

                // Check for customer: e.g. "to Ramesh Kumar"
                let (customer_ref, cust_tokens_to_ignore) = if let Some(to_pos) = tokens.iter().position(|t| *t == "to") {
                    let mut cust_name_parts = Vec::new();
                    for t in &tokens[to_pos + 1..] {
                        if *t == "on" || *t == "credit" || *t == "for" || *t == "cash" || *t == "via" || *t == "upi" || *t == "card" {
                            break;
                        }
                        cust_name_parts.push(*t);
                    }
                    if !cust_name_parts.is_empty() {
                        (Some(cust_name_parts.join(" ")), true)
                    } else {
                        (None, false)
                    }
                } else {
                    (None, false)
                };

                // Collect product tokens
                let mut prod_tokens = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    if i == qty_idx {
                        continue;
                    }
                    if *t == "sell" || *t == "sold" || *t == "for" || *t == "cash" || *t == "upi" || *t == "card" || *t == "credit"
                        || *t == "packets" || *t == "packet" || *t == "kg" || *t == "g" || *t == "pcs" || *t == "to"
                        || *t == "of" || *t == "on" || *t == "me" || *t == "pe" {
                        continue;
                    }
                    if cust_tokens_to_ignore {
                        if let Some(ref c_ref) = customer_ref {
                            if c_ref.split_whitespace().any(|cp| cp == *t) {
                                continue;
                            }
                        }
                    }
                    prod_tokens.push(*t);
                }
                let prod_ref = prod_tokens.join(" ");

                if prod_ref.is_empty() {
                    return Err(AIError::AmbiguousIntent(vec![
                        "Sale requires a product reference (e.g. 'Sold 2 packets Basmati Rice for cash')".to_string(),
                    ]));
                }

                return Ok(StructuredIntent::CreateSale {
                    items: vec![SaleIntentItem {
                        product_reference: prod_ref,
                        product_id: None,
                        product_name: None,
                        quantity_display: qty_disp,
                        quantity_millie: qty_millie,
                        unit_price_cents: None,
                    }],
                    customer_reference: customer_ref,
                    customer_id: None,
                    payment_method: payment_method.to_string(),
                    settlement_mode: settlement_mode.to_string(),
                });
            } else {
                return Err(AIError::AmbiguousIntent(vec![
                    "Missing quantity in sale request. Please specify quantity (e.g. 'Sold 5 packets of Rice for cash')".to_string(),
                ]));
            }
        }

        // 7. Create Purchase ("purchase", "bought", "buy")
        if lower.starts_with("purchase") || lower.starts_with("bought") || lower.starts_with("buy") || lower.contains(" purchase ") || lower.contains(" bought ") {
            if let Some((qty_disp, qty_millie, qty_idx)) = Self::extract_quantity(&tokens) {
                // Collect product tokens
                let mut prod_tokens = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    if i == qty_idx {
                        continue;
                    }
                    if *t == "purchase" || *t == "bought" || *t == "buy" || *t == "from" || *t == "packets" || *t == "packet"
                        || *t == "kg" || *t == "g" || *t == "pcs" || *t == "for" || *t == "cash" || *t == "credit" || *t == "of" {
                        continue;
                    }
                    prod_tokens.push(*t);
                }
                let prod_ref = prod_tokens.join(" ");
                if prod_ref.is_empty() {
                    return Err(AIError::AmbiguousIntent(vec![
                        "Purchase requires a product reference (e.g. 'Bought 10 bags of Rice from Apex Wholesale')".to_string(),
                    ]));
                }

                return Ok(StructuredIntent::CreatePurchase {
                    items: vec![PurchaseIntentItem {
                        product_reference: prod_ref,
                        product_id: None,
                        product_name: None,
                        quantity_display: qty_disp,
                        quantity_millie: qty_millie,
                        unit_cost_cents: None,
                    }],
                    supplier_reference: "Apex Wholesale".to_string(),
                    supplier_id: None,
                    payment_method: "CASH".to_string(),
                });
            }
        }

        // 8. General Query / Fallback
        Ok(StructuredIntent::GeneralQuery {
            query: query.trim().to_string(),
        })
    }
}

/// Mock provider for testing failure and edge-case behavior.
pub struct MockProvider {
    pub mode: MockMode,
}

#[derive(Clone)]
pub enum MockMode {
    AlwaysTimeout,
    AlwaysError(String),
    MalformedOutput,
    OfflineUnavailable,
}

impl AIProvider for MockProvider {
    fn name(&self) -> &'static str {
        "Mock Remote Provider (Test Only)"
    }

    fn is_available(&self) -> bool {
        !matches!(self.mode, MockMode::OfflineUnavailable)
    }

    fn is_offline_capable(&self) -> bool {
        false
    }

    fn interpret(&self, _query: &str) -> Result<StructuredIntent, AIError> {
        match &self.mode {
            MockMode::AlwaysTimeout => Err(AIError::TimeoutError),
            MockMode::AlwaysError(msg) => Err(AIError::ProviderError(msg.clone())),
            MockMode::MalformedOutput => Err(AIError::MalformedIntent("Invalid JSON payload from remote model".to_string())),
            MockMode::OfflineUnavailable => Err(AIError::OfflineUnavailable("Remote AI server is unreachable".to_string())),
        }
    }
}
