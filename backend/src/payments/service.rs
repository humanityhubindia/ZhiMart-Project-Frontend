//! Payment service shared by the guest endpoints, the webhook, the expiry
//! sweep and the admin refund endpoint (Cashfree).
//!
//! Invariants:
//! * An order becomes paid only through [`settle_order`], after the payment
//!   has been read back from the Cashfree API itself
//!   (`GET /orders/{id}/payments`) with status `SUCCESS`, matching Cashfree
//!   order id, currency `INR` and amount (exactly `total_paise`). Nothing the
//!   browser says and nothing a webhook carries is trusted on its own: verify
//!   calls, webhooks and the sweep are all just TRIGGERS for that re-read.
//!   There is no capture step (we never use pre-auth; a `SUCCESS` payment is
//!   captured).
//! * Every state change happens in one transaction holding the order row lock,
//!   so a verify poll, the webhook and the sweep for the same payment (or any
//!   redelivery/double click) produce exactly one transition and one admin
//!   email; the others observe the result and report "already paid".
//! * HTTP calls to Cashfree are never made while holding a row lock. Refunds
//!   are written to the `refunds` ledger (status `pending`) and committed
//!   BEFORE the API call, then completed; the refund row id is Cashfree's
//!   `refund_id` and the idempotency key. Cashfree refunds are per ORDER (they
//!   apply to the order's successful payment), and our Cashfree order id is
//!   deterministic per order, so an order has at most one Cashfree order.
//! * A second successful payment on the same Cashfree order is NOT refunded by
//!   us (a refund call would hit the order's settled payment): Cashfree
//!   auto-refunds duplicate payments; we flag the order for attention and
//!   record Cashfree's auto-refund when its webhook arrives.
//! * Late payments (order already cancelled by the expiry sweep) revive the
//!   order when the stock is still there, else are refunded automatically; a
//!   payment for an order cancelled by an admin is refunded automatically.

use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::AppError,
    payments::{
        cashfree::{self, CfOrder, CfPayment, CfRefund, GatewayError, NewGatewayOrder},
        signature,
    },
    repo::{
        self,
        orders::{EXPIRY_CANCEL_REASON, METHOD_CASHFREE, OrderRef, OrderRow, ReviveOutcome},
        payments::{NewEvent, NewRefund, REFUND_FAILED, REFUND_PENDING, REFUND_PROCESSED},
    },
    state::AppState,
};

/// What triggered a settlement (logging only: every source re-reads the API).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `POST /pay/verify` (the storefront after checkout, and its polling).
    Checkout,
    /// `POST /pay` found the Cashfree order already `PAID`.
    Pay,
    /// A signature-verified webhook.
    Webhook,
    /// The expiry sweep, before cancelling an unpaid order.
    Sweep,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Checkout => "checkout",
            Source::Pay => "pay",
            Source::Webhook => "webhook",
            Source::Sweep => "sweep",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleOutcome {
    /// This call moved the order to paid / `pending_pack`.
    Paid,
    /// The order was already paid with this payment (idempotent no-op).
    AlreadyPaid,
    /// The order was already paid with a different payment; flagged for
    /// attention (Cashfree auto-refunds duplicate payments on an order).
    DuplicatePayment,
    /// Paid after the order expired and stock is gone; refunded (`order_expired`).
    LateRefunded,
    /// Order cancelled (not by expiry) or otherwise not payable; refunded.
    NotPayableRefunded,
    /// No successful payment yet, but one is `PENDING` at Cashfree.
    Pending,
    /// No successful or pending payment: nothing changed.
    NotPaid,
}

impl SettleOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            SettleOutcome::Paid => "paid",
            SettleOutcome::AlreadyPaid => "already_paid",
            SettleOutcome::DuplicatePayment => "duplicate_payment",
            SettleOutcome::LateRefunded => "late_payment_refunded",
            SettleOutcome::NotPayableRefunded => "not_payable_refunded",
            SettleOutcome::Pending => "payment_pending",
            SettleOutcome::NotPaid => "payment_not_completed",
        }
    }
}

#[derive(Debug)]
pub enum SettleError {
    /// Not one of our Cashfree orders (unknown, or a legacy Razorpay order).
    UnknownOrder,
    /// The successful payment (or the Cashfree order) does not match our
    /// order (order id, amount, currency). Treated as unpaid.
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

/// A successful payment that matches the order it claims to pay.
#[derive(Debug, Clone)]
struct ConfirmedPayment {
    id: String,
    amount_paise: i64,
    instrument: Option<String>,
}

/// Check a Cashfree payment against our order.
fn check_payment(
    p: &CfPayment,
    gateway_order_id: &str,
    total_paise: i64,
) -> Result<ConfirmedPayment, &'static str> {
    if !signature::is_gateway_payment_id(&p.cf_payment_id) {
        return Err("malformed payment id");
    }
    if p.order_id.as_deref().is_some_and(|o| o != gateway_order_id) {
        return Err("order id mismatch");
    }
    if p.payment_currency != "INR" {
        return Err("currency mismatch");
    }
    if p.amount_paise() != Some(total_paise) {
        return Err("amount mismatch");
    }
    Ok(ConfirmedPayment {
        id: p.cf_payment_id.clone(),
        amount_paise: total_paise,
        instrument: cashfree::instrument(p.payment_group.as_deref()),
    })
}

