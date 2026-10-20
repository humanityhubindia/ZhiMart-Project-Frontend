//! End-to-end tests for the admin auth API against a real PostgreSQL database.
//!
//! Each test gets a fresh database from `#[sqlx::test]` (requires
//! `DATABASE_URL` pointing at a server where the user may create databases).

use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::anyhow;
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use zhimart_backend::{
    auth::password::hash_password,
    config::{AppEnv, Config, EmailProvider},
    email::{BoxFuture, EmailSender, OtpEmail},
    net::ResolvedClientIp,
    repo::admins::insert_admin,
    routes::auth::{credential_routes, session_routes},
    state::AppState,
};

const ADMIN_EMAIL: &str = "admin@zhimart.com";
const ADMIN_NAME: &str = "Admin (Operations)";
const ADMIN_PASSWORD: &str = "Correct-Horse-Battery-9";
const ORIGIN: &str = "http://localhost:5173";
const COOKIE: &str = "zm_rt";
const DEVICE_COOKIE: &str = "zm_dev";
const DEFAULT_IP: &str = "198.51.100.1";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Captures every OTP code "sent".
#[derive(Default)]
struct CapturingSender {
    codes: Mutex<Vec<String>>,
}

impl CapturingSender {
    fn last_code(&self) -> String {
        self.codes
            .lock()
            .expect("mutex")
            .last()
            .cloned()
            .expect("an OTP email was sent")
    }
    fn sent_count(&self) -> usize {
        self.codes.lock().expect("mutex").len()
    }
}

impl EmailSender for CapturingSender {
    fn send_otp<'a>(&'a self, email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            self.codes
                .lock()
                .map_err(|_| anyhow!("poisoned"))?
                .push(email.code.expose_secret().to_owned());
            Ok(())
        })
    }
}

fn test_config() -> Config {
    Config {
        app_env: AppEnv::Development,
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        database_url: SecretString::from("postgres://unused"),
        db_max_connections: 5,
        db_min_connections: 0,
        db_acquire_timeout: Duration::from_secs(5),
        jwt_secret: SecretString::from("integration-test-jwt-secret-0123456789abcdef"),
        jwt_issuer: "zhimart-api".into(),
        jwt_audience: "zhimart-admin".into(),
        access_token_ttl: Duration::from_secs(900),
        session_ttl: Duration::from_secs(30 * 24 * 60 * 60),
        otp_pepper: SecretString::from("integration-test-otp-pepper-0123456789abcdef"),
        otp_ttl: Duration::from_secs(300),
        otp_max_attempts: 5,
        otp_resend_cooldown: Duration::from_secs(60),
        otp_max_resends: 3,
        login_max_failures: 5,
        login_lockout: Duration::from_secs(900),
        cookie_secure: false,
        allowed_origins: vec![HeaderValue::from_static(ORIGIN)],
        trusted_proxies: vec![],
        rate_limit_global_per_minute: 300,
        rate_limit_auth_per_minute: 10,
        request_timeout: Duration::from_secs(15),
        max_body_bytes: 16_384,
        max_concurrent_requests: 1024,
        email_provider: EmailProvider::Log,
        ses_from_address: String::new(),
        ses_configuration_set: None,
        payments_enabled: true,
        payment_test_mode: false,
        cashfree_env: zhimart_backend::config::CashfreeEnv::Sandbox,
        cashfree_app_id: "TEST1234567890abcdef".into(),
        cashfree_secret_key: SecretString::from("cfsk_ma_test_0123456789abcdef"),
        cashfree_api_base: "https://sandbox.cashfree.com/pg".into(),
        cashfree_notify_url: None,
        payment_expiry: std::time::Duration::from_secs(30 * 60),
        store_name: "Zhi-Mart Fireworks".into(),
        order_notifications_enabled: false,
        order_notification_emails: Vec::new(),
        public_site_url: None,
        customer_order_emails_enabled: false,
        pricing: Default::default(),
        google_sheets: None,
    }
}

#[derive(Clone)]
struct Harness {
    app: Router,
    pool: PgPool,
    mail: Arc<CapturingSender>,
}

async fn setup(pool: PgPool, tweak: impl FnOnce(&mut Config)) -> Harness {
    let mut config = test_config();
    tweak(&mut config);
    let mail = Arc::new(CapturingSender::default());
    let sender: Arc<dyn EmailSender> = mail.clone();
    let state = AppState::new(Arc::new(config), pool.clone(), sender).expect("state");

    let hash = hash_password(SecretString::from(ADMIN_PASSWORD))
        .await
        .expect("hash");
    // Mixed case on purpose: storage must be normalised.
    insert_admin(&pool, "  Admin@ZhiMart.com ", ADMIN_NAME, &hash)
        .await
        .expect("insert admin");

    let app = Router::new()
        .nest("/api/v1", credential_routes().merge(session_routes()))
        .with_state(state);
    Harness { app, pool, mail }
}

