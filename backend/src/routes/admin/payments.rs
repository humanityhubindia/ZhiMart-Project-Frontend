//! Admin payment operations: Razorpay refunds and the payment event log.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::AuthAdmin,
    error::{AppError, AppResult},
    money::{self, serialize_rupees},
    net::ClientMeta,
    payments::service::{self, RefundJob},
    repo::{
        self, audit,
        orders::{METHOD_RAZORPAY, Order, OrderRef},
        payments::NewRefund,
    },
    routes::{json::ApiJson, payments::ONLINE_PAYMENTS_DISABLED},
    state::AppState,
};

const MAX_REASON_CHARS: usize = 300;
const MAX_EVENTS: i64 = 200;

/// POST /orders/{id}/refund (Razorpay refund), GET /orders/{id}/payment-events.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders/{id}/refund", post(refund))
        .route("/orders/{id}/payment-events", get(payment_events))
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RefundRequest {
    /// Rupees; `null`/absent = everything not yet refunded.
    #[serde(default)]
    amount: Option<f64>,
    reason: String,
    expected_version: i32,
}

const VERSION_CONFLICT: AppError = AppError::Conflict {
    code: "version_conflict",
    message: "This order was changed by someone else. Refresh and try again.",
};
const NOT_REFUNDABLE: AppError = AppError::Conflict {
    code: "invalid_transition",
    message: "This order has no Razorpay payment left to refund.",
};
const EXCEEDS_REMAINING: AppError = AppError::Conflict {
    code: "refund_exceeds_remaining",
    message: "The refund amount is more than what is left to refund on this payment.",
};

/// POST /admin/orders/{id}/refund
async fn refund(
    admin: AuthAdmin,
    meta: ClientMeta,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    ApiJson(body): ApiJson<RefundRequest>,
) -> AppResult<Json<Order>> {
    let order_ref = parse_ref(&raw_id).ok_or(AppError::NotFound)?;
    let reason = body.reason.trim();
    let reason_len = reason.chars().count();
    if reason_len == 0 || reason_len > MAX_REASON_CHARS || reason.chars().any(char::is_control) {
        return Err(AppError::Validation("reason must be 1-300 characters"));
    }
    let requested = match body.amount {
        None => None,
        Some(rupees) => match money::from_rupees(rupees) {
            Some(p) if p > 0 => Some(p),
            _ => {
                return Err(AppError::Validation(
                    "amount must be a positive rupee amount with at most 2 decimals",
                ));
            }
        },
    };

    // Resolve earlier refunds whose outcome was never confirmed, so their
    // reservation is released (or recorded) before computing what is left.
    // (Skipped while online payments are disabled: no Razorpay calls then.)
    if state.config.payments_enabled
        && let Some(current) = repo::orders::find(&state.db, &order_ref).await?
    {
        service::reconcile_unconfirmed_refunds(&state, &current).await;
    }

    let mut tx = state.db.begin().await?;
    repo::admins::set_lock_timeout(&mut tx).await?;
    let order = repo::orders::find_for_update(&mut tx, &order_ref)
        .await?
        .ok_or(AppError::NotFound)?;
    if order.version != body.expected_version {
        return Err(VERSION_CONFLICT);
    }
    let Some(payment_id) = order.razorpay_payment_id.clone() else {
        return Err(NOT_REFUNDABLE);
    };
    if order.payment_method != METHOD_RAZORPAY || order.payment_status != "paid" {
        return Err(NOT_REFUNDABLE);
    }
    // A Razorpay-paid order from before payments were switched off: the
    // gateway may not be configured, so refund it from the Razorpay Dashboard.
    if !state.config.payments_enabled {
        return Err(ONLINE_PAYMENTS_DISABLED);
    }
    let reserved = repo::payments::refunds_reserved_total(&mut tx, &payment_id).await?;
    let remaining = order.total_paise - reserved;
    if remaining <= 0 {
        return Err(NOT_REFUNDABLE);
    }
    let amount = requested.unwrap_or(remaining);
    if amount > remaining {
        return Err(EXCEEDS_REMAINING);
    }

    let refund_id = Uuid::now_v7();
    repo::payments::insert_refund(
        &mut tx,
        &NewRefund {
            id: refund_id,
            order_id: order.id,
            razorpay_payment_id: &payment_id,
            amount_paise: amount,
            reason,
            initiated_by: Some(admin.admin_id),
        },
    )
    .await?;
    // A double click with the same expectedVersion now gets version_conflict.
    repo::payments::bump_version(&mut *tx, order.id).await?;
    tx.commit().await?;

    tracing::info!(
        admin_id = %admin.admin_id, order_number = %order.order_number, amount_paise = amount,
        refund_id = %refund_id, "admin refund initiated"
    );
    audit::spawn_record(
        &state.db,
        Some(admin.admin_id),
        "order_refund_initiated",
        &meta,
        json!({ "orderNumber": order.order_number, "amountPaise": amount, "refundId": refund_id }),
    );

    let job = RefundJob {
        refund_id,
        order_id: order.id,
        order_number: order.order_number.clone(),
        payment_id,
        amount_paise: amount,
    };
    if let Err(e) = service::execute_refund(&state, &job, false).await {
        tracing::warn!(
            admin_id = %admin.admin_id, order_number = %order.order_number, refund_id = %refund_id,
            error = %e.summary(), "admin refund not completed"
        );
        return Err(AppError::PaymentGateway);
    }

    let mut conn = state.db.acquire().await?;
    let full = repo::orders::load_full(&mut conn, &OrderRef::Id(order.id))
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(full))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventView {
    id: String,
    event: String,
    outcome: Option<String>,
    received_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefundView {
    id: Uuid,
    razorpay_refund_id: Option<String>,
    razorpay_payment_id: String,
    #[serde(serialize_with = "serialize_rupees")]
    amount: i64,
    status: String,
    reason: String,
    failure_reason: Option<String>,
    admin_email: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct PaymentEventsResponse {
    events: Vec<EventView>,
    refunds: Vec<RefundView>,
}

/// GET /admin/orders/{id}/payment-events
async fn payment_events(
    _admin: AuthAdmin,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> AppResult<Json<PaymentEventsResponse>> {
    let order_ref = parse_ref(&raw_id).ok_or(AppError::NotFound)?;
    let order = repo::orders::find(&state.db, &order_ref)
        .await?
        .ok_or(AppError::NotFound)?;
    let events = repo::payments::list_events(&state.db, order.id, MAX_EVENTS).await?;
    let refunds = repo::payments::list_refunds(&state.db, order.id).await?;
    Ok(Json(PaymentEventsResponse {
        events: events
            .into_iter()
            .map(|e| EventView {
                id: e.id,
                event: e.event,
                outcome: e.outcome,
                received_at: e.received_at,
            })
            .collect(),
        refunds: refunds
            .into_iter()
            .map(|r| RefundView {
                id: r.id,
                razorpay_refund_id: r.razorpay_refund_id,
                razorpay_payment_id: r.razorpay_payment_id,
                amount: r.amount_paise,
                status: r.status,
                reason: r.reason,
                failure_reason: r.failure_reason,
                admin_email: r.admin_email,
                created_at: r.created_at,
            })
            .collect(),
    }))
}
