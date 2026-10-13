//! `sessions` + `refresh_tokens` + `trusted_devices`: creation, rotation with
//! reuse detection (and a short retry grace window), revocation, trusted
//! devices, and the per-request session check used by the auth extractor.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::client_ip;
use crate::net::ClientMeta;

pub const REVOKE_LOGOUT: &str = "logout";
pub const REVOKE_REFRESH_REUSE: &str = "refresh_token_reuse";
pub const REVOKE_PASSWORD_RESET: &str = "password_reset";

#[derive(Debug, Clone, Copy)]
pub struct NewSession {
    pub id: Uuid,
    pub absolute_expires_at: DateTime<Utc>,
}

/// Create a session and its first refresh token in one statement.
/// `absolute_expires_at = now() + ttl` is fixed here and never extended.
pub async fn create_session<'e>(
    conn: impl PgExecutor<'e>,
    admin_id: Uuid,
    ttl: Duration,
    meta: &ClientMeta,
    token_hash: &[u8; 32],
) -> sqlx::Result<NewSession> {
    let session_id = Uuid::now_v7();
    let absolute_expires_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        WITH s AS (
            INSERT INTO sessions (id, admin_id, absolute_expires_at, ip, user_agent)
            VALUES ($1, $2, now() + make_interval(secs => $3), $4, $5)
            RETURNING id, absolute_expires_at
        )
        INSERT INTO refresh_tokens (id, session_id, token_hash, expires_at)
        SELECT $6, s.id, $7, s.absolute_expires_at FROM s
        RETURNING expires_at
        "#,
    )
    .bind(session_id)
    .bind(admin_id)
    .bind(ttl.as_secs_f64())
    .bind(client_ip(meta))
    .bind(meta.user_agent.as_deref())
    .bind(Uuid::now_v7())
    .bind(token_hash.as_slice())
    .fetch_one(conn)
    .await?;
    Ok(NewSession {
        id: session_id,
        absolute_expires_at,
    })
}

/// A rotated-out token presented again within this window, whose successor
/// has never been used, is treated as a client retry (the refresh response was
/// lost) rather than theft.
pub const REFRESH_GRACE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, sqlx::FromRow)]
struct LockedSessionRow {
    session_id: Uuid,
    admin_id: Uuid,
    absolute_expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    email: String,
    display_name: String,
    is_active: bool,
    db_now: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PresentedTokenRow {
    token_id: Uuid,
    session_id: Uuid,
    used_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    replaced_by: Option<Uuid>,
    successor_used_at: Option<DateTime<Utc>>,
}

/// Why a refresh token was not accepted (session-level reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Revoked,
    AbsoluteExpiry,
    AdminInactive,
    TokenExpired,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EndReason::Revoked => "revoked",
            EndReason::AbsoluteExpiry => "absolute_expiry",
            EndReason::AdminInactive => "admin_inactive",
            EndReason::TokenExpired => "token_expired",
        }
    }
}

#[derive(Debug, Clone)]
pub enum RotateOutcome {
    /// No such token.
    NotFound,
    /// Token was already rotated (outside the grace window, or its successor
    /// was already used) while the session was live: treat as theft. The
    /// caller must revoke the session (same transaction) and commit.
    Reused { session_id: Uuid, admin_id: Uuid },
    /// The session can no longer be refreshed.
    Ended {
        session_id: Uuid,
        admin_id: Uuid,
        reason: EndReason,
    },
    /// Rotation done (caller must commit). `grace_retry` is true when a
    /// just-rotated token was replayed within [`REFRESH_GRACE`].
    Rotated {
        session_id: Uuid,
        admin_id: Uuid,
        absolute_expires_at: DateTime<Utc>,
        email: String,
        display_name: String,
        grace_retry: bool,
    },
}