fn ip(s: &str) -> IpAddr {
    s.parse().expect("ip")
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Resp {
    fn error_code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("<none>")
    }

    fn set_cookie(&self, name: &str) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with(&format!("{name}=")))
            .map(str::to_owned)
    }

    fn cookie_value(&self, name: &str) -> String {
        let sc = self
            .set_cookie(name)
            .unwrap_or_else(|| panic!("{name} cookie set"));
        let pair = sc.split(';').next().expect("cookie pair");
        pair[name.len() + 1..].to_owned()
    }

    fn refresh_set_cookie(&self) -> Option<String> {
        self.set_cookie(COOKIE)
    }

    fn refresh_cookie_value(&self) -> String {
        self.cookie_value(COOKIE)
    }

    fn assert_cookie_cleared(&self) {
        let sc = self.refresh_set_cookie().expect("clearing cookie set");
        assert!(
            sc.starts_with(&format!("{COOKIE}=;")),
            "cookie not emptied: {sc}"
        );
        assert!(sc.contains("Max-Age=0"), "cookie not expired: {sc}");
        assert!(sc.contains("Path=/api/v1/auth"), "wrong path: {sc}");
    }

    fn assert_no_store(&self) {
        assert_eq!(
            self.headers
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
    }
}

/// Per-request client context: source IP and cookies.
#[derive(Clone, Default)]
struct Client {
    ip: Option<IpAddr>,
    device: Option<String>,
}

impl Client {
    fn at(addr: &str) -> Self {
        Self {
            ip: Some(ip(addr)),
            device: None,
        }
    }
    fn with_device(mut self, device: &str) -> Self {
        self.device = Some(device.to_owned());
        self
    }
}

impl Harness {
    async fn send(&self, mut req: Request<Body>, client: &Client) -> Resp {
        let addr = client.ip.unwrap_or_else(|| ip(DEFAULT_IP));
        req.extensions_mut().insert(ResolvedClientIp(addr));
        let res = self.app.clone().oneshot(req).await.expect("infallible");
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        let resp = Resp {
            status,
            headers,
            body,
        };
        resp.assert_no_store();
        resp
    }