/// Check a Cashfree order entity (create/fetch response) against our order.
fn check_gateway_order(o: &CfOrder, gateway_order_id: &str, total_paise: i64) -> bool {
    o.order_id == gateway_order_id
        && o.order_currency == "INR"
        && cashfree::rupees_to_paise(o.order_amount) == Some(total_paise)
}

/// Set `payment_attention` unless it already says this (re-reads, polls and
/// webhook redeliveries must not bump the order's version every time).
async fn attention_once(state: &AppState, row: &OrderRow, note: &str) -> Result<(), sqlx::Error> {
    if row.payment_attention.as_deref() == Some(note) {
        return Ok(());
    }
    repo::payments::set_attention(&state.db, row.id, note).await
}

/// Reconcile our order with Cashfree: read the order's payments from the API
/// and settle a matching `SUCCESS` payment. Idempotent; safe to call
/// concurrently (verify polling, webhook, sweep) for the same order.
pub async fn settle_order(
    state: &AppState,
    order_id: Uuid,
    source: Source,
) -> Result<Settlement, SettleError> {
    let order = repo::orders::find(&state.db, &OrderRef::Id(order_id))
        .await?
        .ok_or(SettleError::UnknownOrder)?;
    if order.payment_method != METHOD_CASHFREE {
        return Err(SettleError::UnknownOrder);
    }
    let done = |outcome| Ok(Settlement { order_id, outcome });
    let Some(gid) = order.gateway_order_id.clone() else {
        return done(SettleOutcome::NotPaid);
    };
    if !signature::is_gateway_order_id(&gid) {
        return Err(SettleError::Mismatch("malformed gateway order id"));
    }
    let source_label = source.label();

    let payments = match state.cashfree.fetch_order_payments(&gid).await {
        Ok(p) => p,
        // The Cashfree order was never created (pay failed half-way): no payments.
        Err(e) if e.is_not_found() => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let successes: Vec<&CfPayment> = payments.iter().filter(|p| p.is_success()).collect();
    if successes.is_empty() {
        let outcome = if order.gateway_payment_id.is_some() {
            SettleOutcome::AlreadyPaid
        } else if payments.iter().any(CfPayment::is_pending) {
            SettleOutcome::Pending
        } else {
            SettleOutcome::NotPaid
        };
        return done(outcome);
    }

    // The payment already recorded on the order first, then the oldest match.
    let mut ordered = successes.clone();
    ordered.sort_by_key(|p| Some(p.cf_payment_id.as_str()) != order.gateway_payment_id.as_deref());
    let mut confirmed: Option<ConfirmedPayment> = None;
    let mut first_mismatch: Option<(&str, &'static str)> = None;
    for p in &ordered {
        match check_payment(p, &gid, order.total_paise) {
            Ok(c) if confirmed.is_none() => confirmed = Some(c),
            Ok(_) => {}
            Err(why) => {
                if first_mismatch.is_none() {
                    first_mismatch = Some((p.cf_payment_id.as_str(), why));
                }
            }
        }
    }
    let Some(payment) = confirmed else {
        let (pid, why) = first_mismatch.unwrap_or(("?", "no matching payment"));
        tracing::error!(
            order_number = %order.order_number, payment_id = pid, source = source_label, reason = why,
            "cashfree payment does not match the order"
        );
        let pid: String = pid
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(64)
            .collect();
        let note = format!(
            "Payment {pid} does not match this order ({why}); review it in the Cashfree Merchant Dashboard"
        );
        attention_once(state, &order, &note).await?;
        return Err(SettleError::Mismatch(why));
    };

    let (outcome, refund) = settle_locked(state, order_id, &gid, &payment).await?;
    match outcome {
        SettleOutcome::Paid => tracing::info!(
            order_number = %order.order_number, payment_id = %payment.id, source = source_label, "order paid"
        ),
        SettleOutcome::AlreadyPaid => tracing::debug!(
            order_number = %order.order_number, payment_id = %payment.id, source = source_label, "payment already settled"
        ),
        other => tracing::warn!(
            order_number = %order.order_number, payment_id = %payment.id, source = source_label,
            outcome = other.as_str(), "payment cannot be applied to the order"
        ),
    }

    // Further SUCCESS payments on the same Cashfree order are duplicates.
    let settled_id = repo::orders::find(&state.db, &OrderRef::Id(order_id))
        .await?
        .and_then(|r| r.gateway_payment_id);
    let duplicates: Vec<&str> = successes
        .iter()
        .map(|p| p.cf_payment_id.as_str())
        .filter(|id| Some(*id) != settled_id.as_deref() && signature::is_gateway_payment_id(id))
        .collect();
    if let Some(dup) = duplicates.first() {
        flag_duplicate(state, order_id, dup).await?;
    }

    if let Some(job) = refund {
        // Failures are recorded on the refund row and flagged for attention;
        // the outcome for the caller is the same either way.
        if let Err(e) = execute_refund(state, &job, true).await {
            tracing::error!(
                order_number = %job.order_number, payment_id = %payment.id, error = %e.summary(),
                "automatic refund did not complete"
            );
        }
    }
    let outcome = if outcome == SettleOutcome::AlreadyPaid && !duplicates.is_empty() {
        SettleOutcome::DuplicatePayment
    } else {
        outcome
    };
    done(outcome)
}

async fn flag_duplicate(state: &AppState, order_id: Uuid, dup: &str) -> Result<(), sqlx::Error> {
    let Some(row) = repo::orders::find(&state.db, &OrderRef::Id(order_id)).await? else {
        return Ok(());
    };
    if row
        .payment_attention
        .as_deref()
        .is_some_and(|a| a.contains(dup))
    {
        return Ok(());
    }
    tracing::warn!(order_number = %row.order_number, payment_id = dup, "duplicate successful payment on one order");
    let note = format!(
        "Duplicate payment {dup} received (order already paid); Cashfree refunds duplicate payments automatically: confirm it in the Cashfree Merchant Dashboard"
    );
    repo::payments::set_attention(&state.db, order_id, &note).await
}

/// A refund recorded in the ledger, to be sent to Cashfree.
#[derive(Debug, Clone)]
pub struct RefundJob {
    pub refund_id: Uuid,
    pub order_id: Uuid,
    pub order_number: String,
    /// Cashfree refunds are per order: `POST /orders/{gateway_order_id}/refunds`.
    pub gateway_order_id: String,
    /// The order's settled payment (ledger bookkeeping).
    pub payment_id: String,
    pub amount_paise: i64,
}

/// Queue the order emails inside the transaction that moves the order to paid
/// (only the transaction holding the row lock that performs the transition
/// gets here; the outbox's unique index makes it idempotent anyway).
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
    gateway_order_id: &str,
    payment: &ConfirmedPayment,
) -> Result<(SettleOutcome, Option<RefundJob>), SettleError> {
    let pid = payment.id.as_str();
    let instrument = payment.instrument.as_deref();
    let mut tx = state.db.begin().await?;
    repo::admins::set_lock_timeout(&mut tx).await?;
    let row = repo::orders::find_for_update(&mut tx, &OrderRef::Id(order_id))
        .await?
        .ok_or(SettleError::UnknownOrder)?;

    let mut refund_reason: Option<&'static str> = None;
    let outcome = if row.gateway_payment_id.as_deref() == Some(pid) {
        SettleOutcome::AlreadyPaid
    } else if row.gateway_payment_id.is_some() {
        // Caller flags it; Cashfree auto-refunds duplicates (see module docs).
        SettleOutcome::DuplicatePayment
    } else if row.status == "pending_payment" {
        // Even if the window has just passed, the sweep has not released the
        // stock yet (the order would be `cancelled`), so the payment is honoured.
        if !repo::payments::mark_paid(&mut tx, order_id, pid, instrument).await? {
            return Err(SettleError::Db(sqlx::Error::RowNotFound));
        }
        enqueue_order_notification(state, &mut tx, order_id).await?;
        SettleOutcome::Paid
    } else if row.status == "cancelled" {
        match repo::orders::revive_expired_order(&mut tx, order_id).await? {
            ReviveOutcome::Revived => {
                if !repo::payments::mark_paid(&mut tx, order_id, pid, instrument).await? {
                    return Err(SettleError::Db(sqlx::Error::RowNotFound));
                }
                enqueue_order_notification(state, &mut tx, order_id).await?;
                tracing::info!(order_number = %row.order_number, "expired order revived by a late payment");
                SettleOutcome::Paid
            }
            ReviveOutcome::InsufficientStock => {
                let note = "Payment received after the order expired and the stock was no longer available; automatic refund issued";
                repo::payments::record_unfulfillable_payment(
                    &mut tx, order_id, pid, instrument, note,
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
                    &mut tx, order_id, pid, instrument, note,
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
        let amount = payment.amount_paise - reserved;
        if amount > 0 {
            let refund_id = Uuid::now_v7();
            repo::payments::insert_refund(
                &mut tx,
                &NewRefund {
                    id: refund_id,
                    order_id,
                    gateway_payment_id: pid,
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
                gateway_order_id: gateway_order_id.to_owned(),
                payment_id: pid.to_owned(),
                amount_paise: amount,
            });
        }
    }
    tx.commit().await?;
    Ok((outcome, job))
}

// ---------------------------------------------------------------------------
// Starting a payment
// ---------------------------------------------------------------------------

/// Cashfree is given at least this long before its order expires, whatever is
/// left of our window (it may refuse very short expiries). A payment after our
/// window closes takes the late-payment path (revive or refund).
const MIN_GATEWAY_ORDER_LIFETIME: chrono::Duration = chrono::Duration::minutes(16);

/// Create (once) or fetch the Cashfree order for `row` under `gateway_order_id`.
///
/// `attached_before`: the id was already stored on the order, so the Cashfree
/// order most likely exists (fetch first; 404 ⇒ create). Otherwise create
/// first; 409 "already exists" (a lost response, a second tab) ⇒ fetch. The
/// result is checked against our order (id, amount, currency).
pub async fn ensure_gateway_order(
    state: &AppState,
    row: &OrderRow,
    gateway_order_id: &str,
    attached_before: bool,
) -> Result<CfOrder, SettleError> {
    let found = if attached_before {
        match state.cashfree.fetch_order(gateway_order_id).await {
            Ok(o) => Some(o),
            Err(e) if e.is_not_found() => None,
            Err(e) => return Err(e.into()),
        }
    } else {
        None
    };
    let order = match found {
        Some(o) => o,
        None => {
            let return_url = state.config.cashfree_return_url(&row.order_number);
            let floor = Utc::now() + MIN_GATEWAY_ORDER_LIFETIME;
            let expires_at = row.payment_expires_at.map_or(floor, |e| e.max(floor));
            let req = NewGatewayOrder {
                gateway_order_id,
                order_id: row.id,
                order_number: &row.order_number,
                amount_paise: row.total_paise,
                customer_name: &row.customer_name,
                customer_phone: &row.customer_phone,
                customer_email: row.customer_email.as_deref(),
                return_url: &return_url,
                notify_url: state.config.cashfree_notify_url.as_deref(),
                expires_at,
            };
            match state.cashfree.create_order(&req).await {
                Ok(o) => {
                    tracing::info!(order_number = %row.order_number, gateway_order_id, "cashfree order created");
                    o
                }
                Err(e) if e.is_conflict() => state.cashfree.fetch_order(gateway_order_id).await?,
                Err(e) => return Err(e.into()),
            }
        }
    };
    if !check_gateway_order(&order, gateway_order_id, row.total_paise) {
        tracing::error!(order_number = %row.order_number, gateway_order_id, "cashfree order does not match our order");
        return Err(SettleError::Mismatch("gateway order does not match"));
    }
    Ok(order)
}

// ---------------------------------------------------------------------------
// Refunds
// ---------------------------------------------------------------------------

/// Send a ledger refund to Cashfree and record the result.
///
/// * accepted ⇒ Cashfree refund id stored, order totals recomputed;
/// * rejected (4xx) ⇒ refund `failed` with the reason;
/// * no answer (or "already exists") ⇒ reconcile with
///   `GET /orders/{id}/refunds/{refund_id}`; if still unknown the row stays
///   `pending` (its amount stays reserved, so it can never be refunded twice)
///   until a refund webhook or a later reconciliation.
///
/// `automatic` refunds flag the order for attention when they fail.
pub async fn execute_refund(
    state: &AppState,
    job: &RefundJob,
    automatic: bool,
) -> Result<CfRefund, GatewayError> {
    let result = state
        .cashfree
        .create_refund(
            &job.gateway_order_id,
            job.refund_id,
            job.amount_paise,
            &job.order_number,
        )
        .await;
    let err = match result {
        Ok(r) => return apply_gateway_refund(state, job, r).await,
        Err(e) => e,
    };

    if matches!(err, GatewayError::Rejected { .. }) && !err.is_conflict() {
        let summary = err.summary();
        if let Err(e) = repo::payments::fail_refund(&state.db, job.refund_id, &summary).await {
            tracing::error!(refund_id = %job.refund_id, error = %e, "could not mark refund failed");
        }
        let note = format!(
            "Automatic refund of payment {} failed; refund it from the Cashfree Merchant Dashboard",
            job.payment_id
        );
        flag_or_bump(state, job, automatic.then_some(note.as_str())).await;
        return Err(err);
    }

    // Unavailable, or our refund id already exists at Cashfree.
    if let Ok(found) = reconcile_one(state, job).await {
        return Ok(found);
    }
    let note = "Cashfree did not confirm this refund yet; awaiting confirmation";
    if let Err(e) = repo::payments::note_refund(&state.db, job.refund_id, note).await {
        tracing::error!(refund_id = %job.refund_id, error = %e, "could not annotate refund");
    }
    let note = format!(
        "Automatic refund of payment {} is unconfirmed; check the Cashfree Merchant Dashboard",
        job.payment_id
    );
    flag_or_bump(state, job, automatic.then_some(note.as_str())).await;
    Err(GatewayError::Unavailable)
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

/// Look our refund up at Cashfree by our refund id.
async fn reconcile_one(state: &AppState, job: &RefundJob) -> Result<CfRefund, GatewayError> {
    let r = state
        .cashfree
        .fetch_refund(&job.gateway_order_id, &cashfree::refund_ref(job.refund_id))
        .await?;
    apply_gateway_refund(state, job, r).await
}

/// Cashfree refund status → ledger status.
fn refund_status(s: &str) -> &'static str {
    match s {
        "SUCCESS" => REFUND_PROCESSED,
        "CANCELLED" | "FAILED" | "REJECTED" => REFUND_FAILED,
        _ => REFUND_PENDING,
    }
}

/// Store a refund entity Cashfree returned for one of our ledger rows.
async fn apply_gateway_refund(
    state: &AppState,
    job: &RefundJob,
    r: CfRefund,
) -> Result<CfRefund, GatewayError> {
    if !signature::is_gateway_refund_id(&r.cf_refund_id)
        || r.our_refund_id() != Some(job.refund_id)
        || r.order_id
            .as_deref()
            .is_some_and(|o| o != job.gateway_order_id)
        || r.amount_paise() != Some(job.amount_paise)
    {
        tracing::error!(refund_id = %job.refund_id, "cashfree refund response does not match the request");
        return Err(GatewayError::Unavailable);
    }
    let status = refund_status(&r.refund_status);
    let failure = (status == REFUND_FAILED).then_some("Cashfree reported the refund as failed");
    let stored = async {
        let mut tx = state.db.begin().await?;
        repo::admins::set_lock_timeout(&mut tx).await?;
        repo::orders::find_for_update(&mut tx, &OrderRef::Id(job.order_id)).await?;
        repo::payments::apply_refund_status(
            &mut tx,
            job.refund_id,
            &r.cf_refund_id,
            status,
            failure,
        )
        .await?;
        if !repo::payments::sync_refund_totals(&mut tx, job.order_id).await? {
            repo::payments::bump_version(&mut *tx, job.order_id).await?;
        }
        tx.commit().await
    }
    .await;
    if let Err(e) = stored {
        // Cashfree accepted the refund; the refund webhook (which carries our
        // refund_id) or the next reconciliation completes the bookkeeping.
        tracing::error!(refund_id = %job.refund_id, error = %e, "refund accepted by Cashfree but not recorded");
        return Err(GatewayError::Unavailable);
    }
    tracing::info!(
        order_number = %job.order_number, refund_id = %job.refund_id, cf_refund_id = %r.cf_refund_id,
        amount_paise = job.amount_paise, status, "refund accepted by Cashfree"
    );
    if status == REFUND_FAILED {
        return Err(GatewayError::Rejected {
            status: 200,
            code: "REFUND_FAILED".into(),
            message: "Cashfree reported the refund as failed".into(),
        });
    }
    Ok(r)
}

/// Before a new admin refund: resolve refunds whose outcome was never
/// confirmed. Found at Cashfree ⇒ recorded; unknown to Cashfree (404) for 15+
/// minutes ⇒ marked failed (the request never arrived). Best effort.
pub async fn reconcile_unconfirmed_refunds(state: &AppState, order: &OrderRow) {
    if !state.config.payments_enabled || order.payment_method != METHOD_CASHFREE {
        return;
    }
    let Some(gid) = order.gateway_order_id.clone() else {
        return;
    };
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
            gateway_order_id: gid.clone(),
            payment_id: row.gateway_payment_id.clone(),
            amount_paise: row.amount_paise,
        };
        match state
            .cashfree
            .fetch_refund(&gid, &cashfree::refund_ref(row.id))
            .await
        {
            Ok(r) => {
                let _ = apply_gateway_refund(state, &job, r).await;
            }
            Err(e) if e.is_not_found() && row.stale => {
                repo::payments::fail_refund(
                    &state.db,
                    row.id,
                    "Never reached Cashfree (reconciled)",
                )
                .await
                .unwrap_or_else(
                    |e| tracing::warn!(error = %e, "could not mark stale refund failed"),
                );
                flag_or_bump(state, &job, None).await;
            }
            Err(e) if e.is_not_found() => {}
            Err(_) => return,
        }
    }
}

// ---------------------------------------------------------------------------
// Expiry sweep
// ---------------------------------------------------------------------------

/// What [`reconcile_expiring_orders`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReconcile {
    pub checked: usize,
    /// Orders that turned out to be paid (settled instead of cancelled).
    pub paid: usize,
    /// Cashfree was unavailable: the rest were not checked (the sweep cancels
    /// them anyway; a late payment revives or refunds).
    pub gateway_unavailable: bool,
}

/// Before the expiry sweep cancels unpaid orders: ask Cashfree whether any of
/// them was actually paid (a webhook lost or delayed, a customer who closed
/// the tab), and settle those instead. Best effort and bounded: at most
/// `limit` orders and `budget` of wall time per call; never holds a row lock
/// across an HTTP call (settlement takes its own lock afterwards).
pub async fn reconcile_expiring_orders(
    state: &AppState,
    limit: i64,
    budget: Duration,
) -> SweepReconcile {
    let mut run = SweepReconcile::default();
    if !state.config.payments_enabled {
        return run;
    }
    let ids = match repo::payments::expired_awaiting_gateway(&state.db, limit).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(error = %e, "payment expiry: could not list orders to reconcile");
            return run;
        }
    };
    let started = Instant::now();
    for id in ids {
        if started.elapsed() >= budget {
            break;
        }
        run.checked += 1;
        match settle_order(state, id, Source::Sweep).await {
            Ok(s) if s.outcome == SettleOutcome::Paid => run.paid += 1,
            Ok(_) | Err(SettleError::Mismatch(_) | SettleError::UnknownOrder) => {}
            Err(SettleError::Gateway(GatewayError::Rejected { .. })) => {}
            Err(SettleError::Gateway(GatewayError::Unavailable)) => {
                run.gateway_unavailable = true;
                break;
            }
            Err(SettleError::Db(e)) => {
                tracing::warn!(error = %e, "payment expiry: reconcile failed");
                break;
            }
        }
    }
    run
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

