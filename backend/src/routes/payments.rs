//! Guest payment endpoints and the Razorpay webhook.
//!
//! Guests have no account: the `X-Order-Token` header (see
//! `payments::order_token`) authorizes pay / verify / status for one order. A
//! missing or wrong token is answered exactly like an unknown order (404), and
//! the token is checked before anything about the order is revealed.

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
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    money::serialize_rupees,
    payments::{
        order_token,
        service::{self, SettleError, SettleOutcome, Source},
        signature,
    },
    repo::{
        self,
        orders::{OrderItem, OrderRef, OrderRow, Shipping},
    },
    routes::{catalog::PricingInfo, json::ApiJson},
    state::AppState,
};

/// Header carrying the guest order token.
pub const ORDER_TOKEN_HEADER: &str = "x-order-token";
pub const WEBHOOK_SIGNATURE_HEADER: &str = "x-razorpay-signature";
pub const WEBHOOK_EVENT_ID_HEADER: &str = "x-razorpay-event-id";
/// Webhook bodies are small JSON documents.
pub const MAX_WEBHOOK_BODY_BYTES: usize = 64 * 1024;

/// Public guest payment endpoints: GET /storefront/config, POST /orders/{orderNumber}/pay,
/// POST /orders/{orderNumber}/pay/verify, GET /orders/{orderNumber}.
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/storefront/config", get(storefront_config))
        .route("/orders/{order_number}", get(guest_order))
        .route("/orders/{order_number}/pay", post(pay))
        .route("/orders/{order_number}/pay/verify", post(verify))
}