    async fn post_json_as(&self, path: &str, body: Value, client: &Client) -> Resp {
        let mut b = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1{path}"))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(d) = &client.device {
            b = b.header(header::COOKIE, format!("{DEVICE_COOKIE}={d}"));
        }
        self.send(
            b.body(Body::from(body.to_string())).expect("request"),
            client,
        )
        .await
    }

    async fn post_json(&self, path: &str, body: Value) -> Resp {
        self.post_json_as(path, body, &Client::default()).await
    }

    async fn login_as(&self, email: &str, password: &str, client: &Client) -> Resp {
        self.post_json_as(
            "/auth/login",
            json!({"email": email, "password": password}),
            client,
        )
        .await
    }

    async fn login(&self, email: &str, password: &str) -> Resp {
        self.login_as(email, password, &Client::default()).await
    }

    async fn verify_as(&self, challenge_id: &str, code: &str, client: &Client) -> Resp {
        self.post_json_as(
            "/auth/otp/verify",
            json!({"challengeId": challenge_id, "code": code}),
            client,
        )
        .await
    }

    async fn verify(&self, challenge_id: &str, code: &str) -> Resp {
        self.verify_as(challenge_id, code, &Client::default()).await
    }

    async fn resend(&self, challenge_id: &str) -> Resp {
        self.post_json("/auth/otp/resend", json!({"challengeId": challenge_id}))
            .await
    }

    /// POST /auth/refresh or /auth/logout with the refresh cookie.
    async fn cookie_post(&self, path: &str, cookie: Option<&str>, extra: &[(&str, &str)]) -> Resp {
        let mut b = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1{path}"));
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, format!("{COOKIE}={c}"));
        }
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        self.send(b.body(Body::empty()).expect("request"), &Client::default())
            .await
    }

    async fn refresh(&self, cookie: &str) -> Resp {
        self.cookie_post(
            "/auth/refresh",
            Some(cookie),
            &[("x-requested-with", "XMLHttpRequest"), ("origin", ORIGIN)],
        )
        .await
    }

    async fn me(&self, access_token: Option<&str>) -> Resp {
        let mut b = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/auth/me");
        if let Some(t) = access_token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        self.send(b.body(Body::empty()).expect("request"), &Client::default())
            .await
    }

    /// Password step; returns the challenge id.
    async fn start_login_as(&self, client: &Client) -> String {
        let r = self.login_as(ADMIN_EMAIL, ADMIN_PASSWORD, client).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        r.body["challengeId"]
            .as_str()
            .expect("challengeId")
            .to_owned()
    }

    async fn start_login(&self) -> String {
        self.start_login_as(&Client::default()).await
    }

    /// Full login; returns the verify response.
    async fn full_login_as(&self, client: &Client) -> Resp {
        let challenge = self.start_login_as(client).await;
        let r = self
            .verify_as(&challenge, &self.mail.last_code(), client)
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        r
    }

    /// Full login; returns (access token, refresh cookie value).
    async fn full_login(&self) -> (String, String) {
        let r = self.full_login_as(&Client::default()).await;
        (
            r.body["accessToken"]
                .as_str()
                .expect("accessToken")
                .to_owned(),
            r.refresh_cookie_value(),
        )
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&self.pool)
            .await
            .expect("count query")
    }

    /// Audit rows are written by background tasks; wait for them to land.
    async fn wait_audit(&self, event: &str, at_least: i64) -> i64 {
        let mut n = 0;
        for _ in 0..100 {
            n = sqlx::query_scalar("SELECT count(*) FROM auth_audit_log WHERE event = $1")
                .bind(event)
                .fetch_one(&self.pool)
                .await
                .expect("audit count");
            if n >= at_least {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        n
    }
}

/// A 6-digit code guaranteed to differ from `code`.
fn wrong_code(code: &str) -> String {
    let n: u32 = code.parse().expect("numeric code");
    format!("{:06}", (n + 1) % 1_000_000)
}

// ---------------------------------------------------------------------------
// Happy path, refresh rotation, reuse detection, grace window
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn happy_path_refresh_rotation_and_reuse_detection(pool: PgPool) {
    let h = setup(pool, |_| {}).await;

    // Password step (email is normalised server-side).
    let r = h.login("  ADMIN@zhimart.com ", ADMIN_PASSWORD).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["maskedEmail"], "a***n@zhimart.com");
    assert_eq!(r.body["expiresIn"], 300);
    assert_eq!(r.body["resendAvailableIn"], 60);
    let challenge = r.body["challengeId"]
        .as_str()
        .expect("challengeId")
        .to_owned();
    assert!(r.refresh_set_cookie().is_none());

    // OTP step.
    let code = h.mail.last_code();
    assert_eq!(code.len(), 6);
    let r = h.verify(&challenge, &code).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["tokenType"], "Bearer");
    assert_eq!(r.body["expiresIn"], 900);
    assert_eq!(r.body["admin"]["email"], ADMIN_EMAIL);
    assert_eq!(r.body["admin"]["displayName"], ADMIN_NAME);
    assert!(
        r.body["sessionExpiresAt"]
            .as_str()
            .expect("sessionExpiresAt")
            .ends_with('Z')
    );
    let access = r.body["accessToken"].as_str().expect("token").to_owned();
    let set_cookie = r.refresh_set_cookie().expect("cookie");
    for attr in ["HttpOnly", "SameSite=Strict", "Path=/api/v1/auth"] {
        assert!(set_cookie.contains(attr), "missing {attr}: {set_cookie}");
    }
    let max_age: i64 = set_cookie
        .split(';')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("Max-Age="))
        .expect("Max-Age")
        .parse()
        .expect("int");
    assert!(
        (2_591_990..=2_592_000).contains(&max_age),
        "Max-Age {max_age}"
    );
    let cookie1 = r.refresh_cookie_value();
    assert_eq!(cookie1.len(), 43);

    // A trusted-device cookie is issued too.
    let device = r.set_cookie(DEVICE_COOKIE).expect("device cookie");
    for attr in [
        "HttpOnly",
        "SameSite=Strict",
        "Path=/api/v1/auth",
        "Max-Age=15552000",
    ] {
        assert!(device.contains(attr), "missing {attr}: {device}");
    }
    assert_eq!(h.count("SELECT count(*) FROM trusted_devices").await, 1);

    // The code is single-use.
    assert_eq!(
        h.verify(&challenge, &code).await.error_code(),
        "otp_challenge_expired"
    );

    // Bearer access works; missing token does not.
    let r = h.me(Some(&access)).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["admin"]["email"], ADMIN_EMAIL);
    let r = h.me(None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error_code(), "unauthorized");

    // Refresh rotates the cookie and returns a new access token.
    let r = h.refresh(&cookie1).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cookie2 = r.refresh_cookie_value();
    assert_ne!(cookie1, cookie2);
    let access2 = r.body["accessToken"].as_str().expect("token").to_owned();
    assert_eq!(h.me(Some(&access2)).await.status, StatusCode::OK);

    // Using the successor ends cookie1's grace window.
    let r = h.refresh(&cookie2).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cookie3 = r.refresh_cookie_value();

    // Replaying a rotated-out cookie whose successor was used = theft.
    let r = h.refresh(&cookie1).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error_code(), "session_expired");
    r.assert_cookie_cleared();

    let r = h.refresh(&cookie3).await;
    assert_eq!(r.error_code(), "session_expired");
    r.assert_cookie_cleared();
    assert_eq!(h.me(Some(&access2)).await.status, StatusCode::UNAUTHORIZED);

    let reason: Option<String> = sqlx::query_scalar("SELECT revoke_reason FROM sessions")
        .fetch_one(&h.pool)
        .await
        .expect("session row");
    assert_eq!(reason.as_deref(), Some("refresh_token_reuse"));

    assert_eq!(h.wait_audit("refresh_reuse_detected", 1).await, 1);
    for event in [
        "login_password_ok",
        "otp_sent",
        "otp_verified",
        "session_refreshed",
        "session_expired",
    ] {
        assert!(
            h.wait_audit(event, 1).await >= 1,
            "missing audit event {event}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_retry_within_grace_window_succeeds(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (_, cookie1) = h.full_login().await;

    // The response carrying cookie2 is "lost".
    let r = h.refresh(&cookie1).await;
    assert_eq!(r.status, StatusCode::OK);
    let lost = r.refresh_cookie_value();

    // The client retries with the previous cookie: accepted, fresh successor.
    let r = h.refresh(&cookie1).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cookie2 = r.refresh_cookie_value();
    assert_ne!(cookie2, lost);
    assert_eq!(
        h.me(Some(r.body["accessToken"].as_str().expect("token")))
            .await
            .status,
        StatusCode::OK
    );

    // The session continues normally from the retried successor.
    let r = h.refresh(&cookie2).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cookie3 = r.refresh_cookie_value();

    // The lost cookie was retired by the retry; presenting it now is reuse.
    let r = h.refresh(&lost).await;
    assert_eq!(r.error_code(), "session_expired");
    assert_eq!(h.refresh(&cookie3).await.error_code(), "session_expired");
    assert_eq!(
        h.count("SELECT count(*) FROM sessions WHERE revoke_reason = 'refresh_token_reuse'")
            .await,
        1
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_reuse_after_grace_window_revokes(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (access, cookie1) = h.full_login().await;

    let r = h.refresh(&cookie1).await;
    assert_eq!(r.status, StatusCode::OK);
    let cookie2 = r.refresh_cookie_value();

    // Pretend the rotation happened 31 seconds ago.
    sqlx::query("UPDATE refresh_tokens SET used_at = now() - interval '31 seconds' WHERE used_at IS NOT NULL")
        .execute(&h.pool)
        .await
        .expect("age rotation");

    let r = h.refresh(&cookie1).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error_code(), "session_expired");
    r.assert_cookie_cleared();
    assert_eq!(h.refresh(&cookie2).await.error_code(), "session_expired");
    assert_eq!(h.me(Some(&access)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        h.count("SELECT count(*) FROM sessions WHERE revoke_reason = 'refresh_token_reuse'")
            .await,
        1
    );
}

// ---------------------------------------------------------------------------
// Credentials, throttling, account ceiling, trusted devices
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn unknown_email_and_wrong_password_are_indistinguishable(pool: PgPool) {
    let h = setup(pool, |_| {}).await;

    let unknown = h.login("nobody@zhimart.com", ADMIN_PASSWORD).await;
    let wrong = h.login(ADMIN_EMAIL, "not-the-password").await;
    for r in [&unknown, &wrong] {
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(r.error_code(), "invalid_credentials");
        assert_eq!(r.body["error"]["message"], "Invalid email or password.");
    }
    assert_eq!(h.mail.sent_count(), 0);

    // The unregistered address is never stored in clear in the audit log.
    assert_eq!(h.wait_audit("login_unknown_email", 1).await, 1);
    assert_eq!(
        h.count("SELECT count(*) FROM auth_audit_log WHERE metadata::text LIKE '%nobody@%'")
            .await,
        0
    );

    // Malformed input never reaches the credential check.
    let r = h
        .post_json(
            "/auth/login",
            json!({"email": ADMIN_EMAIL, "password": "x", "extra": 1}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.error_code(), "validation_error");
    let r = h.login("not-an-email", "x").await;
    assert_eq!(r.error_code(), "validation_error");
    let r = h.login(ADMIN_EMAIL, "").await;
    assert_eq!(r.error_code(), "validation_error");
}

#[sqlx::test(migrations = "./migrations")]
async fn throttle_is_identical_for_unknown_and_known_emails(pool: PgPool) {
    let h = setup(pool, |c| c.login_max_failures = 3).await;
    let attacker = Client::at("203.0.113.9");

    for email in [ADMIN_EMAIL, "ghost@zhimart.com"] {
        for _ in 0..3 {
            let r = h.login_as(email, "wrong-password", &attacker).await;
            assert_eq!(r.error_code(), "invalid_credentials");
        }
    }
    // Known and unknown e-mail: same status, code, message and retry window.
    let known = h.login_as(ADMIN_EMAIL, ADMIN_PASSWORD, &attacker).await;
    let unknown = h
        .login_as("ghost@zhimart.com", ADMIN_PASSWORD, &attacker)
        .await;
    for r in [&known, &unknown] {
        assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(r.error_code(), "too_many_requests");
        assert!(r.headers.contains_key(header::RETRY_AFTER));
    }
    assert_eq!(
        known.body["error"]["message"],
        unknown.body["error"]["message"]
    );
    let (a, b) = (
        known.body["error"]["retryAfter"]
            .as_u64()
            .expect("retryAfter"),
        unknown.body["error"]["retryAfter"]
            .as_u64()
            .expect("retryAfter"),
    );
    assert!(
        (890..=900).contains(&a) && (890..=900).contains(&b),
        "{a} vs {b}"
    );
    assert_eq!(h.mail.sent_count(), 0);

    // The real admin is not locked out from elsewhere (no account-wide DoS).
    let r = h
        .login_as(ADMIN_EMAIL, ADMIN_PASSWORD, &Client::at("192.0.2.44"))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Once the key's lock lapses the next failure doubles the lock.
    sqlx::query("UPDATE login_throttles SET locked_until = now() - interval '1 second'")
        .execute(&h.pool)
        .await
        .expect("expire locks");
    for _ in 0..3 {
        assert_eq!(
            h.login_as(ADMIN_EMAIL, "wrong-password", &attacker)
                .await
                .error_code(),
            "invalid_credentials"
        );
    }
    let r = h.login_as(ADMIN_EMAIL, ADMIN_PASSWORD, &attacker).await;
    assert_eq!(r.error_code(), "too_many_requests");
    let retry = r.body["error"]["retryAfter"].as_u64().expect("retryAfter");
    assert!(
        (1790..=1800).contains(&retry),
        "second lock should be 30 min, got {retry}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_logins_cannot_exceed_the_budget(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let attacker = Client::at("203.0.113.50");

    // 4 of 5 attempts used: exactly one slot left.
    for _ in 0..4 {
        assert_eq!(
            h.login_as(ADMIN_EMAIL, "wrong-password", &attacker)
                .await
                .error_code(),
            "invalid_credentials"
        );
    }

    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..30 {
        let (h, client) = (h.clone(), attacker.clone());
        let password = if i == 15 {
            ADMIN_PASSWORD.to_owned()
        } else {
            format!("wrong-password-{i}")
        };
        tasks.spawn(async move {
            let r = h.login_as(ADMIN_EMAIL, &password, &client).await;
            (
                password == ADMIN_PASSWORD,
                r.status,
                r.error_code().to_owned(),
            )
        });
    }
    let mut wrong_evaluated = 0;
    let mut correct_status = None;
    while let Some(res) = tasks.join_next().await {
        let (is_correct, status, code) = res.expect("task");
        if is_correct {
            correct_status = Some(status);
        } else if code == "invalid_credentials" {
            wrong_evaluated += 1;
        } else {
            assert_eq!(
                code, "too_many_requests",
                "unexpected outcome {status} {code}"
            );
        }
    }

    // Never more evaluations than the remaining budget allows (the correct
    // password's charge is refunded, which can free one slot afterwards).
    assert!(
        wrong_evaluated <= 1,
        "{wrong_evaluated} wrong passwords evaluated"
    );
    let challenges = h.count("SELECT count(*) FROM otp_challenges").await;
    match correct_status {
        Some(StatusCode::OK) => assert_eq!(challenges, 1),
        Some(StatusCode::TOO_MANY_REQUESTS) => {
            // Throttled ⇒ no challenge was issued.
            assert_eq!(challenges, 0);
            assert_eq!(wrong_evaluated, 1);
        }
        other => panic!("unexpected status for the correct password: {other:?}"),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_burst_from_scratch_is_bounded(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let attacker = Client::at("203.0.113.51");

    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..30 {
        let (h, client) = (h.clone(), attacker.clone());
        let password = if i == 7 {
            ADMIN_PASSWORD.to_owned()
        } else {
            format!("wrong-password-{i}")
        };
        tasks.spawn(async move {
            let r = h.login_as(ADMIN_EMAIL, &password, &client).await;
            (
                password == ADMIN_PASSWORD,
                r.status,
                r.error_code().to_owned(),
            )
        });
    }
    let mut wrong_evaluated = 0;
    let mut correct_ok = false;
    while let Some(res) = tasks.join_next().await {
        let (is_correct, status, code) = res.expect("task");
        if is_correct {
            correct_ok = status == StatusCode::OK;
        } else if code == "invalid_credentials" {
            wrong_evaluated += 1;
        }
    }
    assert!(
        wrong_evaluated <= 5,
        "{wrong_evaluated} wrong passwords evaluated with a budget of 5"
    );
    if !correct_ok {
        assert_eq!(h.count("SELECT count(*) FROM otp_challenges").await, 0);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn account_ceiling_blocks_new_devices_but_not_trusted_ones(pool: PgPool) {
    let h = setup(pool, |_| {}).await;

    // A completed login from the admin's usual browser makes it trusted.
    let r = h.full_login_as(&Client::at("192.0.2.10")).await;
    let device = r.cookie_value(DEVICE_COOKIE);
    assert_eq!(device.len(), 43);

    // A distributed attack brings the account to the edge of its ceiling…
    sqlx::query("UPDATE admins SET failed_login_count = 49, failure_window_started_at = now()")
        .execute(&h.pool)
        .await
        .expect("prime ceiling");
    // …and one more failure from yet another IP trips it.
    let r = h
        .login_as(ADMIN_EMAIL, "wrong-password", &Client::at("203.0.113.77"))
        .await;
    assert_eq!(r.error_code(), "invalid_credentials");

    // New devices are refused account-wide (even with the right password)…
    let r = h
        .login_as(ADMIN_EMAIL, ADMIN_PASSWORD, &Client::at("192.0.2.99"))
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.error_code(), "account_locked");
    assert!(r.body["error"]["retryAfter"].as_u64().expect("retryAfter") <= 900);
    assert_eq!(h.wait_audit("login_locked", 1).await, 1);

    // …while the trusted browser logs in normally, even from a new IP.
    let trusted = Client::at("192.0.2.11").with_device(&device);
    let challenge = h.start_login_as(&trusted).await;

    // The OTP step enforces the same rule.
    let code = h.mail.last_code();
    let r = h
        .verify_as(&challenge, &code, &Client::at("192.0.2.11"))
        .await;
    assert_eq!(r.error_code(), "account_locked");
    let r = h.verify_as(&challenge, &code, &trusted).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // The trusted cookie is renewed with the same value.
    assert_eq!(r.cookie_value(DEVICE_COOKIE), device);

    // A forged / foreign device cookie does not help.
    let forged = Client::at("192.0.2.12").with_device(&"A".repeat(43));
    assert_eq!(
        h.login_as(ADMIN_EMAIL, ADMIN_PASSWORD, &forged)
            .await
            .error_code(),
        "account_locked"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn wrong_otp_counts_down_then_expires(pool: PgPool) {
    let h = setup(pool, |c| {
        c.otp_max_attempts = 3;
        c.login_max_failures = 10;
    })
    .await;
    let challenge = h.start_login().await;
    let code = h.mail.last_code();
    let bad = wrong_code(&code);

    let r = h.verify(&challenge, &bad).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error_code(), "otp_invalid");
    assert_eq!(r.body["error"]["attemptsRemaining"], 2);

    let r = h.verify(&challenge, &bad).await;
    assert_eq!(r.error_code(), "otp_invalid");
    assert_eq!(r.body["error"]["attemptsRemaining"], 1);

    let r = h.verify(&challenge, &bad).await;
    assert_eq!(r.error_code(), "otp_challenge_expired");

    // The real code no longer helps: the challenge is dead.
    assert_eq!(
        h.verify(&challenge, &code).await.error_code(),
        "otp_challenge_expired"
    );

    // OTP failures feed both the per-key throttle and the account-wide counter
    // (the successful password step refunded its own charge).
    assert_eq!(
        h.count("SELECT failure_count::bigint FROM login_throttles")
            .await,
        3
    );
    assert_eq!(
        h.count("SELECT failed_login_count::bigint FROM admins")
            .await,
        3
    );

    // Malformed codes are rejected before touching the challenge.
    let r = h.verify(&challenge, "12345").await;
    assert_eq!(r.error_code(), "validation_error");
    let r = h.verify("not-a-uuid", "123456").await;
    assert_eq!(r.error_code(), "validation_error");

    assert_eq!(h.wait_audit("otp_failed", 3).await, 3);
    assert_eq!(h.wait_audit("otp_challenge_exhausted", 1).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn otp_guessing_is_throttled_per_key(pool: PgPool) {
    let h = setup(pool, |c| {
        c.login_max_failures = 3;
        c.otp_max_attempts = 10;
    })
    .await;
    let challenge = h.start_login().await;
    let bad = wrong_code(&h.mail.last_code());

    for _ in 0..3 {
        assert_eq!(h.verify(&challenge, &bad).await.error_code(), "otp_invalid");
    }
    let r = h.verify(&challenge, &bad).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.error_code(), "too_many_requests");
    // The throttled attempt did not consume a challenge attempt.
    assert_eq!(
        h.count("SELECT attempts::bigint FROM otp_challenges").await,
        3
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn resend_respects_cooldown_and_replaces_code(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let challenge = h.start_login().await;
    let old_code = h.mail.last_code();

    let r = h.resend(&challenge).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.error_code(), "too_many_requests");
    let retry = r.body["error"]["retryAfter"].as_u64().expect("retryAfter");
    assert!((1..=60).contains(&retry), "retryAfter {retry}");
    assert!(r.headers.contains_key(header::RETRY_AFTER));

    // Pretend the cooldown elapsed.
    sqlx::query("UPDATE otp_challenges SET last_sent_at = now() - interval '2 minutes'")
        .execute(&h.pool)
        .await
        .expect("age challenge");
    let r = h.resend(&challenge).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["expiresIn"], 300);
    assert_eq!(r.body["resendAvailableIn"], 60);
    assert_eq!(h.mail.sent_count(), 2);
    let new_code = h.mail.last_code();

    if new_code != old_code {
        assert_eq!(
            h.verify(&challenge, &old_code).await.error_code(),
            "otp_invalid"
        );
    }
    assert_eq!(h.verify(&challenge, &new_code).await.status, StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn contended_challenge_lock_answers_429(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let challenge = h.start_login().await;
    let code = h.mail.last_code();

    // Simulate a resend holding the challenge row (e.g. waiting on SES).
    let mut holder = h.pool.begin().await.expect("tx");
    sqlx::query("SELECT 1 FROM otp_challenges FOR UPDATE")
        .execute(&mut *holder)
        .await
        .expect("lock");

    let started = std::time::Instant::now();
    let r = h.verify(&challenge, &code).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{:?}", r.body);
    assert_eq!(r.error_code(), "too_many_requests");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited {:?}",
        started.elapsed()
    );
    holder.rollback().await.expect("rollback");

    // Nothing was consumed: the code still works once the lock is gone.
    assert_eq!(h.verify(&challenge, &code).await.status, StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn otp_issuance_is_capped(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    for _ in 0..5 {
        h.start_login().await;
    }
    let r = h.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.error_code(), "too_many_requests");
    assert_eq!(h.mail.sent_count(), 5);

    // Only the newest challenge is still pending.
    assert_eq!(
        h.count("SELECT count(*) FROM otp_challenges WHERE consumed_at IS NULL AND invalidated_at IS NULL").await,
        1
    );
}

// ---------------------------------------------------------------------------
// CSRF, expiry, logout, password reset, maintenance
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn refresh_requires_csrf_headers(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (_, cookie) = h.full_login().await;

    let r = h.cookie_post("/auth/refresh", Some(&cookie), &[]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.error_code(), "forbidden");

    let r = h
        .cookie_post(
            "/auth/refresh",
            Some(&cookie),
            &[
                ("x-requested-with", "XMLHttpRequest"),
                ("origin", "https://evil.example"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let r = h
        .cookie_post(
            "/auth/refresh",
            Some(&cookie),
            &[
                ("x-requested-with", "XMLHttpRequest"),
                ("sec-fetch-site", "cross-site"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // The rejected attempts did not burn the token.
    assert_eq!(h.refresh(&cookie).await.status, StatusCode::OK);

    // Missing / garbage cookie → session_expired with the cookie cleared.
    let r = h
        .cookie_post(
            "/auth/refresh",
            None,
            &[("x-requested-with", "XMLHttpRequest")],
        )
        .await;
    assert_eq!(r.error_code(), "session_expired");
    r.assert_cookie_cleared();
    let r = h.refresh("garbage").await;
    assert_eq!(r.error_code(), "session_expired");
    r.assert_cookie_cleared();
}

#[sqlx::test(migrations = "./migrations")]
async fn session_ends_at_absolute_expiry(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (access, cookie) = h.full_login().await;

    sqlx::query(
        "UPDATE sessions SET created_at = now() - interval '31 days', \
         absolute_expires_at = now() - interval '1 second'",
    )
    .execute(&h.pool)
    .await
    .expect("expire session");

    let r = h.refresh(&cookie).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error_code(), "session_expired");
    r.assert_cookie_cleared();

    // The still-unexpired access token is dead too (session checked per request).
    assert_eq!(h.me(Some(&access)).await.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_revokes_session_immediately(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (access, cookie) = h.full_login().await;
    assert_eq!(h.me(Some(&access)).await.status, StatusCode::OK);

    // CSRF applies to logout too.
    let r = h.cookie_post("/auth/logout", Some(&cookie), &[]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let r = h
        .cookie_post(
            "/auth/logout",
            Some(&cookie),
            &[("x-requested-with", "XMLHttpRequest")],
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    r.assert_cookie_cleared();

    let r = h.me(Some(&access)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error_code(), "unauthorized");
    assert_eq!(h.refresh(&cookie).await.error_code(), "session_expired");

    // Idempotent.
    let r = h
        .cookie_post(
            "/auth/logout",
            Some(&cookie),
            &[("x-requested-with", "XMLHttpRequest")],
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = h
        .cookie_post(
            "/auth/logout",
            None,
            &[("x-requested-with", "XMLHttpRequest")],
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    let reason: Option<String> = sqlx::query_scalar("SELECT revoke_reason FROM sessions")
        .fetch_one(&h.pool)
        .await
        .expect("session");
    assert_eq!(reason.as_deref(), Some("logout"));
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_with_bearer_only(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (access, _cookie) = h.full_login().await;

    let auth = format!("Bearer {access}");
    let r = h
        .cookie_post(
            "/auth/logout",
            None,
            &[
                ("x-requested-with", "XMLHttpRequest"),
                ("authorization", &auth),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(h.me(Some(&access)).await.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn password_reset_revokes_sessions_and_devices(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    let (access, cookie) = h.full_login().await;
    assert_eq!(h.count("SELECT count(*) FROM trusted_devices").await, 1);

    let new_hash = hash_password(SecretString::from("Another-Strong-Pass-42"))
        .await
        .expect("hash");
    let found =
        zhimart_backend::repo::admins::reset_password(&h.pool, "ADMIN@zhimart.com", &new_hash)
            .await
            .expect("reset");
    assert!(found);

    assert_eq!(h.me(Some(&access)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(h.refresh(&cookie).await.error_code(), "session_expired");
    assert_eq!(h.count("SELECT count(*) FROM trusted_devices").await, 0);
    assert_eq!(
        h.login(ADMIN_EMAIL, ADMIN_PASSWORD).await.error_code(),
        "invalid_credentials"
    );
    assert_eq!(
        h.login(ADMIN_EMAIL, "Another-Strong-Pass-42").await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn purge_handles_rotated_token_chains(pool: PgPool) {
    let h = setup(pool, |_| {}).await;
    assert_eq!(
        h.count(
            "SELECT count(*) FROM pg_indexes WHERE indexname = 'refresh_tokens_replaced_by_idx'"
        )
        .await,
        1
    );

    // A chain of rotated tokens linked through replaced_by.
    let (_, mut cookie) = h.full_login().await;
    for _ in 0..3 {
        let r = h.refresh(&cookie).await;
        assert_eq!(r.status, StatusCode::OK);
        cookie = r.refresh_cookie_value();
    }
    assert_eq!(
        h.count("SELECT count(*) FROM refresh_tokens WHERE replaced_by IS NOT NULL")
            .await,
        3
    );

    // A stale throttle key from a failed attempt.
    assert_eq!(
        h.login("ghost@zhimart.com", "x").await.error_code(),
        "invalid_credentials"
    );

    sqlx::query("UPDATE otp_challenges SET expires_at = now() - interval '2 days'")
        .execute(&h.pool)
        .await
        .expect("age challenges");
    sqlx::query("UPDATE refresh_tokens SET expires_at = now() - interval '2 days'")
        .execute(&h.pool)
        .await
        .expect("age tokens");
    sqlx::query("UPDATE login_throttles SET updated_at = now() - interval '2 days'")
        .execute(&h.pool)
        .await
        .expect("age throttles");
    sqlx::query("UPDATE trusted_devices SET expires_at = now() - interval '1 second'")
        .execute(&h.pool)
        .await
        .expect("expire devices");

    let stats = zhimart_backend::repo::maintenance::purge_expired(&h.pool)
        .await
        .expect("purge");
    assert_eq!(stats.otp_challenges, 1);
    assert_eq!(stats.sessions, 0);
    assert_eq!(stats.refresh_tokens, 4);
    assert_eq!(stats.login_throttles, 1);
    assert_eq!(stats.trusted_devices, 1);
    assert_eq!(h.count("SELECT count(*) FROM refresh_tokens").await, 0);
}
