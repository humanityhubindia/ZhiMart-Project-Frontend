//! Online (Cashfree) payment state on `orders`, the `refunds` ledger and the
//! `payment_events` webhook log.
//!
//! Every mutation of an order's payment state runs inside a transaction that
//! has already locked the order row (`repo::orders::find_for_update`), so the
//! guards in the `WHERE` clauses are a second line of defence, not the only one.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::repo::orders::METHOD_CASHFREE;

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

/// Our order id for a gateway (Cashfree) order id.
pub async fn order_id_by_gateway_order<'e>(
    conn: impl PgExecutor<'e>,
    gateway_order_id: &str,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM orders WHERE gateway_order_id = $1")
        .bind(gateway_order_id)
        .fetch_optional(conn)
        .await
}

/// Our order id for the gateway payment recorded on it.
pub async fn order_id_by_gateway_payment<'e>(
    conn: impl PgExecutor<'e>,
    payment_id: &str,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM orders WHERE gateway_payment_id = $1")
        .bind(payment_id)
        .fetch_optional(conn)
        .await
}

/// `true` while the order's payment window is open, by the database clock.
pub async fn payment_window_open<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT COALESCE(payment_expires_at > now(), FALSE) FROM orders WHERE id = $1",
    )
    .bind(order_id)
    .fetch_optional(conn)
    .await
    .map(|v| v.unwrap_or(false))
}

/// Attach our (deterministic) Cashfree order id, but only if none is attached
/// yet and the order is still payable. Returns `false` if the guard did not
/// match; the caller re-reads the order (a concurrent request attached the
/// same id, or the order stopped being payable).
pub async fn attach_gateway_order<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
    gateway_order_id: &str,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        r#"
        UPDATE orders
        SET gateway_order_id = $2, version = version + 1, updated_at = now()
        WHERE id = $1
          AND gateway_order_id IS NULL
          AND status = 'pending_payment'
          AND payment_expires_at > now()
        "#,
    )
    .bind(order_id)
    .bind(gateway_order_id)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// `pending_payment` → `pending_pack`, payment `paid`. Returns `false` if the
/// guard did not match (caller must roll back).
pub async fn mark_paid(
    conn: &mut PgConnection,
    order_id: Uuid,
    payment_id: &str,
    instrument: Option<&str>,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        r#"
        UPDATE orders
        SET status = 'pending_pack',
            payment_status = 'paid',
            payment_method = $4,
            gateway_payment_id = $2,
            payment_instrument = $3,
            paid_at = now(),
            cancelled_at = NULL,
            cancel_reason = NULL,
            version = version + 1,
            updated_at = now()
        WHERE id = $1 AND status = 'pending_payment' AND gateway_payment_id IS NULL
        "#,
    )
    .bind(order_id)
    .bind(payment_id)
    .bind(instrument)
    .bind(METHOD_CASHFREE)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Record money received for an order that can no longer be fulfilled
/// (expired without stock, or cancelled): the payment is booked on the order
/// (`paid`, so the books show money in and, after the refund, money out), the
/// order status is left alone and `payment_attention` explains what happened.
pub async fn record_unfulfillable_payment(
    conn: &mut PgConnection,
    order_id: Uuid,
    payment_id: &str,
    instrument: Option<&str>,
    attention: &str,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        r#"
        UPDATE orders
        SET payment_status = 'paid',
            payment_method = $5,
            gateway_payment_id = $2,
            payment_instrument = $3,
            paid_at = now(),
            payment_attention = $4,
            version = version + 1,
            updated_at = now()
        WHERE id = $1 AND gateway_payment_id IS NULL
        "#,
    )
    .bind(order_id)
    .bind(payment_id)
    .bind(instrument)
    .bind(attention)
    .bind(METHOD_CASHFREE)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Flag the order for human attention (shown in the admin console).
pub async fn set_attention<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
    attention: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE orders SET payment_attention = $2, version = version + 1, updated_at = now() WHERE id = $1",
    )
    .bind(order_id)
    .bind(attention)
    .execute(conn)
    .await?;
    Ok(())
}

