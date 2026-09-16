/// Deterministic quantity and monetary arithmetic for Merchant OS.
///
/// Rules:
/// - Quantities are fixed-scale 64-bit integers with a scale factor of 1000 (millie-units).
/// - Monetary amounts are 64-bit integers representing paise/cents.
/// - Rounding uses strict, deterministic round-half-up:
///   Line Total = ((quantity_milli * unit_price_cents) + 500) / 1000
/// - Overflow is explicitly detected using checked arithmetic.

#[derive(Debug, PartialEq, Eq)]
pub enum MathError {
    Overflow,
    InvalidQuantity(String),
    InvalidPrice(String),
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MathError::Overflow => write!(f, "Arithmetic overflow occurred"),
            MathError::InvalidQuantity(msg) => write!(f, "Invalid quantity: {}", msg),
            MathError::InvalidPrice(msg) => write!(f, "Invalid price: {}", msg),
        }
    }
}

impl std::error::Error for MathError {}

pub const QUANTITY_SCALE: i64 = 1000;

/// Calculates total monetary cost in cents from quantity (in millie-units) and unit price (in cents).
///
/// Implements deterministic round-half-up integer division:
/// For positive values: (quantity_milli * unit_price_cents + 500) / 1000
/// For negative values: (quantity_milli * unit_price_cents - 500) / 1000
pub fn calculate_line_total(quantity_milli: i64, unit_price_cents: i64) -> Result<i64, MathError> {
    if unit_price_cents < 0 {
        return Err(MathError::InvalidPrice(
            "Unit price cannot be negative".to_string(),
        ));
    }

    let product = quantity_milli
        .checked_mul(unit_price_cents)
        .ok_or(MathError::Overflow)?;

    let half_scale = QUANTITY_SCALE / 2; // 500

    let rounded = if product >= 0 {
        let with_half = product
            .checked_add(half_scale)
            .ok_or(MathError::Overflow)?;
        with_half / QUANTITY_SCALE
    } else {
        let with_half = product
            .checked_sub(half_scale)
            .ok_or(MathError::Overflow)?;
        with_half / QUANTITY_SCALE
    };

    Ok(rounded)
}

/// Formats millie-units into standard decimal string (e.g. 2500 -> "2.500", 1000 -> "1.000").
pub fn format_quantity_milli(quantity_milli: i64) -> String {
    let sign = if quantity_milli < 0 { "-" } else { "" };
    let abs = quantity_milli.abs();
    let whole = abs / QUANTITY_SCALE;
    let fraction = abs % QUANTITY_SCALE;
    format!("{}{}.{:03}", sign, whole, fraction)
}

/// Parses decimal quantity string into millie-units (e.g. "2.5" -> 2500, "1" -> 1000).
pub fn parse_quantity_to_milli(s: &str) -> Result<i64, MathError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(MathError::InvalidQuantity("Empty quantity string".to_string()));
    }

    let (sign, unsigned_str) = if let Some(stripped) = trimmed.strip_prefix('-') {
        (-1i64, stripped)
    } else if let Some(stripped) = trimmed.strip_prefix('+') {
        (1i64, stripped)
    } else {
        (1i64, trimmed)
    };

    let parts: Vec<&str> = unsigned_str.split('.').collect();
    if parts.len() > 2 {
        return Err(MathError::InvalidQuantity("Multiple decimal points".to_string()));
    }

    let whole: i64 = parts[0]
        .parse()
        .map_err(|_| MathError::InvalidQuantity(format!("Invalid integer part: {}", parts[0])))?;

    let fraction: i64 = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 3 {
            // Take first 3 digits
            let truncated = &frac_str[..3];
            truncated.parse().unwrap_or(0)
        } else {
            let padded = format!("{:0<3}", frac_str);
            padded.parse().map_err(|_| {
                MathError::InvalidQuantity(format!("Invalid fractional part: {}", frac_str))
            })?
        }
    } else {
        0
    };

    let total_milli = whole
        .checked_mul(QUANTITY_SCALE)
        .and_then(|w| w.checked_add(fraction))
        .and_then(|t| t.checked_mul(sign))
        .ok_or(MathError::Overflow)?;

    Ok(total_milli)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_line_total_round_half_up() {
        // 1.000 unit @ 1000 cents (₹10.00) = 1000 cents
        assert_eq!(calculate_line_total(1000, 1000).unwrap(), 1000);

        // 2.500 kg (2500 milli) @ 1500 cents (₹15.00/kg) = 3750 cents (₹37.50)
        assert_eq!(calculate_line_total(2500, 1500).unwrap(), 3750);

        // 0.333 kg (333 milli) @ 1000 cents = (333 * 1000 + 500) / 1000 = 333500 / 1000 = 333 cents
        assert_eq!(calculate_line_total(333, 1000).unwrap(), 333);

        // 0.001 kg (1 milli) @ 500 cents = (500 + 500) / 1000 = 1 cent (round up)
        assert_eq!(calculate_line_total(1, 500).unwrap(), 1);

        // 0.001 kg (1 milli) @ 499 cents = (499 + 500) / 1000 = 0 cents (round down)
        assert_eq!(calculate_line_total(1, 499).unwrap(), 0);
    }

    #[test]
    fn test_overflow_protection() {
        let result = calculate_line_total(i64::MAX, 2);
        assert_eq!(result, Err(MathError::Overflow));
    }

    #[test]
    fn test_parse_and_format_quantity() {
        assert_eq!(parse_quantity_to_milli("2.5").unwrap(), 2500);
        assert_eq!(parse_quantity_to_milli("0.250").unwrap(), 250);
        assert_eq!(parse_quantity_to_milli("10").unwrap(), 10000);
        assert_eq!(format_quantity_milli(2500), "2.500");
        assert_eq!(format_quantity_milli(250), "0.250");
        assert_eq!(format_quantity_milli(10000), "10.000");
    }
}
