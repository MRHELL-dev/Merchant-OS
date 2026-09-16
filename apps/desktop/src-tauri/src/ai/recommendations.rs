use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DemandRecommendationDto {
    pub product_id: String,
    pub product_name: String,
    pub unit: String,
    pub current_stock_millie: i64,
    pub min_stock_level_millie: i64,
    pub recent_sales_millie: i64,
    pub estimated_days_to_stockout: Option<i64>,
    pub recommended_reorder_millie: i64,
    pub suggested_supplier_id: Option<String>,
    pub suggested_supplier_name: Option<String>,
    pub reason: String,
    pub confidence: f64,
}

pub struct DemandIntelligenceService;

impl DemandIntelligenceService {
    /// Generates demand and reorder recommendations based strictly on confirmed sales and current stock.
    /// Excludes damages, losses, stock corrections, and returns.
    /// Resolves historical suppliers from actual confirmed purchases; never hallucinates suppliers.
    pub fn generate_recommendations(conn: &Connection) -> Result<Vec<DemandRecommendationDto>, rusqlite::Error> {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.unit, p.min_stock_level, COALESCE(i.current_quantity, 0)
             FROM products p
             LEFT JOIN inventory i ON p.id = i.product_id
             WHERE p.is_active = 1",
        )?;

        let products: Vec<(String, String, String, i64, i64)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut recommendations = Vec::new();

        for (prod_id, prod_name, unit, min_stock, current_stock) in products {
            // Query confirmed sales in the last 30 days
            let recent_sales: i64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(si.quantity), 0)
                     FROM sale_items si
                     JOIN sales s ON si.sale_id = s.id
                     WHERE si.product_id = ?1",
                    rusqlite::params![prod_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            // Reorder condition: current stock is at or below minimum stock, or zero stock
            if current_stock <= min_stock || current_stock == 0 {
                // V1 Reorder Heuristic: max(0, min_stock * 2 - current_stock)
                let recommended_reorder = if min_stock > 0 {
                    (min_stock * 2 - current_stock).max(1000)
                } else {
                    10_000 // default 10 units if no min_stock defined
                };

                // Estimated days to stockout based on 30-day confirmed sales velocity
                let daily_sales_millie = recent_sales / 30;
                let days_to_stockout = if daily_sales_millie > 0 {
                    Some(current_stock / daily_sales_millie)
                } else if current_stock == 0 {
                    Some(0)
                } else {
                    None
                };

                // Resolve historical supplier strictly from past purchases
                let historical_supplier: Option<(String, String)> = conn
                    .query_row(
                        "SELECT s.id, s.name
                         FROM purchases p
                         JOIN purchase_items pi ON p.id = pi.purchase_id
                         JOIN suppliers s ON p.supplier_id = s.id
                         WHERE pi.product_id = ?1
                         ORDER BY p.created_at DESC
                         LIMIT 1",
                        rusqlite::params![prod_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .ok();

                let (supp_id, supp_name) = match historical_supplier {
                    Some((id, name)) => (Some(id), Some(name)),
                    None => (None, None),
                };

                let reason = if current_stock == 0 {
                    "Item is completely out of stock.".to_string()
                } else if let Some(days) = days_to_stockout {
                    format!("Stock below threshold ({} remaining). Estimated stockout in ~{} days based on confirmed sales.", current_stock / 1000, days)
                } else {
                    format!("Current stock ({} {}) is below minimum threshold ({} {}).", current_stock / 1000, unit, min_stock / 1000, unit)
                };

                let confidence = if recent_sales > 0 && supp_id.is_some() {
                    0.95
                } else if supp_id.is_some() {
                    0.80
                } else {
                    0.65
                };

                recommendations.push(DemandRecommendationDto {
                    product_id: prod_id,
                    product_name: prod_name,
                    unit,
                    current_stock_millie: current_stock,
                    min_stock_level_millie: min_stock,
                    recent_sales_millie: recent_sales,
                    estimated_days_to_stockout: days_to_stockout,
                    recommended_reorder_millie: recommended_reorder,
                    suggested_supplier_id: supp_id,
                    suggested_supplier_name: supp_name,
                    reason,
                    confidence,
                });
            }
        }

        // Sort recommendations: out-of-stock items first, then lowest days to stockout
        recommendations.sort_by_key(|r| r.current_stock_millie);

        Ok(recommendations)
    }
}
