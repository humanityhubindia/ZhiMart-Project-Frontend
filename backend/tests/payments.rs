//! End-to-end tests for Cashfree payments: guest pay/verify/status, the
//! webhook, the expiry-sweep reconcile, late payments and admin refunds —
//! against a real PostgreSQL database (`#[sqlx::test]`) and a mock Cashfree PG
//! API served by axum on 127.0.0.1:0 (API-key headers and API version checked,
//! idempotency keys recorded, controllable responses).
//!
//! Orders are seeded with plain SQL so these tests do not depend on the
//! checkout handler.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use zhimart_backend::{
    config::{AppEnv, CashfreeEnv, Config, EmailProvider},
    email::{BoxFuture, EmailSender, OtpEmail},
    payments::{
        cashfree::{self, API_VERSION},
        order_token,
        service::{self, SweepReconcile},
        signature,
    },
    repo, routes,
    state::AppState,
};

const APP_ID: &str = "TEST1234567890abcdef";
const SECRET_KEY: &str = "cfsk_ma_test_0123456789abcdef";
const PRODUCT: &str = "zm-pay-test";
const UNIT_PRICE: i64 = 16_500;
const QTY: i32 = 10;
const TOTAL: i64 = UNIT_PRICE * QTY as i64;
const SITE: &str = "https://zhimart.in";

// ---------------------------------------------------------------------------
// Mock Cashfree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefundMode {
    /// Accept; refund status `SUCCESS`.
    Success,
    /// Accept; refund status `PENDING` (completed later; webhook).
    Pending,
    /// 400 `request_failed`.
    Reject,
    /// Accept and store the refund, but answer 500 (response lost).
    LostResponse,
}

struct MockState {
    orders: HashMap<String, Value>,
    payments: HashMap<String, Vec<Value>>,
    refunds: Vec<Value>,
    refund_mode: RefundMode,
    create_calls: u32,
    /// Next create stores the order but answers 500 (response lost).
    lose_next_create: bool,
    payment_reads: u32,
    /// GET .../payments answers 503.
    payments_unavailable: bool,
    /// GET .../refunds[/{id}] answers 503.
    refund_reads_unavailable: bool,
    refund_calls: u32,
    unauthorized: u32,
    create_bodies: Vec<Value>,
    /// `(path, x-idempotency-key)` of every POST.
    idempotency_keys: Vec<(String, Option<String>)>,
    next_refund: u64,
}

#[derive(Clone)]
struct Mock(Arc<Mutex<MockState>>);

impl Mock {
    fn with<R>(&self, f: impl FnOnce(&mut MockState) -> R) -> R {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut g)
    }

    /// Record a payment attempt on a Cashfree order. A `SUCCESS` marks the order PAID.
    fn add_payment(
        &self,
        order_id: &str,
        cf_payment_id: u64,
        amount_paise: i64,
        status: &str,
    ) -> Value {
        let p = payment_entity(order_id, cf_payment_id, amount_paise, status);
        self.with(|s| {
            s.payments
                .entry(order_id.to_owned())
                .or_default()
                .push(p.clone());
            if status == "SUCCESS"
                && let Some(o) = s.orders.get_mut(order_id)
            {
                o["order_status"] = json!("PAID");
            }
        });
        p
    }

    fn set_refund_status(&self, cf_refund_id: &str, status: &str) {
        self.with(|s| {
            for r in &mut s.refunds {
                if r["cf_refund_id"] == json!(cf_refund_id) {
                    r["refund_status"] = json!(status);
                }
            }
        });
    }
}

fn rupees(paise: i64) -> f64 {
    paise as f64 / 100.0
}

fn payment_entity(order_id: &str, cf_payment_id: u64, amount_paise: i64, status: &str) -> Value {
    json!({
        "cf_payment_id": cf_payment_id, "order_id": order_id, "entity": "payment",
        "payment_amount": rupees(amount_paise), "payment_currency": "INR",
        "payment_status": status, "payment_message": "00::Transaction success",
        "payment_time": "2026-10-20T12:20:29+05:30", "payment_group": "upi", "is_captured": status == "SUCCESS",
        "bank_reference": "234928698581", "auth_id": null,
        "payment_method": { "upi": { "channel": "collect", "upi_id": "customer@upi" } },
        "error_details": if status == "FAILED" {
            json!({ "error_code": "TRANSACTION_DECLINED", "error_description": "declined", "error_reason": "bank" })
        } else { Value::Null }
    })
}

fn cf_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "message": message, "code": code, "type": "invalid_request_error" })),
    )
        .into_response()
}

fn auth_ok(headers: &HeaderMap) -> bool {
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    h("x-client-id") == Some(APP_ID)
        && h("x-client-secret") == Some(SECRET_KEY)
        && h("x-api-version") == Some(API_VERSION)
        && h("x-request-id").is_some_and(|r| Uuid::parse_str(r).is_ok())
}

macro_rules! require_auth {
    ($mock:expr, $headers:expr) => {
        if !auth_ok(&$headers) {
            $mock.with(|s| s.unauthorized += 1);
            return cf_error(
                StatusCode::UNAUTHORIZED,
                "request_failed",
                "authentication Failed",
            );
        }
    };
}

fn idempotency(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

async fn mock_create_order(
    State(m): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    require_auth!(m, headers);
    let key = idempotency(&headers);
    m.with(|s| {
        s.create_calls += 1;
        s.create_bodies.push(body.clone());
        s.idempotency_keys.push(("/orders".into(), key));
        let id = body["order_id"].as_str().unwrap_or_default().to_owned();
        if s.orders.contains_key(&id) {
            return cf_error(
                StatusCode::CONFLICT,
                "order_already_exists",
                "order with same id is already present",
            );
        }
        let n = s.orders.len() + 1;
        let order = json!({
            "cf_order_id": 2_149_460_000_u64 + n as u64, "order_id": id, "entity": "order",
            "order_currency": body["order_currency"], "order_amount": body["order_amount"],
            "order_status": "ACTIVE", "payment_session_id": format!("session_{id}_abcDEF0123"),
            "order_expiry_time": body["order_expiry_time"], "order_note": body["order_note"],
            "created_at": "2026-10-20T12:00:00+05:30", "order_meta": body["order_meta"],
            "order_tags": body["order_tags"], "customer_details": body["customer_details"]
        });
        s.orders.insert(id, order.clone());
        if s.lose_next_create {
            s.lose_next_create = false;
            return cf_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "upstream timeout",
            );
        }
        Json(order).into_response()
    })
}

async fn mock_fetch_order(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    require_auth!(m, headers);
    match m.with(|s| s.orders.get(&id).cloned()) {
        Some(o) => Json(o).into_response(),
        None => cf_error(StatusCode::NOT_FOUND, "order_not_found", "order not found"),
    }
}

async fn mock_order_payments(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    require_auth!(m, headers);
    m.with(|s| {
        s.payment_reads += 1;
        if s.payments_unavailable {
            return cf_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "internal_error",
                "try again",
            );
        }
        if !s.orders.contains_key(&id) {
            return cf_error(StatusCode::NOT_FOUND, "order_not_found", "order not found");
        }
        Json(json!(s.payments.get(&id).cloned().unwrap_or_default())).into_response()
    })
}

async fn mock_create_refund(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    require_auth!(m, headers);
    let key = idempotency(&headers);
    m.with(|s| {
        s.refund_calls += 1;
        s.idempotency_keys.push((format!("/orders/{id}/refunds"), key.clone()));
        if s.refund_mode == RefundMode::Reject {
            return cf_error(StatusCode::BAD_REQUEST, "request_failed", "refund amount is more than the eligible amount");
        }
        // Idempotency: the same key replays the same refund.
        if let Some(k) = &key
            && let Some(r) = s.refunds.iter().find(|r| r["_idem"].as_str() == Some(k.as_str()))
        {
            return Json(r.clone()).into_response();
        }
        if s.refunds.iter().any(|r| r["refund_id"] == body["refund_id"]) {
            return cf_error(StatusCode::CONFLICT, "refund_already_exists", "refund with same id is already present");
        }
        let Some(paid) = s
            .payments
            .get(&id)
            .and_then(|ps| ps.iter().find(|p| p["payment_status"] == "SUCCESS").cloned())
        else {
            return cf_error(StatusCode::BAD_REQUEST, "request_failed", "order is not paid");
        };
        s.next_refund += 1;
        let status = if s.refund_mode == RefundMode::Pending { "PENDING" } else { "SUCCESS" };
        let r = json!({
            "cf_payment_id": paid["cf_payment_id"], "cf_refund_id": format!("{}", 78_000_000 + s.next_refund),
            "refund_id": body["refund_id"], "order_id": id, "entity": "refund",
            "refund_amount": body["refund_amount"], "refund_currency": "INR", "refund_note": body["refund_note"],
            "refund_status": status, "refund_type": "MERCHANT_INITIATED", "status_description": "In progress",
            "refund_speed": body["refund_speed"], "_idem": key,
        });
        s.refunds.push(r.clone());
        if s.refund_mode == RefundMode::LostResponse {
            return cf_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "upstream timeout");
        }
        Json(r).into_response()
    })
}

async fn mock_fetch_refund(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path((id, rid)): Path<(String, String)>,
) -> Response {
    require_auth!(m, headers);
    m.with(|s| {
        if s.refund_reads_unavailable {
            return cf_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "internal_error",
                "try again",
            );
        }
        match s
            .refunds
            .iter()
            .find(|r| r["order_id"] == json!(id) && r["refund_id"] == json!(rid))
        {
            Some(r) => Json(r.clone()).into_response(),
            None => cf_error(
                StatusCode::NOT_FOUND,
                "refund_not_found",
                "refund does not exist",
            ),
        }
    })
}

async fn mock_order_refunds(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    require_auth!(m, headers);
    m.with(|s| {
        if s.refund_reads_unavailable {
            return cf_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "internal_error",
                "try again",
            );
        }
        let items: Vec<Value> = s
            .refunds
            .iter()
            .filter(|r| r["order_id"] == json!(id))
            .cloned()
            .collect();
        Json(json!(items)).into_response()
    })
}

