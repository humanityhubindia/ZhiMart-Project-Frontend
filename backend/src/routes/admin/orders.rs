//! Admin order management (`/api/v1/admin/orders…`). Every handler takes
//! [`AuthAdmin`]; every mutation is guarded by `expectedVersion` (optimistic
//! concurrency) and bumps `version`/`updated_at`.

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthAdmin,
    error::{AppError, AppResult},
    repo::{
        self,
        orders::{
            Cursor, ListFilter, Order, OrderRef, OrderRow, OrderStatus, PAYMENT_METHODS,
            PAYMENT_STATUSES, PaymentUpdate,
        },
    },
    routes::{json::ApiJson, orders::clean_line},
    state::AppState,
};

pub const DEFAULT_PAGE_SIZE: i64 = 50;
pub const MAX_PAGE_SIZE: i64 = 100;
pub const MAX_QUERY_CHARS: usize = 100;
const MAX_CURSOR_LEN: usize = 96;
const STATUS_MESSAGE: &str = "status must be one of pending_payment, pending_pack, dispatched, in_transit, out_for_delivery, delivered, cancelled.";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders", get(list_orders))
        .route("/orders/{id}", get(get_order))
        .route("/orders/{id}/status", post(update_status))
        .route("/orders/{id}/shipping", put(update_shipping))
        .route("/orders/{id}/cancel", post(cancel_order))
        .route("/orders/{id}/payment", post(update_payment))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// JSON with `Cache-Control: no-store` (orders carry customer PII).
pub(crate) fn no_store<T: Serialize>(value: T) -> Response {
    let mut res = Json(value).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::InvalidField(msg.into())
}

fn version_conflict() -> AppError {
    AppError::Conflict {
        code: "version_conflict",
        message: "This order was changed by someone else. Refresh and try again.",
    }
}

fn invalid_transition(message: &'static str) -> AppError {
    AppError::Conflict {
        code: "invalid_transition",
        message,
    }
}

/// A UUID, or an order number like `ZM-261013-1001`. Anything else is a 404.
fn parse_ref(raw: &str) -> Option<OrderRef> {
    if raw.len() == 36 {
        return Uuid::parse_str(raw).ok().map(OrderRef::Id);
    }
    let rest = raw.strip_prefix("ZM-")?;
    let (date, seq) = rest.split_once('-')?;
    let ok = date.len() == 6
        && date.bytes().all(|b| b.is_ascii_digit())
        && (1..=19).contains(&seq.len())
        && seq.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| OrderRef::Number(raw.to_owned()))
}

async fn load_row(state: &AppState, raw_id: &str) -> AppResult<OrderRow> {
    let r = parse_ref(raw_id).ok_or(AppError::NotFound)?;
    repo::orders::find(&state.db, &r)
        .await?
        .ok_or(AppError::NotFound)
}

fn current_status(row: &OrderRow) -> AppResult<OrderStatus> {
    OrderStatus::parse(&row.status)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("order has unknown status in database")))
}

/// Turn the result of a version-guarded UPDATE into a response: `None` means
/// the row vanished (404) or its version moved (409).
async fn finish(state: &AppState, id: Uuid, updated: Option<OrderRow>) -> AppResult<Response> {
    match updated {
        Some(row) => Ok(no_store(repo::orders::with_items(&state.db, row).await?)),
        None if repo::orders::exists(&state.db, id).await? => Err(version_conflict()),
        None => Err(AppError::NotFound),
    }
}

