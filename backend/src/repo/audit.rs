//! Append-only security audit trail. Writing an audit row must never fail or
//! delay the request: handlers use [`spawn_record`] / [`spawn_record_many`],
//! which write on a background task; errors are logged and swallowed. Never
//! put passwords, OTP codes, tokens or raw unregistered e-mails in `metadata`.

use sqlx::PgPool;
use uuid::Uuid;

use super::client_ip;
use crate::net::ClientMeta;

pub const LOGIN_PASSWORD_OK: &str = "login_password_ok";
pub const LOGIN_FAILED: &str = "login_failed";
pub const LOGIN_UNKNOWN_EMAIL: &str = "login_unknown_email";
/// Account-wide ceiling hit (`account_locked`).
pub const LOGIN_LOCKED: &str = "login_locked";
/// Per-(email, IP) throttle refused the attempt (`too_many_requests`).
pub const LOGIN_THROTTLED: &str = "login_throttled";
pub const OTP_THROTTLED: &str = "otp_throttled";
pub const OTP_SENT: &str = "otp_sent";
pub const OTP_RESENT: &str = "otp_resent";
pub const OTP_FAILED: &str = "otp_failed";
pub const OTP_CHALLENGE_EXHAUSTED: &str = "otp_challenge_exhausted";
pub const OTP_VERIFIED: &str = "otp_verified";
pub const SESSION_REFRESHED: &str = "session_refreshed";
pub const REFRESH_REUSE_DETECTED: &str = "refresh_reuse_detected";
pub const SESSION_EXPIRED: &str = "session_expired";
pub const LOGOUT: &str = "logout";

/// Record one event on a background task (does not delay the response).
pub fn spawn_record(
    pool: &PgPool,
    admin_id: Option<Uuid>,
    event: &'static str,
    meta: &ClientMeta,
    metadata: serde_json::Value,
) {
    let (pool, meta) = (pool.clone(), meta.clone());
    tokio::spawn(async move { record(&pool, admin_id, event, &meta, metadata).await });
}

/// Record several events (in order) on a background task.
pub fn spawn_record_many(
    pool: &PgPool,
    admin_id: Option<Uuid>,
    meta: &ClientMeta,
    events: Vec<(&'static str, serde_json::Value)>,
) {
    let (pool, meta) = (pool.clone(), meta.clone());
    tokio::spawn(async move { record_many(&pool, admin_id, &meta, &events).await });
}

/// Record one event.
pub async fn record(
    pool: &PgPool,
    admin_id: Option<Uuid>,
    event: &str,
    meta: &ClientMeta,
    metadata: serde_json::Value,
) {
    let res = sqlx::query(
        "INSERT INTO auth_audit_log (admin_id, event, ip, user_agent, metadata) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(admin_id)
    .bind(event)
    .bind(client_ip(meta))
    .bind(meta.user_agent.as_deref())
    .bind(&metadata)
    .execute(pool)
    .await;

    if let Err(e) = res {
        tracing::warn!(error = %e, event, "failed to write auth audit row");
    }
}

/// Record several events for the same admin/client in one round trip.
pub async fn record_many(
    pool: &PgPool,
    admin_id: Option<Uuid>,
    meta: &ClientMeta,
    events: &[(&str, serde_json::Value)],
) {
    if events.is_empty() {
        return;
    }
    let names: Vec<&str> = events.iter().map(|(e, _)| *e).collect();
    let metas: Vec<&serde_json::Value> = events.iter().map(|(_, m)| m).collect();
    let res = sqlx::query(
        r#"
        INSERT INTO auth_audit_log (admin_id, event, ip, user_agent, metadata)
        SELECT $1, e.event, $3, $4, e.metadata
        FROM UNNEST($2::text[], $5::jsonb[]) WITH ORDINALITY AS e(event, metadata, ord)
        ORDER BY e.ord
        "#,
    )
    .bind(admin_id)
    .bind(&names)
    .bind(client_ip(meta))
    .bind(meta.user_agent.as_deref())
    .bind(&metas)
    .execute(pool)
    .await;

    if let Err(e) = res {
        tracing::warn!(error = %e, events = ?names, "failed to write auth audit rows");
    }
}
