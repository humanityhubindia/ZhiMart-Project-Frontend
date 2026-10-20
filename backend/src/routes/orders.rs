//! Public checkout: `POST /orders`.
//!
//! One transaction per order: abuse caps are checked under per-IP/per-phone
//! advisory locks, the products are locked `FOR UPDATE` in id order, every line
//! is validated against current stock, and then stock decrement + order + items +
//! `order_placed` movements are written by a single statement. Prices, names and
//! pack info always come from the locked rows, never from the client.
//!
//! With online payments enabled (`CASHFREE_ENABLED`), new orders start in
//! `pending_payment` with their stock reserved until `payment_expires_at` (see
//! `repo::orders::expire_unpaid_orders`); the admin notification is queued by
//! the payment service once payment is confirmed. With online payments off,
//! orders start in `pending_pack` / `pay_later` (no expiry, payment collected
//! offline) and the admin notification is queued here, in the order's own
//! transaction (transactional outbox). Under `PAYMENT_TEST_MODE` every unit
//! is charged `TEST_MODE_UNIT_PRICE_PAISE` (₹1) and the order is flagged `is_test`.
//!
//! The payable total is the goods ("subtotal", after tier pricing) plus the
//! delivery charge, which is waived from `FREE_DELIVERY_MIN_RUPEES` up (see
//! [`Pricing::delivery_fee_for`]); both are decided here, never by the client.

use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::{Config, Pricing, TEST_MODE_UNIT_PRICE_PAISE},
    error::{AppError, AppResult, StockShortage},
    net::ClientMeta,
    payments::order_token,
    repo::{
        self,
        orders::{
            IDEMPOTENCY_CONSTRAINT, LockedProduct, NewOrder, NewOrderLine, OrderItemRow, OrderRow,
            PricingTier, PublicOrder,
        },
    },
    routes::json::ApiJson,
    state::AppState,
};

/// Max orders per client IP per rolling hour.
pub const MAX_ORDERS_PER_IP_PER_HOUR: i64 = 10;
/// Max open (`pending_payment` + `pending_pack`) orders per phone number per rolling 24 h
/// (pay-later mode only ever creates `pending_pack`, so that is what it counts there).
pub const MAX_PENDING_ORDERS_PER_PHONE_PER_DAY: i64 = 5;
pub const MAX_LINES: usize = 50;
pub const MAX_QUANTITY: i64 = 1000;
/// `customer.state` when the client omits it (the store only delivers within Tamil Nadu).
pub const DEFAULT_STATE: &str = "Tamil Nadu";

/// Public checkout: POST /orders (behind the strict per-IP `orders` limiter).
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/orders", post(place_order))
}

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlaceOrderRequest {
    pub idempotency_key: String,
    pub customer: CustomerInput,
    #[serde(default)]
    pub notes: Option<String>,
    pub items: Vec<LineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CustomerInput {
    pub name: String,
    pub phone: String,
    #[serde(default)]
    pub email: Option<String>,
    pub address: String,
    pub city: String,
    /// Optional: missing or blank ⇒ [`DEFAULT_STATE`].
    #[serde(default)]
    pub state: Option<String>,
    pub pincode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LineInput {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderEnvelope {
    order: PublicOrder,
    /// Capability token for the guest's pay/verify/status calls (`X-Order-Token`).
    order_token: String,
}

/// Fully validated and normalised checkout request.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidOrder {
    idempotency_key: Uuid,
    name: String,
    phone: String,
    email: Option<String>,
    address: String,
    city: String,
    state: String,
    pincode: String,
    notes: Option<String>,
    /// Sorted by product id (= lock order).
    lines: Vec<(String, i32)>,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn place_order(
    State(state): State<AppState>,
    meta: ClientMeta,
    ApiJson(body): ApiJson<PlaceOrderRequest>,
) -> AppResult<Response> {
    let order = validate(body)?;

    match place(&state, &order, &meta).await? {
        Placed::Created(row, items) => {
            tracing::info!(order_number = %row.order_number, item_count = row.item_count, "order placed");
            Ok(respond(&state.config, StatusCode::CREATED, row, items))
        }
        Placed::Existing(row) => replay(&state, &order, row).await,
        Placed::Raced => {
            // A concurrent submit with the same key committed first.
            let row = repo::orders::find_by_idempotency_key(&state.db, order.idempotency_key)
                .await?
                .ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!("idempotency conflict but no order found"))
                })?;
            replay(&state, &order, row).await
        }
    }
}

