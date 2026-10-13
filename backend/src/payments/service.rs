//! Payment service shared by the guest endpoints, the webhook and the admin
//! refund endpoint.
//!
//! Invariants:
//! * An order becomes paid only through [`settle_payment`], after the payment
//!   has been confirmed by Razorpay itself — fetched from the API (checkout
//!   verify path) or taken from a signature-verified webhook — with matching
//!   order id, amount and currency, and status `captured` (an `authorized`
//!   payment is captured first). A checkout signature alone is never enough.
//! * Every state change happens in one transaction holding the order row lock,
//!   so the checkout verify call and the webhook for the same payment (or any
//!   redelivery/double click) produce exactly one transition; the others observe
//!   the result and return "already paid".
//! * HTTP calls to Razorpay are never made while holding a row lock. Refunds
//!   are written to the `refunds` ledger (status `pending`) and committed
//!   BEFORE the API call, then completed; the refund row id is the Razorpay
//!   `receipt`/idempotency key.

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::AppError,
    payments::{
        razorpay::{GatewayError, RzpPayment, RzpRefund},
        signature,
    },
    repo::{
        self,
        orders::{EXPIRY_CANCEL_REASON, OrderRef, OrderRow, ReviveOutcome},
        payments::{NewEvent, NewRefund, REFUND_FAILED, REFUND_PENDING, REFUND_PROCESSED},
    },
    state::AppState,
};

/// Where a payment confirmation came from.
#[derive(Debug, Clone)]
pub enum Source {
    /// Checkout handler (`/pay/verify`): the payment is fetched from the API.
    Checkout,
    /// Signature-verified webhook: the signed payment entity is authoritative.
    Webhook(Box<RzpPayment>),
}

impl Source {
    fn label(&self) -> &'static str {
        match self {
            Source::Checkout => "checkout",
            Source::Webhook(_) => "webhook",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleOutcome {
    /// This call moved the order to paid / `pending_pack`.
    Paid,
    /// The order was already paid with this payment (idempotent no-op).
    AlreadyPaid,
    /// The order was already paid with a different payment; this one is refunded.
    DuplicateRefunded,
    /// Paid after the order expired and stock is gone; refunded (`order_expired`).
    LateRefunded,
    /// Order cancelled (not by expiry) or otherwise not payable; refunded.
    NotPayableRefunded,
    /// Payment is not captured/authorized (failed, created, refunded): nothing changed.
    NotCaptured,
}

impl SettleOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            SettleOutcome::Paid => "paid",
            SettleOutcome::AlreadyPaid => "already_paid",
            SettleOutcome::DuplicateRefunded => "duplicate_payment_refunded",
            SettleOutcome::LateRefunded => "late_payment_refunded",
            SettleOutcome::NotPayableRefunded => "not_payable_refunded",
            SettleOutcome::NotCaptured => "payment_not_captured",
        }
    }
}

