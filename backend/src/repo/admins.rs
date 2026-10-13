//! `admins` and `login_throttles`: lookups, failure throttling, the
//! account-wide ceiling, provisioning.
//!
//! # Brute-force defence
//!
//! * **Per-key throttle** (`login_throttles`), keyed by
//!   `HMAC(e-mail, client IP bucket)` and identical for registered and unknown
//!   e-mails. An attempt is *charged before* the expensive check (one atomic
//!   upsert that refuses while the key is locked), so concurrent requests can
//!   never get more than `max_failures` evaluations per lock period. The charge
//!   that reaches `max_failures` locks the key for `base * 2^lock_level`
//!   (capped at 24h). A successful check refunds its own charge.
//! * **Account-wide ceiling** (`admins.failed_login_count` / `locked_until`):
//!   at most [`ACCOUNT_MAX_FAILURES`] failures per [`ACCOUNT_WINDOW`] across all
//!   clients, then the account locks for [`ACCOUNT_LOCKOUT`] — except for
//!   trusted devices, which neither count toward nor are blocked by it.
//!
//! # Lock ordering
//!
//! Every transaction that touches an admin's auth state locks the `admins`
//! row *first* (`lock_by_challenge`, `lock_admin`, or the first `UPDATE`), then
//! challenge / throttle / session rows. Single-statement autocommit updates
//! hold one row lock and never wait while holding it.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor, PgPool};
use uuid::Uuid;

/// Account-wide ceiling: failures (all clients) per window before the account locks.
pub const ACCOUNT_MAX_FAILURES: i32 = 50;
pub const ACCOUNT_WINDOW: Duration = Duration::from_secs(60 * 60);
pub const ACCOUNT_LOCKOUT: Duration = Duration::from_secs(15 * 60);
/// Upper bound for a per-key lock (base lockout doubles per lock up to this).
pub const MAX_KEY_LOCKOUT: Duration = Duration::from_secs(24 * 60 * 60);
/// A key idle for this long starts over (count and lock level reset).
pub const KEY_DECAY: Duration = Duration::from_secs(24 * 60 * 60);
/// Waiting longer than this for an auth row lock answers 429 instead.
pub const LOCK_TIMEOUT_SQL: &str = "SET LOCAL lock_timeout = '2s'";