/// Unpaid orders whose payment window has closed and that have a Cashfree
/// order, i.e. what the expiry sweep is about to cancel and should first check
/// with Cashfree. Oldest first. No lock is taken (the caller calls the API).
pub async fn expired_awaiting_gateway<'e>(
    conn: impl PgExecutor<'e>,
    limit: i64,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT id FROM orders
        WHERE status = 'pending_payment'
          AND payment_expires_at <= now()
          AND payment_method = $2
          AND gateway_order_id IS NOT NULL
          AND gateway_payment_id IS NULL
        ORDER BY payment_expires_at
        LIMIT $1
        "#,
    )
    .bind(limit)
    .bind(METHOD_CASHFREE)
    .fetch_all(conn)
    .await
}

/// Optimistic-concurrency bump (e.g. a refund was initiated or failed), so the
/// admin console's sync marker notices the change.
pub async fn bump_version<'e>(conn: impl PgExecutor<'e>, order_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE orders SET version = version + 1, updated_at = now() WHERE id = $1")
        .bind(order_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Recompute `amount_refunded_paise` / `payment_status` / `refunded_at` from the
/// refunds ledger: the sum of refunds Cashfree has accepted (have a
/// `gateway_refund_id`) and that have not failed, for the order's own payment
/// (refunds of duplicate payments do not count). Idempotent; bumps `version`
/// only when something changed. The order row must be locked by the caller.
pub async fn sync_refund_totals(conn: &mut PgConnection, order_id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query(
        r#"
        WITH t AS (
            SELECT COALESCE(sum(r.amount_paise), 0)::bigint AS refunded
            FROM refunds r
            JOIN orders o ON o.id = r.order_id
            WHERE r.order_id = $1
              AND r.gateway_payment_id = o.gateway_payment_id
              AND r.gateway_refund_id IS NOT NULL
              AND r.status <> 'failed'
        ),
        n AS (
            SELECT o.id,
                   LEAST(t.refunded, o.total_paise) AS amt,
                   CASE
                       WHEN o.payment_status IN ('paid', 'refunded')
                           THEN CASE WHEN t.refunded >= o.total_paise THEN 'refunded' ELSE 'paid' END
                       ELSE o.payment_status
                   END AS ps
            FROM orders o, t
            WHERE o.id = $1
        )
        UPDATE orders o
        SET amount_refunded_paise = n.amt,
            payment_status = n.ps,
            refunded_at = CASE WHEN n.ps = 'refunded' THEN COALESCE(o.refunded_at, now()) ELSE NULL END,
            version = o.version + 1,
            updated_at = now()
        FROM n
        WHERE o.id = n.id
          AND (o.amount_refunded_paise <> n.amt OR o.payment_status <> n.ps)
        "#,
    )
    .bind(order_id)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

// ---------------------------------------------------------------------------
// Refunds
// ---------------------------------------------------------------------------

pub const REFUND_PENDING: &str = "pending";
pub const REFUND_PROCESSED: &str = "processed";
pub const REFUND_FAILED: &str = "failed";

#[derive(Debug, Clone)]
pub struct NewRefund<'a> {
    pub id: Uuid,
    pub order_id: Uuid,
    pub gateway_payment_id: &'a str,
    pub amount_paise: i64,
    pub reason: &'a str,
    pub initiated_by: Option<Uuid>,
}

pub async fn insert_refund(conn: &mut PgConnection, r: &NewRefund<'_>) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO refunds (id, order_id, gateway_payment_id, amount_paise, status, reason, initiated_by)
        VALUES ($1, $2, $3, $4, 'pending', $5, $6)
        "#,
    )
    .bind(r.id)
    .bind(r.order_id)
    .bind(r.gateway_payment_id)
    .bind(r.amount_paise)
    .bind(r.reason)
    .bind(r.initiated_by)
    .execute(conn)
    .await?;
    Ok(())
}