/// Optional short admin text (transport partner, docket, reference): trimmed,
/// whitespace collapsed; blank → `None`.
fn optional_text(raw: Option<&str>, field: &str, max: usize) -> AppResult<Option<String>> {
    match raw {
        Some(s) if !s.trim().is_empty() => clean_line(s, field, 1, max, false).map(Some),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// List / get
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    status: Option<String>,
    payment_status: Option<String>,
    q: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
    include_unpaid: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderPage {
    orders: Vec<Order>,
    next_cursor: Option<String>,
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

pub(crate) fn encode_cursor(c: Cursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        c.created_at.timestamp_micros(),
        c.id.hyphenated()
    ))
}

pub(crate) fn decode_cursor(raw: &str) -> Option<Cursor> {
    if raw.is_empty() || raw.len() > MAX_CURSOR_LEN {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(raw).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let (micros, id) = text.split_once(':')?;
    if micros.is_empty()
        || micros.len() > 19
        || !micros.bytes().all(|b| b.is_ascii_digit())
        || id.len() != 36
    {
        return None;
    }
    let cursor = Cursor {
        created_at: DateTime::from_timestamp_micros(micros.parse().ok()?)?,
        id: Uuid::parse_str(id).ok()?,
    };
    // Canonical form only: rejects alternate encodings of the same position.
    (encode_cursor(cursor) == raw).then_some(cursor)
}

/// Escape `ILIKE` metacharacters (`\`, `%`, `_`) for use with `ESCAPE '\'`.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn build_filter(q: ListQuery) -> AppResult<(ListFilter, i64)> {
    let status = match non_empty(q.status) {
        None => None,
        Some(s) => Some(OrderStatus::parse(s.trim()).ok_or_else(|| invalid(STATUS_MESSAGE))?),
    };
    let payment_status = match non_empty(q.payment_status) {
        None => None,
        Some(s) => Some(
            PAYMENT_STATUSES
                .into_iter()
                .find(|p| *p == s.trim())
                .ok_or_else(|| {
                    invalid("paymentStatus must be one of pending, paid, refunded, failed.")
                })?,
        ),
    };
    let limit = match non_empty(q.limit) {
        None => DEFAULT_PAGE_SIZE,
        Some(s) => s
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| (1..=MAX_PAGE_SIZE).contains(n))
            .ok_or_else(|| invalid(format!("limit must be between 1 and {MAX_PAGE_SIZE}.")))?,
    };
    let include_unpaid = match non_empty(q.include_unpaid).as_deref().map(str::trim) {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => return Err(invalid("includeUnpaid must be true or false.")),
    };
    // Unpaid checkouts are noise for fulfilment; shown only when asked for.
    let exclude_unpaid = !include_unpaid && status != Some(OrderStatus::PendingPayment);
    let cursor = match non_empty(q.cursor) {
        None => None,
        Some(s) => Some(decode_cursor(s.trim()).ok_or_else(|| invalid("cursor is invalid."))?),
    };
    let (text_pattern, phone_pattern) = match non_empty(q.q) {
        None => (None, None),
        Some(s) => {
            let s = s.trim();
            if s.chars().count() > MAX_QUERY_CHARS {
                return Err(invalid(format!(
                    "q must be at most {MAX_QUERY_CHARS} characters."
                )));
            }
            if s.chars().any(char::is_control) {
                return Err(invalid("q contains invalid characters."));
            }
            // "98400 12345" should find "+919840012345".
            let compact: String = s.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
            let phone = (compact.len() >= 3
                && compact.bytes().all(|b| b.is_ascii_digit() || b == b'+'))
            .then(|| format!("%{compact}%"));
            (Some(format!("%{}%", escape_like(s))), phone)
        }
    };
    Ok((
        ListFilter {
            status,
            payment_status,
            text_pattern,
            phone_pattern,
            cursor,
            exclude_unpaid,
        },
        limit,
    ))
}

