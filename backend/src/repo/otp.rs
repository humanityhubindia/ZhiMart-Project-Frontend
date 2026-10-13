//! `otp_challenges`: issuance (with a per-admin cap), locking for verify/resend,
//! attempt bookkeeping, consumption and code rotation.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::client_ip;
use crate::net::ClientMeta;

/// At most this many challenges per admin per [`ISSUANCE_WINDOW_SECS`], so a
/// stolen password cannot farm unlimited fresh OTP guesses.
pub const MAX_CHALLENGES_PER_WINDOW: i64 = 5;
pub const ISSUANCE_WINDOW_SECS: i64 = 15 * 60;

pub struct NewChallenge<'a> {
    pub id: Uuid,
    pub admin_id: Uuid,
    pub code_hash: &'a [u8; 32],
    pub ttl: Duration,
    pub max_attempts: i32,
    pub meta: &'a ClientMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateOutcome {
    Created,
    /// Issuance cap reached; retry after this many seconds.
    RateLimited {
        retry_after_secs: u64,
    },
}

#[derive(sqlx::FromRow)]
struct WindowStats {
    issued: i64,
    retry_after_secs: Option<i64>,
}

/// Create a challenge, enforcing the per-admin issuance cap and invalidating
/// every other pending challenge of that admin.
///
/// The caller must hold the admin's row lock in the same transaction
/// (`admins::lock_admin`): that serialises concurrent issuance for one admin
/// (so the cap cannot be raced) and keeps the admin → challenge lock order.
pub async fn create_challenge(
    conn: &mut PgConnection,
    c: &NewChallenge<'_>,
) -> sqlx::Result<CreateOutcome> {
    let stats = sqlx::query_as::<_, WindowStats>(
        r#"
        SELECT count(*) AS issued,
               CEIL(EXTRACT(EPOCH FROM (min(created_at) + make_interval(secs => $2) - now())))::bigint
                   AS retry_after_secs
        FROM otp_challenges
        WHERE admin_id = $1 AND created_at > now() - make_interval(secs => $2)
        "#,
    )
    .bind(c.admin_id)
    .bind(ISSUANCE_WINDOW_SECS as f64)
    .fetch_one(&mut *conn)
    .await?;

    if stats.issued >= MAX_CHALLENGES_PER_WINDOW {
        let retry = stats
            .retry_after_secs
            .unwrap_or(ISSUANCE_WINDOW_SECS)
            .clamp(1, ISSUANCE_WINDOW_SECS);
        return Ok(CreateOutcome::RateLimited {
            retry_after_secs: retry.unsigned_abs(),
        });
    }

    invalidate_pending_for_admin(&mut *conn, c.admin_id).await?;

    sqlx::query(
        r#"
        INSERT INTO otp_challenges (id, admin_id, code_hash, expires_at, max_attempts, ip, user_agent)
        VALUES ($1, $2, $3, now() + make_interval(secs => $4), $5, $6, $7)
        "#,
    )
    .bind(c.id)
    .bind(c.admin_id)
    .bind(c.code_hash.as_slice())
    .bind(c.ttl.as_secs_f64())
    .bind(c.max_attempts)
    .bind(client_ip(c.meta))
    .bind(c.meta.user_agent.as_deref())
    .execute(&mut *conn)
    .await?;

    Ok(CreateOutcome::Created)
}

/// Invalidate all un-consumed, still-valid challenges of an admin.
pub async fn invalidate_pending_for_admin<'e>(
    conn: impl PgExecutor<'e>,
    admin_id: Uuid,
) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "UPDATE otp_challenges SET invalidated_at = now() \
         WHERE admin_id = $1 AND consumed_at IS NULL AND invalidated_at IS NULL",
    )
    .bind(admin_id)
    .execute(conn)
    .await?;
    Ok(r.rows_affected())
}