enum Placed {
    Created(OrderRow, Vec<OrderItemRow>),
    /// An order with this idempotency key already existed.
    Existing(OrderRow),
    /// Lost the unique-index race against a concurrent submit with the same key.
    Raced,
}

async fn place(state: &AppState, o: &ValidOrder, meta: &ClientMeta) -> AppResult<Placed> {
    let ip = repo::client_ip(meta);
    let mut tx = state.db.begin().await?;

    // Serialise this client's checkouts so the caps below cannot be raced, and
    // so a double-submit (same phone) sees the first attempt's committed order.
    repo::orders::lock_checkout_keys(&mut tx, ip, &o.phone).await?;

    if let Some(existing) =
        repo::orders::find_by_idempotency_key(&mut *tx, o.idempotency_key).await?
    {
        tx.rollback().await?;
        return Ok(Placed::Existing(existing));
    }

    let throttle = repo::orders::checkout_throttle(
        &mut tx,
        ip,
        MAX_ORDERS_PER_IP_PER_HOUR,
        &o.phone,
        MAX_PENDING_ORDERS_PER_PHONE_PER_DAY,
    )
    .await?;
    if let Some(secs) = throttle.ip_retry_secs.or(throttle.phone_retry_secs) {
        tx.rollback().await?;
        tracing::info!(
            cap = if throttle.ip_retry_secs.is_some() {
                "ip"
            } else {
                "phone"
            },
            "checkout cap reached"
        );
        return Err(AppError::TooManyRequests {
            retry_after_secs: u64::try_from(secs).unwrap_or(1).max(1),
        });
    }

    let ids: Vec<String> = o.lines.iter().map(|(id, _)| id.clone()).collect();
    let products = repo::orders::lock_products(&mut tx, &ids).await?;

    // Both lists are sorted by id; any gap or archived row means unavailable.
    let available = products.len() == o.lines.len()
        && products
            .iter()
            .zip(&o.lines)
            .all(|(p, (id, _))| p.id == *id && p.is_active);
    if !available {
        tx.rollback().await?;
        return Err(AppError::Conflict {
            code: "product_unavailable",
            message: "One or more products in your order are no longer available.",
        });
    }

    let shortages: Vec<StockShortage> = products
        .iter()
        .zip(&o.lines)
        .filter(|(p, (_, qty))| *qty > p.stock)
        .map(|(p, (id, qty))| StockShortage {
            product_id: id.clone(),
            requested: *qty,
            available: p.stock,
        })
        .collect();
    if !shortages.is_empty() {
        tx.rollback().await?;
        return Err(AppError::OutOfStock { items: shortages });
    }

    let test_mode = state.config.payment_test_mode;
    let pricing = &state.config.effective_pricing();
    let quote = price_lines(&products, &o.lines, pricing)?;
    let pricing_tier = quote.tier;
    let mut lines = Vec::with_capacity(products.len());
    let (mut item_count, mut total, mut mrp_total) = (0i32, 0i64, 0i64);
    for ((p, (_, qty)), real_unit_price) in
        products.into_iter().zip(&o.lines).zip(quote.unit_prices)
    {
        let q = i64::from(*qty);
        // Test mode: ₹1/unit, MRP kept (floored at the unit price so the
        // `unit_mrp >= unit_price` CHECK always holds). The tier is still
        // decided on real list prices.
        let (unit_price, unit_mrp) = if test_mode {
            (
                TEST_MODE_UNIT_PRICE_PAISE,
                p.original_price_paise.max(TEST_MODE_UNIT_PRICE_PAISE),
            )
        } else {
            (real_unit_price, p.original_price_paise)
        };
        item_count = item_count.checked_add(*qty).ok_or_else(overflow)?;
        total = unit_price
            .checked_mul(q)
            .and_then(|v| v.checked_add(total))
            .ok_or_else(overflow)?;
        mrp_total = unit_mrp
            .checked_mul(q)
            .and_then(|v| v.checked_add(mrp_total))
            .ok_or_else(overflow)?;
        lines.push(NewOrderLine {
            product_id: p.id,
            product_name: p.name,
            pack_info: p.pack_info,
            unit_price_paise: unit_price,
            unit_mrp_paise: unit_mrp,
            quantity: *qty,
        });
    }

    // `total` is the goods (subtotal); delivery is decided on what is charged
    // for them (₹1/unit in test mode, matching the storefront's cart).
    let delivery_fee = pricing.delivery_fee_for(total);
    let total = total.checked_add(delivery_fee).ok_or_else(overflow)?;

    let new = NewOrder {
        id: Uuid::now_v7(),
        idempotency_key: o.idempotency_key,
        customer_name: &o.name,
        customer_phone: &o.phone,
        customer_email: o.email.as_deref(),
        address_line: &o.address,
        city: &o.city,
        state: &o.state,
        pincode: &o.pincode,
        notes: o.notes.as_deref(),
        client_ip: ip,
        user_agent: meta.user_agent.as_deref(),
        item_count,
        mrp_total_paise: mrp_total,
        total_paise: total,
        delivery_fee_paise: delivery_fee,
        is_test: test_mode,
        pricing_tier,
        online_payment: state.config.payments_enabled,
        payment_expiry: state.config.payment_expiry,
        lines,
    };

    let row = match repo::orders::insert_order(&mut tx, &new).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            tx.rollback().await?;
            return Err(AppError::Internal(anyhow::anyhow!(
                "order write did not update every product line"
            )));
        }
        Err(sqlx::Error::Database(db)) if db.constraint() == Some(IDEMPOTENCY_CONSTRAINT) => {
            tx.rollback().await?;
            return Ok(Placed::Raced);
        }
        Err(e) => return Err(e.into()),
    };
    // Pay-later orders are real orders the moment they are placed: queue the
    // admin notification + customer confirmation atomically with the order
    // (online orders queue them when the payment is confirmed).
    if !state.config.payments_enabled {
        repo::outbox::enqueue_order_emails(&mut *tx, &state.config, row.id).await?;
    }
    tx.commit().await?;

    let items = new.lines.iter().map(|l| l.to_item_row(row.id)).collect();
    Ok(Placed::Created(row, items))
}