/// Event id: `cf_` + sha256(raw body) hex. Cashfree redelivers the same body
/// (re-signed with a new timestamp), so retries deduplicate.
pub fn event_id(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("cf_{}", signature::hex_encode(&Sha256::digest(body)))
}

/// A short, log/DB-safe token from a webhook field (ids, statuses, codes).
fn token(v: Option<&Value>) -> Option<String> {
    let s = match v? {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let s: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.' | '+'))
        .take(64)
        .collect();
    (!s.is_empty()).then_some(s)
}

fn number(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// What a payment/refund webhook says (a hint only; state is re-read).
struct WebhookFacts<'a> {
    order: Option<&'a Value>,
    payment: Option<&'a Value>,
    refund: Option<&'a Value>,
    gateway_order_id: Option<String>,
    payment_id: Option<String>,
}

impl<'a> WebhookFacts<'a> {
    fn from(data: &'a Value) -> Self {
        let order = data.get("order").filter(|v| v.is_object());
        let payment = data.get("payment").filter(|v| v.is_object());
        let refund = data
            .get("refund")
            .or_else(|| data.get("auto_refund"))
            .filter(|v| v.is_object());
        let gateway_order_id = order
            .and_then(|o| o.get("order_id"))
            .or_else(|| refund.and_then(|r| r.get("order_id")))
            .and_then(Value::as_str)
            .filter(|s| signature::is_gateway_order_id(s))
            .map(str::to_owned);
        let payment_id = token(payment.and_then(|p| p.get("cf_payment_id")))
            .or_else(|| token(refund.and_then(|r| r.get("cf_payment_id"))))
            .filter(|s| signature::is_gateway_payment_id(s));
        WebhookFacts {
            order,
            payment,
            refund,
            gateway_order_id,
            payment_id,
        }
    }

    fn error_code(&self) -> Option<String> {
        token(
            self.payment
                .and_then(|p| p.get("error_details"))
                .and_then(|e| e.get("error_code")),
        )
    }

    /// Only ids, amounts, statuses and codes are kept — never contact details,
    /// UPI ids, card data or free-text messages.
    fn redacted(&self, kind: &str, event_time: Option<String>) -> Value {
        let o = self.order;
        let p = self.payment;
        let r = self.refund;
        json!({
            "type": kind,
            "eventTime": event_time,
            "order": o.map(|o| json!({
                "orderId": token(o.get("order_id")),
                "amount": number(o.get("order_amount")),
                "currency": token(o.get("order_currency")),
            })),
            "payment": p.map(|p| json!({
                "cfPaymentId": token(p.get("cf_payment_id")),
                "status": token(p.get("payment_status")),
                "amount": number(p.get("payment_amount")),
                "currency": token(p.get("payment_currency")),
                "group": token(p.get("payment_group")),
                "errorCode": self.error_code(),
            })),
            "refund": r.map(|r| json!({
                "cfRefundId": token(r.get("cf_refund_id")),
                "refundId": token(r.get("refund_id")),
                "cfPaymentId": token(r.get("cf_payment_id")),
                "amount": number(r.get("refund_amount")),
                "status": token(r.get("refund_status")),
                "refundType": token(r.get("refund_type")),
            })),
        })
    }
}

/// Handle a webhook whose signature has ALREADY been verified by the caller.
///
/// Errors are transient (database / Cashfree unavailable, or a success that
/// the API does not show yet): the event row stays unprocessed and the caller
/// answers 5xx so Cashfree redelivers.
pub async fn handle_webhook(
    state: &AppState,
    event_id: &str,
    body: &[u8],
) -> Result<WebhookResult, AppError> {
    let parsed: Value = serde_json::from_slice(body)
        .map_err(|_| AppError::Validation("webhook body is not valid JSON"))?;
    let kind = parsed
        .get("type")
        .and_then(Value::as_str)
        .filter(|e| {
            (1..=100).contains(&e.len())
                && e.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .ok_or(AppError::Validation("webhook type is missing"))?
        .to_owned();
    let event_time = token(parsed.get("event_time"));
    let empty = Value::Null;
    let data = parsed.get("data").unwrap_or(&empty);
    let facts = WebhookFacts::from(data);

    let order_id = match &facts.gateway_order_id {
        Some(g) => repo::payments::order_id_by_gateway_order(&state.db, g).await?,
        None => None,
    };

    let new_event = NewEvent {
        id: event_id,
        event: &kind,
        order_id,
        gateway_order_id: facts.gateway_order_id.as_deref(),
        gateway_payment_id: facts.payment_id.as_deref(),
        payload: facts.redacted(&kind, event_time),
    };
    if repo::payments::record_event(&state.db, &new_event).await? {
        tracing::debug!(event = %kind, "duplicate webhook delivery acknowledged");
        return Ok(WebhookResult::Duplicate);
    }

    let processed = process_event(state, &kind, &facts, order_id).await;
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
            tracing::info!(event = %kind, outcome = %outcome, "webhook processed");
            Ok(WebhookResult::Processed(outcome))
        }
        Err((label, e)) => {
            if let Err(db) =
                repo::payments::finish_event(&state.db, event_id, label, false, order_id).await
            {
                tracing::error!(error = %db, "could not record webhook failure");
            }
            tracing::warn!(event = %kind, reason = label, "webhook processing failed; Cashfree will redeliver");
            Err(e)
        }
    }
}

type Processed = Result<(String, Option<Uuid>), (&'static str, AppError)>;

fn gateway_down() -> (&'static str, AppError) {
    (
        "error: payment gateway unavailable",
        AppError::PaymentGateway,
    )
}

/// Returns `(outcome, order id)`; `Err` only for transient failures.
async fn process_event(
    state: &AppState,
    kind: &str,
    facts: &WebhookFacts<'_>,
    order_id: Option<Uuid>,
) -> Processed {
    let db_err = |e: sqlx::Error| ("error: processing failed", AppError::from(e));
    match kind {
        "PAYMENT_SUCCESS_WEBHOOK" => {
            let Some(oid) = order_id else {
                return Ok(("ignored: unknown order".into(), None));
            };
            match settle_order(state, oid, Source::Webhook).await {
                // The webhook says SUCCESS but the API does not show it (yet):
                // let Cashfree redeliver rather than drop the trigger.
                Ok(s) if matches!(s.outcome, SettleOutcome::Pending | SettleOutcome::NotPaid) => {
                    Err((
                        "error: success not yet visible at the gateway",
                        AppError::PaymentGateway,
                    ))
                }
                Ok(s) => Ok((s.outcome.as_str().to_owned(), Some(s.order_id))),
                Err(SettleError::UnknownOrder) => Ok(("ignored: unknown order".into(), order_id)),
                Err(SettleError::Mismatch(why)) => Ok((format!("rejected: {why}"), order_id)),
                Err(SettleError::Gateway(GatewayError::Rejected { status, .. })) => {
                    Ok((format!("rejected: gateway answered {status}"), order_id))
                }
                Err(SettleError::Gateway(GatewayError::Unavailable)) => Err(gateway_down()),
                Err(SettleError::Db(e)) => Err(db_err(e)),
            }
        }
        // The order stays `pending_payment`: the customer may retry until expiry.
        "PAYMENT_FAILED_WEBHOOK" => {
            let code = facts.error_code().unwrap_or_else(|| "unknown".into());
            Ok((format!("payment_failed: {code}"), order_id))
        }
        "PAYMENT_USER_DROPPED_WEBHOOK" => Ok(("payment_user_dropped".into(), order_id)),
        "REFUND_STATUS_WEBHOOK" | "AUTO_REFUND_STATUS_WEBHOOK" => {
            let (Some(oid), Some(gid), Some(hint)) =
                (order_id, facts.gateway_order_id.as_deref(), facts.refund)
            else {
                return Ok(("ignored: unknown refund".into(), order_id));
            };
            let cf_refund_id = token(hint.get("cf_refund_id"));
            let refund_id = hint
                .get("refund_id")
                .and_then(Value::as_str)
                .filter(|s| signature::is_gateway_id(s));
            // Authoritative state: by our/Cashfree's refund_id, else from the list.
            let mut found = None;
            if let Some(rid) = refund_id {
                match state.cashfree.fetch_refund(gid, rid).await {
                    Ok(r) => found = Some(r),
                    Err(e) if e.is_not_found() => {}
                    Err(GatewayError::Unavailable) => return Err(gateway_down()),
                    Err(_) => {}
                }
            }
            if found.is_none() {
                match state.cashfree.fetch_order_refunds(gid).await {
                    Ok(list) => {
                        found = list.into_iter().find(|r| {
                            Some(r.cf_refund_id.as_str()) == cf_refund_id.as_deref()
                                || (refund_id.is_some() && r.refund_id.as_deref() == refund_id)
                        });
                    }
                    Err(GatewayError::Unavailable) => return Err(gateway_down()),
                    Err(_) => {}
                }
            }
            match found {
                Some(r) => apply_refund_event(state, oid, gid, r).await.map_err(db_err),
                None => Ok(("ignored: refund not found at the gateway".into(), Some(oid))),
            }
        }
        _ => Ok(("ignored: unhandled event".into(), order_id)),
    }
}

/// Apply an (API-confirmed) Cashfree refund to the ledger and the order totals.
async fn apply_refund_event(
    state: &AppState,
    order_id: Uuid,
    gateway_order_id: &str,
    r: CfRefund,
) -> Result<(String, Option<Uuid>), sqlx::Error> {
    let amount = match r.amount_paise() {
        Some(a) if a > 0 => a,
        _ => return Ok(("rejected: malformed refund".into(), Some(order_id))),
    };
    if !signature::is_gateway_refund_id(&r.cf_refund_id)
        || r.order_id.as_deref().is_some_and(|o| o != gateway_order_id)
    {
        return Ok(("rejected: malformed refund".into(), Some(order_id)));
    }
    let status = refund_status(&r.refund_status);

    // Locate our ledger row: by Cashfree id, else by our id (refund_id).
    let mut row = repo::payments::find_refund_by_gateway_id(&state.db, &r.cf_refund_id).await?;
    if row.is_none()
        && let Some(ours) = r.our_refund_id()
    {
        row = repo::payments::find_refund(&state.db, ours)
            .await?
            .filter(|x| x.order_id == order_id && x.amount_paise == amount);
    }
    if row.as_ref().is_some_and(|x| x.order_id != order_id) {
        return Ok((
            "ignored: refund belongs to another order".into(),
            Some(order_id),
        ));
    }

    let mut tx = state.db.begin().await?;
    repo::admins::set_lock_timeout(&mut tx).await?;
    let Some(order) = repo::orders::find_for_update(&mut tx, &OrderRef::Id(order_id)).await? else {
        return Ok(("ignored: unknown order".into(), None));
    };
    let failure = (status == REFUND_FAILED).then_some("Cashfree reported the refund as failed");
    match &row {
        Some(x) => {
            // Re-read under the order lock: a processed refund never regresses.
            let current = repo::payments::find_refund(&mut *tx, x.id).await?;
            if current.is_some_and(|c| c.status == REFUND_PROCESSED) && status != REFUND_PROCESSED {
                tx.rollback().await?;
                return Ok(("ignored: refund already processed".into(), Some(order_id)));
            }
            if !repo::payments::apply_refund_status(&mut tx, x.id, &r.cf_refund_id, status, failure)
                .await?
            {
                tx.rollback().await?;
                return Ok(("ignored: refund id conflict".into(), Some(order_id)));
            }
        }
        None => {
            // Issued outside this system: Cashfree Merchant Dashboard, or one of
            // Cashfree's automatic refunds (e.g. a duplicate payment).
            let payment_id = r
                .cf_payment_id
                .clone()
                .filter(|p| signature::is_gateway_payment_id(p))
                .or_else(|| order.gateway_payment_id.clone());
            let Some(payment_id) = payment_id else {
                tx.rollback().await?;
                return Ok(("ignored: refund without payment".into(), Some(order_id)));
            };
            let automatic = r
                .refund_type
                .as_deref()
                .is_some_and(|t| t.contains("AUTO_REFUND"));
            let reason = if automatic {
                "Automatic refund by Cashfree"
            } else {
                "Refund issued from the Cashfree Merchant Dashboard"
            };
            repo::payments::insert_external_refund(
                &mut tx,
                order_id,
                &payment_id,
                &r.cf_refund_id,
                amount,
                status,
                reason,
            )
            .await?;
        }
    }
    if status == REFUND_FAILED {
        let note = format!(
            "Refund {} failed at Cashfree; retry the refund",
            r.cf_refund_id
        );
        repo::payments::set_attention(&mut *tx, order_id, &note).await?;
    }
    if !repo::payments::sync_refund_totals(&mut tx, order_id).await? {
        repo::payments::bump_version(&mut *tx, order_id).await?;
    }
    tx.commit().await?;
    tracing::info!(order_number = %order.order_number, cf_refund_id = %r.cf_refund_id, status, "refund webhook applied");
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
            SettleError::Mismatch(_) => AppError::PaymentGateway,
            SettleError::Gateway(_) => AppError::PaymentGateway,
            SettleError::Db(e) => e.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refund_status_map() {
        assert_eq!(refund_status("SUCCESS"), REFUND_PROCESSED);
        assert_eq!(refund_status("CANCELLED"), REFUND_FAILED);
        assert_eq!(refund_status("FAILED"), REFUND_FAILED);
        assert_eq!(refund_status("REJECTED"), REFUND_FAILED);
        for s in ["PENDING", "ONHOLD", "PENDING_APPROVAL", "whatever"] {
            assert_eq!(refund_status(s), REFUND_PENDING, "{s}");
        }
    }

    #[test]
    fn event_id_is_stable_and_bounded() {
        let a = event_id(b"{}");
        assert_eq!(a, event_id(b"{}"));
        assert_ne!(a, event_id(b"{ }"));
        assert!(a.starts_with("cf_") && a.len() == 67);
    }

    #[test]
    fn webhook_tokens_are_sanitised() {
        assert_eq!(token(Some(&json!("SUCCESS"))).as_deref(), Some("SUCCESS"));
        assert_eq!(token(Some(&json!(12376123))).as_deref(), Some("12376123"));
        assert_eq!(token(Some(&json!("a b<c>@d"))).as_deref(), Some("abcd"));
        assert_eq!(token(Some(&json!({ "x": 1 }))), None);
        assert_eq!(token(Some(&json!(""))), None);
    }

    #[test]
    fn payment_checks() {
        let p = |amount: f64, currency: &str, order: Option<&str>| CfPayment {
            cf_payment_id: "5114910253538".into(),
            order_id: order.map(str::to_owned),
            payment_status: "SUCCESS".into(),
            payment_amount: amount,
            payment_currency: currency.into(),
            payment_group: Some("upi".into()),
            error_details: None,
        };
        let gid = "ZM-261020-1001_8f3a2b1c9d0e";
        let ok = check_payment(&p(1650.0, "INR", Some(gid)), gid, 165_000);
        assert_eq!(
            ok.as_ref().map(|c| c.instrument.clone()),
            Ok(Some("upi".into()))
        );
        assert_eq!(
            check_payment(&p(1649.99, "INR", Some(gid)), gid, 165_000).err(),
            Some("amount mismatch")
        );
        assert_eq!(
            check_payment(&p(1650.0, "USD", Some(gid)), gid, 165_000).err(),
            Some("currency mismatch")
        );
        assert_eq!(
            check_payment(&p(1650.0, "INR", Some("ZM-1_other")), gid, 165_000).err(),
            Some("order id mismatch")
        );
    }
}
