//! `email_outbox`: transactional outbox for notification emails.
//!
//! Producers call [`enqueue_admin_order_notification`] inside the transaction
//! that creates/settles the order. The worker (`jobs.rs`) loops
//! [`claim_due`] → render + send → [`mark_sent`] / [`mark_retry`].

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::config::Config;

pub const KIND_ADMIN_ORDER_NOTIFICATION: &str = "admin_order_notification";
pub const KIND_CUSTOMER_ORDER_CONFIRMATION: &str = "customer_order_confirmation";
/// After this many failed attempts a row is parked as `failed` (visible in the DB for ops).
pub const MAX_ATTEMPTS: i32 = 12;
/// A claimed row is invisible to other workers for this long; if the worker
/// crashes mid-send the row becomes due again afterwards.
pub const CLAIM_LEASE: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxItem {
    pub id: Uuid,
    pub kind: String,
    pub order_id: Option<Uuid>,
    /// Attempt number of the current claim (1 for the first try).
    pub attempts: i32,
    pub created_at: DateTime<Utc>,
}

/// Queue the "new order" email for the admins. Idempotent per order: a second
/// call (e.g. verify + webhook both settling a payment) is a no-op.
pub async fn enqueue_admin_order_notification<'e>(
    conn: impl PgExecutor<'e>,
    order_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO email_outbox (id, kind, order_id) VALUES ($1, $2, $3) \
         ON CONFLICT (kind, order_id) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(KIND_ADMIN_ORDER_NOTIFICATION)
    .bind(order_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Queue every email that a newly *confirmed* order triggers, per config: the
/// admin "new order" notification and the customer's confirmation with their
/// tracking link. Call it inside the transaction that places a pay-later order
/// or settles an online payment. One statement; idempotent per (kind, order).
pub async fn enqueue_order_emails<'e>(
    conn: impl PgExecutor<'e>,
    config: &Config,
    order_id: Uuid,
) -> sqlx::Result<()> {
    let mut kinds: Vec<&str> = Vec::with_capacity(2);
    if config.order_notifications_enabled {
        kinds.push(KIND_ADMIN_ORDER_NOTIFICATION);
    }
    if config.customer_order_emails_enabled {
        kinds.push(KIND_CUSTOMER_ORDER_CONFIRMATION);
    }
    if kinds.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = kinds.iter().map(|_| Uuid::now_v7()).collect();
    sqlx::query(
        "INSERT INTO email_outbox (id, kind, order_id) \
         SELECT id, kind, $3 FROM unnest($1::uuid[], $2::text[]) AS t(id, kind) \
         ON CONFLICT (kind, order_id) DO NOTHING",
    )
    .bind(&ids)
    .bind(&kinds)
    .bind(order_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Claim up to `limit` due rows (lease-based, `SKIP LOCKED` ⇒ safe with several workers/replicas).
pub async fn claim_due(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<OutboxItem>> {
    sqlx::query_as::<_, OutboxItem>(
        "UPDATE email_outbox o SET attempts = o.attempts + 1, \
                next_attempt_at = now() + make_interval(secs => $2) \
         WHERE o.id IN ( \
             SELECT id FROM email_outbox \
             WHERE status = 'pending' AND next_attempt_at <= now() \
             ORDER BY next_attempt_at \
             LIMIT $1 \
             FOR UPDATE SKIP LOCKED) \
         RETURNING o.id, o.kind, o.order_id, o.attempts, o.created_at",
    )
    .bind(limit)
    .bind(CLAIM_LEASE.as_secs_f64())
    .fetch_all(pool)
    .await
}

pub async fn mark_sent(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE email_outbox SET status = 'sent', sent_at = now(), last_error = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record a failed attempt: retry with exponential backoff (30 s × 2^(n-1),
/// capped at 1 h), or park as `failed` after [`MAX_ATTEMPTS`]. `error` must be
/// a short, PII-free description.
pub async fn mark_retry(pool: &PgPool, item: &OutboxItem, error: &str) -> sqlx::Result<()> {
    let error: String = error.chars().take(500).collect();
    let backoff_secs = 30f64 * 2f64.powi((item.attempts - 1).clamp(0, 7));
    sqlx::query(
        "UPDATE email_outbox SET \
            status = CASE WHEN attempts >= $3 THEN 'failed' ELSE 'pending' END, \
            next_attempt_at = now() + make_interval(secs => LEAST($2, 3600)), \
            last_error = $4 \
         WHERE id = $1",
    )
    .bind(item.id)
    .bind(backoff_secs)
    .bind(MAX_ATTEMPTS)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Permanently drop a row that can never be sent (e.g. its order was deleted).
pub async fn mark_failed(pool: &PgPool, id: Uuid, error: &str) -> sqlx::Result<()> {
    let error: String = error.chars().take(500).collect();
    sqlx::query("UPDATE email_outbox SET status = 'failed', last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

/// Recipients when `ORDER_NOTIFICATION_EMAILS` is not set: every active admin.
pub async fn active_admin_emails(pool: &PgPool) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar::<_, String>("SELECT email FROM admins WHERE is_active ORDER BY created_at")
        .fetch_all(pool)
        .await
}

/// Delete sent rows older than 90 days and failed rows older than 1 year.
pub async fn purge_old<'e>(conn: impl PgExecutor<'e>) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "DELETE FROM email_outbox WHERE (status = 'sent' AND sent_at < now() - interval '90 days') \
            OR (status = 'failed' AND created_at < now() - interval '365 days')",
    )
    .execute(conn)
    .await?;
    Ok(r.rows_affected())
}