fn overflow() -> AppError {
    AppError::Internal(anyhow::anyhow!("order total overflow"))
}

/// Real (non-test-mode) unit prices for an order, one per line, plus its tier.
#[derive(Debug, PartialEq, Eq)]
struct Quote {
    tier: PricingTier,
    unit_prices: Vec<i64>,
}

/// Two-tier pricing: the order's value at price-list prices decides the tier
/// for every line. At or above `pricing.bulk_min_order_paise` each line is
/// charged its list price; below it, its standard price (MRP less the
/// standard discount, never below the list price).
fn price_lines(
    products: &[LockedProduct],
    lines: &[(String, i32)],
    pricing: &Pricing,
) -> AppResult<Quote> {
    let mut list_total = 0i64;
    for (p, (_, qty)) in products.iter().zip(lines) {
        list_total = p
            .price_paise
            .checked_mul(i64::from(*qty))
            .and_then(|v| v.checked_add(list_total))
            .ok_or_else(overflow)?;
    }
    let tier = if pricing.is_bulk(list_total) {
        PricingTier::Bulk
    } else {
        PricingTier::Standard
    };
    let unit_prices = products
        .iter()
        .map(|p| match tier {
            PricingTier::Bulk => p.price_paise,
            PricingTier::Standard => {
                pricing.standard_price_paise(p.price_paise, p.original_price_paise)
            }
        })
        .collect();
    Ok(Quote { tier, unit_prices })
}