async fn start_mock() -> (Mock, String) {
    let mock = Mock(Arc::new(Mutex::new(MockState {
        orders: HashMap::new(),
        payments: HashMap::new(),
        refunds: Vec::new(),
        refund_mode: RefundMode::Success,
        create_calls: 0,
        lose_next_create: false,
        payment_reads: 0,
        payments_unavailable: false,
        refund_reads_unavailable: false,
        refund_calls: 0,
        unauthorized: 0,
        create_bodies: Vec::new(),
        idempotency_keys: Vec::new(),
        next_refund: 0,
    })));
    let app = Router::new()
        .route("/pg/orders", post(mock_create_order))
        .route("/pg/orders/{id}", get(mock_fetch_order))
        .route("/pg/orders/{id}/payments", get(mock_order_payments))
        .route(
            "/pg/orders/{id}/refunds",
            post(mock_create_refund).get(mock_order_refunds),
        )
        .route("/pg/orders/{id}/refunds/{rid}", get(mock_fetch_refund))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (mock, format!("http://{addr}/pg"))
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct NoopSender;

impl EmailSender for NoopSender {
    fn send_otp<'a>(&'a self, _email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn test_config(api_base: String) -> Config {
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
        allowed_origins: vec![HeaderValue::from_static("http://localhost:5173")],
        trusted_proxies: vec![],
        rate_limit_global_per_minute: 300,
        rate_limit_auth_per_minute: 10,
        request_timeout: Duration::from_secs(15),
        max_body_bytes: 65_536,
        max_concurrent_requests: 1024,
        email_provider: EmailProvider::Log,
        ses_from_address: String::new(),
        ses_configuration_set: None,
        payments_enabled: true,
        payment_test_mode: false,
        cashfree_env: CashfreeEnv::Sandbox,
        cashfree_app_id: APP_ID.into(),
        cashfree_secret_key: SecretString::from(SECRET_KEY),
        cashfree_api_base: api_base,
        cashfree_notify_url: None,
        payment_expiry: Duration::from_secs(30 * 60),
        store_name: "Zhi-Mart Fireworks".into(),
        order_notifications_enabled: false,
        order_notification_emails: Vec::new(),
        public_site_url: Some(SITE.into()),
        customer_order_emails_enabled: false,
        pricing: Default::default(),
        google_sheets: None,
    }
}

#[derive(Clone)]
struct H {
    app: Router,
    pool: PgPool,
    state: AppState,
    mock: Mock,
    admin_token: String,
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Resp {
    fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("")
    }
}

/// A seeded `pending_payment` order.
#[derive(Debug, Clone)]
struct Seeded {
    id: Uuid,
    number: String,
    token: String,
}

impl Seeded {
    /// The Cashfree order id the backend uses for this order.
    fn gid(&self) -> String {
        cashfree::gateway_order_id(&self.number, self.id)
    }
}

static ORDER_SEQ: AtomicU32 = AtomicU32::new(5001);
static PAYMENT_SEQ: AtomicU32 = AtomicU32::new(1);

/// A fresh numeric `cf_payment_id`.
fn next_pid() -> u64 {
    5_114_910_000_000 + u64::from(PAYMENT_SEQ.fetch_add(1, Ordering::Relaxed))
}

async fn setup(pool: PgPool) -> H {
    setup_with(pool, |_| {}).await
}

async fn setup_with(pool: PgPool, tweak: impl FnOnce(&mut Config)) -> H {
    let (mock, base) = start_mock().await;
    let mut config = test_config(base);
    tweak(&mut config);
    let state = AppState::new(Arc::new(config), pool.clone(), Arc::new(NoopSender)).expect("state");

    sqlx::query(
        "INSERT INTO products (id, name, category, brand, pack_info, price_paise, original_price_paise, image_url, stock) \
         VALUES ($1, 'Payment Test Box', 'TEST', 'Zhi-mart Fireworks', '10 Packet(s)', $2, 30000, '/assets/x.jpg', 100)",
    )
    .bind(PRODUCT)
    .bind(UNIT_PRICE)
    .execute(&pool)
    .await
    .expect("insert product");

    let admin_id = Uuid::now_v7();
    sqlx::query("INSERT INTO admins (id, email, password_hash, display_name) VALUES ($1, 'ops@zhimart.com', 'unused', 'Ops')")
        .bind(admin_id)
        .execute(&pool)
        .await
        .expect("insert admin");
    let session_id = Uuid::now_v7();
    let expires: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO sessions (id, admin_id, absolute_expires_at) VALUES ($1, $2, now() + interval '1 day') \
         RETURNING absolute_expires_at",
    )
    .bind(session_id)
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .expect("insert session");
    let admin_token = state
        .jwt
        .issue(admin_id, session_id, expires)
        .expect("token")
        .token;

    let app = Router::new()
        .nest(
            "/api/v1",
            routes::payments::public_routes()
                .merge(routes::payments::verify_routes())
                .merge(routes::payments::webhook_routes())
                .nest("/admin", routes::admin::payments::routes()),
        )
        .with_state(state.clone());
    H {
        app,
        pool,
        state,
        mock,
        admin_token,
    }
}