#[derive(Debug)]
pub enum SettleError {
    /// The Razorpay order is not one of ours.
    UnknownOrder,
    /// Malformed ids, or the payment does not match the order (order id,
    /// amount, currency). Treat as unpaid.
    Mismatch(&'static str),
    Gateway(GatewayError),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for SettleError {
    fn from(e: sqlx::Error) -> Self {
        SettleError::Db(e)
    }
}

impl From<GatewayError> for SettleError {
    fn from(e: GatewayError) -> Self {
        SettleError::Gateway(e)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Settlement {
    pub order_id: Uuid,
    pub outcome: SettleOutcome,
}

/// Instrument as reported by Razorpay (`upi`, `card`, …), if it looks sane.
fn instrument(p: &RzpPayment) -> Option<String> {
    p.method
        .as_deref()
        .filter(|m| {
            !m.is_empty()
                && m.len() <= 30
                && m.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .map(str::to_ascii_lowercase)
}

/// Check a Razorpay payment against the order it claims to pay.
fn check_payment(
    p: &RzpPayment,
    payment_id: &str,
    rzp_order_id: &str,
    total_paise: i64,
) -> Result<(), SettleError> {
    if p.id != payment_id {
        return Err(SettleError::Mismatch("payment id mismatch"));
    }
    if p.order_id.as_deref() != Some(rzp_order_id) {
        return Err(SettleError::Mismatch("order id mismatch"));
    }
    if p.currency != "INR" {
        return Err(SettleError::Mismatch("currency mismatch"));
    }
    if p.amount != total_paise {
        return Err(SettleError::Mismatch("amount mismatch"));
    }
    Ok(())
}

/// Settle a Razorpay payment against our order. Idempotent and safe to call
/// concurrently for the same payment from the verify endpoint and the webhook.
pub async fn settle_payment(
    state: &AppState,
    rzp_order_id: &str,
    payment_id: &str,
    source: Source,
) -> Result<Settlement, SettleError> {
    if !signature::is_order_id(rzp_order_id) || !signature::is_payment_id(payment_id) {
        return Err(SettleError::Mismatch("malformed razorpay id"));
    }
    let order_id = repo::payments::order_id_by_razorpay_order(&state.db, rzp_order_id)
        .await?
        .ok_or(SettleError::UnknownOrder)?;
    let order = repo::orders::find(&state.db, &OrderRef::Id(order_id))
        .await?
        .ok_or(SettleError::UnknownOrder)?;
    let source_label = source.label();

    let payment = match source {
        Source::Checkout => match state.razorpay.fetch_payment(payment_id).await {
            Ok(p) => p,
            // Razorpay does not know this payment id (400/404): the browser
            // relayed something that is not a real payment.
            Err(GatewayError::Rejected {
                status: 400 | 404, ..
            }) => {
                return Err(SettleError::Mismatch("payment not found at Razorpay"));
            }
            Err(e) => return Err(e.into()),
        },
        Source::Webhook(p) => *p,
    };
    if let Err(e) = check_payment(&payment, payment_id, rzp_order_id, order.total_paise) {
        if let SettleError::Mismatch(why) = &e {
            tracing::error!(
                order_number = %order.order_number, payment_id, source = source_label, reason = why,
                "razorpay payment does not match the order"
            );
            if matches!(payment.status.as_str(), "captured" | "authorized")
                && payment.id == payment_id
            {
                let note = format!(
                    "Payment {payment_id} does not match this order ({why}); review it in the Razorpay Dashboard"
                );
                repo::payments::set_attention(&state.db, order_id, &note).await?;
            }
        }
        return Err(e);
    }

    let payment = match payment.status.as_str() {
        "captured" => payment,
        "authorized" => {
            let captured = capture(state, &payment).await?;
            check_payment(&captured, payment_id, rzp_order_id, order.total_paise)?;
            captured
        }
        _ => {
            let outcome = if order.razorpay_payment_id.as_deref() == Some(payment_id) {
                SettleOutcome::AlreadyPaid
            } else {
                SettleOutcome::NotCaptured
            };
            return Ok(Settlement { order_id, outcome });
        }
    };

    let (outcome, refund) = settle_locked(state, order_id, &payment).await?;
    match outcome {
        SettleOutcome::Paid => tracing::info!(
            order_number = %order.order_number, payment_id, source = source_label, "order paid"
        ),
        SettleOutcome::AlreadyPaid => tracing::debug!(
            order_number = %order.order_number, payment_id, source = source_label, "payment already settled"
        ),
        other => tracing::warn!(
            order_number = %order.order_number, payment_id, source = source_label, outcome = other.as_str(),
            "payment cannot be applied to the order; refunding"
        ),
    }

    if let Some(job) = refund {
        // Failures are recorded on the refund row and flagged for attention;
        // the outcome for the caller is the same either way.
        if let Err(e) = execute_refund(state, &job, true).await {
            tracing::error!(
                order_number = %job.order_number, payment_id, error = %e.summary(), "automatic refund did not complete"
            );
        }
    }
    Ok(Settlement { order_id, outcome })
}

/// Capture an `authorized` payment. If the capture call fails (e.g. Razorpay
/// auto-captured it meanwhile), re-fetch and accept it if it is now captured.
async fn capture(state: &AppState, p: &RzpPayment) -> Result<RzpPayment, SettleError> {
    match state.razorpay.capture_payment(&p.id, p.amount).await {
        Ok(c) if c.status == "captured" => Ok(c),
        Ok(_) | Err(_) => {
            let again = state.razorpay.fetch_payment(&p.id).await?;
            if again.status == "captured" {
                Ok(again)
            } else {
                tracing::warn!(payment_id = %p.id, status = %again.status, "payment could not be captured");
                Err(SettleError::Gateway(GatewayError::Unavailable))
            }
        }
    }
}

/// A refund recorded in the ledger, to be sent to Razorpay.
#[derive(Debug, Clone)]
pub struct RefundJob {
    pub refund_id: Uuid,
    pub order_id: Uuid,
    pub order_number: String,
    pub payment_id: String,
    pub amount_paise: i64,
}

/// Queue the admin "new order" email inside the transaction that moves the
/// order to paid (only the transaction holding the row lock that performs the
/// transition gets here; the outbox's unique index makes it idempotent anyway).
async fn enqueue_order_notification(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    order_id: Uuid,
) -> Result<(), SettleError> {
    repo::outbox::enqueue_order_emails(&mut **tx, &state.config, order_id).await?;
    Ok(())
}

/// The locked part of settlement: decide and apply the transition.
async fn settle_locked(
    state: &AppState,
    order_id: Uuid,
    payment: &RzpPayment,
) -> Result<(SettleOutcome, Option<RefundJob>), SettleError> {
    let pid = payment.id.as_str();
    let instrument = instrument(payment);
    let mut tx = state.db.begin().await?;
    repo::admins::set_lock_timeout(&mut tx).await?;
    let row = repo::orders::find_for_update(&mut tx, &OrderRef::Id(order_id))
        .await?
        .ok_or(SettleError::UnknownOrder)?;

    let mut refund_reason: Option<&'static str> = None;
    let outcome = if row.razorpay_payment_id.as_deref() == Some(pid) {
        SettleOutcome::AlreadyPaid
    } else if let Some(existing) = row.razorpay_payment_id.as_deref() {
        let note = format!(
            "Duplicate payment {pid} received (order already paid by {existing}); automatic refund issued"
        );
        repo::payments::set_attention(&mut *tx, order_id, &note).await?;
        refund_reason =
            Some("Automatic refund: duplicate payment for an order that was already paid");
        SettleOutcome::DuplicateRefunded
    } else if row.status == "pending_payment" {
        // Even if the window has just passed, the sweep has not released the
        // stock yet (the order would be `cancelled`), so the payment is honoured.
        if !repo::payments::mark_paid(&mut tx, order_id, pid, instrument.as_deref()).await? {
            return Err(SettleError::Db(sqlx::Error::RowNotFound));
        }
        enqueue_order_notification(state, &mut tx, order_id).await?;
        SettleOutcome::Paid
    } else if row.status == "cancelled" {
        match repo::orders::revive_expired_order(&mut tx, order_id).await? {
            ReviveOutcome::Revived => {
                if !repo::payments::mark_paid(&mut tx, order_id, pid, instrument.as_deref()).await?
                {
                    return Err(SettleError::Db(sqlx::Error::RowNotFound));
                }
                enqueue_order_notification(state, &mut tx, order_id).await?;
                tracing::info!(order_number = %row.order_number, "expired order revived by a late payment");
                SettleOutcome::Paid
            }
            ReviveOutcome::InsufficientStock => {
                let note = "Payment received after the order expired and the stock was no longer available; automatic refund issued";
                repo::payments::record_unfulfillable_payment(
                    &mut tx,
                    order_id,
                    pid,
                    instrument.as_deref(),
                    note,
                )
                .await?;
                refund_reason = Some(
                    "Automatic refund: payment received after the order expired (stock unavailable)",
                );
                SettleOutcome::LateRefunded
            }
            ReviveOutcome::NotExpired => {
                let note = "Payment received for a cancelled order; automatic refund issued";
                repo::payments::record_unfulfillable_payment(
                    &mut tx,
                    order_id,
                    pid,
                    instrument.as_deref(),
                    note,
                )
                .await?;
                refund_reason = Some("Automatic refund: payment received for a cancelled order");
                SettleOutcome::NotPayableRefunded
            }
        }
    } else {
        // Not awaiting an online payment (e.g. settled by another method).
        let note = format!(
            "Unexpected payment {pid} for an order in status {}; automatic refund issued",
            row.status
        );
        repo::payments::set_attention(&mut *tx, order_id, &note).await?;
        refund_reason =
            Some("Automatic refund: payment received for an order that was not awaiting payment");
        SettleOutcome::NotPayableRefunded
    };

    // Queue the refund unless one for this payment is already in the ledger
    // (concurrent verify + webhook, or a redelivery).
    let mut job = None;
    if let Some(reason) = refund_reason {
        let reserved = repo::payments::refunds_reserved_total(&mut tx, pid).await?;
        let amount = payment.amount - reserved;
        if amount > 0 {
            let refund_id = Uuid::now_v7();
            repo::payments::insert_refund(
                &mut tx,
                &NewRefund {
                    id: refund_id,
                    order_id,
                    razorpay_payment_id: pid,
                    amount_paise: amount,
                    reason,
                    initiated_by: None,
                },
            )
            .await?;
            job = Some(RefundJob {
                refund_id,
                order_id,
                order_number: row.order_number.clone(),
                payment_id: pid.to_owned(),
                amount_paise: amount,
            });
        }
    }
    tx.commit().await?;
    Ok((outcome, job))
}

/// Send a ledger refund to Razorpay and record the result.
///
/// * accepted ⇒ refund id stored, order totals recomputed;
/// * rejected (4xx) ⇒ refund `failed` with the reason;
/// * no answer ⇒ reconcile by listing the payment's refunds; if still unknown
///   the row stays `pending` (its amount stays reserved, so it can never be
///   refunded twice) until a `refund.*` webhook or a later reconciliation.
///
/// `automatic` refunds flag the order for attention when they fail.
pub async fn execute_refund(
    state: &AppState,
    job: &RefundJob,
    automatic: bool,
) -> Result<RzpRefund, GatewayError> {
    let result = state
        .razorpay
        .refund_payment(
            &job.payment_id,
            job.amount_paise,
            job.refund_id,
            &job.order_number,
        )
        .await;
    let err = match result {
        Ok(r) => return apply_gateway_refund(state, job, r).await,
        Err(e) => e,
    };

    match &err {
        GatewayError::Rejected { .. } => {
            let summary = err.summary();
            if let Err(e) = repo::payments::fail_refund(&state.db, job.refund_id, &summary).await {
                tracing::error!(refund_id = %job.refund_id, error = %e, "could not mark refund failed");
            }
            let note = format!(
                "Automatic refund of payment {} failed; refund it manually",
                job.payment_id
            );
            flag_or_bump(state, job, automatic.then_some(note.as_str())).await;
            Err(err)
        }
        GatewayError::Unavailable => {
            if let Ok(found) = reconcile_one(state, job).await {
                return Ok(found);
            }
            let note = "Razorpay did not confirm this refund yet; awaiting confirmation";
            if let Err(e) = repo::payments::note_refund(&state.db, job.refund_id, note).await {
                tracing::error!(refund_id = %job.refund_id, error = %e, "could not annotate refund");
            }
            let note = format!(
                "Automatic refund of payment {} is unconfirmed; check the Razorpay Dashboard",
                job.payment_id
            );
            flag_or_bump(state, job, automatic.then_some(note.as_str())).await;
            Err(err)
        }
    }
}

/// After a refund failed or went unconfirmed: flag the order for attention
/// (automatic refunds) or just bump its version so the admin console refreshes.
async fn flag_or_bump(state: &AppState, job: &RefundJob, attention: Option<&str>) {
    let r = match attention {
        Some(note) => repo::payments::set_attention(&state.db, job.order_id, note).await,
        None => repo::payments::bump_version(&state.db, job.order_id).await,
    };
    if let Err(e) = r {
        tracing::error!(order_id = %job.order_id, error = %e, "could not update order after refund failure");
    }
}

/// Look for our refund among the payment's refunds at Razorpay.
async fn reconcile_one(state: &AppState, job: &RefundJob) -> Result<RzpRefund, GatewayError> {
    let refunds = state
        .razorpay
        .fetch_payment_refunds(&job.payment_id)
        .await?;
    match refunds
        .into_iter()
        .find(|r| r.our_refund_id() == Some(job.refund_id))
    {
        Some(r) => apply_gateway_refund(state, job, r).await,
        None => Err(GatewayError::Unavailable),
    }
}

fn refund_status(s: &str) -> &'static str {
    match s {
        "processed" => REFUND_PROCESSED,
        "failed" => REFUND_FAILED,
        _ => REFUND_PENDING,
    }
}

/// Store a refund entity Razorpay returned for one of our ledger rows.
async fn apply_gateway_refund(
    state: &AppState,
    job: &RefundJob,
    r: RzpRefund,
) -> Result<RzpRefund, GatewayError> {
    if !signature::is_refund_id(&r.id)
        || r.payment_id != job.payment_id
        || r.amount != job.amount_paise
    {
        tracing::error!(refund_id = %job.refund_id, "razorpay refund response does not match the request");
        return Err(GatewayError::Unavailable);
    }
    let status = refund_status(&r.status);
    let failure = (status == REFUND_FAILED).then_some("Razorpay reported the refund as failed");
    let stored = async {
        let mut tx = state.db.begin().await?;
        repo::admins::set_lock_timeout(&mut tx).await?;
        repo::orders::find_for_update(&mut tx, &OrderRef::Id(job.order_id)).await?;
        repo::payments::apply_refund_status(&mut tx, job.refund_id, &r.id, status, failure).await?;
        if !repo::payments::sync_refund_totals(&mut tx, job.order_id).await? {
            repo::payments::bump_version(&mut *tx, job.order_id).await?;
        }
        tx.commit().await
    }
    .await;
    if let Err(e) = stored {
        // Razorpay accepted the refund; the refund.processed webhook (which
        // carries our id in `receipt`/`notes`) will complete the bookkeeping.
        tracing::error!(refund_id = %job.refund_id, error = %e, "refund accepted by Razorpay but not recorded");
        return Err(GatewayError::Unavailable);
    }
    tracing::info!(
        order_number = %job.order_number, refund_id = %job.refund_id, razorpay_refund_id = %r.id,
        amount_paise = job.amount_paise, status, "refund accepted by Razorpay"
    );
    if status == REFUND_FAILED {
        return Err(GatewayError::Rejected {
            status: 200,
            code: "REFUND_FAILED".into(),
            description: "Razorpay reported the refund as failed".into(),
        });
    }
    Ok(r)
}

/// Before a new admin refund: resolve refunds whose outcome was never
/// confirmed. Found at Razorpay ⇒ recorded; unknown to Razorpay for 15+
/// minutes ⇒ marked failed (the request never arrived). Best effort.
pub async fn reconcile_unconfirmed_refunds(state: &AppState, order: &OrderRow) {
    if !state.config.payments_enabled {
        return;
    }
    let rows = match repo::payments::unconfirmed_refunds(&state.db, order.id).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!(error = %e, "could not load unconfirmed refunds");
            return;
        }
    };
    for row in rows {
        let job = RefundJob {
            refund_id: row.id,
            order_id: order.id,
            order_number: order.order_number.clone(),
            payment_id: row.razorpay_payment_id.clone(),
            amount_paise: row.amount_paise,
        };
        match state
            .razorpay
            .fetch_payment_refunds(&row.razorpay_payment_id)
            .await
        {
            Ok(list) => match list.into_iter().find(|r| r.our_refund_id() == Some(row.id)) {
                Some(r) => {
                    let _ = apply_gateway_refund(state, &job, r).await;
                }
                None if row.stale => {
                    repo::payments::fail_refund(
                        &state.db,
                        row.id,
                        "Never reached Razorpay (reconciled)",
                    )
                    .await
                    .unwrap_or_else(
                        |e| tracing::warn!(error = %e, "could not mark stale refund failed"),
                    );
                    flag_or_bump(state, &job, None).await;
                }
                None => {}
            },
            Err(_) => return,
        }
    }
}

// ---------------------------------------------------------------------------
// Webhook
// ---------------------------------------------------------------------------

/// Result of handling a verified webhook delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookResult {
    /// Already fully processed earlier; acknowledged without reprocessing.
    Duplicate,
    /// Processed now (outcome recorded on the event row).
    Processed(String),
}

fn entity<T: serde::de::DeserializeOwned>(payload: &Value, key: &str) -> Option<T> {
    payload
        .get(key)?
        .get("entity")
        .and_then(|e| serde_json::from_value(e.clone()).ok())
}

/// Event id: Razorpay's `X-Razorpay-Event-Id` when it is well-formed, else a
/// stable id derived from the body hash (so redeliveries still deduplicate).
pub fn event_id(header: Option<&str>, body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    match header.map(str::trim) {
        Some(h)
            if (1..=100).contains(&h.len())
                && h.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') =>
        {
            h.to_owned()
        }
        _ => format!("body_{}", signature::hex_encode(&Sha256::digest(body))),
    }
}

/// Only ids, amounts and statuses are kept — never contact details.
fn redacted_payload(
    event: &str,
    created_at: Option<i64>,
    p: Option<&RzpPayment>,
    r: Option<&RzpRefund>,
    order_entity: Option<&Value>,
) -> Value {
    json!({
        "event": event,
        "createdAt": created_at,
        "payment": p.map(|p| json!({
            "id": p.id, "orderId": p.order_id, "amount": p.amount, "currency": p.currency,
            "status": p.status, "method": p.method, "errorCode": p.error_code, "errorReason": p.error_reason,
        })),
        "refund": r.map(|r| json!({ "id": r.id, "paymentId": r.payment_id, "amount": r.amount, "status": r.status })),
        "order": order_entity.map(|o| json!({ "id": o.get("id"), "amount": o.get("amount"), "status": o.get("status") })),
    })
}

/// Handle a webhook whose signature has ALREADY been verified by the caller.
///
/// Errors are transient (database / Razorpay unavailable): the event row stays
/// unprocessed and the caller answers 5xx so Razorpay redelivers.
pub async fn handle_webhook(
    state: &AppState,
    event_id: &str,
    body: &[u8],
) -> Result<WebhookResult, AppError> {
    let parsed: Value = serde_json::from_slice(body)
        .map_err(|_| AppError::Validation("webhook body is not valid JSON"))?;
    let event = parsed
        .get("event")
        .and_then(Value::as_str)
        .filter(|e| (1..=100).contains(&e.len()))
        .ok_or(AppError::Validation("webhook event is missing"))?
        .to_owned();
    let created_at = parsed.get("created_at").and_then(Value::as_i64);
    let empty = Value::Null;
    let payload = parsed.get("payload").unwrap_or(&empty);

    let payment: Option<RzpPayment> = entity(payload, "payment");
    let refund: Option<RzpRefund> = entity(payload, "refund");
    let order_entity = payload.get("order").and_then(|o| o.get("entity"));

    let rzp_order_id = payment
        .as_ref()
        .and_then(|p| p.order_id.clone())
        .or_else(|| {
            order_entity
                .and_then(|o| o.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|s| signature::is_order_id(s));
    let payment_id = payment
        .as_ref()
        .map(|p| p.id.clone())
        .or_else(|| refund.as_ref().map(|r| r.payment_id.clone()))
        .filter(|s| signature::is_payment_id(s));

    let mut order_id = match &rzp_order_id {
        Some(o) => repo::payments::order_id_by_razorpay_order(&state.db, o).await?,
        None => None,
    };
    if order_id.is_none() {
        if let Some(r) = refund.as_ref().filter(|r| signature::is_refund_id(&r.id)) {
            order_id = repo::payments::find_refund_by_razorpay_id(&state.db, &r.id)
                .await?
                .map(|row| row.order_id);
        }
    }
    if order_id.is_none() {
        if let Some(p) = &payment_id {
            order_id = repo::payments::order_id_by_razorpay_payment(&state.db, p).await?;
        }
    }

    let new_event = NewEvent {
        id: event_id,
        event: &event,
        order_id,
        razorpay_order_id: rzp_order_id.as_deref(),
        razorpay_payment_id: payment_id.as_deref(),
        payload: redacted_payload(
            &event,
            created_at,
            payment.as_ref(),
            refund.as_ref(),
            order_entity,
        ),
    };
    if repo::payments::record_event(&state.db, &new_event).await? {
        tracing::debug!(event = %event, "duplicate webhook delivery acknowledged");
        return Ok(WebhookResult::Duplicate);
    }

    let processed = process_event(
        state,
        &event,
        payment,
        refund,
        rzp_order_id.as_deref(),
        order_id,
    )
    .await;
    match processed {
        Ok((outcome, resolved_order)) => {
            repo::payments::finish_event(
                &state.db,
                event_id,
                &outcome,
                true,
                resolved_order.or(order_id),
            )
            .await?;
            tracing::info!(event = %event, outcome = %outcome, "webhook processed");
            Ok(WebhookResult::Processed(outcome))
        }
        Err(e) => {
            let label = match &e {
                AppError::PaymentGateway => "error: payment gateway unavailable",
                _ => "error: processing failed",
            };
            if let Err(db) =
                repo::payments::finish_event(&state.db, event_id, label, false, order_id).await
            {
                tracing::error!(error = %db, "could not record webhook failure");
            }
            tracing::warn!(event = %event, "webhook processing failed; Razorpay will redeliver");
            Err(e)
        }
    }
}

/// Returns `(outcome, order id)`; `Err` only for transient failures.
async fn process_event(
    state: &AppState,
    event: &str,
    payment: Option<RzpPayment>,
    refund: Option<RzpRefund>,
    rzp_order_id: Option<&str>,
    order_id: Option<Uuid>,
) -> Result<(String, Option<Uuid>), AppError> {
    match event {
        "payment.captured" | "payment.authorized" | "order.paid" => {
            let (Some(p), Some(oid)) = (payment, rzp_order_id) else {
                return Ok(("ignored: payload without payment/order".into(), order_id));
            };
            if order_id.is_none() {
                return Ok(("ignored: unknown order".into(), None));
            }
            let pid = p.id.clone();
            match settle_payment(state, oid, &pid, Source::Webhook(Box::new(p))).await {
                Ok(s) => Ok((s.outcome.as_str().to_owned(), Some(s.order_id))),
                Err(SettleError::UnknownOrder) => Ok(("ignored: unknown order".into(), order_id)),
                Err(SettleError::Mismatch(why)) => Ok((format!("rejected: {why}"), order_id)),
                Err(SettleError::Gateway(_)) => Err(AppError::PaymentGateway),
                Err(SettleError::Db(e)) => Err(e.into()),
            }
        }
        "payment.failed" => {
            // The order stays `pending_payment`: the customer may retry until expiry.
            let code = payment
                .as_ref()
                .and_then(|p| p.error_code.as_deref())
                .map(|c| {
                    c.chars()
                        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                        .take(60)
                        .collect::<String>()
                })
                .unwrap_or_else(|| "unknown".into());
            Ok((format!("payment_failed: {code}"), order_id))
        }
        "refund.processed" | "refund.failed" | "refund.created" => match refund {
            Some(r) => apply_refund_event(state, event, r)
                .await
                .map_err(AppError::from),
            None => Ok(("ignored: payload without refund".into(), order_id)),
        },
        _ => Ok(("ignored: unhandled event".into(), order_id)),
    }
}

/// Apply a `refund.*` webhook to the ledger and the order totals.
async fn apply_refund_event(
    state: &AppState,
    event: &str,
    r: RzpRefund,
) -> Result<(String, Option<Uuid>), sqlx::Error> {
    if !signature::is_refund_id(&r.id) || !signature::is_payment_id(&r.payment_id) || r.amount <= 0
    {
        return Ok(("rejected: malformed refund".into(), None));
    }
    let status = match event {
        "refund.processed" => REFUND_PROCESSED,
        "refund.failed" => REFUND_FAILED,
        _ => refund_status(&r.status),
    };

    // Locate our ledger row: by Razorpay id, else by our id (receipt/notes).
    let mut row = repo::payments::find_refund_by_razorpay_id(&state.db, &r.id).await?;
    if row.is_none() {
        if let Some(ours) = r.our_refund_id() {
            row = repo::payments::find_refund(&state.db, ours)
                .await?
                .filter(|x| x.razorpay_payment_id == r.payment_id && x.amount_paise == r.amount);
        }
    }
    let order_id = match &row {
        Some(x) => Some(x.order_id),
        None => repo::payments::order_id_by_razorpay_payment(&state.db, &r.payment_id).await?,
    };
    let Some(order_id) = order_id else {
        return Ok(("ignored: unknown refund".into(), None));
    };

    let mut tx = state.db.begin().await?;
    repo::admins::set_lock_timeout(&mut tx).await?;
    let Some(order) = repo::orders::find_for_update(&mut tx, &OrderRef::Id(order_id)).await? else {
        return Ok(("ignored: unknown order".into(), None));
    };
    let failure = (status == REFUND_FAILED).then_some("Razorpay reported the refund as failed");
    match &row {
        Some(x) => {
            // Re-read under the order lock: a processed refund never regresses.
            let current = repo::payments::find_refund(&mut *tx, x.id).await?;
            if current.is_some_and(|c| c.status == REFUND_PROCESSED) && status != REFUND_PROCESSED {
                tx.rollback().await?;
                return Ok(("ignored: refund already processed".into(), Some(order_id)));
            }
            if !repo::payments::apply_refund_status(&mut tx, x.id, &r.id, status, failure).await? {
                tx.rollback().await?;
                return Ok(("ignored: refund id conflict".into(), Some(order_id)));
            }
        }
        None => {
            // Refund issued outside this system (Razorpay Dashboard).
            repo::payments::insert_external_refund(
                &mut tx,
                order_id,
                &r.payment_id,
                &r.id,
                r.amount,
                status,
                "Refund issued from the Razorpay Dashboard",
            )
            .await?;
        }
    }
    if status == REFUND_FAILED {
        let note = format!("Refund {} failed at Razorpay; retry the refund", r.id);
        repo::payments::set_attention(&mut *tx, order_id, &note).await?;
    }
    if !repo::payments::sync_refund_totals(&mut tx, order_id).await? {
        repo::payments::bump_version(&mut *tx, order_id).await?;
    }
    tx.commit().await?;
    tracing::info!(order_number = %order.order_number, razorpay_refund_id = %r.id, status, "refund webhook applied");
    Ok((format!("refund_{status}"), Some(order_id)))
}

/// `true` when a cancelled order was cancelled by the payment-expiry sweep.
pub fn cancelled_by_expiry(row: &OrderRow) -> bool {
    row.status == "cancelled" && row.cancel_reason.as_deref() == Some(EXPIRY_CANCEL_REASON)
}

/// Convenience for handlers: settle errors as API errors (verification
/// failures are mapped by the caller, which has its own error code).
impl From<SettleError> for AppError {
    fn from(e: SettleError) -> Self {
        match e {
            SettleError::UnknownOrder => AppError::NotFound,
            SettleError::Mismatch(_) => AppError::Validation("payment could not be verified"),
            SettleError::Gateway(_) => AppError::PaymentGateway,
            SettleError::Db(e) => e.into(),
        }
    }
}
