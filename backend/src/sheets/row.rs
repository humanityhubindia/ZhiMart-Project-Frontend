//! Order → spreadsheet row, and reading order numbers back out of column A.
//! Pure functions, no I/O.
//!
//! Money cells are JSON numbers in rupees (`875`, `52.5`), never "₹" strings,
//! so the sheet owner can `SUM` them. Every other cell is a string written
//! with `valueInputOption=RAW`, so customer text such as `=HYPERLINK(…)` is
//! stored literally and never evaluated as a formula.

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, Utc};
use serde_json::Value;

use crate::{
    email::templates::format_inr,
    repo::orders::{Order, OrderItem},
};

/// Column headers, in sheet order. Column A must stay the order number: rows
/// are found by it.
pub const HEADER: [&str; 27] = [
    "Order No",
    "Placed At (IST)",
    "Customer Name",
    "Phone",
    "WhatsApp",
    "Email",
    "Address",
    "City",
    "Pincode",
    "State",
    "Items",
    "Total Units",
    "MRP Total",
    "Subtotal",
    "Delivery Charge",
    "Total Payable",
    "Pricing Tier",
    "Payment Status",
    "Amount Paid",
    "Amount Refunded",
    "Payment Method / Reference",
    "Order Status",
    "Courier",
    "LR / Docket No",
    "Customer Notes",
    "Test Order",
    "Last Updated (IST)",
];

pub const COLUMN_COUNT: usize = HEADER.len();

/// Google's limit is 50,000 characters per cell.
const MAX_CELL_CHARS: usize = 50_000;

/// India Standard Time (UTC+05:30, no DST).
const IST_OFFSET_SECS: i32 = 5 * 3600 + 30 * 60;

