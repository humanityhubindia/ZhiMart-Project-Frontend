//! Guest payment endpoints and the Cashfree webhook.
//!
//! Guests have no account: the `X-Order-Token` header (see
//! `payments::order_token`) authorizes pay / verify / status for one order. A
//! missing or wrong token is answered exactly like an unknown order (404), and
//! the token is checked before anything about the order is revealed.
//!
//! Flow: `POST /pay` creates (once) or fetches the order's Cashfree order and
//! returns its `paymentSessionId` for the JS SDK; after checkout the storefront
//! calls `POST /pay/verify` (repeatedly while a payment is pending), which
//! re-reads the payments from the Cashfree API and settles a successful one.
//! The signed webhook triggers the same re-read server-to-server.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use secrecy::ExposeSecret;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    money::serialize_rupees,
    payments::{
        cashfree, order_token,
        service::{self, SettleError, SettleOutcome, Source},
        signature,
    },
    repo::{
        self,
        orders::{METHOD_CASHFREE, OrderItem, OrderRef, OrderRow, Shipping},
    },
    routes::catalog::PricingInfo,
    state::AppState,
};

/// Header carrying the guest order token.
pub const ORDER_TOKEN_HEADER: &str = "x-order-token";
pub const WEBHOOK_SIGNATURE_HEADER: &str = "x-webhook-signature";
pub const WEBHOOK_TIMESTAMP_HEADER: &str = "x-webhook-timestamp";
/// Webhook bodies are small JSON documents.
pub const MAX_WEBHOOK_BODY_BYTES: usize = 64 * 1024;
/// Gateway name in the storefront config / pay response.
pub const GATEWAY: &str = "cashfree";
/// Longest `payment_session_id` we pass on (Cashfree's are a few hundred chars).
const MAX_SESSION_ID_LEN: usize = 1024;

/// Public guest payment endpoints: GET /storefront/config, POST /orders/{orderNumber}/pay,
/// GET /orders/{orderNumber}. `POST /pay/verify` is separate ([`verify_routes`])
/// so it can have its own rate limit (the storefront polls it).
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/storefront/config", get(storefront_config))
        .route("/orders/{order_number}", get(guest_order))
        .route("/orders/{order_number}/pay", post(pay))
}

/// POST /orders/{orderNumber}/pay/verify.
pub fn verify_routes() -> Router<AppState> {
    Router::new().route("/orders/{order_number}/pay/verify", post(verify))
}

/// POST /webhooks/cashfree. Authenticated by the HMAC signature only; mounted
/// outside the per-IP limiters by app.rs.
pub fn webhook_routes() -> Router<AppState> {
    Router::new()
        .route("/webhooks/cashfree", post(webhook))
        .layer(DefaultBodyLimit::max(MAX_WEBHOOK_BODY_BYTES))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// `AppError` plus the two 400s of `/pay/verify`.
#[derive(Debug)]
enum PayError {
    App(AppError),
    /// No successful (or pending) payment on the order: the customer may retry.
    NotCompleted,
    /// A successful payment exists but does not match the order (amount,
    /// currency, order id). Never paid; flagged for the admins.
    VerificationFailed,
}

impl From<AppError> for PayError {
    fn from(e: AppError) -> Self {
        PayError::App(e)
    }
}

impl From<sqlx::Error> for PayError {
    fn from(e: sqlx::Error) -> Self {
        PayError::App(e.into())
    }
}

impl IntoResponse for PayError {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            PayError::App(e) => return e.into_response(),
            PayError::NotCompleted => (
                "payment_not_completed",
                "The payment was not completed. You can try again.",
            ),
            PayError::VerificationFailed => (
                "payment_verification_failed",
                "We could not verify this payment. If money was deducted, it will be confirmed or refunded automatically.",
            ),
        };
        let body = json!({ "error": { "code": code, "message": message } });
        let mut res = (StatusCode::BAD_REQUEST, Json(body)).into_response();
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        res
    }
}

const fn conflict(code: &'static str, message: &'static str) -> AppError {
    AppError::Conflict { code, message }
}

const ORDER_ALREADY_PAID: AppError =
    conflict("order_already_paid", "This order has already been paid.");