/// Invalidate one challenge (e.g. the e-mail could not be sent).
pub async fn invalidate<'e>(pool: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE otp_challenges SET invalidated_at = now() WHERE id = $1 AND invalidated_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// A challenge row (locked `FOR UPDATE`) joined with its admin.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LockedChallenge {
    pub id: Uuid,
    pub admin_id: Uuid,
    pub code_hash: Vec<u8>,
    pub expires_at: DateTime<Utc>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub resend_count: i32,
    pub last_sent_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub invalidated_at: Option<DateTime<Utc>>,
    pub email: String,
    pub display_name: String,
    pub is_active: bool,
    pub locked_until: Option<DateTime<Utc>>,
    /// The presented device cookie is a live trusted device of this admin.
    pub trusted_device: bool,
    pub db_now: DateTime<Utc>,
}

impl LockedChallenge {
    /// Not consumed, not invalidated, not expired, attempts left, admin active.
    pub fn is_usable(&self) -> bool {
        self.consumed_at.is_none()
            && self.invalidated_at.is_none()
            && self.expires_at > self.db_now
            && self.attempts < self.max_attempts
            && self.is_active
    }

    /// Seconds left on the account-wide lock, unless this is a trusted device.
    pub fn account_lock_secs(&self) -> Option<u64> {
        if self.trusted_device {
            None
        } else {
            super::admins::lock_remaining(self.locked_until, self.db_now)
        }
    }
}

/// Lock a challenge row for the rest of the transaction (admin data is read,
/// not locked). Callers that will also write the admin row must lock it
/// first (`admins::lock_by_challenge`) to keep the admin → challenge order.
pub async fn lock_challenge<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    device_hash: Option<&[u8; 32]>,
) -> sqlx::Result<Option<LockedChallenge>> {
    sqlx::query_as::<_, LockedChallenge>(
        r#"
        SELECT c.id, c.admin_id, c.code_hash, c.expires_at, c.attempts, c.max_attempts,
               c.resend_count, c.last_sent_at, c.consumed_at, c.invalidated_at,
               a.email, a.display_name, a.is_active, a.locked_until,
               EXISTS (SELECT 1 FROM trusted_devices d
                       WHERE d.admin_id = a.id AND d.device_hash = $2 AND d.expires_at > now())
                   AS trusted_device,
               now() AS db_now
        FROM otp_challenges c
        JOIN admins a ON a.id = c.admin_id
        WHERE c.id = $1
        FOR UPDATE OF c
        "#,
    )
    .bind(id)
    .bind(device_hash.map(|h| h.as_slice()))
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct AttemptResult {
    pub attempts: i32,
    pub max_attempts: i32,
}

impl AttemptResult {
    pub fn remaining(&self) -> i32 {
        (self.max_attempts - self.attempts).max(0)
    }
    pub fn exhausted(&self) -> bool {
        self.attempts >= self.max_attempts
    }
}

/// Count a wrong code; invalidates the challenge in the same statement once
/// the attempt budget is spent.
pub async fn record_failed_attempt<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
) -> sqlx::Result<AttemptResult> {
    sqlx::query_as::<_, AttemptResult>(
        r#"
        UPDATE otp_challenges SET
            attempts = attempts + 1,
            invalidated_at = CASE WHEN attempts + 1 >= max_attempts THEN now() ELSE invalidated_at END
        WHERE id = $1
        RETURNING attempts, max_attempts
        "#,
    )
    .bind(id)
    .fetch_one(conn)
    .await
}

/// Mark a challenge consumed. Returns false if it was no longer pending.
pub async fn consume<'e>(conn: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE otp_challenges SET consumed_at = now() \
         WHERE id = $1 AND consumed_at IS NULL AND invalidated_at IS NULL",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Replace the code on resend: new hash, attempts reset, fresh expiry window.
pub async fn rotate_code<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    code_hash: &[u8; 32],
    ttl: Duration,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE otp_challenges SET
            code_hash = $2,
            attempts = 0,
            expires_at = now() + make_interval(secs => $3),
            resend_count = resend_count + 1,
            last_sent_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(code_hash.as_slice())
    .bind(ttl.as_secs_f64())
    .execute(conn)
    .await?;
    Ok(())
}
