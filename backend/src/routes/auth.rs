//! Admin auth endpoints (see `API.md`). Paths are relative to `/api/v1`.
//!
//! Flow: password (`/auth/login`) → emailed 6-digit code (`/auth/otp/verify`)
//! → short-lived access token + rotating refresh cookie bound to a session with
//! a hard absolute expiry (`/auth/refresh`, `/auth/logout`, `/auth/me`).

use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use chrono::{DateTime, SecondsFormat, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use super::json::ApiJson;
use crate::{
    auth::{
        AuthAdmin, CsrfGuard, cookies,
        extractor::bearer_token,
        otp,
        password::{self, MAX_PASSWORD_BYTES},
        throttle, tokens,
    },
    email::OtpEmail,
    error::{AppError, AppResult},
    net::ClientMeta,
    repo::{
        self,
        admins::{self, ChargeOutcome},
        audit,
        otp::{CreateOutcome, NewChallenge},
        sessions::{self, RotateOutcome},
    },
    state::AppState,
};

/// Upper bound on waiting for the email provider. Kept well under the DB's
/// `idle_in_transaction_session_timeout`, since resend sends inside a transaction.
const EMAIL_SEND_TIMEOUT: Duration = Duration::from_secs(8);
/// SQLSTATE `lock_not_available` (raised when `lock_timeout` expires).
const SQLSTATE_LOCK_NOT_AVAILABLE: &str = "55P03";
const MAX_EMAIL_CHARS: usize = 254;

/// Unauthenticated, brute-force-sensitive endpoints. Mounted under /api/v1 with
/// the strict per-IP rate limit: POST /auth/login, /auth/otp/verify, /auth/otp/resend.
pub fn credential_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/otp/verify", post(verify_otp))
        .route("/auth/otp/resend", post(resend_otp))
        .layer(no_store())
}

/// Session endpoints: POST /auth/refresh, POST /auth/logout, GET /auth/me.
pub fn session_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .layer(no_store())
}