/// POST /webhooks/razorpay. Authenticated by the HMAC signature only; mounted
/// outside the per-IP limiters by app.rs.
pub fn webhook_routes() -> Router<AppState> {
    Router::new()
        .route("/webhooks/razorpay", post(webhook))
        .layer(DefaultBodyLimit::max(MAX_WEBHOOK_BODY_BYTES))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// `AppError` plus `400 payment_verification_failed`.
#[derive(Debug)]
enum PayError {
    App(AppError),
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
        match self {
            PayError::App(e) => e.into_response(),
            PayError::VerificationFailed => {
                let body = json!({ "error": {
                    "code": "payment_verification_failed",
                    "message": "We could not verify this payment. If money was deducted, it will be confirmed or refunded automatically.",
                }});
                let mut res = (StatusCode::BAD_REQUEST, Json(body)).into_response();
                res.headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                res
            }
        }
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
/// `RAZORPAY_ENABLED=false`: orders are pay-later and settled by an admin.
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
    /// `null` while online payments are disabled (the key may be unset).
    razorpay_key_id: Option<&'a str>,
    currency: &'static str,
    payment_expiry_minutes: u64,
    store_name: &'a str,
    pricing: PricingInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuestPayment {
    /// `pay_later` | `razorpay` | … (the storefront keys its "order received" view on it).
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Prefill {
    name: String,
    email: Option<String>,
    contact: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckoutNotes {
    order_number: String,
}

/// Options for Razorpay Checkout (`new Razorpay({...})`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PayResponse {
    key_id: String,
    razorpay_order_id: String,
    amount_paise: i64,
    currency: &'static str,
    name: String,
    description: String,
    prefill: Prefill,
    notes: CheckoutNotes,
    payment_expires_at: Option<DateTime<Utc>>,
    test_mode: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct VerifyRequest {
    razorpay_order_id: String,
    razorpay_payment_id: String,
    razorpay_signature: String,
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
    let mut res = Json(StorefrontConfig {
        test_mode: c.payment_test_mode,
        online_payments_enabled: c.payments_enabled,
        razorpay_key_id: (c.payments_enabled && !c.razorpay_key_id.is_empty())
            .then_some(c.razorpay_key_id.as_str()),
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
        || row.razorpay_payment_id.is_some();
    match row.status.as_str() {
        "pending_payment" if money_in => Err(ORDER_ALREADY_PAID),
        "pending_payment" if window_open => Ok(()),
        "pending_payment" => Err(ORDER_EXPIRED),
        "cancelled" if service::cancelled_by_expiry(row) => Err(ORDER_EXPIRED),
        "cancelled" => Err(ORDER_NOT_PAYABLE),
        _ if money_in => Err(ORDER_ALREADY_PAID),
        _ => Err(ORDER_NOT_PAYABLE),
    }
}

fn pay_response(state: &AppState, row: &OrderRow, rzp_order_id: String) -> PayResponse {
    PayResponse {
        key_id: state.config.razorpay_key_id.clone(),
        razorpay_order_id: rzp_order_id,
        amount_paise: row.total_paise,
        currency: "INR",
        name: state.config.store_name.clone(),
        description: format!("Order {}", row.order_number),
        prefill: Prefill {
            name: row.customer_name.clone(),
            email: row.customer_email.clone(),
            contact: row.customer_phone.clone(),
        },
        notes: CheckoutNotes {
            order_number: row.order_number.clone(),
        },
        payment_expires_at: row.payment_expires_at,
        test_mode: row.is_test,
    }
}

/// POST /orders/{orderNumber}/pay — create (once) or reuse the Razorpay order.
///
/// Race handling (two tabs / double click): the order is checked under its row
/// lock, the lock is released, the Razorpay order is created WITHOUT holding
/// any lock, and the id is attached with a guarded
/// `UPDATE … WHERE razorpay_order_id IS NULL`. The loser of a race uses the
/// winner's id; its own Razorpay order is never shown to anyone and simply
/// lapses (no money can move on it).
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

    if let Some(existing) = row.razorpay_order_id.clone() {
        return Ok(no_store(pay_response(&state, &row, existing)));
    }

    let created = state
        .razorpay
        .create_order(row.total_paise, row.id, &row.order_number)
        .await?;
    if !signature::is_order_id(&created.id)
        || created.amount != row.total_paise
        || created.currency != "INR"
    {
        tracing::error!(order_number = %row.order_number, "razorpay order response does not match the request");
        return Err(AppError::PaymentGateway);
    }

    if repo::payments::attach_razorpay_order(&state.db, row.id, &created.id).await? {
        tracing::info!(order_number = %row.order_number, razorpay_order_id = %created.id, "razorpay order created");
        return Ok(no_store(pay_response(&state, &row, created.id)));
    }

    // Lost a race, or the order stopped being payable meanwhile.
    let now = repo::orders::find(&state.db, &OrderRef::Id(row.id))
        .await?
        .ok_or(AppError::NotFound)?;
    let window_open = repo::payments::payment_window_open(&state.db, row.id).await?;
    payability(&now, window_open)?;
    match now.razorpay_order_id.clone() {
        Some(winner) => Ok(no_store(pay_response(&state, &now, winner))),
        None => Err(AppError::PaymentGateway),
    }
}

/// POST /orders/{orderNumber}/pay/verify — Checkout `handler` callback.
async fn verify(
    State(state): State<AppState>,
    Path(order_number): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<VerifyRequest>,
) -> Result<Response, PayError> {
    let row = load_guest_order(&state, &order_number, &headers).await?;
    require_payments_enabled(&state)?;

    if !signature::is_order_id(&body.razorpay_order_id)
        || !signature::is_payment_id(&body.razorpay_payment_id)
    {
        return Err(PayError::VerificationFailed);
    }
    let key_secret = state.config.razorpay_key_secret.expose_secret().as_bytes();
    if !signature::verify_checkout(
        key_secret,
        &body.razorpay_order_id,
        &body.razorpay_payment_id,
        &body.razorpay_signature,
    ) {
        tracing::warn!(order_number = %row.order_number, "checkout signature mismatch");
        return Err(PayError::VerificationFailed);
    }
    if row.razorpay_order_id.as_deref() != Some(body.razorpay_order_id.as_str()) {
        tracing::warn!(order_number = %row.order_number, "checkout razorpay order id does not belong to this order");
        return Err(PayError::VerificationFailed);
    }
    // Idempotent fast path: already settled with exactly this payment.
    if row.razorpay_payment_id.as_deref() == Some(body.razorpay_payment_id.as_str()) {
        return Ok(no_store(guest_view(&state, row).await?));
    }

    match service::settle_payment(
        &state,
        &body.razorpay_order_id,
        &body.razorpay_payment_id,
        Source::Checkout,
    )
    .await
    {
        Ok(s) => match s.outcome {
            SettleOutcome::Paid | SettleOutcome::AlreadyPaid | SettleOutcome::DuplicateRefunded => {
                Ok(no_store(reload_guest_view(&state, s.order_id).await?))
            }
            SettleOutcome::LateRefunded => Err(ORDER_EXPIRED.into()),
            SettleOutcome::NotPayableRefunded => Err(ORDER_NOT_PAYABLE.into()),
            SettleOutcome::NotCaptured => Err(PayError::VerificationFailed),
        },
        Err(SettleError::Mismatch(_) | SettleError::UnknownOrder) => {
            Err(PayError::VerificationFailed)
        }
        Err(e) => Err(PayError::App(e.into())),
    }
}

/// POST /webhooks/razorpay
async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    // Disabled: the endpoint does not exist (nothing verified, processed or recorded).
    if !state.config.payments_enabled {
        return Err(AppError::NotFound);
    }
    let signature_ok = headers
        .get(WEBHOOK_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|sig| {
            signature::verify_webhook(
                state
                    .config
                    .razorpay_webhook_secret
                    .expose_secret()
                    .as_bytes(),
                &body,
                sig.trim(),
            )
        });
    if !signature_ok {
        tracing::warn!(
            body_len = body.len(),
            "razorpay webhook with missing or invalid signature"
        );
        return Err(AppError::Validation("invalid webhook signature"));
    }

    let event_id = service::event_id(
        headers
            .get(WEBHOOK_EVENT_ID_HEADER)
            .and_then(|v| v.to_str().ok()),
        &body,
    );
    service::handle_webhook(&state, &event_id, &body).await?;
    Ok(no_store(json!({ "status": "ok" })))
}

#[cfg(test)]
mod tests {
    use super::*;

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