/// Same idempotency key again: return the original order if it is the same
/// checkout (same phone, same products and quantities), else 409.
async fn replay(state: &AppState, o: &ValidOrder, row: OrderRow) -> AppResult<Response> {
    let items = repo::orders::items_for(&state.db, &[row.id]).await?;
    let mut existing: Vec<(&str, i32)> = items
        .iter()
        .map(|i| (i.product_id.as_str(), i.quantity))
        .collect();
    existing.sort_unstable();
    let requested: Vec<(&str, i32)> = o.lines.iter().map(|(id, q)| (id.as_str(), *q)).collect();

    if row.customer_phone != o.phone || existing != requested {
        return Err(AppError::Conflict {
            code: "idempotency_conflict",
            message: "This checkout was already submitted with different details. Please start a new checkout.",
        });
    }
    tracing::info!(order_number = %row.order_number, "idempotent order replay");
    Ok(respond(&state.config, StatusCode::OK, row, items))
}

fn respond(
    config: &Config,
    status: StatusCode,
    row: OrderRow,
    items: Vec<OrderItemRow>,
) -> Response {
    // Derived (not stored), so an idempotent replay yields the same token.
    let order_token = order_token::issue(config, row.id);
    let mut res = (
        status,
        Json(OrderEnvelope {
            order: PublicOrder::from_parts(row, items),
            order_token,
        }),
    )
        .into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::InvalidField(msg.into())
}

fn validate(body: PlaceOrderRequest) -> AppResult<ValidOrder> {
    let idempotency_key = parse_uuid(&body.idempotency_key)
        .ok_or_else(|| invalid("idempotencyKey must be a UUID."))?;

    let c = body.customer;
    let name = clean_line(&c.name, "customer.name", 2, 100, false)?;
    let phone = normalize_phone(&c.phone)
        .ok_or_else(|| invalid("customer.phone must be a valid 10-digit Indian mobile number."))?;
    // Email is required: the customer's order confirmation and private tracking
    // link are delivered there.
    let email = match c.email.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        None => {
            return Err(invalid(
                "customer.email is required (we send your order confirmation and tracking link there).",
            ));
        }
        Some(e) => Some(
            normalize_email(e)
                .ok_or_else(|| invalid("customer.email is not a valid email address."))?,
        ),
    };
    let address = clean_line(&c.address, "customer.address", 5, 300, true)?;
    let city = clean_line(&c.city, "customer.city", 2, 80, false)?;
    let state = match c.state.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => DEFAULT_STATE.to_owned(),
        Some(s) => clean_line(s, "customer.state", 2, 80, false)?,
    };
    let pincode = c.pincode.trim();
    if !is_pincode(pincode) {
        return Err(invalid(
            "customer.pincode must be a 6-digit Indian PIN code.",
        ));
    }
    let notes = match body.notes.as_deref() {
        None => None,
        Some(n) => clean_notes(n, 500)?,
    };

    if body.items.is_empty() {
        return Err(invalid("items must contain at least one product."));
    }
    if body.items.len() > MAX_LINES {
        return Err(invalid(format!(
            "items may contain at most {MAX_LINES} products."
        )));
    }
    let mut seen = HashSet::with_capacity(body.items.len());
    let mut lines = Vec::with_capacity(body.items.len());
    for (i, line) in body.items.into_iter().enumerate() {
        if !is_product_id(&line.product_id) {
            return Err(invalid(format!(
                "items[{i}].productId is not a valid product id."
            )));
        }
        if !(1..=MAX_QUANTITY).contains(&line.quantity) {
            return Err(invalid(format!(
                "items[{i}].quantity must be between 1 and {MAX_QUANTITY}."
            )));
        }
        if !seen.insert(line.product_id.clone()) {
            return Err(invalid(format!(
                "items[{i}].productId appears more than once."
            )));
        }
        let qty = i32::try_from(line.quantity)
            .map_err(|_| invalid(format!("items[{i}].quantity is out of range.")))?;
        lines.push((line.product_id, qty));
    }
    lines.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    Ok(ValidOrder {
        idempotency_key,
        name,
        phone,
        email,
        address,
        city,
        state,
        pincode: pincode.to_owned(),
        notes,
        lines,
    })
}