const ORDER_EXPIRED: AppError = conflict(
    "order_expired",
    "The payment window for this order has closed and the order was cancelled.",
);
const ORDER_NOT_PAYABLE: AppError =
    conflict("order_not_payable", "This order can no longer be paid.");
const PAYMENT_PENDING: AppError = conflict(
    "payment_pending",
    "Your payment is still being processed. This page will update automatically once it is confirmed.",
);
/// `CASHFREE_ENABLED=false`: orders are pay-later and settled by an admin.
pub const ONLINE_PAYMENTS_DISABLED: AppError = conflict(
    "online_payments_disabled",
    "Online payment is not available. We will contact you to confirm payment and dispatch.",
);

fn require_payments_enabled(state: &AppState) -> AppResult<()> {
    if state.config.payments_enabled {
        Ok(())
    } else {
        Err(ONLINE_PAYMENTS_DISABLED)
    }
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StorefrontConfig<'a> {
    test_mode: bool,
    online_payments_enabled: bool,
    /// `"cashfree"`, or `null` while online payments are disabled.
    payment_gateway: Option<&'static str>,
    /// Cashfree JS SDK mode (`sandbox` | `production`); `null` when disabled.
    cashfree_mode: Option<&'static str>,
    currency: &'static str,
    payment_expiry_minutes: u64,
    store_name: &'a str,
    pricing: PricingInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuestPayment {
    /// `pay_later` | `cashfree` | … (the storefront keys its "order received" view on it).
    method: String,
    status: String,
    instrument: Option<String>,
    paid_at: Option<DateTime<Utc>>,
}

/// Customer-facing order view: no phone, e-mail or address.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuestOrder {
    order_number: String,
    status: String,
    is_test: bool,
    /// `bulk` | `standard` (see `Pricing`).
    pricing_tier: String,
    created_at: DateTime<Utc>,
    payment_expires_at: Option<DateTime<Utc>>,
    payment: GuestPayment,
    /// Goods after tier pricing.
    #[serde(serialize_with = "serialize_rupees")]
    subtotal: i64,
    /// 0 = free delivery.
    #[serde(serialize_with = "serialize_rupees")]
    delivery_fee: i64,
    /// Payable: `subtotal + delivery_fee`.
    #[serde(serialize_with = "serialize_rupees")]
    total_amount: i64,
    #[serde(serialize_with = "serialize_rupees")]
    mrp_total: i64,
    item_count: i32,
    items: Vec<OrderItem>,
    shipping: Shipping,
    customer_name: String,
    city: String,
}

/// What the storefront needs to open Cashfree checkout
/// (`Cashfree({ mode }).checkout({ paymentSessionId })`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PayResponse {
    gateway: &'static str,
    payment_session_id: String,
    gateway_order_id: String,
    amount_paise: i64,
    currency: &'static str,
    mode: &'static str,
    payment_expires_at: Option<DateTime<Utc>>,
    test_mode: bool,
}

fn no_store<T: Serialize>(body: T) -> Response {
    let mut res = Json(body).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

// ---------------------------------------------------------------------------
// Guest order lookup
// ---------------------------------------------------------------------------

/// `ZM-<6 digits>-<digits>`; anything else cannot be an order number.
fn valid_order_number(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("ZM-") else {
        return false;
    };
    let Some((date, seq)) = rest.split_once('-') else {
        return false;
    };
    date.len() == 6
        && date.bytes().all(|b| b.is_ascii_digit())
        && (1..=19).contains(&seq.len())
        && seq.bytes().all(|b| b.is_ascii_digit())
}

/// The presented token: exactly one `X-Order-Token` header.
fn presented_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(ORDER_TOKEN_HEADER).iter();
    match (values.next(), values.next()) {
        (Some(v), None) => v.to_str().ok().map(str::trim),
        _ => None,
    }
}