impl H {
    async fn raw(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, String)],
        body: Option<Vec<u8>>,
    ) -> Resp {
        let mut req = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let body = match body {
            Some(b) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(b)
            }
            None => Body::empty(),
        };
        let res = self
            .app
            .clone()
            .oneshot(req.body(body).expect("request"))
            .await
            .expect("response");
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        Resp {
            status,
            headers,
            body,
        }
    }

    async fn guest(&self, method: Method, uri: &str, token: &str, body: Option<Value>) -> Resp {
        self.raw(
            method,
            uri,
            &[("x-order-token", token.to_owned())],
            body.map(|b| b.to_string().into_bytes()),
        )
        .await
    }

    async fn pay(&self, o: &Seeded) -> Resp {
        self.guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay", o.number),
            &o.token,
            None,
        )
        .await
    }

    async fn verify(&self, o: &Seeded) -> Resp {
        self.guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay/verify", o.number),
            &o.token,
            Some(json!({})),
        )
        .await
    }

    /// POST the webhook signed with `key` at timestamp `ts`.
    async fn webhook_signed(&self, body: &[u8], key: &str, ts: &str) -> Resp {
        let sig = signature::webhook_signature(key.as_bytes(), ts, body);
        self.raw(
            Method::POST,
            "/api/v1/webhooks/cashfree",
            &[
                ("x-webhook-signature", sig),
                ("x-webhook-timestamp", ts.to_owned()),
                ("x-webhook-version", API_VERSION.to_owned()),
            ],
            Some(body.to_vec()),
        )
        .await
    }

    async fn webhook(&self, body: &Value) -> Resp {
        let ts = Utc::now().timestamp_millis().to_string();
        self.webhook_signed(body.to_string().as_bytes(), SECRET_KEY, &ts)
            .await
    }

    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> Resp {
        self.raw(
            method,
            uri,
            &[("authorization", format!("Bearer {}", self.admin_token))],
            body.map(|b| b.to_string().into_bytes()),
        )
        .await
    }

    async fn refund(&self, o: &Seeded, body: Value) -> Resp {
        self.admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(body),
        )
        .await
    }

    /// Seed a `pending_payment` Cashfree order for QTY units of PRODUCT (stock reserved).
    async fn seed_order(&self) -> Seeded {
        let id = Uuid::now_v7();
        let number = format!("ZM-261020-{}", ORDER_SEQ.fetch_add(1, Ordering::Relaxed));
        let mut tx = self.pool.begin().await.expect("tx");
        sqlx::query(
            "INSERT INTO orders (id, order_number, idempotency_key, customer_name, customer_phone, customer_email, \
             address_line, city, state, pincode, item_count, mrp_total_paise, total_paise, status, payment_method, \
             payment_status, payment_expires_at) \
             VALUES ($1, $2, $3, 'Test Customer', '+919840012345', 'customer@example.com', '12 Test Street', 'Chennai', \
             'Tamil Nadu', '600028', $4, $5, $6, 'pending_payment', 'cashfree', 'pending', now() + interval '30 minutes')",
        )
        .bind(id)
        .bind(&number)
        .bind(Uuid::new_v4())
        .bind(QTY)
        .bind(30_000_i64 * i64::from(QTY))
        .bind(TOTAL)
        .execute(&mut *tx)
        .await
        .expect("insert order");
        sqlx::query(
            "INSERT INTO order_items (order_id, product_id, product_name, pack_info, unit_price_paise, unit_mrp_paise, \
             quantity, line_total_paise) VALUES ($1, $2, 'Payment Test Box', '10 Packet(s)', $3, 30000, $4, $5)",
        )
        .bind(id)
        .bind(PRODUCT)
        .bind(UNIT_PRICE)
        .bind(QTY)
        .bind(TOTAL)
        .execute(&mut *tx)
        .await
        .expect("insert item");
        let after: i32 = sqlx::query_scalar(
            "UPDATE products SET stock = stock - $2 WHERE id = $1 RETURNING stock",
        )
        .bind(PRODUCT)
        .bind(QTY)
        .fetch_one(&mut *tx)
        .await
        .expect("reserve");
        sqlx::query(
            "INSERT INTO stock_movements (product_id, delta, stock_after, reason, order_id) VALUES ($1, $2, $3, 'order_placed', $4)",
        )
        .bind(PRODUCT)
        .bind(-QTY)
        .bind(after)
        .bind(id)
        .execute(&mut *tx)
        .await
        .expect("movement");
        tx.commit().await.expect("commit");
        let token = order_token::issue(&self.state.config, id);
        Seeded { id, number, token }
    }

    /// `pay` must succeed; returns the Cashfree order id.
    async fn cf_order(&self, o: &Seeded) -> String {
        let r = self.pay(o).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        let gid = r.body["gatewayOrderId"]
            .as_str()
            .expect("gatewayOrderId")
            .to_owned();
        assert_eq!(gid, o.gid());
        gid
    }

    /// Seed + pay + SUCCESS payment + verify ⇒ paid order. Returns (order, cf_payment_id).
    async fn paid_order(&self) -> (Seeded, u64) {
        let o = self.seed_order().await;
        let gid = self.cf_order(&o).await;
        let pid = next_pid();
        self.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");
        let r = self.verify(&o).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        (o, pid)
    }

    async fn row(&self, id: Uuid) -> DbOrder {
        sqlx::query_as::<_, DbOrder>(
            "SELECT status, payment_status, payment_method, gateway_order_id, gateway_payment_id, payment_instrument, \
             paid_at, amount_refunded_paise, payment_attention, version, refunded_at FROM orders WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .expect("order row")
    }

    async fn refund_rows(&self, id: Uuid) -> Vec<DbRefund> {
        sqlx::query_as::<_, DbRefund>(
            "SELECT id, gateway_payment_id, gateway_refund_id, amount_paise, status, reason, failure_reason \
             FROM refunds WHERE order_id = $1 ORDER BY created_at, id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .expect("refund rows")
    }

    async fn outbox_rows(&self, id: Uuid) -> i64 {
        self.scalar_i64("SELECT count(*) FROM email_outbox WHERE order_id = $1 AND kind = 'admin_order_notification'", id)
            .await
    }

    async fn stock(&self) -> i32 {
        sqlx::query_scalar("SELECT stock FROM products WHERE id = $1")
            .bind(PRODUCT)
            .fetch_one(&self.pool)
            .await
            .expect("stock")
    }

    async fn scalar_i64(&self, sql: &str, id: Uuid) -> i64 {
        sqlx::query_scalar(sql)
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("scalar")
    }

    async fn event_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM payment_events")
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }

    async fn backdate(&self, o: &Seeded) {
        sqlx::query(
            "UPDATE orders SET payment_expires_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(o.id)
        .execute(&self.pool)
        .await
        .expect("backdate");
    }

    /// Expire the order through the real sweep (without the Cashfree reconcile).
    async fn expire(&self, o: &Seeded) {
        self.backdate(o).await;
        let n = repo::orders::expire_unpaid_orders(&self.pool, 10)
            .await
            .expect("sweep");
        assert_eq!(n, 1);
        assert_eq!(self.row(o.id).await.status, "cancelled");
    }
}

#[derive(Debug, sqlx::FromRow)]
struct DbOrder {
    status: String,
    payment_status: String,
    payment_method: String,
    gateway_order_id: Option<String>,
    gateway_payment_id: Option<String>,
    payment_instrument: Option<String>,
    paid_at: Option<DateTime<Utc>>,
    amount_refunded_paise: i64,
    payment_attention: Option<String>,
    version: i32,
    refunded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct DbRefund {
    id: Uuid,
    gateway_payment_id: String,
    gateway_refund_id: Option<String>,
    amount_paise: i64,
    status: String,
    reason: String,
    failure_reason: Option<String>,
}

/// `PAYMENT_*_WEBHOOK` body (2023-08-01 shape), with customer details that
/// must never be stored.
fn payment_webhook(kind: &str, payment: &Value, order_amount_paise: i64) -> Value {
    json!({
        "data": {
            "order": {
                "order_id": payment["order_id"], "order_amount": rupees(order_amount_paise),
                "order_currency": "INR", "order_tags": null
            },
            "payment": {
                "cf_payment_id": payment["cf_payment_id"], "payment_status": payment["payment_status"],
                "payment_amount": payment["payment_amount"], "payment_currency": "INR",
                "payment_message": "Transaction for customer@upi", "payment_time": payment["payment_time"],
                "bank_reference": "234928698581", "auth_id": null,
                "payment_method": { "upi": { "channel": "collect", "upi_id": "customer@upi" } },
                "payment_group": "upi", "error_details": payment["error_details"]
            },
            "customer_details": {
                "customer_name": "Test Customer", "customer_id": "c1",
                "customer_email": "customer@example.com", "customer_phone": "9840012345"
            },
            "payment_gateway_details": { "gateway_name": "CASHFREE" }
        },
        "event_time": "2026-10-20T12:20:30+05:30",
        "type": kind
    })
}

fn refund_webhook(kind: &str, refund: &Value) -> Value {
    let key = if kind == "AUTO_REFUND_STATUS_WEBHOOK" {
        "auto_refund"
    } else {
        "refund"
    };
    let mut r = refund.clone();
    if let Some(o) = r.as_object_mut() {
        o.remove("_idem");
    }
    json!({ "data": { key: r }, "event_time": "2026-10-20T13:04:28+05:30", "type": kind })
}

// ---------------------------------------------------------------------------
// Tests: storefront config, pay
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn storefront_config_is_public_and_cacheable(pool: PgPool) {
    let h = setup(pool).await;
    let r = h
        .raw(Method::GET, "/api/v1/storefront/config", &[], None)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.body,
        json!({ "testMode": false, "onlinePaymentsEnabled": true, "paymentGateway": "cashfree",
                "cashfreeMode": "sandbox", "currency": "INR",
                "paymentExpiryMinutes": 30, "storeName": "Zhi-Mart Fireworks",
                "pricing": { "bulkMinOrder": 5000, "bulkDiscountPercent": 85, "standardDiscountPercent": 80,
                             "deliveryFee": 500, "freeDeliveryMin": 5000 } })
    );
    assert_eq!(r.headers[header::CACHE_CONTROL], "public, max-age=60");

    // Production credentials ⇒ the SDK runs in production mode.
    let mut cfg = test_config("http://127.0.0.1:9/pg".into());
    cfg.cashfree_env = CashfreeEnv::Production;
    let state = AppState::new(Arc::new(cfg), h.pool.clone(), Arc::new(NoopSender)).expect("state");
    let app = Router::new()
        .nest("/api/v1", routes::payments::public_routes())
        .with_state(state);
    let res = app
        .oneshot(
            Request::get("/api/v1/storefront/config")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("response");
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(body["cashfreeMode"], "production");
}

#[sqlx::test(migrations = "./migrations")]
async fn pay_creates_cashfree_order_once_and_reuses_it(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.cashfree_notify_url = Some("https://api.zhimart.in/api/v1/webhooks/cashfree".into())
    })
    .await;
    let o = h.seed_order().await;

    let r1 = h.pay(&o).await;
    assert_eq!(r1.status, StatusCode::OK, "{}", r1.body);
    assert_eq!(r1.headers[header::CACHE_CONTROL], "no-store");
    let b = &r1.body;
    assert_eq!(
        b,
        &json!({
            "gateway": "cashfree", "paymentSessionId": format!("session_{}_abcDEF0123", o.gid()),
            "gatewayOrderId": o.gid(), "amountPaise": TOTAL, "currency": "INR", "mode": "sandbox",
            "paymentExpiresAt": b["paymentExpiresAt"].clone(), "testMode": false
        })
    );
    assert!(b["paymentExpiresAt"].is_string());
    assert!(signature::is_gateway_order_id(&o.gid()) && o.gid().len() <= 45);

    // What Cashfree was sent.
    let sent = h.mock.with(|s| s.create_bodies[0].clone());
    assert_eq!(sent["order_id"], o.gid());
    assert_eq!(sent["order_amount"], json!(1650.0));
    assert_eq!(sent["order_currency"], "INR");
    assert_eq!(sent["customer_details"]["customer_phone"], "9840012345");
    assert_eq!(
        sent["customer_details"]["customer_id"],
        o.id.simple().to_string()
    );
    assert_eq!(
        sent["customer_details"]["customer_email"],
        "customer@example.com"
    );
    assert_eq!(sent["customer_details"]["customer_name"], "Test Customer");
    let return_url = sent["order_meta"]["return_url"]
        .as_str()
        .expect("return_url");
    assert_eq!(
        return_url,
        format!("{SITE}/?page=order&order={}&cf_return=1", o.number)
    );
    assert!(
        !return_url.contains(&o.token),
        "the guest token never goes to Cashfree"
    );
    assert_eq!(
        sent["order_meta"]["notify_url"],
        "https://api.zhimart.in/api/v1/webhooks/cashfree"
    );
    assert_eq!(
        sent["order_tags"],
        json!({ "order_number": o.number, "order_uuid": o.id.to_string() })
    );
    assert!(
        sent["order_expiry_time"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z'))
    );
    let (path, key) = h.mock.with(|s| s.idempotency_keys[0].clone());
    assert_eq!(path, "/orders");
    assert!(
        key.is_some_and(|k| Uuid::parse_str(&k).is_ok()),
        "POST carries a UUID idempotency key"
    );

    // Second click: the stored id is fetched, not created again.
    let r2 = h.pay(&o).await;
    assert_eq!(r2.status, StatusCode::OK);
    assert_eq!(r2.body["gatewayOrderId"], o.gid());
    assert_eq!(r2.body["paymentSessionId"], b["paymentSessionId"]);

    // Concurrent double click: still one Cashfree order, and both answers agree.
    let (a, c) = tokio::join!(h.pay(&o), h.pay(&o));
    assert_eq!(a.body["gatewayOrderId"], o.gid());
    assert_eq!(c.body["gatewayOrderId"], o.gid());

    assert_eq!(h.mock.with(|s| s.create_calls), 1);
    assert_eq!(h.mock.with(|s| s.orders.len()), 1);
    assert_eq!(h.mock.with(|s| s.unauthorized), 0);
    assert_eq!(
        h.row(o.id).await.gateway_order_id.as_deref(),
        Some(o.gid().as_str())
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn pay_recovers_from_lost_create_and_already_exists(pool: PgPool) {
    let h = setup(pool).await;

    // The create reached Cashfree but the answer was lost: 502, no retry of the POST.
    let o = h.seed_order().await;
    h.mock.with(|s| s.lose_next_create = true);
    let r = h.pay(&o).await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    assert_eq!(r.code(), "payment_gateway_error");
    assert_eq!(h.mock.with(|s| s.create_calls), 1);
    // The next click fetches the order Cashfree already has.
    let r = h.pay(&o).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["gatewayOrderId"], o.gid());
    assert_eq!(h.mock.with(|s| s.create_calls), 1);

    // The id is not stored here but exists at Cashfree (409 order_already_exists ⇒ GET).
    let o2 = h.seed_order().await;
    let existing = json!({
        "cf_order_id": 1, "order_id": o2.gid(), "order_amount": rupees(TOTAL), "order_currency": "INR",
        "order_status": "ACTIVE", "payment_session_id": "session_existing_0001"
    });
    h.mock.with(|s| s.orders.insert(o2.gid(), existing));
    let r = h.pay(&o2).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["paymentSessionId"], "session_existing_0001");
    assert_eq!(h.mock.with(|s| s.create_calls), 2);

    // A stored id unknown to Cashfree (404) is created.
    let o3 = h.seed_order().await;
    sqlx::query("UPDATE orders SET gateway_order_id = $2 WHERE id = $1")
        .bind(o3.id)
        .bind(o3.gid())
        .execute(&h.pool)
        .await
        .expect("attach");
    let r = h.pay(&o3).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(h.mock.with(|s| s.create_calls), 3);

    // A Cashfree order whose amount does not match ours is never handed out.
    let o4 = h.seed_order().await;
    let wrong = json!({
        "cf_order_id": 2, "order_id": o4.gid(), "order_amount": 1.0, "order_currency": "INR",
        "order_status": "ACTIVE", "payment_session_id": "session_wrong_0001"
    });
    h.mock.with(|s| s.orders.insert(o4.gid(), wrong));
    let r = h.pay(&o4).await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
}

#[sqlx::test(migrations = "./migrations")]
async fn pay_on_an_order_paid_at_cashfree_settles_it(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    let pid = next_pid();
    h.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");
    // The customer clicks "Pay" again before the verify / webhook arrived.
    let r = h.pay(&o).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "order_already_paid");
    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert_eq!(row.gateway_payment_id, Some(pid.to_string()));
}