fn parse_uuid(s: &str) -> Option<Uuid> {
    let s = s.trim();
    // Hyphenated form only (36 chars); `Uuid::parse_str` also accepts other encodings.
    if s.len() != 36 {
        return None;
    }
    Uuid::parse_str(s).ok().filter(|u| !u.is_nil())
}

/// `^[a-z0-9][a-z0-9-]{1,39}$` (matches the `products_id_format` CHECK).
pub(crate) fn is_product_id(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=40).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_pincode(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 6 && (b'1'..=b'9').contains(&b[0]) && b.iter().all(u8::is_ascii_digit)
}

/// Invisible formatting characters that can disguise text (bidi overrides,
/// zero-width characters, BOM). Never allowed in customer-entered fields.
fn is_disallowed_format_char(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Trim, collapse internal whitespace to single spaces, reject control and
/// invisible formatting characters, and bound the length (in characters).
/// With `allow_breaks`, line breaks/tabs are accepted and folded into spaces.
pub(crate) fn clean_line(
    raw: &str,
    field: &str,
    min: usize,
    max: usize,
    allow_breaks: bool,
) -> AppResult<String> {
    if raw.len() > max * 4 + 256 {
        return Err(invalid(format!(
            "{field} must be at most {max} characters."
        )));
    }
    let bad = raw.chars().any(|c| {
        is_disallowed_format_char(c)
            || (c.is_control() && !(allow_breaks && matches!(c, '\n' | '\r' | '\t')))
    });
    if bad {
        return Err(invalid(format!("{field} contains invalid characters.")));
    }
    let cleaned = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = cleaned.chars().count();
    if n < min || n > max {
        return Err(invalid(format!(
            "{field} must be between {min} and {max} characters."
        )));
    }
    Ok(cleaned)
}

/// Multi-line free text: keeps line breaks (normalised to `\n`), rejects other
/// control characters; blank → `None`.
fn clean_notes(raw: &str, max: usize) -> AppResult<Option<String>> {
    if raw.len() > max * 4 + 256 {
        return Err(invalid(format!("notes must be at most {max} characters.")));
    }
    let text = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', " ");
    if text
        .chars()
        .any(|c| is_disallowed_format_char(c) || (c.is_control() && c != '\n'))
    {
        return Err(invalid("notes contains invalid characters."));
    }
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.chars().count() > max {
        return Err(invalid(format!("notes must be at most {max} characters.")));
    }
    Ok(Some(text.to_owned()))
}

/// Accepts `9840012345`, `98400 12345`, `98400-12345`, `+91 98400 12345`,
/// `919840012345`, `09840012345`; returns `+919840012345`.
pub(crate) fn normalize_phone(raw: &str) -> Option<String> {
    if raw.len() > 32 {
        return None;
    }
    let compact: String = raw.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    let digits = match compact.strip_prefix('+') {
        Some(rest) => rest.strip_prefix("91")?,
        None => compact.as_str(),
    };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let has_plus = compact.starts_with('+');
    let national = match (has_plus, digits.len()) {
        (_, 10) => digits,
        (false, 11) => digits.strip_prefix('0')?,
        (false, 12) => digits.strip_prefix("91")?,
        _ => return None,
    };
    if national.len() != 10 || !matches!(national.as_bytes()[0], b'6'..=b'9') {
        return None;
    }
    Some(format!("+91{national}"))
}

/// Basic shape check (`local@domain.tld`), lower-cased. Not RFC 5322; the
/// address is only used for order communication.
pub(crate) fn normalize_email(raw: &str) -> Option<String> {
    if raw.len() > 254
        || raw.chars().any(|c| {
            c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | ',' | ';')
        })
    {
        return None;
    }
    let (local, domain) = raw.split_once('@')?;
    if local.is_empty() || local.len() > 64 || domain.contains('@') || domain.len() < 3 {
        return None;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2
        || labels
            .iter()
            .any(|l| l.is_empty() || l.starts_with('-') || l.ends_with('-'))
    {
        return None;
    }
    Some(raw.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_normalisation() {
        for ok in [
            "9840012345",
            "98400 12345",
            "98400-12345",
            "+91 98400 12345",
            "+919840012345",
            "919840012345",
            "09840012345",
            " 6000000000 ",
        ] {
            assert!(normalize_phone(ok).is_some(), "{ok}");
        }
        assert_eq!(
            normalize_phone("+91 98400 12345").as_deref(),
            Some("+919840012345")
        );
        assert_eq!(
            normalize_phone("09840012345").as_deref(),
            Some("+919840012345")
        );
        for bad in [
            "",
            "12345",
            "5840012345",
            "+1 9840012345",
            "+9109840012345",
            "98400123456",
            "9840O12345",
            "(984) 001-2345",
            "+91+9840012345",
            "0919840012345",
        ] {
            assert!(normalize_phone(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn line_cleaning() {
        assert_eq!(
            clean_line("  Ravi   Kumar ", "n", 2, 100, false)
                .ok()
                .as_deref(),
            Some("Ravi Kumar")
        );
        assert!(clean_line("Ravi\nKumar", "n", 2, 100, false).is_err());
        assert_eq!(
            clean_line("12 Main St\r\nChennai", "a", 5, 300, true)
                .ok()
                .as_deref(),
            Some("12 Main St Chennai")
        );
        assert!(clean_line("A", "n", 2, 100, false).is_err());
        assert!(clean_line("Ra\u{202E}vi", "n", 2, 100, false).is_err());
        assert!(clean_line(&"x".repeat(101), "n", 2, 100, false).is_err());
        assert!(clean_line(&"த".repeat(100), "n", 2, 100, false).is_ok());
    }

    #[test]
    fn notes_cleaning() {
        assert_eq!(clean_notes("  ", 500).ok(), Some(None));
        assert_eq!(
            clean_notes(" a\r\nb ", 500).ok(),
            Some(Some("a\nb".to_owned()))
        );
        assert!(clean_notes("a\u{0007}", 500).is_err());
        assert!(clean_notes(&"x".repeat(501), 500).is_err());
    }

    #[test]
    fn email_shape() {
        assert_eq!(
            normalize_email("Ravi@Example.COM").as_deref(),
            Some("ravi@example.com")
        );
        for bad in [
            "ravi",
            "ravi@",
            "@x.com",
            "ravi@localhost",
            "a@b@c.com",
            "a b@c.com",
            "a@.com",
            "a@b..com",
        ] {
            assert!(normalize_email(bad).is_none(), "{bad}");
        }
    }

    fn product(id: &str, price_paise: i64, mrp_paise: i64) -> LockedProduct {
        LockedProduct {
            id: id.into(),
            name: id.into(),
            pack_info: "1 Box".into(),
            price_paise,
            original_price_paise: mrp_paise,
            stock: 1000,
            is_active: true,
        }
    }

    #[test]
    fn small_orders_pay_standard_prices_on_every_line() {
        let pricing = Pricing::default();
        // ₹32 (MRP ₹210) × 10 + ₹165 (MRP ₹300) × 2 = ₹650 at list prices.
        let products = [product("a", 3_200, 21_000), product("b", 16_500, 30_000)];
        let lines = [("a".to_owned(), 10), ("b".to_owned(), 2)];
        let q = price_lines(&products, &lines, &pricing).expect("quote");
        assert_eq!(q.tier, PricingTier::Standard);
        // ₹42 (20 % of MRP ₹210), and ₹165 (only 45 % off, so the list
        // price stays).
        assert_eq!(q.unit_prices, vec![4_200, 16_500]);
    }

    #[test]
    fn bulk_threshold_is_inclusive_and_uses_list_prices() {
        let pricing = Pricing::default();
        let products = [product("a", 3_200, 21_000), product("b", 100_000, 600_000)];
        // ₹32 × 125 + ₹1,000 × 1 = ₹5,000 exactly → bulk.
        let lines = [("a".to_owned(), 125), ("b".to_owned(), 1)];
        let q = price_lines(&products, &lines, &pricing).expect("quote");
        assert_eq!(q.tier, PricingTier::Bulk);
        assert_eq!(q.unit_prices, vec![3_200, 100_000]);
        // One unit fewer: ₹4,968 → standard, even though standard prices
        // would push the total well above ₹5,000.
        let lines = [("a".to_owned(), 124), ("b".to_owned(), 1)];
        let q = price_lines(&products, &lines, &pricing).expect("quote");
        assert_eq!(q.tier, PricingTier::Standard);
        assert_eq!(q.unit_prices, vec![4_200, 120_000]);
    }

    #[test]
    fn zero_minimum_means_always_bulk() {
        let pricing = Pricing {
            bulk_min_order_paise: 0,
            ..Pricing::default()
        };
        let products = [product("a", 3_200, 21_000)];
        let q = price_lines(&products, &[("a".to_owned(), 1)], &pricing).expect("quote");
        assert_eq!(q.tier, PricingTier::Bulk);
        assert_eq!(q.unit_prices, vec![3_200]);
    }

    fn request(customer: serde_json::Value) -> PlaceOrderRequest {
        serde_json::from_value(serde_json::json!({
            "idempotencyKey": "0199b5f2-7c1e-7b1a-9d1e-1f2a3b4c5d6e",
            "customer": customer,
            "items": [{ "productId": "zm-01", "quantity": 1 }],
        }))
        .expect("request")
    }

    #[test]
    fn state_defaults_to_tamil_nadu() {
        let base = serde_json::json!({
            "name": "Ravi", "phone": "9840012345", "email": "ravi@example.com",
            "address": "12 Main St", "city": "Chennai", "pincode": "600028",
        });
        let missing = validate(request(base.clone())).expect("valid");
        assert_eq!(missing.state, "Tamil Nadu");
        for blank in [
            serde_json::json!(""),
            serde_json::json!("   "),
            serde_json::Value::Null,
        ] {
            let mut c = base.clone();
            c["state"] = blank;
            assert_eq!(validate(request(c)).expect("valid").state, "Tamil Nadu");
        }
        let mut c = base.clone();
        c["state"] = serde_json::json!("  Kerala ");
        assert_eq!(validate(request(c)).expect("valid").state, "Kerala");
        let mut c = base;
        c["state"] = serde_json::json!("K");
        assert!(validate(request(c)).is_err(), "too short");
    }

    #[test]
    fn ids_and_pincodes() {
        assert!(is_product_id("zm-03"));
        assert!(!is_product_id("ZM-03"));
        assert!(!is_product_id("-zm"));
        assert!(!is_product_id("z"));
        assert!(!is_product_id(&"a".repeat(41)));
        assert!(is_pincode("600028"));
        assert!(!is_pincode("060028"));
        assert!(!is_pincode("60002"));
        assert!(!is_pincode("6000a8"));
        assert!(parse_uuid("0199b5f2-7c1e-7b1a-9d1e-1f2a3b4c5d6e").is_some());
        assert!(parse_uuid("0199b5f27c1e7b1a9d1e1f2a3b4c5d6e").is_none());
        assert!(parse_uuid("00000000-0000-0000-0000-000000000000").is_none());
    }
}