/// Every auth response (success, error, extractor rejection) is uncacheable.
fn no_store() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LoginRequest {
    email: String,
    password: SecretString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct VerifyOtpRequest {
    challenge_id: Uuid,
    code: SecretString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ResendOtpRequest {
    challenge_id: Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    challenge_id: Uuid,
    masked_email: String,
    expires_in: u64,
    resend_available_in: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResendResponse {
    expires_in: u64,
    resend_available_in: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminDto {
    id: Uuid,
    email: String,
    display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthSessionResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    #[serde(serialize_with = "rfc3339")]
    session_expires_at: DateTime<Utc>,
    admin: AdminDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeResponse {
    admin: AdminDto,
    #[serde(serialize_with = "rfc3339")]
    session_expires_at: DateTime<Utc>,
}

fn rfc3339<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_email(raw: &str) -> AppResult<String> {
    const MSG: &str = "email must be a valid address of at most 254 characters";
    if raw.len() > MAX_EMAIL_CHARS * 4 {
        return Err(AppError::Validation(MSG));
    }
    let email = admins::normalize_email(raw);
    let len = email.chars().count();
    let valid = (3..=MAX_EMAIL_CHARS).contains(&len)
        && email.contains('@')
        && !email.starts_with('@')
        && !email.ends_with('@')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    if valid {
        Ok(email)
    } else {
        Err(AppError::Validation(MSG))
    }
}

fn validate_password(password: &SecretString) -> AppResult<()> {
    let len = password.expose_secret().len();
    if (1..=MAX_PASSWORD_BYTES).contains(&len) {
        Ok(())
    } else {
        Err(AppError::Validation("password must be 1-256 characters"))
    }
}

/// Start an auth transaction whose row-lock waits are bounded (see [`map_db`]).
async fn begin_auth_tx(state: &AppState) -> AppResult<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = state.db.begin().await?;
    admins::set_lock_timeout(&mut tx).await?;
    Ok(tx)
}

/// Database error → API error; a row-lock wait that hit `lock_timeout`
/// (contention on the same challenge/session/admin) becomes a retryable 429.
fn map_db(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e {
        if db.code().as_deref() == Some(SQLSTATE_LOCK_NOT_AVAILABLE) {
            return AppError::TooManyRequests {
                retry_after_secs: 2,
            };
        }
    }
    e.into()
}

/// The trusted-device cookie, if present and well-formed: (raw value, SHA-256).
fn presented_device(jar: &CookieJar, state: &AppState) -> Option<(SecretString, [u8; 32])> {
    let value = jar.get(cookies::device_cookie_name(&state.config))?.value();
    tokens::is_well_formed(value).then(|| {
        (
            SecretString::from(value.to_owned()),
            tokens::hash_token(value),
        )
    })
}

fn request_ip(meta: &ClientMeta) -> Option<std::net::IpAddr> {
    (!meta.ip.is_unspecified()).then_some(meta.ip)
}

/// Send the code, bounded by [`EMAIL_SEND_TIMEOUT`]. Any failure → 503.
async fn send_otp_email(state: &AppState, email: &OtpEmail) -> AppResult<()> {
    match tokio::time::timeout(EMAIL_SEND_TIMEOUT, state.email.send_otp(email)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            tracing::error!(error = ?e, "failed to send OTP email");
            Err(AppError::ServiceUnavailable)
        }
        Err(_) => {
            tracing::error!("timed out sending OTP email");
            Err(AppError::ServiceUnavailable)
        }
    }
}

fn otp_email(
    state: &AppState,
    to: &str,
    display_name: &str,
    code: SecretString,
    meta: &ClientMeta,
) -> OtpEmail {
    OtpEmail {
        to: to.to_owned(),
        display_name: display_name.to_owned(),
        code,
        expires_in_minutes: state.config.otp_ttl.as_secs().div_ceil(60),
        request_ip: request_ip(meta),
    }
}

// ---------------------------------------------------------------------------
// POST /auth/login
// ---------------------------------------------------------------------------

async fn login(
    State(state): State<AppState>,
    meta: ClientMeta,
    jar: CookieJar,
    ApiJson(req): ApiJson<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let email = validate_email(&req.email)?;
    validate_password(&req.password)?;
    let cfg = &state.config;
    let max_failures = cfg.login_max_failures;
    let key = throttle::throttle_key(&cfg.otp_pepper, &email, meta.ip)?;
    let device = presented_device(&jar, &state);
    let device_hash = device.as_ref().map(|d| d.1);

    // 1. Charge the (email, IP) key before anything that depends on whether the
    //    address exists, so throttling is identical for unknown and known e-mails
    //    and concurrent requests cannot exceed the budget.
    let mut conn = state.db.acquire().await?;
    let key_charge =
        match admins::charge_key(&mut conn, &key, max_failures, cfg.login_lockout).await? {
            ChargeOutcome::Charged(c) => c,
            ChargeOutcome::Locked { retry_after_secs } => {
                drop(conn);
                let mut fields = throttle::email_audit_fields(&cfg.otp_pepper, &email);
                fields["retryAfter"] = retry_after_secs.into();
                audit::spawn_record(&state.db, None, audit::LOGIN_THROTTLED, &meta, fields);
                return Err(AppError::TooManyRequests { retry_after_secs });
            }
        };

    let admin = admins::find_for_login(&mut *conn, &email, device_hash.as_ref()).await?;
    let admin = match admin {
        Some(admin) if admin.is_active => admin,
        other => {
            drop(conn);
            // Same Argon2 cost as a real check, so timing does not reveal
            // whether the address exists. The charge stays (a failure).
            password::dummy_verify(&state, req.password).await?;
            match other {
                Some(inactive) => audit::spawn_record(
                    &state.db,
                    Some(inactive.id),
                    audit::LOGIN_FAILED,
                    &meta,
                    json!({"reason": "inactive"}),
                ),
                None => audit::spawn_record(
                    &state.db,
                    None,
                    audit::LOGIN_UNKNOWN_EMAIL,
                    &meta,
                    throttle::email_audit_fields(&cfg.otp_pepper, &email),
                ),
            }
            return Err(AppError::InvalidCredentials);
        }
    };

    // 2. Account-wide ceiling for distributed attacks; trusted devices are exempt.
    let account_charge = if admin.trusted_device {
        None
    } else {
        match admins::charge_account(&mut conn, admin.id).await? {
            ChargeOutcome::Charged(c) => Some(c),
            ChargeOutcome::Locked { retry_after_secs } => {
                // Not evaluated: give the per-key charge back.
                admins::refund_key(&mut *conn, &key, &key_charge, max_failures).await?;
                drop(conn);
                audit::spawn_record(
                    &state.db,
                    Some(admin.id),
                    audit::LOGIN_LOCKED,
                    &meta,
                    json!({"retryAfter": retry_after_secs}),
                );
                return Err(AppError::AccountLocked { retry_after_secs });
            }
        }
    };
    drop(conn);

    // 3. The expensive check. On failure the charges simply stand.
    if !password::verify_password(&state, &admin.password_hash, req.password).await? {
        audit::spawn_record(
            &state.db,
            Some(admin.id),
            audit::LOGIN_FAILED,
            &meta,
            json!({
                "reason": "bad_password",
                "keyLockTriggered": key_charge.triggered_lock.is_some(),
                "accountLockTriggered": account_charge.is_some_and(|c| c.triggered_lock.is_some()),
                "trustedDevice": admin.trusted_device,
            }),
        );
        return Err(AppError::InvalidCredentials);
    }

    // 4. Password accepted. In one transaction (admin row locked first): refund
    //    this attempt's charges, re-check locks other attempts may have set
    //    while Argon2 ran, and only then issue the challenge.
    let code = otp::generate_code();
    let challenge_id = Uuid::now_v7();
    let code_hash = otp::hash_code(&cfg.otp_pepper, challenge_id, code.expose_secret())?;

    let mut tx = begin_auth_tx(&state).await?;
    admins::lock_admin(&mut tx, admin.id, device_hash.as_ref())
        .await
        .map_err(map_db)?
        .filter(|a| a.is_active)
        .ok_or(AppError::InvalidCredentials)?;
    if let Some(charge) = &account_charge {
        if let Some(retry_after_secs) = admins::refund_account(&mut *tx, admin.id, charge)
            .await
            .map_err(map_db)?
        {
            return Err(AppError::AccountLocked { retry_after_secs });
        }
    }
    if let Some(retry_after_secs) = admins::refund_key(&mut *tx, &key, &key_charge, max_failures)
        .await
        .map_err(map_db)?
    {
        return Err(AppError::TooManyRequests { retry_after_secs });
    }
    let outcome = repo::otp::create_challenge(
        &mut tx,
        &NewChallenge {
            id: challenge_id,
            admin_id: admin.id,
            code_hash: &code_hash,
            ttl: cfg.otp_ttl,
            max_attempts: cfg.otp_max_attempts,
            meta: &meta,
        },
    )
    .await
    .map_err(map_db)?;
    if let CreateOutcome::RateLimited { retry_after_secs } = outcome {
        drop(tx);
        audit::spawn_record(
            &state.db,
            Some(admin.id),
            audit::LOGIN_PASSWORD_OK,
            &meta,
            json!({"otpIssued": false, "reason": "otp_issuance_cap"}),
        );
        return Err(AppError::TooManyRequests { retry_after_secs });
    }
    tx.commit().await.map_err(map_db)?;

    let message = otp_email(&state, &admin.email, &admin.display_name, code, &meta);
    if let Err(e) = send_otp_email(&state, &message).await {
        if let Err(db_err) = repo::otp::invalidate(&state.db, challenge_id).await {
            tracing::warn!(error = %db_err, %challenge_id, "failed to invalidate unsent OTP challenge");
        }
        return Err(e);
    }
    drop(message);

    audit::spawn_record_many(
        &state.db,
        Some(admin.id),
        &meta,
        vec![
            (
                audit::LOGIN_PASSWORD_OK,
                json!({"challengeId": challenge_id, "trustedDevice": admin.trusted_device}),
            ),
            (audit::OTP_SENT, json!({"challengeId": challenge_id})),
        ],
    );

    Ok(Json(LoginResponse {
        challenge_id,
        masked_email: otp::mask_email(&admin.email),
        expires_in: cfg.otp_ttl.as_secs(),
        resend_available_in: cfg.otp_resend_cooldown.as_secs(),
    }))
}

// ---------------------------------------------------------------------------
// POST /auth/otp/verify
// ---------------------------------------------------------------------------

async fn verify_otp(
    State(state): State<AppState>,
    meta: ClientMeta,
    jar: CookieJar,
    ApiJson(req): ApiJson<VerifyOtpRequest>,
) -> AppResult<(CookieJar, Json<AuthSessionResponse>)> {
    let code = req.code.expose_secret();
    if !otp::is_valid_code_format(code) {
        return Err(AppError::Validation("code must be exactly 6 digits"));
    }
    let cfg = &state.config;
    let max_failures = cfg.login_max_failures;
    let device = presented_device(&jar, &state);
    let device_hash = device.as_ref().map(|d| d.1);

    // Lock order: admin row, then challenge row (same as login and password reset).
    let mut tx = begin_auth_tx(&state).await?;
    let admin = admins::lock_by_challenge(&mut tx, req.challenge_id, device_hash.as_ref())
        .await
        .map_err(map_db)?
        .ok_or(AppError::OtpChallengeExpired)?;
    let challenge = repo::otp::lock_challenge(&mut *tx, req.challenge_id, device_hash.as_ref())
        .await
        .map_err(map_db)?
        .filter(|c| c.is_usable() && c.admin_id == admin.id && admin.is_active)
        .ok_or(AppError::OtpChallengeExpired)?;

    if let Some(retry_after_secs) = admin.account_lock_secs() {
        return Err(AppError::AccountLocked { retry_after_secs });
    }

    // Charge the (email, IP) key and the account ceiling before comparing.
    let key = throttle::throttle_key(&cfg.otp_pepper, &admin.email, meta.ip)?;
    let key_charge = match admins::charge_key(&mut tx, &key, max_failures, cfg.login_lockout)
        .await
        .map_err(map_db)?
    {
        ChargeOutcome::Charged(c) => c,
        ChargeOutcome::Locked { retry_after_secs } => {
            drop(tx);
            audit::spawn_record(
                &state.db,
                Some(admin.id),
                audit::OTP_THROTTLED,
                &meta,
                json!({"challengeId": challenge.id, "retryAfter": retry_after_secs}),
            );
            return Err(AppError::TooManyRequests { retry_after_secs });
        }
    };
    let account_charge = if admin.trusted_device {
        None
    } else {
        match admins::charge_account(&mut tx, admin.id)
            .await
            .map_err(map_db)?
        {
            ChargeOutcome::Charged(c) => Some(c),
            ChargeOutcome::Locked { retry_after_secs } => {
                return Err(AppError::AccountLocked { retry_after_secs });
            }
        }
    };

    if !otp::verify_code(&cfg.otp_pepper, challenge.id, code, &challenge.code_hash)? {
        let attempt = repo::otp::record_failed_attempt(&mut *tx, challenge.id)
            .await
            .map_err(map_db)?;
        // Persist the failure bookkeeping (charges + attempt) before answering.
        tx.commit().await.map_err(map_db)?;

        let account_lock = account_charge.and_then(|c| c.triggered_lock);
        let mut events = vec![(
            audit::OTP_FAILED,
            json!({
                "challengeId": challenge.id,
                "attemptsRemaining": attempt.remaining(),
                "keyLockTriggered": key_charge.triggered_lock.is_some(),
                "accountLockTriggered": account_lock.is_some(),
            }),
        )];
        if attempt.exhausted() {
            events.push((
                audit::OTP_CHALLENGE_EXHAUSTED,
                json!({"challengeId": challenge.id}),
            ));
        }
        audit::spawn_record_many(&state.db, Some(admin.id), &meta, events);

        return Err(if attempt.exhausted() {
            AppError::OtpChallengeExpired
        } else if let Some(until) = account_lock {
            AppError::AccountLocked {
                retry_after_secs: admins::lock_remaining(Some(until), Utc::now()).unwrap_or(1),
            }
        } else {
            AppError::OtpInvalid {
                attempts_remaining: attempt.remaining(),
            }
        });
    }

    // Success: consume, forget this key's failures, refund the account charge,
    // trust this device, open the session — all in this transaction.
    if !repo::otp::consume(&mut *tx, challenge.id)
        .await
        .map_err(map_db)?
    {
        return Err(AppError::OtpChallengeExpired);
    }
    admins::clear_key(&mut *tx, &key).await.map_err(map_db)?;
    if let Some(charge) = &account_charge {
        admins::refund_account(&mut *tx, admin.id, charge)
            .await
            .map_err(map_db)?;
    }
    admins::record_login_success(&mut *tx, admin.id)
        .await
        .map_err(map_db)?;

    let device_token = match device {
        Some((token, _)) if admin.trusted_device => token,
        _ => tokens::generate().token,
    };
    let device_hash = tokens::hash_token(device_token.expose_secret());
    sessions::upsert_trusted_device(&mut *tx, admin.id, &device_hash, cookies::DEVICE_TTL, &meta)
        .await
        .map_err(map_db)?;

    let refresh_token = tokens::generate();
    let session = sessions::create_session(
        &mut *tx,
        admin.id,
        cfg.session_ttl,
        &meta,
        &refresh_token.hash,
    )
    .await
    .map_err(map_db)?;
    tx.commit().await.map_err(map_db)?;

    let access = state
        .jwt
        .issue(admin.id, session.id, session.absolute_expires_at)?;
    let jar = jar
        .add(cookies::refresh_cookie(
            cfg,
            &refresh_token.token,
            session.absolute_expires_at,
            Utc::now(),
        ))
        .add(cookies::device_cookie(cfg, &device_token));

    audit::spawn_record(
        &state.db,
        Some(admin.id),
        audit::OTP_VERIFIED,
        &meta,
        json!({"challengeId": challenge.id, "sessionId": session.id, "trustedDevice": admin.trusted_device}),
    );

    Ok((
        jar,
        Json(AuthSessionResponse {
            access_token: access.token,
            token_type: "Bearer",
            expires_in: access.expires_in,
            session_expires_at: session.absolute_expires_at,
            admin: AdminDto {
                id: admin.id,
                email: admin.email,
                display_name: admin.display_name,
            },
        }),
    ))
}

// ---------------------------------------------------------------------------
// POST /auth/otp/resend
// ---------------------------------------------------------------------------

async fn resend_otp(
    State(state): State<AppState>,
    meta: ClientMeta,
    jar: CookieJar,
    ApiJson(req): ApiJson<ResendOtpRequest>,
) -> AppResult<Json<ResendResponse>> {
    let cfg = &state.config;
    let device_hash = presented_device(&jar, &state).map(|d| d.1);

    // Only the challenge row is locked (no admin write here, so no admin lock).
    let mut tx = begin_auth_tx(&state).await?;
    let challenge = repo::otp::lock_challenge(&mut *tx, req.challenge_id, device_hash.as_ref())
        .await
        .map_err(map_db)?
        .filter(|c| c.is_usable() && c.resend_count < cfg.otp_max_resends)
        .ok_or(AppError::OtpChallengeExpired)?;

    if let Some(retry_after_secs) = challenge.account_lock_secs() {
        return Err(AppError::AccountLocked { retry_after_secs });
    }

    let elapsed_ms = (challenge.db_now - challenge.last_sent_at)
        .num_milliseconds()
        .max(0);
    let cooldown_ms = i64::try_from(cfg.otp_resend_cooldown.as_millis()).unwrap_or(i64::MAX);
    if elapsed_ms < cooldown_ms {
        let retry_after_secs = (cooldown_ms - elapsed_ms)
            .unsigned_abs()
            .div_ceil(1000)
            .max(1);
        return Err(AppError::TooManyRequests { retry_after_secs });
    }

    let code = otp::generate_code();
    let code_hash = otp::hash_code(&cfg.otp_pepper, challenge.id, code.expose_secret())?;
    repo::otp::rotate_code(&mut *tx, challenge.id, &code_hash, cfg.otp_ttl)
        .await
        .map_err(map_db)?;

    // Deliberate exception to "short transactions": the e-mail is sent while
    // the challenge row is locked, so the stored code only changes if the new
    // code was actually handed to the provider. Bounded by EMAIL_SEND_TIMEOUT
    // (8s, well under idle_in_transaction_session_timeout); a concurrent verify
    // waits at most `lock_timeout` (2s) and then gets 429. On failure the
    // transaction is dropped (rolled back) and the old code stays valid.
    let message = otp_email(
        &state,
        &challenge.email,
        &challenge.display_name,
        code,
        &meta,
    );
    send_otp_email(&state, &message).await?;
    drop(message);
    tx.commit().await.map_err(map_db)?;

    audit::spawn_record(
        &state.db,
        Some(challenge.admin_id),
        audit::OTP_RESENT,
        &meta,
        json!({"challengeId": challenge.id, "resendCount": challenge.resend_count + 1}),
    );

    Ok(Json(ResendResponse {
        expires_in: cfg.otp_ttl.as_secs(),
        resend_available_in: cfg.otp_resend_cooldown.as_secs(),
    }))
}

// ---------------------------------------------------------------------------
// POST /auth/refresh
// ---------------------------------------------------------------------------

enum RefreshResult {
    Rotated(Box<(Cookie<'static>, AuthSessionResponse)>),
    /// Session is over: answer `session_expired` and clear the cookie.
    Ended,
}

async fn refresh(
    State(state): State<AppState>,
    _csrf: CsrfGuard,
    meta: ClientMeta,
    jar: CookieJar,
) -> Response {
    match refresh_inner(&state, &meta, &jar).await {
        Ok(RefreshResult::Rotated(rotated)) => {
            let (cookie, body) = *rotated;
            (jar.add(cookie), Json(body)).into_response()
        }
        Ok(RefreshResult::Ended) => (
            jar.add(cookies::clear_refresh_cookie(&state.config)),
            AppError::SessionExpired,
        )
            .into_response(),
        // Transient failures (DB down, lock timeout) keep the cookie so the client can retry.
        Err(e) => e.into_response(),
    }
}

async fn refresh_inner(
    state: &AppState,
    meta: &ClientMeta,
    jar: &CookieJar,
) -> AppResult<RefreshResult> {
    let Some(cookie) = jar.get(state.config.refresh_cookie_name()) else {
        return Ok(RefreshResult::Ended);
    };
    if !tokens::is_well_formed(cookie.value()) {
        return Ok(RefreshResult::Ended);
    }
    let presented = tokens::hash_token(cookie.value());
    let successor = tokens::generate();

    let mut tx = begin_auth_tx(state).await?;
    match sessions::rotate(&mut tx, &presented, &successor.hash)
        .await
        .map_err(map_db)?
    {
        RotateOutcome::NotFound => Ok(RefreshResult::Ended),

        RotateOutcome::Reused {
            session_id,
            admin_id,
        } => {
            sessions::revoke_session(&mut *tx, session_id, sessions::REVOKE_REFRESH_REUSE)
                .await
                .map_err(map_db)?;
            tx.commit().await.map_err(map_db)?;
            tracing::warn!(%session_id, %admin_id, ip = %meta.ip, "refresh token reuse detected; session revoked");
            audit::spawn_record(
                &state.db,
                Some(admin_id),
                audit::REFRESH_REUSE_DETECTED,
                meta,
                json!({"sessionId": session_id}),
            );
            Ok(RefreshResult::Ended)
        }

        RotateOutcome::Ended {
            session_id,
            admin_id,
            reason,
        } => {
            drop(tx);
            audit::spawn_record(
                &state.db,
                Some(admin_id),
                audit::SESSION_EXPIRED,
                meta,
                json!({"sessionId": session_id, "reason": reason.as_str()}),
            );
            Ok(RefreshResult::Ended)
        }

        RotateOutcome::Rotated {
            session_id,
            admin_id,
            absolute_expires_at,
            email,
            display_name,
            grace_retry,
        } => {
            tx.commit().await.map_err(map_db)?;
            let access = match state.jwt.issue(admin_id, session_id, absolute_expires_at) {
                Ok(access) => access,
                // Crossed the absolute expiry between the DB check and now.
                Err(AppError::SessionExpired) => return Ok(RefreshResult::Ended),
                Err(e) => return Err(e),
            };
            let cookie = cookies::refresh_cookie(
                &state.config,
                &successor.token,
                absolute_expires_at,
                Utc::now(),
            );
            audit::spawn_record(
                &state.db,
                Some(admin_id),
                audit::SESSION_REFRESHED,
                meta,
                json!({"sessionId": session_id, "graceRetry": grace_retry}),
            );
            Ok(RefreshResult::Rotated(Box::new((
                cookie,
                AuthSessionResponse {
                    access_token: access.token,
                    token_type: "Bearer",
                    expires_in: access.expires_in,
                    session_expires_at: absolute_expires_at,
                    admin: AdminDto {
                        id: admin_id,
                        email,
                        display_name,
                    },
                },
            ))))
        }
    }
}

// ---------------------------------------------------------------------------
// POST /auth/logout
// ---------------------------------------------------------------------------

async fn logout(
    State(state): State<AppState>,
    _csrf: CsrfGuard,
    meta: ClientMeta,
    headers: HeaderMap,
    jar: CookieJar,
) -> Response {
    let result = revoke_for_logout(&state, &headers, &jar).await;
    let jar = jar.add(cookies::clear_refresh_cookie(&state.config));
    match result {
        Ok(revoked) => {
            if let Some((session_id, admin_id)) = revoked {
                audit::spawn_record(
                    &state.db,
                    Some(admin_id),
                    audit::LOGOUT,
                    &meta,
                    json!({"sessionId": session_id}),
                );
            }
            (jar, StatusCode::NO_CONTENT).into_response()
        }
        Err(e) => (jar, e).into_response(),
    }
}

/// Revoke the session identified by the refresh cookie (used or not), else by
/// a valid Bearer token. Returns the revoked `(session_id, admin_id)`, if any.
async fn revoke_for_logout(
    state: &AppState,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> AppResult<Option<(Uuid, Uuid)>> {
    if let Some(cookie) = jar.get(state.config.refresh_cookie_name()) {
        if tokens::is_well_formed(cookie.value()) {
            let hash = tokens::hash_token(cookie.value());
            if let Some(revoked) =
                sessions::revoke_by_refresh_token(&state.db, &hash, sessions::REVOKE_LOGOUT).await?
            {
                return Ok(Some(revoked));
            }
        }
    }

    let Ok(Some(token)) = bearer_token(headers) else {
        return Ok(None);
    };
    let Ok(claims) = state.jwt.verify(token) else {
        return Ok(None);
    };
    let revoked = sessions::revoke_session_of_admin(
        &state.db,
        claims.sid,
        claims.sub,
        sessions::REVOKE_LOGOUT,
    )
    .await?;
    Ok(revoked.then_some((claims.sid, claims.sub)))
}

// ---------------------------------------------------------------------------
// GET /auth/me
// ---------------------------------------------------------------------------

async fn me(admin: AuthAdmin) -> Json<MeResponse> {
    Json(MeResponse {
        admin: AdminDto {
            id: admin.admin_id,
            email: admin.email,
            display_name: admin.display_name,
        },
        session_expires_at: admin.session_expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation() {
        assert_eq!(
            validate_email("  Admin@ZhiMart.com ").ok().as_deref(),
            Some("admin@zhimart.com")
        );
        assert!(validate_email("no-at-sign").is_err());
        assert!(validate_email("a b@c.d").is_err());
        assert!(validate_email("ab@c\u{0000}d").is_err());
        assert!(validate_email("@zhimart.com").is_err());
        assert!(validate_email("admin@").is_err());
        assert!(validate_email(&format!("{}@x.io", "a".repeat(250))).is_err());
    }

    #[test]
    fn password_validation() {
        assert!(validate_password(&SecretString::from("x")).is_ok());
        assert!(validate_password(&SecretString::from("")).is_err());
        assert!(validate_password(&SecretString::from("a".repeat(256))).is_ok());
        assert!(validate_password(&SecretString::from("a".repeat(257))).is_err());
    }

    #[test]
    fn dates_are_rfc3339_utc_seconds() {
        let dt = DateTime::parse_from_rfc3339("2026-11-12T10:00:00.123456Z")
            .expect("dt")
            .with_timezone(&Utc);
        let body = MeResponse {
            admin: AdminDto {
                id: Uuid::nil(),
                email: "a@b.c".into(),
                display_name: "A".into(),
            },
            session_expires_at: dt,
        };
        let v = serde_json::to_value(&body).expect("json");
        assert_eq!(v["sessionExpiresAt"], "2026-11-12T10:00:00Z");
        assert_eq!(v["admin"]["displayName"], "A");
    }
}