#[sqlx::test(migrations = "./migrations")]
async fn delivery_fee_is_part_of_the_payable_amount(pool: PgPool) {
    const FEE: i64 = 50_000;
    let h = setup(pool).await;
    let o = h.seed_order().await;
    sqlx::query(
        "UPDATE orders SET delivery_fee_paise = $2, total_paise = total_paise + $2 WHERE id = $1",
    )
    .bind(o.id)
    .bind(FEE)
    .execute(&h.pool)
    .await
    .expect("add delivery fee");

    let g = h
        .guest(
            Method::GET,
            &format!("/api/v1/orders/{}", o.number),
            &o.token,
            None,
        )
        .await;
    assert_eq!(g.status, StatusCode::OK);
    assert_eq!(g.body["subtotal"], 1650);
    assert_eq!(g.body["deliveryFee"], 500);
    assert_eq!(g.body["totalAmount"], 2150);

    // Cashfree is asked for goods + delivery, and only that amount verifies.
    let p = h.pay(&o).await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.body);
    assert_eq!(p.body["amountPaise"], TOTAL + FEE);
    assert_eq!(
        h.mock.with(|s| s.create_bodies[0]["order_amount"].clone()),
        json!(2150.0)
    );
    h.mock
        .add_payment(&o.gid(), next_pid(), TOTAL + FEE, "SUCCESS");
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["totalAmount"], 2150);

    // A full refund returns the delivery charge too.
    let v = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "amount": null, "reason": "Customer request", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["amountRefunded"], 2150);
    assert_eq!(
        h.mock.with(|s| s.refunds[0]["refund_amount"].clone()),
        json!(2150.0)
    );
    assert_eq!(h.row(o.id).await.amount_refunded_paise, TOTAL + FEE);
}

#[sqlx::test(migrations = "./migrations")]
async fn wrong_or_missing_token_is_404(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let other = h.seed_order().await;

    for token in ["", "nope", other.token.as_str(), &"A".repeat(43)] {
        let g = h
            .guest(
                Method::GET,
                &format!("/api/v1/orders/{}", o.number),
                token,
                None,
            )
            .await;
        assert_eq!(g.status, StatusCode::NOT_FOUND, "token {token:?}");
        assert_eq!(g.code(), "not_found");
        let p = h
            .guest(
                Method::POST,
                &format!("/api/v1/orders/{}/pay", o.number),
                token,
                None,
            )
            .await;
        assert_eq!(p.status, StatusCode::NOT_FOUND);
        let v = h
            .guest(
                Method::POST,
                &format!("/api/v1/orders/{}/pay/verify", o.number),
                token,
                Some(json!({})),
            )
            .await;
        assert_eq!(v.status, StatusCode::NOT_FOUND);
    }
    let r = h
        .raw(
            Method::GET,
            &format!("/api/v1/orders/{}", o.number),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = h
        .guest(
            Method::GET,
            "/api/v1/orders/ZM-261020-999999",
            &o.token,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = h
        .guest(Method::GET, "/api/v1/orders/not-an-order", &o.token, None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(
        h.mock.with(|s| s.create_calls + s.payment_reads),
        0,
        "Cashfree untouched without a valid token"
    );

    // The right token works and reveals no contact details.
    let g = h
        .guest(
            Method::GET,
            &format!("/api/v1/orders/{}", o.number),
            &o.token,
            None,
        )
        .await;
    assert_eq!(g.status, StatusCode::OK);
    assert_eq!(g.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(g.body["status"], "pending_payment");
    assert_eq!(g.body["customerName"], "Test Customer");
    assert_eq!(g.body["totalAmount"], 1650);
    assert_eq!(g.body["pricingTier"], "bulk");
    assert_eq!(
        g.body["payment"],
        json!({ "method": "cashfree", "status": "pending", "instrument": null, "paidAt": null })
    );
    let text = g.body.to_string();
    assert!(
        !text.contains("9840012345")
            && !text.contains("Test Street")
            && !text.contains("customer@example.com")
    );
}

// ---------------------------------------------------------------------------
// Tests: verify
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn verify_success_marks_paid_and_is_idempotent(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    // A failed attempt first, then the successful one.
    h.mock.add_payment(&gid, next_pid(), TOTAL, "FAILED");
    let pid = next_pid();
    h.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");

    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(r.body["status"], "pending_pack");
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["payment"]["instrument"], "upi");
    assert_eq!(r.body["payment"]["method"], "cashfree");
    assert!(r.body["payment"]["paidAt"].is_string());
    assert!(r.body["paymentExpiresAt"].is_null());

    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert_eq!(row.payment_status, "paid");
    assert_eq!(row.payment_method, "cashfree");
    assert_eq!(
        row.gateway_payment_id,
        Some(pid.to_string()),
        "numeric cf_payment_id stored as digits"
    );
    assert_eq!(row.payment_instrument.as_deref(), Some("upi"));

    // Polling again: 200, no second transition, no further Cashfree call.
    let reads = h.mock.with(|s| s.payment_reads);
    let again = h.verify(&o).await;
    assert_eq!(again.status, StatusCode::OK);
    let row2 = h.row(o.id).await;
    assert_eq!(row2.version, row.version);
    assert_eq!(row2.paid_at, row.paid_at);
    assert_eq!(
        h.mock.with(|s| s.payment_reads),
        reads,
        "fast path once paid"
    );

    // Pay on a paid order.
    let p = h.pay(&o).await;
    assert_eq!(p.status, StatusCode::CONFLICT);
    assert_eq!(p.code(), "order_already_paid");
}

#[sqlx::test(migrations = "./migrations")]
async fn verify_pending_and_not_completed(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;

    // Before checkout started.
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.code(), "payment_not_completed");
    assert_eq!(h.mock.with(|s| s.payment_reads), 0);

    let gid = h.cf_order(&o).await;
    // Checkout closed without paying.
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.code(), "payment_not_completed");
    assert_eq!(r.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        r.body["error"]["message"],
        "The payment was not completed. You can try again."
    );
    // Declined / dropped attempts.
    h.mock.add_payment(&gid, next_pid(), TOTAL, "FAILED");
    h.mock.add_payment(&gid, next_pid(), TOTAL, "USER_DROPPED");
    let r = h.verify(&o).await;
    assert_eq!(r.code(), "payment_not_completed");
    // UPI collect awaiting approval.
    let pid = next_pid();
    h.mock.add_payment(&gid, pid, TOTAL, "PENDING");
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "payment_pending");
    assert_eq!(h.row(o.id).await.status, "pending_payment");

    // It goes through.
    h.mock.with(|s| {
        for p in s.payments.get_mut(&gid).into_iter().flatten() {
            if p["cf_payment_id"] == json!(pid) {
                p["payment_status"] = json!("SUCCESS");
            }
        }
    });
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");

    // The body must be `{}` (or empty): the client has nothing to claim.
    let o2 = h.seed_order().await;
    let bad = h
        .guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay/verify", o2.number),
            &o2.token,
            Some(json!({ "paid": true })),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad.code(), "validation_error");
    let empty = h
        .raw(
            Method::POST,
            &format!("/api/v1/orders/{}/pay/verify", o2.number),
            &[("x-order-token", o2.token.clone())],
            None,
        )
        .await;
    assert_eq!(empty.code(), "payment_not_completed", "{}", empty.body);
}

#[sqlx::test(migrations = "./migrations")]
async fn amount_or_currency_mismatch_never_pays(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    h.mock.add_payment(&gid, next_pid(), 100, "SUCCESS");

    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.code(), "payment_verification_failed");
    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_payment");
    assert_eq!(row.payment_status, "pending");
    assert!(row.gateway_payment_id.is_none());
    assert!(
        row.payment_attention.is_some(),
        "money that does not match is flagged"
    );
    // Polling again does not keep bumping the version.
    let v = row.version;
    let _ = h.verify(&o).await;
    assert_eq!(h.row(o.id).await.version, v);

    let o2 = h.seed_order().await;
    let gid2 = h.cf_order(&o2).await;
    let mut usd = payment_entity(&gid2, next_pid(), TOTAL, "SUCCESS");
    usd["payment_currency"] = json!("USD");
    h.mock
        .with(|s| s.payments.entry(gid2.clone()).or_default().push(usd));
    let r = h.verify(&o2).await;
    assert_eq!(r.code(), "payment_verification_failed");
    assert_eq!(h.row(o2.id).await.payment_status, "pending");

    // A payment claiming another Cashfree order.
    let o3 = h.seed_order().await;
    let gid3 = h.cf_order(&o3).await;
    let foreign = payment_entity("ZM-261020-1_000000000000", next_pid(), TOTAL, "SUCCESS");
    h.mock
        .with(|s| s.payments.entry(gid3.clone()).or_default().push(foreign));
    let r = h.verify(&o3).await;
    assert_eq!(r.code(), "payment_verification_failed");
    assert_eq!(h.row(o3.id).await.status, "pending_payment");
}