/// Sum of all refunds against `payment_id` that have not failed, including
/// ones still being sent to Cashfree. Used to compute what is left to refund,
/// so concurrent refunds can never exceed the payment.
pub async fn refunds_reserved_total(
    conn: &mut PgConnection,
    payment_id: &str,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(amount_paise), 0)::bigint FROM refunds WHERE gateway_payment_id = $1 AND status <> 'failed'",
    )
    .bind(payment_id)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RefundRow {
    pub id: Uuid,
    pub order_id: Uuid,
    pub gateway_payment_id: String,
    pub gateway_refund_id: Option<String>,
    pub amount_paise: i64,
    pub status: String,
    pub initiated_by: Option<Uuid>,
}

const REFUND_COLS: &str =
    "id, order_id, gateway_payment_id, gateway_refund_id, amount_paise, status, initiated_by";

pub async fn find_refund_by_gateway_id<'e>(
    conn: impl PgExecutor<'e>,
    gateway_refund_id: &str,
) -> sqlx::Result<Option<RefundRow>> {
    sqlx::query_as::<_, RefundRow>(&format!(
        "SELECT {REFUND_COLS} FROM refunds WHERE gateway_refund_id = $1"
    ))
    .bind(gateway_refund_id)
    .fetch_optional(conn)
    .await
}

pub async fn find_refund<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
) -> sqlx::Result<Option<RefundRow>> {
    sqlx::query_as::<_, RefundRow>(&format!("SELECT {REFUND_COLS} FROM refunds WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Refunds of this order that were never confirmed by Cashfree (no refund id
/// yet, still `pending`), with whether they are old enough to give up on.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UnconfirmedRefund {
    pub id: Uuid,
    pub gateway_payment_id: String,
    pub amount_paise: i64,
    pub stale: bool,
}

pub async fn unconfirmed_refunds<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
) -> sqlx::Result<Vec<UnconfirmedRefund>> {
    sqlx::query_as::<_, UnconfirmedRefund>(
        r#"
        SELECT id, gateway_payment_id, amount_paise, created_at < now() - interval '15 minutes' AS stale
        FROM refunds
        WHERE order_id = $1 AND status = 'pending' AND gateway_refund_id IS NULL
        ORDER BY created_at
        "#,
    )
    .bind(order_id)
    .fetch_all(conn)
    .await
}

/// Record Cashfree's refund id (`cf_refund_id`) and status. A `processed` refund never goes
/// back to `pending`/`failed`. Returns `false` if the row was not updated
/// (unknown id, or already bound to a different Cashfree refund).
pub async fn apply_refund_status(
    conn: &mut PgConnection,
    id: Uuid,
    gateway_refund_id: &str,
    status: &str,
    failure_reason: Option<&str>,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        r#"
        UPDATE refunds
        SET gateway_refund_id = $2,
            status = CASE WHEN status = 'processed' THEN status ELSE $3 END,
            failure_reason = CASE WHEN status = 'processed' THEN failure_reason ELSE $4 END,
            updated_at = now()
        WHERE id = $1 AND (gateway_refund_id IS NULL OR gateway_refund_id = $2)
        "#,
    )
    .bind(id)
    .bind(gateway_refund_id)
    .bind(status)
    .bind(failure_reason)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Mark a refund that Cashfree definitively did not accept.
pub async fn fail_refund<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    reason: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE refunds SET status = 'failed', failure_reason = $2, updated_at = now()
        WHERE id = $1 AND status = 'pending' AND gateway_refund_id IS NULL
        "#,
    )
    .bind(id)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(())
}

/// Keep a refund `pending` but note why it is unconfirmed.
pub async fn note_refund<'e>(conn: impl PgExecutor<'e>, id: Uuid, note: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE refunds SET failure_reason = $2, updated_at = now() WHERE id = $1 AND status = 'pending'")
        .bind(id)
        .bind(note)
        .execute(conn)
        .await?;
    Ok(())
}