/// Load the order addressed by `order_number`, but only if the request carries
/// its token. Unknown order, malformed number and wrong/missing token are all
/// the same 404.
async fn load_guest_order(
    state: &AppState,
    order_number: &str,
    headers: &HeaderMap,
) -> AppResult<OrderRow> {
    let token = presented_token(headers);
    let row = if valid_order_number(order_number) {
        repo::orders::find(&state.db, &OrderRef::Number(order_number.to_owned())).await?
    } else {
        None
    };
    // Always run one MAC verification so the work done does not depend on
    // whether the order exists.
    let (id, exists) = row.as_ref().map_or((Uuid::nil(), false), |r| (r.id, true));
    let ok = token.is_some_and(|t| order_token::verify(&state.config, id, t));
    match row {
        Some(r) if ok && exists => Ok(r),
        _ => Err(AppError::NotFound),
    }
}

async fn guest_view(state: &AppState, row: OrderRow) -> AppResult<GuestOrder> {
    let items = repo::orders::items_for(&state.db, &[row.id]).await?;
    let pending = row.status == "pending_payment";
    let subtotal = row.subtotal_paise();
    Ok(GuestOrder {
        order_number: row.order_number,
        status: row.status,
        is_test: row.is_test,
        pricing_tier: row.pricing_tier,
        created_at: row.created_at,
        payment_expires_at: if pending {
            row.payment_expires_at
        } else {
            None
        },
        payment: GuestPayment {
            method: row.payment_method,
            status: row.payment_status,
            instrument: row.payment_instrument,
            paid_at: row.paid_at,
        },
        subtotal,
        delivery_fee: row.delivery_fee_paise,
        total_amount: row.total_paise,
        mrp_total: row.mrp_total_paise,
        item_count: row.item_count,
        items: items.into_iter().map(OrderItem::from).collect(),
        shipping: Shipping {
            transport_partner: row.transport_partner,
            docket_number: row.docket_number,
            dispatched_at: row.dispatched_at,
            delivered_at: row.delivered_at,
        },
        customer_name: row.customer_name,
        city: row.city,
    })
}

async fn reload_guest_view(state: &AppState, order_id: Uuid) -> AppResult<GuestOrder> {
    let row = repo::orders::find(&state.db, &OrderRef::Id(order_id))
        .await?
        .ok_or(AppError::NotFound)?;
    guest_view(state, row).await
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn storefront_config(State(state): State<AppState>) -> Response {
    let c = &state.config;
    let on = c.payments_enabled;
    let mut res = Json(StorefrontConfig {
        test_mode: c.payment_test_mode,
        online_payments_enabled: on,
        payment_gateway: on.then_some(GATEWAY),
        cashfree_mode: on.then(|| c.cashfree_env.as_str()),
        currency: "INR",
        payment_expiry_minutes: c.payment_expiry.as_secs() / 60,
        store_name: &c.store_name,
        pricing: (&c.effective_pricing()).into(),
    })
    .into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=60"),
    );
    res
}

/// GET /orders/{orderNumber}
async fn guest_order(
    State(state): State<AppState>,
    Path(order_number): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let row = load_guest_order(&state, &order_number, &headers).await?;
    Ok(no_store(guest_view(&state, row).await?))
}

/// Why an order cannot be paid now, if it cannot.
fn payability(row: &OrderRow, window_open: bool) -> Result<(), AppError> {
    let money_in = matches!(row.payment_status.as_str(), "paid" | "refunded")
        || row.gateway_payment_id.is_some();
    match row.status.as_str() {
        "pending_payment" if money_in => Err(ORDER_ALREADY_PAID),
        "pending_payment" if row.payment_method != METHOD_CASHFREE => Err(ORDER_NOT_PAYABLE),
        "pending_payment" if window_open => Ok(()),
        "pending_payment" => Err(ORDER_EXPIRED),
        "cancelled" if service::cancelled_by_expiry(row) => Err(ORDER_EXPIRED),
        "cancelled" => Err(ORDER_NOT_PAYABLE),
        _ if money_in => Err(ORDER_ALREADY_PAID),
        _ => Err(ORDER_NOT_PAYABLE),
    }
}

/// Map a settlement to the answer for the storefront (pay / verify).
fn settled_answer(outcome: SettleOutcome) -> Option<AppError> {
    match outcome {
        SettleOutcome::LateRefunded => Some(ORDER_EXPIRED),
        SettleOutcome::NotPayableRefunded => Some(ORDER_NOT_PAYABLE),
        _ => None,
    }
}