/// Canonical form of an e-mail address (trimmed, lowercased).
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Whole seconds (rounded up, at least 1) until `locked_until`, if it is in the future.
pub fn lock_remaining(locked_until: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<u64> {
    let until = locked_until?;
    if until <= now {
        return None;
    }
    let ms = (until - now).num_milliseconds().max(0).unsigned_abs();
    Some(ms.div_ceil(1000).max(1))
}

/// Bound the wait for row locks in the current transaction (see [`LOCK_TIMEOUT_SQL`]).
pub async fn set_lock_timeout(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query(LOCK_TIMEOUT_SQL).execute(conn).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminRow {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub is_active: bool,
    pub failed_login_count: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub db_now: DateTime<Utc>,
}

const ADMIN_COLUMNS: &str = "id, email, password_hash, display_name, is_active, failed_login_count, \
                             locked_until, now() AS db_now";

pub async fn find_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<AdminRow>> {
    sqlx::query_as::<_, AdminRow>(&format!(
        "SELECT {ADMIN_COLUMNS} FROM admins WHERE email = $1"
    ))
    .bind(normalize_email(email))
    .fetch_optional(pool)
    .await
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<AdminRow>> {
    sqlx::query_as::<_, AdminRow>(&format!("SELECT {ADMIN_COLUMNS} FROM admins WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Admin data for the password step, plus whether `device_hash` is a live
/// trusted device of this admin.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LoginAdmin {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub is_active: bool,
    pub trusted_device: bool,
}

pub async fn find_for_login<'e>(
    exec: impl PgExecutor<'e>,
    normalized_email: &str,
    device_hash: Option<&[u8; 32]>,
) -> sqlx::Result<Option<LoginAdmin>> {
    sqlx::query_as::<_, LoginAdmin>(
        r#"
        SELECT a.id, a.email, a.password_hash, a.display_name, a.is_active,
               EXISTS (SELECT 1 FROM trusted_devices d
                       WHERE d.admin_id = a.id AND d.device_hash = $2 AND d.expires_at > now())
                   AS trusted_device
        FROM admins a
        WHERE a.email = $1
        "#,
    )
    .bind(normalized_email)
    .bind(device_hash.map(|h| h.as_slice()))
    .fetch_optional(exec)
    .await
}

/// An admin row locked `FOR UPDATE` for the rest of the transaction.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LockedAdmin {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub is_active: bool,
    pub locked_until: Option<DateTime<Utc>>,
    pub trusted_device: bool,
    pub db_now: DateTime<Utc>,
}

impl LockedAdmin {
    /// Seconds left on the account-wide lock, unless this is a trusted device.
    pub fn account_lock_secs(&self) -> Option<u64> {
        if self.trusted_device {
            None
        } else {
            lock_remaining(self.locked_until, self.db_now)
        }
    }
}

const LOCKED_ADMIN_SELECT: &str = r#"
    SELECT a.id, a.email, a.display_name, a.is_active, a.locked_until,
           EXISTS (SELECT 1 FROM trusted_devices d
                   WHERE d.admin_id = a.id AND d.device_hash = $2 AND d.expires_at > now())
               AS trusted_device,
           now() AS db_now
    FROM admins a
"#;

/// Lock the admin owning OTP challenge `challenge_id` (admin row first, per the
/// lock order). `None` if the challenge does not exist.
pub async fn lock_by_challenge(
    conn: &mut PgConnection,
    challenge_id: Uuid,
    device_hash: Option<&[u8; 32]>,
) -> sqlx::Result<Option<LockedAdmin>> {
    sqlx::query_as::<_, LockedAdmin>(&format!(
        "{LOCKED_ADMIN_SELECT} WHERE a.id = (SELECT admin_id FROM otp_challenges WHERE id = $1) FOR UPDATE OF a"
    ))
    .bind(challenge_id)
    .bind(device_hash.map(|h| h.as_slice()))
    .fetch_optional(conn)
    .await
}

/// Lock an admin row by id.
pub async fn lock_admin(
    conn: &mut PgConnection,
    admin_id: Uuid,
    device_hash: Option<&[u8; 32]>,
) -> sqlx::Result<Option<LockedAdmin>> {
    sqlx::query_as::<_, LockedAdmin>(&format!(
        "{LOCKED_ADMIN_SELECT} WHERE a.id = $1 FOR UPDATE OF a"
    ))
    .bind(admin_id)
    .bind(device_hash.map(|h| h.as_slice()))
    .fetch_optional(conn)
    .await
}

// ---------------------------------------------------------------------------
// Per-key throttle
// ---------------------------------------------------------------------------

/// A charged attempt against a throttle key.
#[derive(Debug, Clone, Copy)]
pub struct KeyCharge {
    /// Set iff this charge reached the limit and locked the key (the attempt
    /// itself is still evaluated). Used to undo exactly this lock on success.
    pub triggered_lock: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy)]
pub enum ChargeOutcome<T> {
    Charged(T),
    /// Not charged: locked. Retry after this many seconds.
    Locked {
        retry_after_secs: u64,
    },
}

#[derive(sqlx::FromRow)]
struct ChargedRow {
    locked_until: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct LockRow {
    locked_until: Option<DateTime<Utc>>,
    db_now: DateTime<Utc>,
}

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

/// Charge one attempt against `key` *before* evaluating it. Refuses (without
/// charging) while the key is locked. One atomic upsert, so concurrent
/// attempts serialise on the row.
pub async fn charge_key(
    conn: &mut PgConnection,
    key: &[u8; 32],
    max_failures: i32,
    base_lockout: Duration,
) -> sqlx::Result<ChargeOutcome<KeyCharge>> {
    // $2 = max_failures, $3 = base lockout secs, $4 = max lockout secs, $5 = decay secs.
    let row = sqlx::query_as::<_, ChargedRow>(
        r#"
        INSERT INTO login_throttles AS t (key_hash, failure_count, lock_level, locked_until, updated_at)
        VALUES (
            $1,
            CASE WHEN 1 >= $2 THEN 0 ELSE 1 END,
            CASE WHEN 1 >= $2 THEN 1 ELSE 0 END,
            CASE WHEN 1 >= $2 THEN now() + make_interval(secs => LEAST($3, $4)) END,
            now()
        )
        ON CONFLICT (key_hash) DO UPDATE SET
            failure_count = CASE
                WHEN (CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.failure_count END) + 1 >= $2
                    THEN 0
                ELSE (CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.failure_count END) + 1
            END,
            lock_level = CASE
                WHEN (CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.failure_count END) + 1 >= $2
                    THEN (CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.lock_level END) + 1
                ELSE (CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.lock_level END)
            END,
            locked_until = CASE
                WHEN (CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.failure_count END) + 1 >= $2
                    THEN now() + make_interval(secs => LEAST(
                        $3 * power(2, LEAST(
                            CASE WHEN t.updated_at < now() - make_interval(secs => $5) THEN 0 ELSE t.lock_level END,
                            30)),
                        $4))
                ELSE NULL
            END,
            updated_at = now()
        WHERE t.locked_until IS NULL OR t.locked_until <= now()
        RETURNING t.locked_until
        "#,
    )
    .bind(key.as_slice())
    .bind(max_failures)
    .bind(secs(base_lockout))
    .bind(secs(MAX_KEY_LOCKOUT))
    .bind(secs(KEY_DECAY))
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = row {
        return Ok(ChargeOutcome::Charged(KeyCharge {
            triggered_lock: row.locked_until,
        }));
    }

    // Locked: a fresh statement (new snapshot) reads the current lock.
    let lock = sqlx::query_as::<_, LockRow>(
        "SELECT locked_until, now() AS db_now FROM login_throttles WHERE key_hash = $1",
    )
    .bind(key.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let retry_after_secs = lock
        .and_then(|l| lock_remaining(l.locked_until, l.db_now))
        .unwrap_or(1);
    Ok(ChargeOutcome::Locked { retry_after_secs })
}

/// Give back a charge whose attempt succeeded (or was never evaluated). If the
/// charge itself set the lock, that exact lock is undone. Returns the seconds
/// left on a lock set by *another* attempt meanwhile, if any.
pub async fn refund_key<'e>(
    exec: impl PgExecutor<'e>,
    key: &[u8; 32],
    charge: &KeyCharge,
    max_failures: i32,
) -> sqlx::Result<Option<u64>> {
    let row = sqlx::query_as::<_, LockRow>(
        r#"
        UPDATE login_throttles SET
            failure_count = CASE WHEN locked_until = $2 THEN GREATEST($3 - 1, 0)
                                 ELSE GREATEST(failure_count - 1, 0) END,
            lock_level    = CASE WHEN locked_until = $2 THEN GREATEST(lock_level - 1, 0) ELSE lock_level END,
            locked_until  = CASE WHEN locked_until = $2 THEN NULL ELSE locked_until END,
            updated_at    = now()
        WHERE key_hash = $1
        RETURNING locked_until, now() AS db_now
        "#,
    )
    .bind(key.as_slice())
    .bind(charge.triggered_lock)
    .bind(max_failures)
    .fetch_optional(exec)
    .await?;
    Ok(row.and_then(|r| lock_remaining(r.locked_until, r.db_now)))
}

/// Full success (password + OTP) from this key: forget its history.
pub async fn clear_key<'e>(exec: impl PgExecutor<'e>, key: &[u8; 32]) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM login_throttles WHERE key_hash = $1")
        .bind(key.as_slice())
        .execute(exec)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account-wide ceiling
// ---------------------------------------------------------------------------

/// A charged attempt against the account-wide ceiling.
#[derive(Debug, Clone, Copy)]
pub struct AccountCharge {
    pub triggered_lock: Option<DateTime<Utc>>,
}

/// Charge one attempt against the account-wide ceiling before evaluating it.
/// Refuses while the account is locked. (Never call this for trusted devices.)
pub async fn charge_account(
    conn: &mut PgConnection,
    admin_id: Uuid,
) -> sqlx::Result<ChargeOutcome<AccountCharge>> {
    let row = sqlx::query_as::<_, ChargedRow>(
        r#"
        WITH prev AS (
            SELECT id,
                   (failure_window_started_at IS NULL
                    OR failure_window_started_at <= now() - make_interval(secs => $3)) AS window_expired,
                   failed_login_count
            FROM admins
            WHERE id = $1 AND (locked_until IS NULL OR locked_until <= now())
            FOR UPDATE
        ), calc AS (
            SELECT id, window_expired,
                   (CASE WHEN window_expired THEN 0 ELSE failed_login_count END) + 1 AS new_count
            FROM prev
        )
        UPDATE admins a SET
            failed_login_count = CASE WHEN c.new_count >= $2 THEN 0 ELSE c.new_count END,
            failure_window_started_at = CASE
                WHEN c.new_count >= $2 THEN NULL
                WHEN c.window_expired THEN now()
                ELSE a.failure_window_started_at
            END,
            locked_until = CASE WHEN c.new_count >= $2 THEN now() + make_interval(secs => $4) ELSE NULL END,
            updated_at = now()
        FROM calc c
        WHERE a.id = c.id
        RETURNING a.locked_until
        "#,
    )
    .bind(admin_id)
    .bind(ACCOUNT_MAX_FAILURES)
    .bind(secs(ACCOUNT_WINDOW))
    .bind(secs(ACCOUNT_LOCKOUT))
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = row {
        return Ok(ChargeOutcome::Charged(AccountCharge {
            triggered_lock: row.locked_until,
        }));
    }
    let lock = sqlx::query_as::<_, LockRow>(
        "SELECT locked_until, now() AS db_now FROM admins WHERE id = $1",
    )
    .bind(admin_id)
    .fetch_optional(&mut *conn)
    .await?;
    let retry_after_secs = lock
        .and_then(|l| lock_remaining(l.locked_until, l.db_now))
        .unwrap_or(1);
    Ok(ChargeOutcome::Locked { retry_after_secs })
}

/// Give back an account-wide charge (see [`refund_key`]). Returns the seconds
/// left on an account lock set by another attempt, if any.
pub async fn refund_account<'e>(
    exec: impl PgExecutor<'e>,
    admin_id: Uuid,
    charge: &AccountCharge,
) -> sqlx::Result<Option<u64>> {
    let row = sqlx::query_as::<_, LockRow>(
        r#"
        UPDATE admins SET
            failed_login_count = CASE WHEN locked_until = $2 THEN GREATEST($3 - 1, 0)
                                      ELSE GREATEST(failed_login_count - 1, 0) END,
            failure_window_started_at = CASE WHEN locked_until = $2 THEN now() ELSE failure_window_started_at END,
            locked_until = CASE WHEN locked_until = $2 THEN NULL ELSE locked_until END,
            updated_at = now()
        WHERE id = $1
        RETURNING locked_until, now() AS db_now
        "#,
    )
    .bind(admin_id)
    .bind(charge.triggered_lock)
    .bind(ACCOUNT_MAX_FAILURES)
    .fetch_optional(exec)
    .await?;
    Ok(row.and_then(|r| lock_remaining(r.locked_until, r.db_now)))
}

/// Stamp a completed login (password + OTP).
pub async fn record_login_success<'e>(
    exec: impl PgExecutor<'e>,
    admin_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE admins SET last_login_at = now(), updated_at = now() WHERE id = $1")
        .bind(admin_id)
        .execute(exec)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Provisioning (CLI)
// ---------------------------------------------------------------------------

/// Insert a new admin. `email` is trimmed + lowercased here as well.
pub async fn insert_admin(
    pool: &PgPool,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO admins (id, email, display_name, password_hash) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(normalize_email(email))
    .bind(display_name.trim())
    .bind(password_hash)
    .fetch_one(pool)
    .await
}

/// Replace an admin's password, clear the account-wide lock, revoke all of
/// their sessions and trusted devices, and invalidate pending OTP challenges —
/// in one transaction (admin row first, per the lock order).
/// Returns false if no admin has that email.
pub async fn reset_password(pool: &PgPool, email: &str, password_hash: &str) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;

    let admin_id = sqlx::query_scalar::<_, Uuid>(
        "UPDATE admins SET password_hash = $2, password_changed_at = now(), failed_login_count = 0, \
         failure_window_started_at = NULL, locked_until = NULL, updated_at = now() \
         WHERE email = $1 RETURNING id",
    )
    .bind(normalize_email(email))
    .bind(password_hash)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(admin_id) = admin_id else {
        tx.rollback().await?;
        return Ok(false);
    };

    super::otp::invalidate_pending_for_admin(&mut *tx, admin_id).await?;
    super::sessions::revoke_all_for_admin(
        &mut *tx,
        admin_id,
        super::sessions::REVOKE_PASSWORD_RESET,
    )
    .await?;
    super::sessions::revoke_trusted_devices(&mut *tx, admin_id).await?;

    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;

    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(
            normalize_email("  Admin@ZhiMart.COM \n"),
            "admin@zhimart.com"
        );
    }

    #[test]
    fn lock_remaining_rounds_up() {
        let now = Utc::now();
        assert_eq!(lock_remaining(None, now), None);
        assert_eq!(
            lock_remaining(Some(now - ChronoDuration::seconds(1)), now),
            None
        );
        assert_eq!(lock_remaining(Some(now), now), None);
        assert_eq!(
            lock_remaining(Some(now + ChronoDuration::milliseconds(10)), now),
            Some(1)
        );
        assert_eq!(
            lock_remaining(Some(now + ChronoDuration::milliseconds(1500)), now),
            Some(2)
        );
        assert_eq!(
            lock_remaining(Some(now + ChronoDuration::seconds(900)), now),
            Some(900)
        );
    }
}
