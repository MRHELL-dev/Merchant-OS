use crate::ai::error::AIError;
use crate::ai::intents::*;
use crate::ai::provider::AIProvider;
use crate::ai::recommendations::{DemandIntelligenceService, DemandRecommendationDto};
use crate::ai::validation::DeterministicIntentValidator;
use crate::ai::voice::VoiceTranscriptHandler;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AIResponseMode {
    Informational,
    Recommendation,
    PreparedAction,
    Ambiguous,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIResponseDto {
    pub mode: AIResponseMode,
    pub intent_type: String,
    pub explanation: String,
    pub prepared_action: Option<StructuredIntent>,
    pub recommendations: Vec<DemandRecommendationDto>,
    pub ambiguity_options: Vec<String>,
    pub offline_mode: bool,
    pub provider_name: String,
}

pub struct AIInterpreter;

impl AIInterpreter {
    /// Interprets a natural language or voice query and prepares an unprivileged AI response.
    /// Invariant: Never executes transactions or modifies database state.
    pub fn process_query(
        conn: &Connection,
        provider: &dyn AIProvider,
        query: &str,
        is_voice: bool,
    ) -> AIResponseDto {
        let clean_text = if is_voice {
            VoiceTranscriptHandler::sanitize_transcript(query)
        } else {
            query.trim().to_string()
        };

        if clean_text.is_empty() {
            return AIResponseDto {
                mode: AIResponseMode::Error,
                intent_type: "GENERAL_QUERY".to_string(),
                explanation: "Please enter a query or speak into the microphone.".to_string(),
                prepared_action: None,
                recommendations: vec![],
                ambiguity_options: vec![],
                offline_mode: provider.is_offline_capable(),
                provider_name: provider.name().to_string(),
            };
        }

        // 1. Interpret query via AIProvider
        let raw_intent = match provider.interpret(&clean_text) {
            Ok(intent) => intent,
            Err(AIError::AmbiguousIntent(candidates)) => {
                return AIResponseDto {
                    mode: AIResponseMode::Ambiguous,
                    intent_type: "GENERAL_QUERY".to_string(),
                    explanation: "Multiple matching options found. Please select or clarify:".to_string(),
                    prepared_action: None,
                    recommendations: vec![],
                    ambiguity_options: candidates,
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                };
            }
            Err(e) => {
                return AIResponseDto {
                    mode: AIResponseMode::Error,
                    intent_type: "GENERAL_QUERY".to_string(),
                    explanation: format!("{}", e),
                    prepared_action: None,
                    recommendations: vec![],
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                };
            }
        };

        // 2. Validate and enrich intent against live database
        let validated_intent = match DeterministicIntentValidator::validate_and_enrich(conn, raw_intent) {
            Ok(valid) => valid,
            Err(AIError::AmbiguousIntent(candidates)) => {
                return AIResponseDto {
                    mode: AIResponseMode::Ambiguous,
                    intent_type: "GENERAL_QUERY".to_string(),
                    explanation: "Multiple matching entities found. Please choose one:".to_string(),
                    prepared_action: None,
                    recommendations: vec![],
                    ambiguity_options: candidates,
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                };
            }
            Err(e) => {
                return AIResponseDto {
                    mode: AIResponseMode::Error,
                    intent_type: "GENERAL_QUERY".to_string(),
                    explanation: format!("{}", e),
                    prepared_action: None,
                    recommendations: vec![],
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                };
            }
        };

        // 3. Assemble response based on intent classification
        Self::build_response(conn, provider, validated_intent)
    }

    fn build_response(
        conn: &Connection,
        provider: &dyn AIProvider,
        intent: StructuredIntent,
    ) -> AIResponseDto {
        let intent_type_str = intent.intent_type_str().to_string();

        match intent {
            // --- ACTION INTENTS (Produce unprivileged PreparedAction proposal) ---
            StructuredIntent::CreateSale { ref items, ref payment_method, ref settlement_mode, .. } => {
                let item_descriptions: Vec<String> = items
                    .iter()
                    .map(|i| {
                        let price_str = i.unit_price_cents.map(|c| format!(" @ ₹{:.2}", c as f64 / 100.0)).unwrap_or_default();
                        format!("{} {} of {}{}", i.quantity_display, if i.quantity_display == "1" { "unit" } else { "units" }, i.product_name.as_deref().unwrap_or(&i.product_reference), price_str)
                    })
                    .collect();

                let explanation = format!(
                    "Prepared sale proposal for {}. Payment: {} (Settlement: {}). Please review and confirm below.",
                    item_descriptions.join(", "),
                    payment_method,
                    settlement_mode
                );

                AIResponseDto {
                    mode: AIResponseMode::PreparedAction,
                    intent_type: intent_type_str,
                    explanation,
                    prepared_action: Some(intent),
                    recommendations: vec![],
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                }
            }

            StructuredIntent::CreatePurchase { ref items, ref supplier_reference, ref payment_method, .. } => {
                let item_descriptions: Vec<String> = items
                    .iter()
                    .map(|i| {
                        let cost_str = i.unit_cost_cents.map(|c| format!(" @ ₹{:.2}", c as f64 / 100.0)).unwrap_or_default();
                        format!("{} units of {}{}", i.quantity_display, i.product_name.as_deref().unwrap_or(&i.product_reference), cost_str)
                    })
                    .collect();

                let explanation = format!(
                    "Prepared purchase proposal: Buy {} from {} via {}. Please review and confirm below.",
                    item_descriptions.join(", "),
                    supplier_reference,
                    payment_method
                );

                AIResponseDto {
                    mode: AIResponseMode::PreparedAction,
                    intent_type: intent_type_str,
                    explanation,
                    prepared_action: Some(intent),
                    recommendations: vec![],
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                }
            }

            StructuredIntent::CreateCustomerOrder { ref items, ref customer_reference, .. } => {
                let item_descriptions: Vec<String> = items
                    .iter()
                    .map(|i| format!("{} units of {}", i.quantity_display, i.product_name.as_deref().unwrap_or(&i.product_reference)))
                    .collect();
                let cust_str = customer_reference.as_deref().unwrap_or("Walk-in Customer");

                let explanation = format!(
                    "Prepared customer order proposal: Order {} for {}. (Note: Draft order does not deduct stock). Please review and confirm below.",
                    item_descriptions.join(", "),
                    cust_str
                );

                AIResponseDto {
                    mode: AIResponseMode::PreparedAction,
                    intent_type: intent_type_str,
                    explanation,
                    prepared_action: Some(intent),
                    recommendations: vec![],
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                }
            }

            // --- READ-ONLY QUERIES (Verified business data lookups) ---
            StructuredIntent::CheckStock { ref product_reference, ref product_id } => {
                if let Some(ref p_id) = product_id {
                    let stock_info: Result<(String, String, i64, i64), _> = conn.query_row(
                        "SELECT p.name, p.unit, p.selling_price_cents, COALESCE(i.current_quantity, 0)
                         FROM products p
                         LEFT JOIN inventory i ON p.id = i.product_id
                         WHERE p.id = ?1",
                        rusqlite::params![p_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    );

                    match stock_info {
                        Ok((name, unit, price, qty)) => {
                            let qty_display = qty as f64 / 1000.0;
                            let price_display = price as f64 / 100.0;
                            let explanation = format!(
                                "{} has {:.3} {} available in inventory. Selling price is ₹{:.2}.",
                                name, qty_display, unit, price_display
                            );
                            AIResponseDto {
                                mode: AIResponseMode::Informational,
                                intent_type: intent_type_str,
                                explanation,
                                prepared_action: None,
                                recommendations: vec![],
                                ambiguity_options: vec![],
                                offline_mode: provider.is_offline_capable(),
                                provider_name: provider.name().to_string(),
                            }
                        }
                        Err(_) => AIResponseDto {
                            mode: AIResponseMode::Error,
                            intent_type: intent_type_str,
                            explanation: format!("Could not retrieve inventory for '{}'", product_reference),
                            prepared_action: None,
                            recommendations: vec![],
                            ambiguity_options: vec![],
                            offline_mode: provider.is_offline_capable(),
                            provider_name: provider.name().to_string(),
                        },
                    }
                } else {
                    AIResponseDto {
                        mode: AIResponseMode::Error,
                        intent_type: intent_type_str,
                        explanation: format!("Product '{}' was not resolved", product_reference),
                        prepared_action: None,
                        recommendations: vec![],
                        ambiguity_options: vec![],
                        offline_mode: provider.is_offline_capable(),
                        provider_name: provider.name().to_string(),
                    }
                }
            }

            StructuredIntent::CheckCustomerCredit { ref customer_reference, ref customer_id } => {
                if customer_reference == "ALL" {
                    let total_credit: i64 = conn
                        .query_row(
                            "SELECT COALESCE(SUM(current_credit_cents), 0) FROM customers WHERE is_active = 1",
                            [],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                    let count: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM customers WHERE current_credit_cents > 0 AND is_active = 1",
                            [],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);

                    let explanation = format!(
                        "Total outstanding customer credit across all customers is ₹{:.2} across {} accounts.",
                        total_credit as f64 / 100.0,
                        count
                    );
                    AIResponseDto {
                        mode: AIResponseMode::Informational,
                        intent_type: intent_type_str,
                        explanation,
                        prepared_action: None,
                        recommendations: vec![],
                        ambiguity_options: vec![],
                        offline_mode: provider.is_offline_capable(),
                        provider_name: provider.name().to_string(),
                    }
                } else if let Some(ref c_id) = customer_id {
                    let (name, credit): (String, i64) = conn
                        .query_row(
                            "SELECT name, current_credit_cents FROM customers WHERE id = ?1",
                            rusqlite::params![c_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .unwrap_or_else(|_| (customer_reference.clone(), 0));

                    let explanation = format!(
                        "Customer '{}' currently has an outstanding credit balance of ₹{:.2}.",
                        name, credit as f64 / 100.0
                    );
                    AIResponseDto {
                        mode: AIResponseMode::Informational,
                        intent_type: intent_type_str,
                        explanation,
                        prepared_action: None,
                        recommendations: vec![],
                        ambiguity_options: vec![],
                        offline_mode: provider.is_offline_capable(),
                        provider_name: provider.name().to_string(),
                    }
                } else {
                    AIResponseDto {
                        mode: AIResponseMode::Error,
                        intent_type: intent_type_str,
                        explanation: format!("Customer '{}' was not found", customer_reference),
                        prepared_action: None,
                        recommendations: vec![],
                        ambiguity_options: vec![],
                        offline_mode: provider.is_offline_capable(),
                        provider_name: provider.name().to_string(),
                    }
                }
            }

            StructuredIntent::CheckSales { ref period } => {
                let total_sales_cents: i64 = conn
                    .query_row(
                        "SELECT COALESCE(SUM(total_amount_cents), 0) FROM sales",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))
                    .unwrap_or(0);

                let period_desc = period.as_deref().unwrap_or("recorded");
                let explanation = format!(
                    "Total confirmed sales ({}) amount to ₹{:.2} across {} transactions.",
                    period_desc,
                    total_sales_cents as f64 / 100.0,
                    count
                );
                AIResponseDto {
                    mode: AIResponseMode::Informational,
                    intent_type: intent_type_str,
                    explanation,
                    prepared_action: None,
                    recommendations: vec![],
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                }
            }

            // --- RECOMMENDATION INTENTS (Demand & Reorder Intelligence) ---
            StructuredIntent::CheckLowStock | StructuredIntent::CheckDemand { .. } | StructuredIntent::PrepareReorder { .. } => {
                let recs = DemandIntelligenceService::generate_recommendations(conn).unwrap_or_default();
                let explanation = if recs.is_empty() {
                    "All inventory items are currently above their minimum stock thresholds. No immediate reorders required.".to_string()
                } else {
                    format!("Identified {} item(s) below or approaching minimum stock thresholds.", recs.len())
                };

                AIResponseDto {
                    mode: AIResponseMode::Recommendation,
                    intent_type: intent_type_str,
                    explanation,
                    prepared_action: None,
                    recommendations: recs,
                    ambiguity_options: vec![],
                    offline_mode: provider.is_offline_capable(),
                    provider_name: provider.name().to_string(),
                }
            }

            // --- GENERAL QUERY ---
            StructuredIntent::GeneralQuery { ref query } => AIResponseDto {
                mode: AIResponseMode::Informational,
                intent_type: intent_type_str,
                explanation: format!(
                    "I understood your query: '{}'. You can ask me to check stock, view customer credit, review low stock, or prepare a sale or purchase.",
                    query
                ),
                prepared_action: None,
                recommendations: vec![],
                ambiguity_options: vec![],
                offline_mode: provider.is_offline_capable(),
                provider_name: provider.name().to_string(),
            },

            _ => AIResponseDto {
                mode: AIResponseMode::Informational,
                intent_type: intent_type_str,
                explanation: "Request acknowledged.".to_string(),
                prepared_action: None,
                recommendations: vec![],
                ambiguity_options: vec![],
                offline_mode: provider.is_offline_capable(),
                provider_name: provider.name().to_string(),
            },
        }
    }
}