/// 1-based column number → A1 letters (1 → `A`, 26 → `Z`, 27 → `AA`).
pub fn column_letter(mut n: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Letters of the last column written ([`COLUMN_COUNT`]).
pub fn last_column() -> String {
    column_letter(COLUMN_COUNT)
}

/// `14-Oct-2026 07:45 PM` in IST.
pub fn ist_datetime(ts: DateTime<Utc>) -> String {
    match FixedOffset::east_opt(IST_OFFSET_SECS) {
        Some(tz) => ts
            .with_timezone(&tz)
            .format("%d-%b-%Y %I:%M %p")
            .to_string(),
        None => ts.format("%d-%b-%Y %H:%M UTC").to_string(),
    }
}

/// Paise → a numeric cell in rupees: `87500` → `875`, `5250` → `52.5`.
pub fn money(paise: i64) -> Value {
    if paise % 100 == 0 {
        Value::from(paise / 100)
    } else {
        Value::from(paise as f64 / 100.0)
    }
}

/// `Flower Pots Special × 2 @ ₹53; 1000 Wala × 1 @ ₹120`.
pub fn items_summary(items: &[OrderItem]) -> String {
    items
        .iter()
        .map(|i| {
            format!(
                "{} × {} @ {}",
                i.product_name.trim(),
                i.quantity,
                format_inr(i.unit_price)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// `https://wa.me/<digits>` (country code included, as stored: `+91…`).
pub fn whatsapp_link(phone: &str) -> String {
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        String::new()
    } else {
        format!("https://wa.me/{digits}")
    }
}

/// Human-readable payment state derived from the order's payment fields
/// (admin "record payment", Razorpay capture and refunds all land there).
pub fn payment_status_label(order: &Order) -> &'static str {
    let p = &order.payment;
    match p.status.as_str() {
        "refunded" => "Refunded",
        "paid" if p.amount_refunded > 0 => "Partially refunded",
        "paid" => "Paid",
        "failed" => "Payment failed",
        _ if order.status == "pending_payment" => "Awaiting payment",
        _ => "Unpaid",
    }
}

/// Money received (before refunds): the payable total once paid, else 0.
pub fn amount_paid_paise(order: &Order) -> i64 {
    match order.payment.status.as_str() {
        "paid" | "refunded" => order.total_amount,
        _ => 0,
    }
}

/// Money returned. A refund recorded by hand (`mark_refunded`) carries no
/// amount and means the whole payment.
pub fn amount_refunded_paise(order: &Order) -> i64 {
    let p = &order.payment;
    if p.status == "refunded" && p.amount_refunded == 0 {
        order.total_amount
    } else {
        p.amount_refunded
    }
}

fn method_label(method: &str) -> &str {
    match method {
        "razorpay" => "Razorpay (online)",
        "pay_later" => "Pay later",
        "upi" => "UPI",
        "card" => "Card",
        "netbanking" => "Net banking",
        "cod" => "Cash on delivery",
        "bank_transfer" => "Bank transfer",
        other => other,
    }
}

/// `UPI · Ref UTR123`, `Razorpay (online) · upi · pay_Abc123`, `Pay later`.
pub fn payment_details(order: &Order) -> String {
    let p = &order.payment;
    let mut parts = vec![method_label(&p.method).to_owned()];
    if let Some(instrument) = p.instrument.as_deref().filter(|s| !s.is_empty()) {
        parts.push(instrument.to_owned());
    }
    if let Some(reference) = p
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("Ref {reference}"));
    }
    if let Some(id) = p.razorpay_payment_id.as_deref() {
        parts.push(id.to_owned());
    }
    parts.join(" · ")
}

/// Admin-console status label; a cancellation carries its reason.
pub fn order_status_label(order: &Order) -> String {
    let label = match order.status.as_str() {
        "pending_payment" => "Awaiting payment",
        "pending_pack" => "Pending Pack",
        "dispatched" => "Dispatched",
        "in_transit" => "In Transit",
        "out_for_delivery" => "Out for Delivery",
        "delivered" => "Delivered",
        "cancelled" => "Cancelled",
        other => other,
    };
    match order.cancel_reason.as_deref().map(str::trim) {
        Some(reason) if order.status == "cancelled" && !reason.is_empty() => {
            format!("{label} ({reason})")
        }
        _ => label.to_owned(),
    }
}

fn pricing_tier_label(tier: &str) -> &str {
    match tier {
        "bulk" => "Bulk",
        "standard" => "Standard",
        other => other,
    }
}

/// A text cell: control characters (other than newlines/tabs) removed and cut
/// to the per-cell limit.
fn text(s: &str) -> Value {
    Value::String(
        s.chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .take(MAX_CELL_CHARS)
            .collect(),
    )
}

fn opt_text(s: Option<&str>) -> Value {
    text(s.unwrap_or_default().trim())
}

/// The full row for `order`, matching [`HEADER`].
pub fn order_row(order: &Order) -> Vec<Value> {
    let c = &order.customer;
    let row = vec![
        text(&order.order_number),
        text(&ist_datetime(order.created_at)),
        text(c.name.trim()),
        text(&c.phone),
        text(&whatsapp_link(&c.phone)),
        opt_text(c.email.as_deref()),
        text(c.address.trim()),
        text(c.city.trim()),
        text(&c.pincode),
        text(c.state.trim()),
        text(&items_summary(&order.items)),
        Value::from(order.item_count),
        money(order.mrp_total),
        money(order.subtotal),
        money(order.delivery_fee),
        money(order.total_amount),
        text(pricing_tier_label(&order.pricing_tier)),
        text(payment_status_label(order)),
        money(amount_paid_paise(order)),
        money(amount_refunded_paise(order)),
        text(&payment_details(order)),
        text(&order_status_label(order)),
        opt_text(order.shipping.transport_partner.as_deref()),
        opt_text(order.shipping.docket_number.as_deref()),
        opt_text(order.notes.as_deref()),
        text(if order.is_test { "Yes" } else { "No" }),
        text(&ist_datetime(order.updated_at)),
    ];
    debug_assert_eq!(row.len(), COLUMN_COUNT);
    row
}

/// The header as a row of cells.
pub fn header_row() -> Vec<Value> {
    HEADER.iter().map(|h| Value::from(*h)).collect()
}

/// Whether a `values.get` result for row 1 is empty (no cell has content).
pub fn row_is_empty(values: &[Vec<Value>]) -> bool {
    values
        .iter()
        .flatten()
        .all(|v| cell_text(v).trim().is_empty())
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Map order number → 1-based sheet row from a `values.get` of column A
/// (`values[i]` is row `i + 1`; empty rows come back as `[]`). Row 1 (the
/// header) is skipped; if an order number appears twice, the first row wins.
pub fn index_column_a(values: &[Vec<Value>]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for (i, row) in values.iter().enumerate().skip(1) {
        let Some(cell) = row.first() else { continue };
        let key = cell_text(cell).trim().to_owned();
        if key.is_empty() {
            continue;
        }
        let Ok(row_number) = u32::try_from(i + 1) else {
            break;
        };
        map.entry(key).or_insert(row_number);
    }
    map
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::repo::orders::{Customer, Payment, Shipping};

    fn item(name: &str, qty: i32, unit_paise: i64) -> OrderItem {
        OrderItem {
            product_id: "p".into(),
            product_name: name.into(),
            pack_info: "1 Box".into(),
            quantity: qty,
            unit_price: unit_paise,
            unit_mrp: unit_paise * 4,
            line_total: unit_paise * i64::from(qty),
        }
    }

    fn order() -> Order {
        let placed = Utc.with_ymd_and_hms(2026, 10, 14, 14, 15, 0).unwrap();
        Order {
            id: Uuid::nil(),
            order_number: "ZM-261014-1001".into(),
            created_at: placed,
            updated_at: placed + chrono::Duration::minutes(90),
            version: 1,
            customer: Customer {
                name: " Ravi Kumar ".into(),
                phone: "+919840012345".into(),
                email: Some("ravi@example.com".into()),
                address: "12, Gandhi Street".into(),
                city: "Chennai".into(),
                state: "Tamil Nadu".into(),
                pincode: "600004".into(),
            },
            notes: Some("=HYPERLINK(\"http://evil\")\u{7}".into()),
            items: vec![
                item("Flower Pots Special", 2, 5_300),
                item("1000 Wala", 1, 12_000),
            ],
            item_count: 3,
            mrp_total: 90_000,
            subtotal: 22_600,
            delivery_fee: 35_000,
            total_amount: 57_600,
            status: "pending_pack".into(),
            payment: Payment {
                method: "pay_later".into(),
                status: "pending".into(),
                reference: None,
                paid_at: None,
                refunded_at: None,
                instrument: None,
                razorpay_order_id: None,
                razorpay_payment_id: None,
                amount_refunded: 0,
                attention: None,
            },
            shipping: Shipping {
                transport_partner: None,
                docket_number: None,
                dispatched_at: None,
                delivered_at: None,
            },
            cancelled_at: None,
            cancel_reason: None,
            is_test: false,
            payment_expires_at: None,
            pricing_tier: "standard".into(),
        }
    }

    #[test]
    fn column_letters() {
        assert_eq!(column_letter(1), "A");
        assert_eq!(column_letter(26), "Z");
        assert_eq!(column_letter(27), "AA");
        assert_eq!(column_letter(52), "AZ");
        assert_eq!(column_letter(53), "BA");
        assert_eq!(column_letter(702), "ZZ");
        assert_eq!(column_letter(703), "AAA");
        assert_eq!(last_column(), "AA");
    }

    #[test]
    fn ist_time_format() {
        // 14:15 UTC = 19:45 IST.
        let t = Utc.with_ymd_and_hms(2026, 10, 14, 14, 15, 0).unwrap();
        assert_eq!(ist_datetime(t), "14-Oct-2026 07:45 PM");
        // Crosses midnight into the next IST day.
        let t = Utc.with_ymd_and_hms(2026, 10, 14, 18, 40, 0).unwrap();
        assert_eq!(ist_datetime(t), "15-Oct-2026 12:10 AM");
    }

    #[test]
    fn money_cells_are_plain_numbers() {
        assert_eq!(money(87_500), json!(875));
        assert_eq!(money(5_250), json!(52.5));
        assert_eq!(money(5_205), json!(52.05));
        assert_eq!(money(0), json!(0));
        assert!(money(87_500).is_i64());
    }

    #[test]
    fn items_string() {
        let o = order();
        assert_eq!(
            items_summary(&o.items),
            "Flower Pots Special × 2 @ ₹53; 1000 Wala × 1 @ ₹120"
        );
        assert_eq!(
            items_summary(&[item("Sky Shot", 1, 125_050)]),
            "Sky Shot × 1 @ ₹1,250.50"
        );
        assert_eq!(items_summary(&[]), "");
    }

    #[test]
    fn header_shape() {
        assert_eq!(HEADER[0], "Order No", "rows are found by column A");
        assert_eq!(header_row().len(), COLUMN_COUNT);
        assert_eq!(order_row(&order()).len(), COLUMN_COUNT);
        let mut seen = std::collections::HashSet::new();
        assert!(HEADER.iter().all(|h| seen.insert(*h)), "unique headers");
    }

    #[test]
    fn full_row() {
        let row = order_row(&order());
        let at = |name: &str| row[HEADER.iter().position(|h| *h == name).unwrap()].clone();
        assert_eq!(at("Order No"), json!("ZM-261014-1001"));
        assert_eq!(at("Placed At (IST)"), json!("14-Oct-2026 07:45 PM"));
        assert_eq!(at("Customer Name"), json!("Ravi Kumar"));
        assert_eq!(at("Phone"), json!("+919840012345"));
        assert_eq!(at("WhatsApp"), json!("https://wa.me/919840012345"));
        assert_eq!(at("Total Units"), json!(3));
        assert_eq!(at("MRP Total"), json!(900));
        assert_eq!(at("Subtotal"), json!(226));
        assert_eq!(at("Delivery Charge"), json!(350));
        assert_eq!(at("Total Payable"), json!(576));
        assert_eq!(at("Pricing Tier"), json!("Standard"));
        assert_eq!(at("Payment Status"), json!("Unpaid"));
        assert_eq!(at("Amount Paid"), json!(0));
        assert_eq!(at("Payment Method / Reference"), json!("Pay later"));
        assert_eq!(at("Order Status"), json!("Pending Pack"));
        assert_eq!(at("Courier"), json!(""));
        // Formula text survives verbatim (RAW); the control character is dropped.
        assert_eq!(at("Customer Notes"), json!("=HYPERLINK(\"http://evil\")"));
        assert_eq!(at("Test Order"), json!("No"));
        assert_eq!(at("Last Updated (IST)"), json!("14-Oct-2026 09:15 PM"));
    }

    #[test]
    fn payment_and_status_labels() {
        let mut o = order();
        o.payment.status = "paid".into();
        o.payment.method = "upi".into();
        o.payment.reference = Some(" UTR123 ".into());
        assert_eq!(payment_status_label(&o), "Paid");
        assert_eq!(amount_paid_paise(&o), 57_600);
        assert_eq!(amount_refunded_paise(&o), 0);
        assert_eq!(payment_details(&o), "UPI · Ref UTR123");

        o.payment.amount_refunded = 10_000;
        assert_eq!(payment_status_label(&o), "Partially refunded");
        assert_eq!(amount_refunded_paise(&o), 10_000);

        o.payment.status = "refunded".into();
        o.payment.amount_refunded = 0;
        assert_eq!(payment_status_label(&o), "Refunded");
        assert_eq!(amount_paid_paise(&o), 57_600);
        assert_eq!(amount_refunded_paise(&o), 57_600, "manual refund = full");

        o.payment = Payment {
            method: "razorpay".into(),
            status: "pending".into(),
            instrument: Some("upi".into()),
            razorpay_payment_id: Some("pay_ABC123".into()),
            reference: None,
            ..o.payment.clone()
        };
        o.status = "pending_payment".into();
        assert_eq!(payment_status_label(&o), "Awaiting payment");
        assert_eq!(payment_details(&o), "Razorpay (online) · upi · pay_ABC123");

        o.status = "cancelled".into();
        o.cancel_reason = Some("Payment not completed".into());
        assert_eq!(order_status_label(&o), "Cancelled (Payment not completed)");
        assert_eq!(payment_status_label(&o), "Unpaid");
        o.payment.status = "failed".into();
        assert_eq!(payment_status_label(&o), "Payment failed");
    }

    #[test]
    fn whatsapp_digits_only() {
        assert_eq!(
            whatsapp_link("+91 98400-12345"),
            "https://wa.me/919840012345"
        );
        assert_eq!(whatsapp_link(""), "");
    }

    #[test]
    fn finds_rows_by_order_number() {
        let values: Vec<Vec<Value>> = serde_json::from_value(json!([
            ["Order No"],
            ["ZM-261014-1001"],
            [],
            [" ZM-261014-1003 "],
            [""],
            ["ZM-261014-1001"],
            [1005]
        ]))
        .unwrap();
        let map = index_column_a(&values);
        assert_eq!(map.get("ZM-261014-1001"), Some(&2), "first match wins");
        assert_eq!(
            map.get("ZM-261014-1003"),
            Some(&4),
            "trimmed; gaps keep numbering"
        );
        assert_eq!(map.get("1005"), Some(&7));
        assert_eq!(map.get("Order No"), None, "header skipped");
        assert_eq!(map.len(), 3);
        assert!(index_column_a(&[]).is_empty());
    }

    #[test]
    fn empty_row_detection() {
        assert!(row_is_empty(&[]));
        assert!(row_is_empty(&[vec![json!(""), json!(" ")]]));
        assert!(!row_is_empty(&[vec![json!(""), json!("Order No")]]));
    }
}