/// Rotate a refresh token inside the caller's transaction.
///
/// Locking: the *session* row is locked first (`FOR UPDATE`); every token
/// mutation of a session happens under that lock, so concurrent refreshes of
/// one session serialise and there is a single lock order (session → tokens).
/// The token rows are read by a separate statement *after* the lock is held,
/// so they reflect any rotation committed while we waited.
pub async fn rotate(
    conn: &mut PgConnection,
    presented_hash: &[u8; 32],
    new_token_hash: &[u8; 32],
) -> sqlx::Result<RotateOutcome> {
    let session = sqlx::query_as::<_, LockedSessionRow>(
        r#"
        SELECT s.id AS session_id, s.admin_id, s.absolute_expires_at, s.revoked_at,
               a.email, a.display_name, a.is_active, now() AS db_now
        FROM sessions s
        JOIN admins a ON a.id = s.admin_id
        WHERE s.id = (SELECT session_id FROM refresh_tokens WHERE token_hash = $1)
        FOR UPDATE OF s
        "#,
    )
    .bind(presented_hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(session) = session else {
        return Ok(RotateOutcome::NotFound);
    };
    let (session_id, admin_id) = (session.session_id, session.admin_id);

    let end_reason = if session.revoked_at.is_some() {
        Some(EndReason::Revoked)
    } else if session.absolute_expires_at <= session.db_now {
        Some(EndReason::AbsoluteExpiry)
    } else if !session.is_active {
        Some(EndReason::AdminInactive)
    } else {
        None
    };
    if let Some(reason) = end_reason {
        return Ok(RotateOutcome::Ended {
            session_id,
            admin_id,
            reason,
        });
    }

    let token = sqlx::query_as::<_, PresentedTokenRow>(
        r#"
        SELECT t.id AS token_id, t.session_id, t.used_at, t.expires_at, t.replaced_by,
               succ.used_at AS successor_used_at
        FROM refresh_tokens t
        LEFT JOIN refresh_tokens succ ON succ.id = t.replaced_by
        WHERE t.token_hash = $1
        "#,
    )
    .bind(presented_hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(token) = token.filter(|t| t.session_id == session_id) else {
        return Ok(RotateOutcome::NotFound);
    };
    if token.expires_at <= session.db_now {
        return Ok(RotateOutcome::Ended {
            session_id,
            admin_id,
            reason: EndReason::TokenExpired,
        });
    }

    // Which token row gets retired in favour of the new one.
    let (retire_id, grace_retry) = match (token.used_at, token.replaced_by) {
        (None, _) => (token.token_id, false),
        (Some(used_at), Some(successor_id))
            if token.successor_used_at.is_none()
                && session.db_now - used_at
                    <= chrono::Duration::from_std(REFRESH_GRACE).unwrap_or_default() =>
        {
            // Retry after a lost response: retire the never-used successor instead.
            (successor_id, true)
        }
        (Some(_), _) => {
            return Ok(RotateOutcome::Reused {
                session_id,
                admin_id,
            });
        }
    };

    // Insert the successor (same hard expiry as the session), mark the retired
    // one used and linked, and bump the session's last use — one round trip.
    sqlx::query(
        r#"
        WITH successor AS (
            INSERT INTO refresh_tokens (id, session_id, token_hash, expires_at)
            VALUES ($2, $3, $4, $5)
            RETURNING id
        ), old AS (
            UPDATE refresh_tokens SET used_at = now(), replaced_by = $2
            WHERE id = $1
        )
        UPDATE sessions SET last_used_at = now() WHERE id = $3
        "#,
    )
    .bind(retire_id)
    .bind(Uuid::now_v7())
    .bind(session_id)
    .bind(new_token_hash.as_slice())
    .bind(session.absolute_expires_at)
    .execute(&mut *conn)
    .await?;

    Ok(RotateOutcome::Rotated {
        session_id,
        admin_id,
        absolute_expires_at: session.absolute_expires_at,
        email: session.email,
        display_name: session.display_name,
        grace_retry,
    })
}

// ---------------------------------------------------------------------------
// Trusted devices
// ---------------------------------------------------------------------------

/// Register (or, for an already-trusted device of the same admin, renew) a
/// trusted device. `expires_at = now() + ttl`.
pub async fn upsert_trusted_device<'e>(
    exec: impl PgExecutor<'e>,
    admin_id: Uuid,
    device_hash: &[u8; 32],
    ttl: Duration,
    meta: &ClientMeta,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO trusted_devices (id, admin_id, device_hash, expires_at, ip, user_agent)
        VALUES ($1, $2, $3, now() + make_interval(secs => $4), $5, $6)
        ON CONFLICT (device_hash) DO UPDATE SET
            expires_at = EXCLUDED.expires_at,
            last_seen_at = now(),
            ip = EXCLUDED.ip,
            user_agent = EXCLUDED.user_agent
        WHERE trusted_devices.admin_id = EXCLUDED.admin_id
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(admin_id)
    .bind(device_hash.as_slice())
    .bind(ttl.as_secs_f64())
    .bind(client_ip(meta))
    .bind(meta.user_agent.as_deref())
    .execute(exec)
    .await?;
    Ok(())
}

/// Forget every trusted device of an admin (password reset).
pub async fn revoke_trusted_devices<'e>(
    exec: impl PgExecutor<'e>,
    admin_id: Uuid,
) -> sqlx::Result<u64> {
    let r = sqlx::query("DELETE FROM trusted_devices WHERE admin_id = $1")
        .bind(admin_id)
        .execute(exec)
        .await?;
    Ok(r.rows_affected())
}