#[sqlx::test(migrations = "./migrations")]
async fn gateway_unavailable_is_502_and_changes_nothing(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    h.mock.with(|s| s.payments_unavailable = true);
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    assert_eq!(r.code(), "payment_gateway_error");
    assert_eq!(h.mock.with(|s| s.payment_reads), 2, "GET retried once");
    assert_eq!(h.row(o.id).await.status, "pending_payment");
    h.mock.with(|s| s.payments_unavailable = false);
    assert_eq!(h.verify(&o).await.status, StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_verify_and_webhook_transition_once(pool: PgPool) {
    let h = setup_with(pool, |c| c.order_notifications_enabled = true).await;
    for _ in 0..5 {
        let o = h.seed_order().await;
        let gid = h.cf_order(&o).await;
        let pid = next_pid();
        let entity = h.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");
        let before = h.row(o.id).await;
        assert_eq!(h.outbox_rows(o.id).await, 0, "no email before payment");

        let hook = payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL);
        let (v1, v2, w1) = tokio::join!(h.verify(&o), h.verify(&o), h.webhook(&hook));
        assert_eq!(v1.status, StatusCode::OK, "{}", v1.body);
        assert_eq!(v2.status, StatusCode::OK, "{}", v2.body);
        assert_eq!(w1.status, StatusCode::OK, "{}", w1.body);

        let after = h.row(o.id).await;
        assert_eq!(after.status, "pending_pack");
        assert_eq!(after.payment_status, "paid");
        assert_eq!(after.gateway_payment_id, Some(pid.to_string()));
        assert_eq!(after.version, before.version + 1, "exactly one transition");
        assert_eq!(
            h.outbox_rows(o.id).await,
            1,
            "exactly one admin email queued"
        );
        assert_eq!(
            h.scalar_i64("SELECT count(*) FROM refunds WHERE order_id = $1", o.id)
                .await,
            0
        );
        let outcomes: Vec<String> =
            sqlx::query_scalar("SELECT outcome FROM payment_events WHERE order_id = $1")
                .bind(o.id)
                .fetch_all(&h.pool)
                .await
                .expect("outcomes");
        assert_eq!(outcomes.len(), 1);
        assert!(
            outcomes.iter().all(|x| x == "paid" || x == "already_paid"),
            "{outcomes:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: webhook
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn webhook_rejects_bad_or_missing_signature(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    let entity = h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    let body = payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL)
        .to_string()
        .into_bytes();
    let ts = Utc::now().timestamp_millis().to_string();
    let good = signature::webhook_signature(SECRET_KEY.as_bytes(), &ts, &body);
    let post = |headers: Vec<(&'static str, String)>, body: Vec<u8>| {
        let h = h.clone();
        async move {
            h.raw(
                Method::POST,
                "/api/v1/webhooks/cashfree",
                &headers,
                Some(body),
            )
            .await
        }
    };

    // Wrong key.
    let r = h
        .webhook_signed(&body, "cfsk_ma_test_not_the_secret_key", &ts)
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Missing signature / timestamp.
    let r = post(vec![("x-webhook-timestamp", ts.clone())], body.clone()).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = post(vec![("x-webhook-signature", good.clone())], body.clone()).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = post(vec![], body.clone()).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Timestamp changed after signing.
    let r = post(
        vec![
            ("x-webhook-signature", good.clone()),
            ("x-webhook-timestamp", format!("{ts}1")),
        ],
        body.clone(),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Body changed after signing.
    let mut tampered = body.clone();
    tampered.push(b' ');
    let r = post(
        vec![
            ("x-webhook-signature", good.clone()),
            ("x-webhook-timestamp", ts.clone()),
        ],
        tampered,
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Hex encoding of the right MAC (wrong scheme).
    let r = post(
        vec![
            ("x-webhook-signature", "0".repeat(64)),
            ("x-webhook-timestamp", ts.clone()),
        ],
        body.clone(),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Duplicated signature header.
    let r = post(
        vec![
            ("x-webhook-signature", good.clone()),
            ("x-webhook-signature", good.clone()),
            ("x-webhook-timestamp", ts.clone()),
        ],
        body.clone(),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    assert_eq!(h.row(o.id).await.status, "pending_payment");
    assert_eq!(h.event_count().await, 0);

    // The genuine delivery is accepted.
    let r = post(
        vec![("x-webhook-signature", good), ("x-webhook-timestamp", ts)],
        body,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body, json!({ "status": "ok" }));
    assert_eq!(r.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(h.row(o.id).await.status, "pending_pack");
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_duplicate_delivery_is_processed_once(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    let entity = h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    let hook = payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL);
    let body = hook.to_string().into_bytes();

    let r1 = h.webhook_signed(&body, SECRET_KEY, "1760950000001").await;
    assert_eq!(r1.status, StatusCode::OK, "{}", r1.body);
    let paid = h.row(o.id).await;
    assert_eq!(paid.status, "pending_pack");
    let reads = h.mock.with(|s| s.payment_reads);

    // Cashfree retries the same body, re-signed with a new timestamp.
    let r2 = h.webhook_signed(&body, SECRET_KEY, "1760950060001").await;
    assert_eq!(r2.status, StatusCode::OK);
    assert_eq!(
        h.mock.with(|s| s.payment_reads),
        reads,
        "redelivery not reprocessed"
    );
    let (count, outcome, id): (i64, Option<String>, Option<String>) =
        sqlx::query_as("SELECT count(*), max(outcome), max(id) FROM payment_events")
            .fetch_one(&h.pool)
            .await
            .expect("event");
    assert_eq!(count, 1);
    assert_eq!(outcome.as_deref(), Some("paid"));
    assert_eq!(id, Some(service::event_id(&body)));
    assert!(id.is_some_and(|i| i.starts_with("cf_")));
    assert_eq!(h.row(o.id).await.version, paid.version);

    // Stored payload is redacted (no contact details, UPI id or messages).
    let (payload, event, gw_order, gw_payment): (Value, String, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT payload, event, gateway_order_id, gateway_payment_id FROM payment_events",
        )
        .fetch_one(&h.pool)
        .await
        .expect("payload");
    let text = payload.to_string();
    assert!(
        !text.contains("customer@example.com")
            && !text.contains("9840012345")
            && !text.contains("customer@upi"),
        "{text}"
    );
    assert_eq!(event, "PAYMENT_SUCCESS_WEBHOOK");
    assert_eq!(gw_order, Some(gid));
    assert_eq!(gw_payment, Some(entity["cf_payment_id"].to_string()));
    assert_eq!(payload["payment"]["status"], "SUCCESS");
    assert_eq!(payload["payment"]["amount"], json!(1650.0));
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_is_a_trigger_state_comes_from_the_api(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;

    // A signed SUCCESS webhook the API does not confirm (yet): 5xx so Cashfree
    // retries, and the order is untouched.
    let claimed = payment_entity(&gid, next_pid(), TOTAL, "SUCCESS");
    let hook = payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &claimed, TOTAL);
    let r = h.webhook(&hook).await;
    assert!(r.status.is_server_error(), "{} {}", r.status, r.body);
    assert_eq!(h.row(o.id).await.status, "pending_payment");
    let processed: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT processed_at FROM payment_events")
            .fetch_one(&h.pool)
            .await
            .expect("event");
    assert!(processed.is_none(), "left open for the redelivery");

    // Now Cashfree has it: the redelivery settles from the API's data.
    let pid = next_pid();
    h.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");
    let r = h.webhook(&hook).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert_eq!(
        row.gateway_payment_id,
        Some(pid.to_string()),
        "the API's payment, not the webhook's"
    );

    // A webhook for an order we do not know is acknowledged and ignored.
    let stranger = payment_entity("ZM-261020-9_aaaaaaaaaaaa", next_pid(), TOTAL, "SUCCESS");
    let r = h
        .webhook(&payment_webhook(
            "PAYMENT_SUCCESS_WEBHOOK",
            &stranger,
            TOTAL,
        ))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let outcome: Option<String> = sqlx::query_scalar(
        "SELECT outcome FROM payment_events WHERE gateway_order_id = 'ZM-261020-9_aaaaaaaaaaaa'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("outcome");
    assert_eq!(outcome.as_deref(), Some("ignored: unknown order"));

    // Cashfree unavailable during the re-read ⇒ 502 (redelivered later).
    let o2 = h.seed_order().await;
    let gid2 = h.cf_order(&o2).await;
    let e2 = h.mock.add_payment(&gid2, next_pid(), TOTAL, "SUCCESS");
    h.mock.with(|s| s.payments_unavailable = true);
    let r = h
        .webhook(&payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &e2, TOTAL))
        .await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    assert_eq!(h.row(o2.id).await.status, "pending_payment");

    // Malformed JSON with a valid signature is a 400.
    let r = h
        .webhook_signed(b"not json", SECRET_KEY, "1760950000001")
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn payment_failed_webhook_keeps_order_payable(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    let failed = h.mock.add_payment(&gid, next_pid(), TOTAL, "FAILED");

    let r = h
        .webhook(&payment_webhook("PAYMENT_FAILED_WEBHOOK", &failed, TOTAL))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let dropped = h.mock.add_payment(&gid, next_pid(), TOTAL, "USER_DROPPED");
    let r = h
        .webhook(&payment_webhook(
            "PAYMENT_USER_DROPPED_WEBHOOK",
            &dropped,
            TOTAL,
        ))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let outcomes: Vec<String> =
        sqlx::query_scalar("SELECT outcome FROM payment_events ORDER BY received_at")
            .fetch_all(&h.pool)
            .await
            .expect("outcomes");
    assert_eq!(
        outcomes,
        vec![
            "payment_failed: TRANSACTION_DECLINED",
            "payment_user_dropped"
        ]
    );

    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_payment");
    assert_eq!(row.payment_status, "pending");
    let again = h.pay(&o).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body["gatewayOrderId"], gid.as_str());

    // The retry succeeds.
    h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    let v = h.verify(&o).await;
    assert_eq!(v.status, StatusCode::OK);
    assert_eq!(v.body["status"], "pending_pack");
}

// ---------------------------------------------------------------------------
// Tests: expiry, late payments
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn expired_order_cannot_start_payment(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    h.backdate(&o).await;
    let r = h.pay(&o).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "order_expired");
    let r = h.verify(&o).await;
    assert_eq!(r.code(), "order_expired");
    repo::orders::expire_unpaid_orders(&h.pool, 10)
        .await
        .expect("sweep");
    let r = h.pay(&o).await;
    assert_eq!(r.code(), "order_expired");
    let r = h.verify(&o).await;
    assert_eq!(r.code(), "order_expired");
    assert_eq!(h.mock.with(|s| s.create_calls), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn expiry_sweep_reconciles_with_cashfree(pool: PgPool) {
    let h = setup_with(pool, |c| c.order_notifications_enabled = true).await;
    // Paid at Cashfree, but neither verify nor the webhook arrived.
    let paid = h.seed_order().await;
    let gid = h.cf_order(&paid).await;
    let pid = next_pid();
    h.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");
    // Checkout opened, never paid.
    let unpaid = h.seed_order().await;
    h.cf_order(&unpaid).await;
    // Never opened checkout (no Cashfree order: nothing to ask).
    let untouched = h.seed_order().await;
    // Still inside its window.
    let fresh = h.seed_order().await;
    h.cf_order(&fresh).await;
    for o in [&paid, &unpaid, &untouched] {
        h.backdate(o).await;
    }
    let reads = h.mock.with(|s| s.payment_reads);

    let run = service::reconcile_expiring_orders(&h.state, 100, Duration::from_secs(20)).await;
    assert_eq!(
        run,
        SweepReconcile {
            checked: 2,
            paid: 1,
            gateway_unavailable: false
        }
    );
    assert_eq!(h.mock.with(|s| s.payment_reads), reads + 2);
    let n = repo::orders::expire_unpaid_orders(&h.pool, 10)
        .await
        .expect("sweep");
    assert_eq!(n, 2);

    let row = h.row(paid.id).await;
    assert_eq!(row.status, "pending_pack", "settled instead of cancelled");
    assert_eq!(row.gateway_payment_id, Some(pid.to_string()));
    assert_eq!(h.outbox_rows(paid.id).await, 1);
    assert_eq!(h.row(unpaid.id).await.status, "cancelled");
    assert_eq!(h.row(untouched.id).await.status, "cancelled");
    assert_eq!(h.row(fresh.id).await.status, "pending_payment");

    // Cashfree down: nothing is settled, the sweep cancels anyway.
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    h.backdate(&o).await;
    h.mock.with(|s| s.payments_unavailable = true);
    let run = service::reconcile_expiring_orders(&h.state, 100, Duration::from_secs(20)).await;
    assert!(run.gateway_unavailable);
    assert_eq!(run.paid, 0);
    assert_eq!(
        repo::orders::expire_unpaid_orders(&h.pool, 10)
            .await
            .expect("sweep"),
        1
    );
    assert_eq!(h.row(o.id).await.status, "cancelled");
    // ...and the late payment path picks it up once Cashfree is back.
    h.mock.with(|s| s.payments_unavailable = false);
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["status"], "pending_pack");

    // Disabled ⇒ no Cashfree calls at all.
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    h.backdate(&o).await;
    let mut cfg = test_config("http://127.0.0.1:9/pg".into());
    payments_off(&mut cfg);
    let off = AppState::new(Arc::new(cfg), h.pool.clone(), Arc::new(NoopSender)).expect("state");
    let run = service::reconcile_expiring_orders(&off, 100, Duration::from_secs(20)).await;
    assert_eq!(run, SweepReconcile::default());
    assert_eq!(h.row(o.id).await.status, "pending_payment");
}

#[sqlx::test(migrations = "./migrations")]
async fn late_payment_with_stock_revives_order(pool: PgPool) {
    let h = setup_with(pool, |c| c.order_notifications_enabled = true).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    let reserved_stock = h.stock().await;
    h.expire(&o).await;
    assert_eq!(
        h.stock().await,
        reserved_stock + QTY,
        "expiry released the stock"
    );

    let entity = h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    let r = h
        .webhook(&payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let v = h.verify(&o).await;
    assert_eq!(v.status, StatusCode::OK, "{}", v.body);
    assert_eq!(v.body["status"], "pending_pack");
    assert_eq!(v.body["payment"]["status"], "paid");
    assert_eq!(h.stock().await, reserved_stock, "stock re-reserved");
    let revived = h
        .scalar_i64("SELECT count(*) FROM stock_movements WHERE order_id = $1 AND reason = 'payment_revived'", o.id)
        .await;
    assert_eq!(revived, 1);
    assert_eq!(h.mock.with(|s| s.refund_calls), 0);
    assert_eq!(
        h.outbox_rows(o.id).await,
        1,
        "revived order notifies the admins"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn late_payment_without_stock_is_refunded(pool: PgPool) {
    let h = setup_with(pool, |c| c.order_notifications_enabled = true).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    h.expire(&o).await;
    sqlx::query("UPDATE products SET stock = 0 WHERE id = $1")
        .bind(PRODUCT)
        .execute(&h.pool)
        .await
        .expect("sell out");

    let pid = next_pid();
    let entity = h.mock.add_payment(&gid, pid, TOTAL, "SUCCESS");
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "order_expired");

    let row = h.row(o.id).await;
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.gateway_payment_id, Some(pid.to_string()));
    assert_eq!(row.payment_status, "refunded");
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert!(row.payment_attention.is_some());
    assert_eq!(h.stock().await, 0, "no stock taken");
    assert_eq!(h.mock.with(|s| s.refund_calls), 1);
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds[0].status, "processed");
    assert_eq!(refunds[0].gateway_payment_id, pid.to_string());
    assert!(refunds[0].gateway_refund_id.is_some());
    assert_eq!(refunds[0].amount_paise, TOTAL);
    // Our refund id and idempotency key reached Cashfree.
    let sent = h.mock.with(|s| s.refunds[0].clone());
    assert_eq!(sent["refund_id"], refunds[0].id.simple().to_string());
    assert_eq!(sent["refund_speed"], "STANDARD");
    let key = h.mock.with(|s| {
        s.idempotency_keys
            .iter()
            .find(|(p, _)| p.ends_with("/refunds"))
            .cloned()
    });
    assert_eq!(key.and_then(|k| k.1), Some(refunds[0].id.to_string()));

    // Verify again and the webhook for the same payment refund nothing more.
    let r = h.verify(&o).await;
    assert_eq!(r.code(), "order_expired");
    let w = h
        .webhook(&payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    assert_eq!(h.mock.with(|s| s.refund_calls), 1);
    assert_eq!(h.refund_rows(o.id).await.len(), 1);
    assert_eq!(
        h.outbox_rows(o.id).await,
        0,
        "refunded late payment: no email"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn payment_for_admin_cancelled_order_is_refunded(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let gid = h.cf_order(&o).await;
    sqlx::query("UPDATE orders SET status = 'cancelled', cancel_reason = 'Customer request', cancelled_at = now() WHERE id = $1")
        .bind(o.id)
        .execute(&h.pool)
        .await
        .expect("cancel");
    h.mock.add_payment(&gid, next_pid(), TOTAL, "SUCCESS");
    let r = h.verify(&o).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "order_not_payable");
    let row = h.row(o.id).await;
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.payment_status, "refunded");
    assert!(row.payment_attention.is_some());
    assert_eq!(h.mock.with(|s| s.refund_calls), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn duplicate_payment_is_flagged_and_cashfree_auto_refund_recorded(pool: PgPool) {
    let h = setup(pool).await;
    let (o, first) = h.paid_order().await;
    let gid = o.gid();
    let second = next_pid();
    let entity = h.mock.add_payment(&gid, second, TOTAL, "SUCCESS");

    let w = h
        .webhook(&payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert_eq!(
        row.payment_status, "paid",
        "the order's own payment is untouched"
    );
    assert_eq!(row.gateway_payment_id, Some(first.to_string()));
    assert!(
        row.payment_attention
            .as_deref()
            .is_some_and(|a| a.contains(&second.to_string()))
    );
    assert_eq!(
        h.mock.with(|s| s.refund_calls),
        0,
        "a per-order refund would hit the settled payment"
    );
    let outcome: Option<String> = sqlx::query_scalar("SELECT outcome FROM payment_events")
        .fetch_one(&h.pool)
        .await
        .expect("outcome");
    assert_eq!(outcome.as_deref(), Some("duplicate_payment"));

    // Polling does not keep bumping the version.
    let v = row.version;
    assert_eq!(h.verify(&o).await.status, StatusCode::OK);
    assert_eq!(h.row(o.id).await.version, v);

    // Cashfree auto-refunds the duplicate: recorded against that payment, the
    // order's own totals unchanged.
    let auto = json!({
        "event": "AUTO-REFUND", "cf_refund_id": 1243460973_u64, "cf_payment_id": second.to_string(),
        "order_id": gid, "refund_amount": rupees(TOTAL), "refund_currency": "INR",
        "refund_type": "PAYMENT_AUTO_REFUND", "refund_status": "SUCCESS",
        "status_description": "Auto-Refund processed successfully",
        "refund_reason": "Multiple payments were performed against same order."
    });
    h.mock.with(|s| s.refunds.push(auto.clone()));
    let w = h
        .webhook(&refund_webhook("AUTO_REFUND_STATUS_WEBHOOK", &auto))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds[0].gateway_payment_id, second.to_string());
    assert_eq!(refunds[0].gateway_refund_id.as_deref(), Some("1243460973"));
    assert_eq!(refunds[0].status, "processed");
    assert_eq!(refunds[0].reason, "Automatic refund by Cashfree");
    let row = h.row(o.id).await;
    assert_eq!(row.payment_status, "paid");
    assert_eq!(row.amount_refunded_paise, 0);
}

// ---------------------------------------------------------------------------
// Tests: admin refunds
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn admin_full_refund(pool: PgPool) {
    let h = setup(pool).await;
    let (o, pid) = h.paid_order().await;
    let v = h.row(o.id).await.version;

    let r = h
        .refund(
            &o,
            json!({ "amount": null, "reason": "Customer request", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");
    assert_eq!(r.body["payment"]["amountRefunded"], 1650);
    assert_eq!(r.body["payment"]["gatewayOrderId"], o.gid());
    assert_eq!(r.body["payment"]["gatewayPaymentId"], pid.to_string());
    let row = h.row(o.id).await;
    assert_eq!(row.payment_status, "refunded");
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert!(row.refunded_at.is_some());
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds.len(), 1);
    let sent: Vec<(Value, Value)> = h.mock.with(|s| {
        s.refunds
            .iter()
            .map(|r| (r["refund_id"].clone(), r["order_id"].clone()))
            .collect()
    });
    assert_eq!(
        sent,
        vec![(json!(refunds[0].id.simple().to_string()), json!(o.gid()))],
        "per-order refund, our id"
    );

    // Nothing left.
    let again = h
        .refund(
            &o,
            json!({ "reason": "again", "expectedVersion": row.version }),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "invalid_transition");

    // Events endpoint lists the refund with the admin's e-mail and gateway ids.
    let ev = h
        .admin(
            Method::GET,
            &format!("/api/v1/admin/orders/{}/payment-events", o.number),
            None,
        )
        .await;
    assert_eq!(ev.status, StatusCode::OK, "{}", ev.body);
    let list = ev.body["refunds"].as_array().expect("refunds");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["adminEmail"], "ops@zhimart.com");
    assert_eq!(list[0]["amount"], 1650);
    assert_eq!(list[0]["status"], "processed");
    assert_eq!(list[0]["reason"], "Customer request");
    assert_eq!(list[0]["gatewayPaymentId"], pid.to_string());
    assert!(list[0]["gatewayRefundId"].is_string());
    assert!(list[0].get("razorpayRefundId").is_none());
    assert!(ev.body["events"].is_array());

    let anon = h
        .raw(
            Method::GET,
            &format!("/api/v1/admin/orders/{}/payment-events", o.id),
            &[],
            None,
        )
        .await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_partial_refunds_and_conflicts(pool: PgPool) {
    let h = setup(pool).await;
    let (o, _) = h.paid_order().await;
    let v0 = h.row(o.id).await.version;

    let r = h
        .refund(
            &o,
            json!({ "amount": 500, "reason": "Damaged box", "expectedVersion": v0 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["payment"]["amountRefunded"], 500);
    assert_eq!(
        h.mock.with(|s| s.refunds[0]["refund_amount"].clone()),
        json!(500.0)
    );

    let stale = h
        .refund(
            &o,
            json!({ "amount": 100, "reason": "Damaged box", "expectedVersion": v0 }),
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.code(), "version_conflict");

    let v1 = h.row(o.id).await.version;
    let over = h
        .refund(
            &o,
            json!({ "amount": 1200, "reason": "Too much", "expectedVersion": v1 }),
        )
        .await;
    assert_eq!(over.status, StatusCode::CONFLICT);
    assert_eq!(over.code(), "refund_exceeds_remaining");

    for bad in [
        json!({ "amount": 0, "reason": "x", "expectedVersion": v1 }),
        json!({ "amount": 1.234, "reason": "x", "expectedVersion": v1 }),
        json!({ "reason": "", "expectedVersion": v1 }),
        json!({ "reason": "x".repeat(301), "expectedVersion": v1 }),
    ] {
        let r = h.refund(&o, bad).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    }

    let rest = h
        .refund(
            &o,
            json!({ "amount": 1150, "reason": "Rest", "expectedVersion": v1 }),
        )
        .await;
    assert_eq!(rest.status, StatusCode::OK, "{}", rest.body);
    assert_eq!(rest.body["payment"]["status"], "refunded");
    assert_eq!(h.row(o.id).await.amount_refunded_paise, TOTAL);
    assert_eq!(h.mock.with(|s| s.refund_calls), 2);

    // An unpaid order cannot be refunded.
    let unpaid = h.seed_order().await;
    let v = h.row(unpaid.id).await.version;
    let r = h
        .refund(&unpaid, json!({ "reason": "x", "expectedVersion": v }))
        .await;
    assert_eq!(r.code(), "invalid_transition");
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_refund_rejected_is_502_and_recorded(pool: PgPool) {
    let h = setup(pool).await;
    let (o, _) = h.paid_order().await;
    h.mock.with(|s| s.refund_mode = RefundMode::Reject);
    let v = h.row(o.id).await.version;

    let r = h
        .refund(
            &o,
            json!({ "reason": "Customer request", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    assert_eq!(r.code(), "payment_gateway_error");
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds[0].status, "failed");
    assert!(
        refunds[0]
            .failure_reason
            .as_deref()
            .is_some_and(|f| f.contains("request_failed"))
    );
    let row = h.row(o.id).await;
    assert_eq!(row.payment_status, "paid");
    assert_eq!(row.amount_refunded_paise, 0);
    assert!(
        row.payment_attention.is_none(),
        "admin refunds are not flagged"
    );

    // The failed refund does not block a retry.
    h.mock.with(|s| s.refund_mode = RefundMode::Success);
    let r = h
        .refund(
            &o,
            json!({ "reason": "Customer request", "expectedVersion": row.version }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_refund_ambiguous_then_reconciled(pool: PgPool) {
    let h = setup(pool).await;
    let (o, _) = h.paid_order().await;

    // The refund reaches Cashfree but the answer is lost; the immediate
    // GET-by-refund_id finds it: completed without a second POST.
    h.mock.with(|s| s.refund_mode = RefundMode::LostResponse);
    let v = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "amount": 300, "reason": "Partial", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["amountRefunded"], 300);
    assert_eq!(h.mock.with(|s| s.refund_calls), 1, "POST never retried");

    // Lost again, and Cashfree cannot be read either: 502, the row stays
    // pending and its amount stays reserved.
    h.mock.with(|s| s.refund_reads_unavailable = true);
    let v = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "amount": 1000, "reason": "More", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds.len(), 2);
    assert_eq!(refunds[1].status, "pending");
    assert!(refunds[1].gateway_refund_id.is_none());
    assert!(
        refunds[1]
            .failure_reason
            .as_deref()
            .is_some_and(|f| f.contains("awaiting confirmation"))
    );
    let row = h.row(o.id).await;
    assert_eq!(
        row.amount_refunded_paise, 30_000,
        "unconfirmed refund not counted yet"
    );
    let over = h
        .refund(
            &o,
            json!({ "amount": 400, "reason": "Too much", "expectedVersion": row.version }),
        )
        .await;
    assert_eq!(
        over.code(),
        "refund_exceeds_remaining",
        "the unconfirmed ₹1,000 stays reserved"
    );

    // Cashfree is back: the next refund attempt first reconciles the pending one.
    h.mock.with(|s| {
        s.refund_reads_unavailable = false;
        s.refund_mode = RefundMode::Success;
    });
    let v = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "amount": 350, "reason": "Rest", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(
        r.code(),
        "version_conflict",
        "reconciliation changed the order first"
    );
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds[1].status, "processed");
    assert_eq!(h.row(o.id).await.amount_refunded_paise, 130_000);
    let v = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "amount": 350, "reason": "Rest", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");

    // A pending refund that never reached Cashfree (404) and is stale is failed.
    let (o2, pid2) = h.paid_order().await;
    let ghost = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO refunds (id, order_id, gateway_payment_id, amount_paise, status, reason, created_at) \
         VALUES ($1, $2, $3, $4, 'pending', 'Lost', now() - interval '1 hour')",
    )
    .bind(ghost)
    .bind(o2.id)
    .bind(pid2.to_string())
    .bind(TOTAL)
    .execute(&h.pool)
    .await
    .expect("ghost refund");
    let v = h.row(o2.id).await.version;
    let r = h
        .refund(
            &o2,
            json!({ "reason": "Customer request", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.code(), "version_conflict", "{}", r.body);
    let rows = h.refund_rows(o2.id).await;
    assert_eq!(rows[0].status, "failed");
    assert_eq!(
        rows[0].failure_reason.as_deref(),
        Some("Never reached Cashfree (reconciled)")
    );
    let v = h.row(o2.id).await.version;
    let r = h
        .refund(
            &o2,
            json!({ "reason": "Customer request", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");
}

#[sqlx::test(migrations = "./migrations")]
async fn refund_webhooks_update_the_ledger(pool: PgPool) {
    let h = setup(pool).await;
    let (o, pid) = h.paid_order().await;
    h.mock.with(|s| s.refund_mode = RefundMode::Pending);
    let v = h.row(o.id).await.version;

    let r = h
        .refund(
            &o,
            json!({ "amount": 650, "reason": "Partial", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(h.refund_rows(o.id).await[0].status, "pending");
    assert_eq!(
        h.row(o.id).await.amount_refunded_paise,
        65_000,
        "accepted refunds count while pending"
    );

    let entity = h.mock.with(|s| s.refunds[0].clone());
    let cf_refund_id = entity["cf_refund_id"].as_str().expect("id").to_owned();

    // A webhook claiming SUCCESS while the API still says PENDING changes nothing.
    let mut claimed = entity.clone();
    claimed["refund_status"] = json!("SUCCESS");
    let w = h
        .webhook(&refund_webhook("REFUND_STATUS_WEBHOOK", &claimed))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    assert_eq!(h.refund_rows(o.id).await[0].status, "pending");

    // Cashfree processed it: the next webhook (different body) applies it.
    h.mock.set_refund_status(&cf_refund_id, "SUCCESS");
    let mut done = claimed.clone();
    done["status_description"] = json!("Refund processed successfully");
    let w = h
        .webhook(&refund_webhook("REFUND_STATUS_WEBHOOK", &done))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    assert_eq!(h.refund_rows(o.id).await[0].status, "processed");
    let row = h.row(o.id).await;
    assert_eq!(row.amount_refunded_paise, 65_000);
    assert_eq!(row.payment_status, "paid");

    // A refund made in the Cashfree dashboard for the rest is recorded too.
    let dashboard = json!({
        "cf_refund_id": 11325632_u64, "cf_payment_id": pid, "refund_id": "refund_dash_0001",
        "order_id": o.gid(), "refund_amount": 1000.0, "refund_currency": "INR", "entity": "Refund",
        "refund_type": "MERCHANT_INITIATED", "refund_status": "SUCCESS", "status_description": "Refund processed successfully"
    });
    h.mock.with(|s| s.refunds.push(dashboard.clone()));
    let w = h
        .webhook(&refund_webhook("REFUND_STATUS_WEBHOOK", &dashboard))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    let row = h.row(o.id).await;
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert_eq!(row.payment_status, "refunded");
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds.len(), 2);
    assert_eq!(
        refunds[1].reason,
        "Refund issued from the Cashfree Merchant Dashboard"
    );
    assert_eq!(refunds[1].gateway_refund_id.as_deref(), Some("11325632"));

    // A later CANCELLED for the processed refund never regresses it.
    h.mock.set_refund_status(&cf_refund_id, "CANCELLED");
    let mut cancelled = entity;
    cancelled["refund_status"] = json!("CANCELLED");
    let w = h
        .webhook(&refund_webhook("REFUND_STATUS_WEBHOOK", &cancelled))
        .await;
    assert_eq!(w.status, StatusCode::OK);
    let row = h.row(o.id).await;
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert_eq!(row.payment_status, "refunded");
    assert!(row.payment_attention.is_none());

    // Refund unknown to Cashfree: acknowledged, nothing recorded.
    let ghost = json!({ "cf_refund_id": 999, "cf_payment_id": pid, "refund_id": "nope_1234", "order_id": o.gid(),
                        "refund_amount": 1.0, "refund_status": "SUCCESS" });
    let w = h
        .webhook(&refund_webhook("REFUND_STATUS_WEBHOOK", &ghost))
        .await;
    assert_eq!(w.status, StatusCode::OK);
    assert_eq!(h.refund_rows(o.id).await.len(), 2);

    let ev = h
        .admin(
            Method::GET,
            &format!("/api/v1/admin/orders/{}/payment-events", o.id),
            None,
        )
        .await;
    let events = ev.body["events"].as_array().expect("events");
    assert!(
        events
            .iter()
            .any(|e| e["event"] == "REFUND_STATUS_WEBHOOK" && e["outcome"] == "refund_processed")
    );
    assert!(
        events
            .iter()
            .any(|e| e["outcome"] == "ignored: refund not found at the gateway")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn refund_failure_reported_by_cashfree_flags_the_order(pool: PgPool) {
    let h = setup(pool).await;
    let (o, _) = h.paid_order().await;
    h.mock.with(|s| s.refund_mode = RefundMode::Pending);
    let v = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "amount": 650, "reason": "Partial", "expectedVersion": v }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let entity = h.mock.with(|s| s.refunds[0].clone());
    let id = entity["cf_refund_id"].as_str().expect("id").to_owned();
    h.mock.set_refund_status(&id, "CANCELLED");
    let mut hook = entity;
    hook["refund_status"] = json!("CANCELLED");
    let w = h
        .webhook(&refund_webhook("REFUND_STATUS_WEBHOOK", &hook))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    let refunds = h.refund_rows(o.id).await;
    assert_eq!(refunds[0].status, "failed");
    let row = h.row(o.id).await;
    assert_eq!(
        row.amount_refunded_paise, 0,
        "a failed refund no longer counts"
    );
    assert!(
        row.payment_attention
            .as_deref()
            .is_some_and(|a| a.contains(&id))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn notifications_disabled_queue_nothing(pool: PgPool) {
    let h = setup(pool).await;
    let (o, _) = h.paid_order().await;
    assert_eq!(h.row(o.id).await.payment_status, "paid");
    assert_eq!(h.outbox_rows(o.id).await, 0);
}

// ---------------------------------------------------------------------------
// Tests: disabled mode, legacy Razorpay orders, migration
// ---------------------------------------------------------------------------

fn payments_off(c: &mut Config) {
    c.payments_enabled = false;
    c.cashfree_app_id = String::new();
    c.cashfree_secret_key = SecretString::from("");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_storefront_config_hides_the_gateway(pool: PgPool) {
    let h = setup_with(pool, payments_off).await;
    let r = h
        .raw(Method::GET, "/api/v1/storefront/config", &[], None)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["onlinePaymentsEnabled"], false);
    assert!(r.body["paymentGateway"].is_null(), "{}", r.body);
    assert!(r.body["cashfreeMode"].is_null(), "{}", r.body);
    assert!(r.body.get("razorpayKeyId").is_none());
    assert_eq!(r.body["currency"], "INR");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_pay_verify_webhook_are_refused(pool: PgPool) {
    let h = setup_with(pool, payments_off).await;
    let o = h.seed_order().await;
    sqlx::query(
        "UPDATE orders SET status = 'pending_pack', payment_method = 'pay_later', payment_expires_at = NULL WHERE id = $1",
    )
    .bind(o.id)
    .execute(&h.pool)
    .await
    .expect("pay later");

    let p = h.pay(&o).await;
    assert_eq!(p.status, StatusCode::CONFLICT, "{}", p.body);
    assert_eq!(p.code(), "online_payments_disabled");
    let v = h.verify(&o).await;
    assert_eq!(v.status, StatusCode::CONFLICT, "{}", v.body);
    assert_eq!(v.code(), "online_payments_disabled");
    let other = h.seed_order().await;
    let r = h
        .guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay", o.number),
            &other.token,
            None,
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::NOT_FOUND,
        "the token is still checked first"
    );

    // Webhook: 404 even with a well-formed body signed with the (empty) key; nothing recorded.
    let entity = payment_entity(&o.gid(), next_pid(), TOTAL, "SUCCESS");
    let body = payment_webhook("PAYMENT_SUCCESS_WEBHOOK", &entity, TOTAL)
        .to_string()
        .into_bytes();
    let w = h.webhook_signed(&body, "", "1760950000001").await;
    assert_eq!(w.status, StatusCode::NOT_FOUND, "{}", w.body);
    assert_eq!(w.code(), "not_found");
    let w = h.webhook_signed(&body, SECRET_KEY, "1760950000001").await;
    assert_eq!(w.status, StatusCode::NOT_FOUND);
    assert_eq!(h.event_count().await, 0);

    let g = h
        .guest(
            Method::GET,
            &format!("/api/v1/orders/{}", o.number),
            &o.token,
            None,
        )
        .await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert_eq!(
        g.body["payment"],
        json!({ "method": "pay_later", "status": "pending", "instrument": null, "paidAt": null })
    );

    let version = h.row(o.id).await.version;
    let r = h
        .refund(&o, json!({ "reason": "x", "expectedVersion": version }))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "invalid_transition");

    assert!(h.row(o.id).await.gateway_order_id.is_none());
    let calls = h
        .mock
        .with(|s| s.create_calls + s.payment_reads + s.refund_calls + s.unauthorized);
    assert_eq!(calls, 0, "Cashfree is never called while disabled");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_mode_does_not_refund_cashfree_orders_online(pool: PgPool) {
    let h = setup_with(pool, payments_off).await;
    let o = h.seed_order().await;
    sqlx::query(
        "UPDATE orders SET status = 'pending_pack', payment_status = 'paid', payment_expires_at = NULL, \
         gateway_order_id = $2, gateway_payment_id = '5114910253538', paid_at = now() WHERE id = $1",
    )
    .bind(o.id)
    .bind(o.gid())
    .execute(&h.pool)
    .await
    .expect("paid before");
    let version = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "reason": "Customer request", "expectedVersion": version }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "online_payments_disabled");
    assert_eq!(
        h.scalar_i64("SELECT count(*) FROM refunds WHERE order_id = $1", o.id)
            .await,
        0
    );
    assert_eq!(h.mock.with(|s| s.refund_calls + s.unauthorized), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn legacy_razorpay_orders_are_not_refunded_or_settled_via_cashfree(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    sqlx::query(
        "UPDATE orders SET status = 'pending_pack', payment_status = 'paid', payment_method = 'razorpay', \
         payment_expires_at = NULL, gateway_order_id = 'order_Before00001', gateway_payment_id = 'pay_Before000001', \
         paid_at = now() WHERE id = $1",
    )
    .bind(o.id)
    .execute(&h.pool)
    .await
    .expect("paid via razorpay");
    let version = h.row(o.id).await.version;
    let r = h
        .refund(
            &o,
            json!({ "reason": "Customer request", "expectedVersion": version }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "invalid_transition");
    assert!(
        r.body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("Razorpay Dashboard"))
    );
    // Verify on it is the idempotent "already paid" view.
    let v = h.verify(&o).await;
    assert_eq!(v.status, StatusCode::OK, "{}", v.body);
    assert_eq!(v.body["payment"]["method"], "razorpay");
    assert_eq!(h.mock.with(|s| s.refund_calls + s.payment_reads), 0);
}

/// Migration 000013 against data written under the Razorpay schema.
#[sqlx::test(migrations = false)]
async fn migration_moves_pending_razorpay_orders_to_cashfree(pool: PgPool) {
    const BEFORE: [&str; 12] = [
        include_str!("../migrations/20261013000001_admin_auth.sql"),
        include_str!("../migrations/20261013000002_catalog_orders.sql"),
        include_str!("../migrations/20261013000003_auth_hardening.sql"),
        include_str!("../migrations/20261013000004_seed_catalog.sql"),
        include_str!("../migrations/20261013000005_razorpay_payments.sql"),
        include_str!("../migrations/20261013000006_catalog_2026_price_list.sql"),
        include_str!("../migrations/20261013000007_order_notifications.sql"),
        include_str!("../migrations/20261013000008_customer_order_emails.sql"),
        include_str!("../migrations/20261013000009_product_photos.sql"),
        include_str!("../migrations/20261013000010_pricing_tier.sql"),
        include_str!("../migrations/20261014000011_delivery_fee.sql"),
        include_str!("../migrations/20261014000012_google_sheets_sync.sql"),
    ];
    for sql in BEFORE {
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .expect("old migration");
    }
    let insert = |n: u32,
                  status: &str,
                  ps: &str,
                  rzp_order: Option<&str>,
                  rzp_pay: Option<&str>| {
        let (status, ps) = (status.to_owned(), ps.to_owned());
        let (rzp_order, rzp_pay) = (rzp_order.map(str::to_owned), rzp_pay.map(str::to_owned));
        let pool = pool.clone();
        async move {
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO orders (id, order_number, idempotency_key, customer_name, customer_phone, address_line, \
                 city, state, pincode, item_count, mrp_total_paise, total_paise, status, payment_method, payment_status, \
                 payment_expires_at, razorpay_order_id, razorpay_payment_id) \
                 VALUES ($1, $2, $3, 'A B', '+919840012345', '12 Test Street', 'Chennai', 'Tamil Nadu', '600028', 1, 2000, 1000, \
                 $4, 'razorpay', $5, now() + interval '10 minutes', $6, $7)",
            )
            .bind(id)
            .bind(format!("ZM-261019-{n}"))
            .bind(Uuid::new_v4())
            .bind(status)
            .bind(ps)
            .bind(rzp_order)
            .bind(rzp_pay)
            .execute(&pool)
            .await
            .expect("legacy order");
            id
        }
    };
    let pending = insert(
        1,
        "pending_payment",
        "pending",
        Some("order_Pending00001"),
        None,
    )
    .await;
    let paid = insert(
        2,
        "pending_pack",
        "paid",
        Some("order_Paid0000001"),
        Some("pay_Paid00000001"),
    )
    .await;
    sqlx::query(
        "INSERT INTO refunds (id, order_id, razorpay_payment_id, razorpay_refund_id, amount_paise, status, reason) \
         VALUES ($1, $2, 'pay_Paid00000001', 'rfnd_Paid0000001', 500, 'processed', 'x')",
    )
    .bind(Uuid::now_v7())
    .bind(paid)
    .execute(&pool)
    .await
    .expect("legacy refund");
    sqlx::query(
        "INSERT INTO payment_events (id, event, order_id, razorpay_order_id, razorpay_payment_id, payload) \
         VALUES ('evt_Legacy0001', 'payment.captured', $1, 'order_Paid0000001', 'pay_Paid00000001', '{}')",
    )
    .bind(paid)
    .execute(&pool)
    .await
    .expect("legacy event");

    sqlx::raw_sql(include_str!(
        "../migrations/20261020000013_cashfree_gateway.sql"
    ))
    .execute(&pool)
    .await
    .expect("migration 13");

    let read = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (String, Option<String>, Option<String>, i32)>(
                "SELECT payment_method, gateway_order_id, gateway_payment_id, version FROM orders WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("order")
        }
    };
    assert_eq!(read(pending).await, ("cashfree".into(), None, None, 2));
    assert_eq!(
        read(paid).await,
        (
            "razorpay".into(),
            Some("order_Paid0000001".into()),
            Some("pay_Paid00000001".into()),
            1
        )
    );
    let r: (String, Option<String>) =
        sqlx::query_as("SELECT gateway_payment_id, gateway_refund_id FROM refunds")
            .fetch_one(&pool)
            .await
            .expect("refund");
    assert_eq!(
        r,
        ("pay_Paid00000001".into(), Some("rfnd_Paid0000001".into()))
    );
    let e: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT gateway_order_id, gateway_payment_id FROM payment_events")
            .fetch_one(&pool)
            .await
            .expect("event");
    assert_eq!(
        e,
        (
            Some("order_Paid0000001".into()),
            Some("pay_Paid00000001".into())
        )
    );

    // New constraints: Cashfree-style ids accepted, junk refused; 'cashfree' allowed.
    sqlx::query("UPDATE orders SET gateway_order_id = 'ZM-261019-1_8f3a2b1c9d0e' WHERE id = $1")
        .bind(pending)
        .execute(&pool)
        .await
        .expect("cashfree id");
    let bad = sqlx::query("UPDATE orders SET gateway_order_id = 'bad id!' WHERE id = $1")
        .bind(pending)
        .execute(&pool)
        .await;
    assert!(bad.is_err());
    let bad = sqlx::query("UPDATE orders SET payment_method = 'paypal' WHERE id = $1")
        .bind(pending)
        .execute(&pool)
        .await;
    assert!(bad.is_err());
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT indexname::text FROM pg_indexes WHERE indexname LIKE '%gateway%' ORDER BY indexname",
    )
    .fetch_all(&pool)
    .await
    .expect("indexes");
    assert_eq!(
        indexes,
        vec![
            "orders_gateway_order_id_key",
            "orders_gateway_payment_id_key",
            "refunds_gateway_refund_id_key"
        ]
    );
}