/// POST /orders/{orderNumber}/pay — create (once) or fetch the Cashfree order
/// and hand its payment session to the storefront.
///
/// The Cashfree order id is deterministic per order, so races (two tabs,
/// double click, a lost create response) all converge on ONE Cashfree order:
/// the id is attached with a guarded `UPDATE … WHERE gateway_order_id IS NULL`
/// (no lock is held across the HTTP calls), and a create that finds the id
/// taken (409) fetches the existing order instead.
async fn pay(
    State(state): State<AppState>,
    Path(order_number): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let row = load_guest_order(&state, &order_number, &headers).await?;
    require_payments_enabled(&state)?;

    let mut tx = state.db.begin().await?;
    repo::admins::set_lock_timeout(&mut tx).await?;
    let row = repo::orders::find_for_update(&mut tx, &OrderRef::Id(row.id))
        .await?
        .ok_or(AppError::NotFound)?;
    let window_open = repo::payments::payment_window_open(&mut *tx, row.id).await?;
    tx.commit().await?;
    payability(&row, window_open)?;

    let attached_before = row.gateway_order_id.is_some();
    let gid = row
        .gateway_order_id
        .clone()
        .unwrap_or_else(|| cashfree::gateway_order_id(&row.order_number, row.id));
    if !signature::is_gateway_order_id(&gid) {
        tracing::error!(order_number = %row.order_number, "stored gateway order id is malformed");
        return Err(AppError::PaymentGateway);
    }
    if !attached_before && !repo::payments::attach_gateway_order(&state.db, row.id, &gid).await? {
        // A concurrent request attached it first, or the order stopped being payable.
        let now = repo::orders::find(&state.db, &OrderRef::Id(row.id))
            .await?
            .ok_or(AppError::NotFound)?;
        let window_open = repo::payments::payment_window_open(&state.db, row.id).await?;
        payability(&now, window_open)?;
        if now.gateway_order_id.as_deref() != Some(gid.as_str()) {
            tracing::error!(order_number = %row.order_number, "a different gateway order id is attached");
            return Err(AppError::PaymentGateway);
        }
    }

    let cf = service::ensure_gateway_order(&state, &row, &gid, attached_before).await?;
    match cf.order_status.as_str() {
        "ACTIVE" => {}
        "PAID" => {
            // Paid but not settled here yet (verify/webhook still on the way).
            let s = service::settle_order(&state, row.id, Source::Pay).await?;
            return Err(match s.outcome {
                SettleOutcome::Pending | SettleOutcome::NotPaid => AppError::PaymentGateway,
                other => settled_answer(other).unwrap_or(ORDER_ALREADY_PAID),
            });
        }
        status => {
            tracing::warn!(order_number = %row.order_number, cf_status = status, "cashfree order is not payable");
            return Err(AppError::PaymentGateway);
        }
    }
    let Some(session) = cf
        .payment_session_id
        .filter(|s| !s.is_empty() && s.len() <= MAX_SESSION_ID_LEN && s.is_ascii())
    else {
        tracing::error!(order_number = %row.order_number, "cashfree order without a usable payment session");
        return Err(AppError::PaymentGateway);
    };
    Ok(no_store(PayResponse {
        gateway: GATEWAY,
        payment_session_id: session,
        gateway_order_id: gid,
        amount_paise: row.total_paise,
        currency: "INR",
        mode: state.config.cashfree_env.as_str(),
        payment_expires_at: row.payment_expires_at,
        test_mode: row.is_test,
    }))
}

/// `{}` (or no body at all). Any other JSON is refused: the client has nothing
/// to tell us — the outcome is read from Cashfree.
fn check_verify_body(body: &[u8]) -> AppResult<()> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) if m.is_empty() => Ok(()),
        _ => Err(AppError::Validation("request body must be {}")),
    }
}

