//! `sheet_sync_queue`: orders whose Google Sheets row is out of date.
//!
//! Producers are database triggers on `orders` (see migration
//! `20261014000012_google_sheets_sync.sql`); nothing in Rust enqueues. The
//! worker (`jobs.rs`) loops [`claim_due`] → write rows → [`complete`] /
//! [`mark_retry`]. Bookkeeping only ever touches this table, never `orders`,
//! so the worker cannot re-dirty the orders it syncs.

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

/// A claimed row is invisible to other workers for this long; if the worker
/// crashes mid-sync the row becomes due again afterwards. The whole batch
/// (token refresh + Sheets calls) is bounded well inside it.
pub const CLAIM_LEASE: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SheetSyncItem {
    pub order_id: Uuid,
    /// The order's revision at claim time (compare-and-clear token).
    pub revision: i64,
    /// Attempt number of the current claim (1 for the first try since the
    /// last success).
    pub attempts: i32,
}

/// Claim up to `limit` due rows, oldest change first (lease-based,
/// `SKIP LOCKED` ⇒ safe with several workers/replicas).
pub async fn claim_due(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<SheetSyncItem>> {
    sqlx::query_as::<_, SheetSyncItem>(
        "UPDATE sheet_sync_queue q SET attempts = q.attempts + 1, \
                next_attempt_at = now() + make_interval(secs => $2) \
         WHERE q.order_id IN ( \
             SELECT order_id FROM sheet_sync_queue \
             WHERE next_attempt_at <= now() \
             ORDER BY next_attempt_at, requested_at \
             LIMIT $1 \
             FOR UPDATE SKIP LOCKED) \
         RETURNING q.order_id, q.revision, q.attempts",
    )
    .bind(limit)
    .bind(CLAIM_LEASE.as_secs_f64())
    .fetch_all(pool)
    .await
}

/// The sheet now shows `item`'s revision. Deletes the row if the order has not
/// changed since the claim; otherwise (a change landed mid-sync) releases it
/// for an immediate re-sync with a fresh attempt count. Returns `true` when
/// the row was cleared.
pub async fn complete(pool: &PgPool, item: &SheetSyncItem) -> sqlx::Result<bool> {
    let cleared = sqlx::query("DELETE FROM sheet_sync_queue WHERE order_id = $1 AND revision = $2")
        .bind(item.order_id)
        .bind(item.revision)
        .execute(pool)
        .await?
        .rows_affected()
        > 0;
    if !cleared {
        sqlx::query(
            "UPDATE sheet_sync_queue SET attempts = 0, next_attempt_at = now(), last_error = NULL \
             WHERE order_id = $1",
        )
        .bind(item.order_id)
        .execute(pool)
        .await?;
    }
    Ok(cleared)
}

/// Record a failed attempt for every row in `order_ids`: due again after
/// `backoff`. Rows are never parked or dropped (the sheet must eventually
/// catch up); the caller grows `backoff` with the attempt count. `error` must
/// be a short, PII-free description.
pub async fn mark_retry(
    pool: &PgPool,
    order_ids: &[Uuid],
    backoff: Duration,
    error: &str,
) -> sqlx::Result<()> {
    if order_ids.is_empty() {
        return Ok(());
    }
    let error: String = error.chars().take(500).collect();
    sqlx::query(
        "UPDATE sheet_sync_queue SET next_attempt_at = now() + make_interval(secs => $2), \
                last_error = $3 \
         WHERE order_id = ANY($1)",
    )
    .bind(order_ids)
    .bind(backoff.as_secs_f64())
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop rows whose order no longer exists (normally already gone through
/// `ON DELETE CASCADE`; kept for robustness).
pub async fn forget(pool: &PgPool, order_ids: &[Uuid]) -> sqlx::Result<()> {
    if order_ids.is_empty() {
        return Ok(());
    }
    sqlx::query("DELETE FROM sheet_sync_queue WHERE order_id = ANY($1)")
        .bind(order_ids)
        .execute(pool)
        .await?;
    Ok(())
}