async fn list_orders(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> AppResult<Response> {
    let Query(query) = query.map_err(|_| AppError::Validation("query string is malformed"))?;
    let (filter, limit) = build_filter(query)?;
    let mut conn = state.db.acquire().await?;
    let (orders, has_more) = repo::orders::list(&mut conn, &filter, limit).await?;
    let next_cursor = if has_more {
        orders.last().map(|o| {
            encode_cursor(Cursor {
                created_at: o.created_at,
                id: o.id,
            })
        })
    } else {
        None
    };
    Ok(no_store(OrderPage {
        orders,
        next_cursor,
    }))
}

async fn get_order(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let r = parse_ref(&id).ok_or(AppError::NotFound)?;
    let mut conn = state.db.acquire().await?;
    let order = repo::orders::load_full(&mut conn, &r)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(no_store(order))
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StatusBody {
    status: String,
    #[serde(default)]
    transport_partner: Option<String>,
    #[serde(default)]
    docket_number: Option<String>,
    expected_version: i32,
}

/// Forward-only along the pipeline (skipping ahead allowed). Cancellation has
/// its own endpoint because it restocks. `pending_payment` is only ever left
/// through the payment service (or cancellation/expiry), never manually.
pub(crate) fn check_transition(from: OrderStatus, to: OrderStatus) -> AppResult<()> {
    if from == OrderStatus::PendingPayment {
        return Err(invalid_transition(
            "This order is awaiting online payment and cannot be advanced manually.",
        ));
    }
    if to == OrderStatus::Cancelled {
        return Err(invalid_transition(
            "Use the cancel action to cancel an order.",
        ));
    }
    if from.is_terminal() {
        return Err(invalid_transition(
            "This order is already delivered or cancelled.",
        ));
    }
    match (from.rank(), to.rank()) {
        (Some(a), Some(b)) if b > a => Ok(()),
        _ => Err(invalid_transition("Order status can only move forward.")),
    }
}

async fn update_status(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<StatusBody>,
) -> AppResult<Response> {
    let target = OrderStatus::parse(body.status.trim()).ok_or_else(|| invalid(STATUS_MESSAGE))?;
    let transport = optional_text(body.transport_partner.as_deref(), "transportPartner", 80)?;
    let docket = optional_text(body.docket_number.as_deref(), "docketNumber", 80)?;

    let row = load_row(&state, &id).await?;
    if row.version != body.expected_version {
        return Err(version_conflict());
    }
    check_transition(current_status(&row)?, target)?;

    let transport = transport.or(row.transport_partner.clone());
    let docket = docket.or(row.docket_number.clone());
    if target.requires_shipment() && (transport.is_none() || docket.is_none()) {
        return Err(invalid(
            "transportPartner and docketNumber are required once an order is dispatched.",
        ));
    }

    let updated = repo::orders::update_status(
        &state.db,
        row.id,
        body.expected_version,
        target,
        transport.as_deref(),
        docket.as_deref(),
    )
    .await?;
    if let Some(u) = &updated {
        tracing::info!(admin_id = %admin.admin_id, order_number = %u.order_number, status = target.as_str(), "order status changed");
    }
    finish(&state, row.id, updated).await
}

// ---------------------------------------------------------------------------
// Shipping
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ShippingBody {
    #[serde(default)]
    transport_partner: Option<String>,
    #[serde(default)]
    docket_number: Option<String>,
    expected_version: i32,
}

/// Replaces both shipping fields (null/blank clears one), until the order is
/// delivered or cancelled. A dispatched order must keep both set.
async fn update_shipping(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<ShippingBody>,
) -> AppResult<Response> {
    let transport = optional_text(body.transport_partner.as_deref(), "transportPartner", 80)?;
    let docket = optional_text(body.docket_number.as_deref(), "docketNumber", 80)?;

    let row = load_row(&state, &id).await?;
    if row.version != body.expected_version {
        return Err(version_conflict());
    }
    let status = current_status(&row)?;
    if status.is_terminal() {
        return Err(invalid_transition(
            "Shipping details cannot be changed after delivery or cancellation.",
        ));
    }
    if status.requires_shipment() && (transport.is_none() || docket.is_none()) {
        return Err(invalid(
            "transportPartner and docketNumber are required once an order is dispatched.",
        ));
    }

    let updated = repo::orders::update_shipping(
        &state.db,
        row.id,
        body.expected_version,
        transport.as_deref(),
        docket.as_deref(),
    )
    .await?;
    if let Some(u) = &updated {
        tracing::info!(admin_id = %admin.admin_id, order_number = %u.order_number, "order shipping updated");
    }
    finish(&state, row.id, updated).await
}

// ---------------------------------------------------------------------------
// Cancel
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CancelBody {
    #[serde(default)]
    reason: Option<String>,
    expected_version: i32,
}

/// Only from `pending_payment` or `pending_pack`. In one transaction: lock the
/// order, lock its products in id order, restock every line, write
/// `order_cancelled` movements and mark the order cancelled. The payment is
/// left as is: a paid order is refunded separately (`/refund`), and a payment
/// that lands after this cancellation is refunded by the payment service.
async fn cancel_order(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<CancelBody>,
) -> AppResult<Response> {
    let reason = optional_text(body.reason.as_deref(), "reason", 300)?;
    let order_id = match parse_ref(&id).ok_or(AppError::NotFound)? {
        OrderRef::Id(id) => id,
        r @ OrderRef::Number(_) => {
            repo::orders::find(&state.db, &r)
                .await?
                .ok_or(AppError::NotFound)?
                .id
        }
    };

    let mut tx = state.db.begin().await?;
    let lock = repo::orders::lock_order(&mut tx, order_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if lock.version != body.expected_version {
        tx.rollback().await?;
        return Err(version_conflict());
    }
    if !OrderStatus::parse(&lock.status).is_some_and(OrderStatus::is_cancellable) {
        tx.rollback().await?;
        return Err(invalid_transition(
            "Only orders that have not been dispatched can be cancelled.",
        ));
    }
    repo::orders::lock_order_products(&mut tx, order_id).await?;
    let row =
        repo::orders::cancel_locked(&mut tx, order_id, admin.admin_id, reason.as_deref()).await?;
    let order = repo::orders::with_items(&mut *tx, row).await?;
    tx.commit().await?;

    tracing::info!(admin_id = %admin.admin_id, order_number = %order.order_number, "order cancelled and restocked");
    Ok(no_store(order))
}

// ---------------------------------------------------------------------------
// Payment
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PaymentBody {
    action: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    reference: Option<String>,
    expected_version: i32,
}

/// `pending|failed → paid` (method required), `pending → failed`, `paid → refunded`.
async fn update_payment(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<PaymentBody>,
) -> AppResult<Response> {
    let action = body.action.trim();
    if !matches!(action, "mark_paid" | "mark_failed" | "mark_refunded") {
        return Err(invalid(
            "action must be one of mark_paid, mark_failed, mark_refunded.",
        ));
    }
    let method_given = non_empty(body.method.clone());
    let reference = optional_text(body.reference.as_deref(), "reference", 120)?;
    if action != "mark_paid" && (method_given.is_some() || reference.is_some()) {
        return Err(invalid(
            "method and reference are only accepted with mark_paid.",
        ));
    }
    let method = match method_given {
        None => None,
        Some(m) => Some(
            PAYMENT_METHODS
                .into_iter()
                .filter(|p| *p != "pay_later")
                .find(|p| *p == m.trim())
                .ok_or_else(|| {
                    invalid("method must be one of upi, card, netbanking, cod, bank_transfer.")
                })?,
        ),
    };

    let row = load_row(&state, &id).await?;
    if row.version != body.expected_version {
        return Err(version_conflict());
    }
    // Online payments (Cashfree, legacy Razorpay) are owned by the payment
    // service (gateway-verified settlement, refunds through the gateway);
    // manual overrides would desync them.
    if repo::orders::is_online_method(&row.payment_method) {
        return Err(invalid_transition(
            "Online (gateway) payments cannot be changed manually; use a refund instead.",
        ));
    }
    let status = current_status(&row)?;
    if status == OrderStatus::PendingPayment {
        return Err(invalid_transition("This order is awaiting online payment."));
    }
    let cancelled = status == OrderStatus::Cancelled;
    let current = row.payment_status.as_str();

    let update = match action {
        "mark_paid" => {
            if cancelled {
                return Err(invalid_transition(
                    "A cancelled order cannot be marked as paid.",
                ));
            }
            if !matches!(current, "pending" | "failed") {
                return Err(invalid_transition(
                    "Only pending or failed payments can be marked as paid.",
                ));
            }
            let method = method.ok_or_else(|| invalid("method is required for mark_paid."))?;
            PaymentUpdate {
                status: "paid",
                method,
                reference: reference.as_deref(),
                set_paid_at: true,
                set_refunded_at: false,
            }
        }
        "mark_failed" => {
            if cancelled {
                return Err(invalid_transition(
                    "A cancelled order cannot change payment to failed.",
                ));
            }
            if current != "pending" {
                return Err(invalid_transition(
                    "Only pending payments can be marked as failed.",
                ));
            }
            PaymentUpdate {
                status: "failed",
                method: &row.payment_method,
                reference: row.payment_reference.as_deref(),
                set_paid_at: false,
                set_refunded_at: false,
            }
        }
        _ => {
            if current != "paid" {
                return Err(invalid_transition("Only paid orders can be refunded."));
            }
            PaymentUpdate {
                status: "refunded",
                method: &row.payment_method,
                reference: row.payment_reference.as_deref(),
                set_paid_at: false,
                set_refunded_at: true,
            }
        }
    };

    let updated =
        repo::orders::update_payment(&state.db, row.id, body.expected_version, update).await?;
    if let Some(u) = &updated {
        tracing::info!(admin_id = %admin.admin_id, order_number = %u.order_number, action, "order payment updated");
    }
    finish(&state, row.id, updated).await
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn transitions() {
        use OrderStatus::*;
        assert!(check_transition(PendingPack, Dispatched).is_ok());
        assert!(check_transition(PendingPack, Delivered).is_ok());
        assert!(check_transition(InTransit, OutForDelivery).is_ok());
        assert!(check_transition(Dispatched, PendingPack).is_err());
        assert!(check_transition(Dispatched, Dispatched).is_err());
        assert!(check_transition(Delivered, Delivered).is_err());
        assert!(check_transition(Cancelled, Dispatched).is_err());
        assert!(check_transition(PendingPack, Cancelled).is_err());
        assert!(check_transition(PendingPayment, PendingPack).is_err());
        assert!(check_transition(PendingPayment, Dispatched).is_err());
        assert!(check_transition(PendingPack, PendingPayment).is_err());
    }

    #[test]
    fn cursor_roundtrip_and_strictness() {
        let c = Cursor {
            created_at: DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
                .unwrap_or_default(),
            id: Uuid::now_v7(),
        };
        let s = encode_cursor(c);
        assert_eq!(decode_cursor(&s), Some(c));
        assert_eq!(decode_cursor(""), None);
        assert_eq!(decode_cursor("not base64!"), None);
        assert_eq!(decode_cursor(&format!("{s}=")), None);
        assert_eq!(decode_cursor(&URL_SAFE_NO_PAD.encode("12:abc")), None);
        assert_eq!(
            decode_cursor(&URL_SAFE_NO_PAD.encode(format!("-1:{}", Uuid::now_v7()))),
            None
        );
        assert_eq!(
            decode_cursor(&URL_SAFE_NO_PAD.encode(format!("01:{}", Uuid::now_v7()))),
            None
        );
        assert_eq!(decode_cursor(&"A".repeat(200)), None);
    }

    #[test]
    fn refs_and_like_escaping() {
        assert!(matches!(
            parse_ref("ZM-261013-1001"),
            Some(OrderRef::Number(_))
        ));
        assert!(matches!(
            parse_ref(&Uuid::now_v7().to_string()),
            Some(OrderRef::Id(_))
        ));
        assert!(parse_ref("ZM-2610-1001").is_none());
        assert!(parse_ref("1; DROP TABLE orders").is_none());
        assert_eq!(escape_like(r"50%_off\"), r"50\%\_off\\");
    }
}