/// POST /orders/{orderNumber}/pay/verify — after checkout (and while polling):
/// re-read the order's payments from Cashfree and settle a successful one.
/// Idempotent; safe to call repeatedly.
async fn verify(
    State(state): State<AppState>,
    Path(order_number): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, PayError> {
    let row = load_guest_order(&state, &order_number, &headers).await?;
    require_payments_enabled(&state)?;
    check_verify_body(&body)?;

    // Fast path: a payment is already booked on the order.
    if row.gateway_payment_id.is_some() {
        if row.status == "cancelled" {
            // Paid too late / after cancellation and refunded.
            return Err(if service::cancelled_by_expiry(&row) {
                ORDER_EXPIRED
            } else {
                ORDER_NOT_PAYABLE
            }
            .into());
        }
        return Ok(no_store(guest_view(&state, row).await?));
    }
    if row.payment_method != METHOD_CASHFREE {
        return Err(ORDER_NOT_PAYABLE.into());
    }
    if row.gateway_order_id.is_none() {
        // Checkout never started: nothing to look up at Cashfree.
        let window_open = repo::payments::payment_window_open(&state.db, row.id).await?;
        payability(&row, window_open)?;
        return Err(PayError::NotCompleted);
    }

    match service::settle_order(&state, row.id, Source::Checkout).await {
        Ok(s) => match s.outcome {
            SettleOutcome::Paid | SettleOutcome::AlreadyPaid | SettleOutcome::DuplicatePayment => {
                Ok(no_store(reload_guest_view(&state, s.order_id).await?))
            }
            SettleOutcome::Pending => Err(PAYMENT_PENDING.into()),
            SettleOutcome::NotPaid => {
                // Nothing paid: say why it cannot be retried, if it cannot.
                let now = repo::orders::find(&state.db, &OrderRef::Id(row.id))
                    .await?
                    .ok_or(AppError::NotFound)?;
                let window_open = repo::payments::payment_window_open(&state.db, row.id).await?;
                payability(&now, window_open)?;
                Err(PayError::NotCompleted)
            }
            other => Err(settled_answer(other).unwrap_or(ORDER_NOT_PAYABLE).into()),
        },
        Err(SettleError::Mismatch(_)) => Err(PayError::VerificationFailed),
        Err(SettleError::UnknownOrder) => Err(AppError::NotFound.into()),
        Err(e) => Err(PayError::App(e.into())),
    }
}

/// POST /webhooks/cashfree
async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    // Disabled: the endpoint does not exist (nothing verified, processed or recorded).
    if !state.config.payments_enabled {
        return Err(AppError::NotFound);
    }
    let header = |name: &str| {
        let mut values = headers.get_all(name).iter();
        match (values.next(), values.next()) {
            (Some(v), None) => v.to_str().ok().map(str::trim),
            _ => None,
        }
    };
    let signature_ok = match (
        header(WEBHOOK_TIMESTAMP_HEADER),
        header(WEBHOOK_SIGNATURE_HEADER),
    ) {
        (Some(ts), Some(sig)) => signature::verify_webhook(
            state.config.cashfree_secret_key.expose_secret().as_bytes(),
            ts,
            &body,
            sig,
        ),
        _ => false,
    };
    if !signature_ok {
        tracing::warn!(
            body_len = body.len(),
            "cashfree webhook with missing or invalid signature"
        );
        return Err(AppError::Validation("invalid webhook signature"));
    }

    let event_id = service::event_id(&body);
    service::handle_webhook(&state, &event_id, &body).await?;
    Ok(no_store(json!({ "status": "ok" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_body_must_be_empty() {
        assert!(check_verify_body(b"").is_ok());
        assert!(check_verify_body(b"  \n").is_ok());
        assert!(check_verify_body(b"{}").is_ok());
        assert!(check_verify_body(b" { } ").is_ok());
        assert!(check_verify_body(br#"{"paid":true}"#).is_err());
        assert!(check_verify_body(b"[]").is_err());
        assert!(check_verify_body(b"nope").is_err());
    }

    #[test]
    fn order_number_format() {
        assert!(valid_order_number("ZM-261013-1001"));
        assert!(!valid_order_number("ZM-26101-1001"));
        assert!(!valid_order_number("ZM-261013-"));
        assert!(!valid_order_number("zm-261013-1001"));
        assert!(!valid_order_number("ZM-261013-10a1"));
        assert!(!valid_order_number(&format!(
            "ZM-261013-{}",
            "1".repeat(20)
        )));
    }
}
