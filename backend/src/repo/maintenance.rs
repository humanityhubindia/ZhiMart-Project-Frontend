//! Periodic cleanup. Deletes run in bounded batches so a large backlog never
//! holds long locks or bloats a single transaction.

use sqlx::PgPool;

const BATCH: i64 = 5_000;

#[derive(Debug, Default, Clone, Copy)]
pub struct PurgeStats {
    pub otp_challenges: u64,
    pub refresh_tokens: u64,
    pub sessions: u64,
    pub audit_rows: u64,
    pub login_throttles: u64,
    pub trusted_devices: u64,
}

/// OTP challenges that expired more than 24h ago.
const PURGE_OTP: &str = r#"
    DELETE FROM otp_challenges WHERE id IN (
        SELECT id FROM otp_challenges WHERE expires_at < now() - interval '24 hours' LIMIT $1
    )"#;

/// Refresh tokens that expired more than 24h ago (tokens expire with their session).
const PURGE_REFRESH: &str = r#"
    DELETE FROM refresh_tokens WHERE id IN (
        SELECT id FROM refresh_tokens WHERE expires_at < now() - interval '24 hours' LIMIT $1
    )"#;

/// Sessions that ended (absolute expiry or revocation) more than 7 days ago.
/// Their refresh tokens go with them (ON DELETE CASCADE).
const PURGE_SESSIONS: &str = r#"
    DELETE FROM sessions WHERE id IN (
        SELECT id FROM sessions
        WHERE absolute_expires_at < now() - interval '7 days'
           OR revoked_at < now() - interval '7 days'
        LIMIT $1
    )"#;

/// Audit rows past the 365-day retention.
const PURGE_AUDIT: &str = r#"
    DELETE FROM auth_audit_log WHERE id IN (
        SELECT id FROM auth_audit_log WHERE created_at < now() - interval '365 days' LIMIT $1
    )"#;

/// Throttle keys idle for over 24h and not locked (they would decay to zero anyway).
const PURGE_THROTTLES: &str = r#"
    DELETE FROM login_throttles WHERE key_hash IN (
        SELECT key_hash FROM login_throttles
        WHERE updated_at < now() - interval '24 hours'
          AND (locked_until IS NULL OR locked_until < now())
        LIMIT $1
    )"#;

/// Expired trusted devices.
const PURGE_DEVICES: &str = r#"
    DELETE FROM trusted_devices WHERE id IN (
        SELECT id FROM trusted_devices WHERE expires_at < now() LIMIT $1
    )"#;

async fn purge_in_batches(pool: &PgPool, sql: &'static str) -> sqlx::Result<u64> {
    let mut total = 0u64;
    loop {
        let n = sqlx::query(sql)
            .bind(BATCH)
            .execute(pool)
            .await?
            .rows_affected();
        total += n;
        if n < BATCH.unsigned_abs() {
            return Ok(total);
        }
        tokio::task::yield_now().await;
    }
}

/// Delete expired OTP challenges, refresh tokens and sessions, and audit rows past retention.
pub async fn purge_expired(pool: &PgPool) -> sqlx::Result<PurgeStats> {
    Ok(PurgeStats {
        otp_challenges: purge_in_batches(pool, PURGE_OTP).await?,
        // Sessions first: cascades remove most stale tokens cheaply.
        sessions: purge_in_batches(pool, PURGE_SESSIONS).await?,
        refresh_tokens: purge_in_batches(pool, PURGE_REFRESH).await?,
        audit_rows: purge_in_batches(pool, PURGE_AUDIT).await?,
        login_throttles: purge_in_batches(pool, PURGE_THROTTLES).await?,
        trusted_devices: purge_in_batches(pool, PURGE_DEVICES).await?,
    })
}
