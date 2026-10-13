//! Money is stored and computed as integer paise (`i64`). The JSON API speaks
//! rupees as numbers (e.g. `165` or `165.5`) because that is what the
//! storefront displays; conversion happens only at the API boundary.

use serde::{Deserialize, Deserializer, Serializer};

/// Largest accepted single price: ₹10,00,000.
pub const MAX_PRICE_PAISE: i64 = 100_000_000;

/// Paise → rupees for JSON output.
pub fn to_rupees(paise: i64) -> f64 {
    paise as f64 / 100.0
}

/// Rupees from client input → paise. Rejects NaN/inf, negatives, more than two
/// decimal places of precision, and values above [`MAX_PRICE_PAISE`].
pub fn from_rupees(rupees: f64) -> Option<i64> {
    if !rupees.is_finite() || rupees < 0.0 {
        return None;
    }
    let paise = (rupees * 100.0).round();
    if (paise - rupees * 100.0).abs() > 1e-6 || paise > MAX_PRICE_PAISE as f64 {
        return None;
    }
    Some(paise as i64)
}

/// Discount percentage shown on the storefront, rounded to the nearest integer.
pub fn discount_percent(price_paise: i64, mrp_paise: i64) -> i32 {
    if mrp_paise <= 0 || price_paise >= mrp_paise {
        return 0;
    }
    (((mrp_paise - price_paise) as f64 / mrp_paise as f64) * 100.0).round() as i32
}

/// "Standard" (small-order) unit price: MRP less `standard_discount_percent`,
/// rounded UP to a whole rupee, but never below the price-list price and never
/// above the MRP. Without an MRP (or when the product is not discounted that
/// much) it is simply `price_paise`.
///
/// Example (70 % off): MRP ₹210, list price ₹32 → ₹63; MRP ₹550 → ₹165.
pub fn standard_price_paise(
    price_paise: i64,
    mrp_paise: i64,
    standard_discount_percent: u32,
) -> i64 {
    if mrp_paise <= 0 {
        return price_paise;
    }
    let payable_percent = i64::from(100 - standard_discount_percent.min(100));
    // mrp × payable% in paise, ceiled to whole rupees: ceil(mrp × pct / 10_000) × 100.
    let standard = mrp_paise
        .saturating_mul(payable_percent)
        .saturating_add(9_999)
        / 10_000
        * 100;
    standard.min(mrp_paise).max(price_paise)
}

/// `#[serde(serialize_with = "money::serialize_rupees")]` for `i64` paise fields.
/// Whole-rupee amounts serialize as integers (`205`), others as decimals (`205.5`).
pub fn serialize_rupees<S: Serializer>(paise: &i64, s: S) -> Result<S::Ok, S::Error> {
    if paise % 100 == 0 {
        s.serialize_i64(paise / 100)
    } else {
        s.serialize_f64(to_rupees(*paise))
    }
}

/// `#[serde(deserialize_with = "money::deserialize_rupees")]` for `i64` paise fields.
pub fn deserialize_rupees<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let rupees = f64::deserialize(d)?;
    from_rupees(rupees).ok_or_else(|| serde::de::Error::custom("invalid rupee amount"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(from_rupees(165.0), Some(16_500));
        assert_eq!(from_rupees(165.5), Some(16_550));
        assert_eq!(from_rupees(0.01), Some(1));
        assert_eq!(to_rupees(16_550), 165.5);
    }

    #[test]
    fn rejects_bad_amounts() {
        assert_eq!(from_rupees(-1.0), None);
        assert_eq!(from_rupees(f64::NAN), None);
        assert_eq!(from_rupees(f64::INFINITY), None);
        assert_eq!(from_rupees(1.234), None);
        assert_eq!(from_rupees(1_000_000.01), None);
        assert_eq!(from_rupees(1_000_000.0), Some(MAX_PRICE_PAISE));
    }

    #[test]
    fn serializes_whole_rupees_as_integers() {
        #[derive(serde::Serialize)]
        struct T {
            #[serde(serialize_with = "serialize_rupees")]
            a: i64,
            #[serde(serialize_with = "serialize_rupees")]
            b: i64,
        }
        let json = serde_json::to_string(&T {
            a: 20_500,
            b: 16_550,
        })
        .unwrap_or_default();
        assert_eq!(json, r#"{"a":205,"b":165.5}"#);
    }

    #[test]
    fn discount() {
        assert_eq!(discount_percent(16_500, 30_000), 45);
        assert_eq!(discount_percent(100, 100), 0);
        assert_eq!(discount_percent(100, 0), 0);
    }

    #[test]
    fn standard_price_is_mrp_less_discount_ceiled_to_whole_rupee() {
        // MRP ₹210 × 30 % = ₹63 exactly.
        assert_eq!(standard_price_paise(3_200, 21_000, 70), 6_300);
        // MRP ₹550 × 30 % = ₹165.
        assert_eq!(standard_price_paise(8_300, 55_000, 70), 16_500);
        // MRP ₹215 × 30 % = ₹64.50 → ceiled to ₹65.
        assert_eq!(standard_price_paise(3_300, 21_500, 70), 6_500);
        // MRP ₹100.01 × 30 % = ₹30.003 → ₹31 (any fraction rounds up).
        assert_eq!(standard_price_paise(1_000, 10_001, 70), 3_100);
        // Other configured discounts.
        assert_eq!(standard_price_paise(1_000, 30_000, 60), 12_000);
        assert_eq!(standard_price_paise(1_000, 30_000, 0), 30_000);
    }

    #[test]
    fn standard_price_never_below_list_price_or_above_mrp() {
        // Only 45 % off MRP: the standard price is the list price.
        assert_eq!(standard_price_paise(16_500, 30_000, 70), 16_500);
        // List price equal to MRP.
        assert_eq!(standard_price_paise(5_000, 5_000, 70), 5_000);
        // No MRP: list price.
        assert_eq!(standard_price_paise(1_000, 0, 70), 1_000);
        assert_eq!(standard_price_paise(1_000, -5, 70), 1_000);
        // Rounding up to a rupee must not exceed a sub-rupee MRP.
        assert_eq!(standard_price_paise(50, 80, 70), 80);
        // 100 % "discount" (or more) falls back to the list price.
        assert_eq!(standard_price_paise(1_000, 30_000, 100), 1_000);
        assert_eq!(standard_price_paise(1_000, 30_000, 250), 1_000);
        // Huge MRP saturates instead of overflowing.
        assert!(standard_price_paise(1, i64::MAX, 70) >= 1);
    }
}