/// A refund created outside this system (Cashfree Merchant Dashboard, or one
/// of Cashfree's automatic refunds), learned from a webhook.
pub async fn insert_external_refund(
    conn: &mut PgConnection,
    order_id: Uuid,
    payment_id: &str,
    gateway_refund_id: &str,
    amount_paise: i64,
    status: &str,
    reason: &str,
) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO refunds (id, order_id, gateway_payment_id, gateway_refund_id, amount_paise, status, reason)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(id)
    .bind(order_id)
    .bind(payment_id)
    .bind(gateway_refund_id)
    .bind(amount_paise)
    .bind(status)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Webhook events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewEvent<'a> {
    pub id: &'a str,
    pub event: &'a str,
    pub order_id: Option<Uuid>,
    pub gateway_order_id: Option<&'a str>,
    pub gateway_payment_id: Option<&'a str>,
    /// Redacted payload (ids, amounts, statuses — no customer contact data).
    pub payload: serde_json::Value,
}

/// Durably record a delivery. Returns `true` if this event was already fully
/// processed earlier (a redelivery to acknowledge without reprocessing). A
/// row whose processing failed (`processed_at IS NULL`) is processed again.
pub async fn record_event<'e>(conn: impl PgExecutor<'e>, e: &NewEvent<'_>) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        r#"
        WITH ins AS (
            INSERT INTO payment_events (id, event, order_id, gateway_order_id, gateway_payment_id, payload)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO NOTHING
            RETURNING id
        )
        SELECT CASE
            WHEN EXISTS (SELECT 1 FROM ins) THEN FALSE
            ELSE COALESCE((SELECT processed_at IS NOT NULL FROM payment_events WHERE id = $1), FALSE)
        END
        "#,
    )
    .bind(e.id)
    .bind(e.event)
    .bind(e.order_id)
    .bind(e.gateway_order_id)
    .bind(e.gateway_payment_id)
    .bind(&e.payload)
    .fetch_one(conn)
    .await
}

/// Store the processing outcome; `processed = false` leaves the event open
/// for reprocessing on redelivery.
pub async fn finish_event<'e>(
    conn: impl PgExecutor<'e>,
    id: &str,
    outcome: &str,
    processed: bool,
    order_id: Option<Uuid>,
) -> sqlx::Result<()> {
    let outcome: String = outcome.chars().take(300).collect();
    sqlx::query(
        r#"
        UPDATE payment_events
        SET outcome = $2,
            processed_at = CASE WHEN $3 THEN now() ELSE NULL END,
            order_id = COALESCE(order_id, $4)
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(outcome)
    .bind(processed)
    .bind(order_id)
    .execute(conn)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventListRow {
    pub id: String,
    pub event: String,
    pub outcome: Option<String>,
    pub received_at: DateTime<Utc>,
}

pub async fn list_events<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<EventListRow>> {
    sqlx::query_as::<_, EventListRow>(
        r#"
        SELECT id, event, outcome, received_at
        FROM payment_events
        WHERE order_id = $1
        ORDER BY received_at DESC, id
        LIMIT $2
        "#,
    )
    .bind(order_id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RefundListRow {
    pub id: Uuid,
    pub gateway_refund_id: Option<String>,
    pub gateway_payment_id: String,
    pub amount_paise: i64,
    pub status: String,
    pub reason: String,
    pub failure_reason: Option<String>,
    pub admin_email: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_refunds<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
) -> sqlx::Result<Vec<RefundListRow>> {
    sqlx::query_as::<_, RefundListRow>(
        r#"
        SELECT r.id, r.gateway_refund_id, r.gateway_payment_id, r.amount_paise, r.status, r.reason,
               r.failure_reason, a.email AS admin_email, r.created_at
        FROM refunds r
        LEFT JOIN admins a ON a.id = r.initiated_by
        WHERE r.order_id = $1
        ORDER BY r.created_at DESC, r.id
        "#,
    )
    .bind(order_id)
    .fetch_all(conn)
    .await
}