/// Revoke one session. Returns true if it was live before.
pub async fn revoke_session<'e>(
    conn: impl PgExecutor<'e>,
    session_id: Uuid,
    reason: &str,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE sessions SET revoked_at = now(), revoke_reason = $2 WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Revoke the session owning a refresh token (used or not). Returns
/// `(session_id, admin_id)` if a live session was revoked.
pub async fn revoke_by_refresh_token<'e>(
    pool: impl PgExecutor<'e>,
    token_hash: &[u8; 32],
    reason: &str,
) -> sqlx::Result<Option<(Uuid, Uuid)>> {
    sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        UPDATE sessions s SET revoked_at = now(), revoke_reason = $2
        FROM refresh_tokens t
        WHERE t.token_hash = $1 AND s.id = t.session_id AND s.revoked_at IS NULL
        RETURNING s.id, s.admin_id
        "#,
    )
    .bind(token_hash.as_slice())
    .bind(reason)
    .fetch_optional(pool)
    .await
}

/// Revoke a session only if it belongs to `admin_id` (for Bearer-based logout).
pub async fn revoke_session_of_admin<'e>(
    pool: impl PgExecutor<'e>,
    session_id: Uuid,
    admin_id: Uuid,
    reason: &str,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE sessions SET revoked_at = now(), revoke_reason = $3 \
         WHERE id = $1 AND admin_id = $2 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(admin_id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Revoke every live session of an admin. Returns the number revoked.
pub async fn revoke_all_for_admin<'e>(
    conn: impl PgExecutor<'e>,
    admin_id: Uuid,
    reason: &str,
) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "UPDATE sessions SET revoked_at = now(), revoke_reason = $2 WHERE admin_id = $1 AND revoked_at IS NULL",
    )
    .bind(admin_id)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(r.rows_affected())
}

/// Session + admin data for an authenticated request.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuthSessionRow {
    pub absolute_expires_at: DateTime<Utc>,
    pub email: String,
    pub display_name: String,
}

/// One primary-key lookup per authenticated request: the session must be live,
/// unexpired, owned by `admin_id`, and the admin active.
pub async fn load_for_auth<'e>(
    pool: impl PgExecutor<'e>,
    session_id: Uuid,
    admin_id: Uuid,
) -> sqlx::Result<Option<AuthSessionRow>> {
    sqlx::query_as::<_, AuthSessionRow>(
        r#"
        SELECT s.absolute_expires_at, a.email, a.display_name
        FROM sessions s
        JOIN admins a ON a.id = s.admin_id
        WHERE s.id = $1
          AND s.admin_id = $2
          AND s.revoked_at IS NULL
          AND s.absolute_expires_at > now()
          AND a.is_active
        "#,
    )
    .bind(session_id)
    .bind(admin_id)
    .fetch_optional(pool)
    .await
}
