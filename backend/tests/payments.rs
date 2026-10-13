//! End-to-end tests for Razorpay payments: guest pay/verify/status, the
//! webhook, late payments and admin refunds — against a real PostgreSQL
//! database (`#[sqlx::test]`) and a mock Razorpay API served by axum on
//! 127.0.0.1:0 (Basic auth checked, controllable responses).
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
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use zhimart_backend::{
    config::{AppEnv, Config, EmailProvider},
    email::{BoxFuture, EmailSender, OtpEmail},
    payments::{order_token, signature},
    repo, routes,
    state::AppState,
};

const KEY_ID: &str = "rzp_test_0123456789abcd";
const KEY_SECRET: &str = "test_key_secret_0123456789";
const WEBHOOK_SECRET: &str = "test_webhook_secret_0123456789";
const PRODUCT: &str = "zm-pay-test";
const UNIT_PRICE: i64 = 16_500;
const QTY: i32 = 10;
const TOTAL: i64 = UNIT_PRICE * QTY as i64;

// ---------------------------------------------------------------------------
// Mock Razorpay
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefundMode {
    /// Accept; refund entity status `processed`.
    Processed,
    /// Accept; refund entity status `pending` (completed later by webhook).
    Pending,
    /// 400 BAD_REQUEST_ERROR.
    Reject,
}

struct MockState {
    orders_created: u32,
    payments: HashMap<String, Value>,
    refunds: Vec<Value>,
    refund_mode: RefundMode,
    capture_calls: u32,
    refund_calls: u32,
    unauthorized: u32,
}

#[derive(Clone)]
struct Mock(Arc<Mutex<MockState>>);

impl Mock {
    fn with<R>(&self, f: impl FnOnce(&mut MockState) -> R) -> R {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut g)
    }

    fn add_payment(&self, id: &str, order_id: &str, amount: i64, status: &str) -> Value {
        let p = payment_entity(id, order_id, amount, status);
        self.with(|s| s.payments.insert(id.to_owned(), p.clone()));
        p
    }
}

fn payment_entity(id: &str, order_id: &str, amount: i64, status: &str) -> Value {
    json!({
        "id": id, "entity": "payment", "amount": amount, "currency": "INR", "status": status,
        "order_id": order_id, "method": "upi", "captured": status == "captured", "amount_refunded": 0,
        "email": "customer@example.com", "contact": "+919840012345", "vpa": "customer@upi",
        "error_code": null, "error_reason": null, "notes": []
    })
}

fn rzp_error(status: StatusCode, description: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": "BAD_REQUEST_ERROR", "description": description } })),
    )
        .into_response()
}

fn basic_ok(headers: &HeaderMap) -> bool {
    let expected = format!(
        "Basic {}",
        STANDARD.encode(format!("{KEY_ID}:{KEY_SECRET}"))
    );
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        == Some(expected.as_str())
}

macro_rules! require_auth {
    ($mock:expr, $headers:expr) => {
        if !basic_ok(&$headers) {
            $mock.with(|s| s.unauthorized += 1);
            return rzp_error(StatusCode::UNAUTHORIZED, "The api key provided is invalid");
        }
    };
}

async fn mock_create_order(
    State(m): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    require_auth!(m, headers);
    let n = m.with(|s| {
        s.orders_created += 1;
        s.orders_created
    });
    Json(json!({
        "id": format!("order_Mock{n:08}"), "entity": "order", "amount": body["amount"], "amount_paid": 0,
        "amount_due": body["amount"], "currency": body["currency"], "receipt": body["receipt"],
        "status": "created", "attempts": 0, "notes": body["notes"], "created_at": 1_760_000_000
    }))
    .into_response()
}

async fn mock_fetch_payment(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    require_auth!(m, headers);
    match m.with(|s| s.payments.get(&id).cloned()) {
        Some(p) => Json(p).into_response(),
        None => rzp_error(StatusCode::BAD_REQUEST, "The id provided does not exist"),
    }
}

async fn mock_capture(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    require_auth!(m, headers);
    m.with(|s| {
        s.capture_calls += 1;
        match s.payments.get_mut(&id) {
            Some(p) if p["status"] == "authorized" && p["amount"] == body["amount"] => {
                p["status"] = json!("captured");
                p["captured"] = json!(true);
                Json(p.clone()).into_response()
            }
            Some(_) => rzp_error(
                StatusCode::BAD_REQUEST,
                "This payment has already been captured",
            ),
            None => rzp_error(StatusCode::BAD_REQUEST, "The id provided does not exist"),
        }
    })
}

async fn mock_refund(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    require_auth!(m, headers);
    let idem = headers
        .get("x-refund-idempotency")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    m.with(|s| {
        s.refund_calls += 1;
        if s.refund_mode == RefundMode::Reject {
            return rzp_error(StatusCode::BAD_REQUEST, "The refund could not be processed");
        }
        // Idempotency: the same key returns the same refund.
        if let Some(k) = &idem {
            if let Some(r) = s.refunds.iter().find(|r| r["receipt"].as_str() == Some(k.as_str())) {
                return Json(r.clone()).into_response();
            }
        }
        if !s.payments.contains_key(&id) {
            return rzp_error(StatusCode::BAD_REQUEST, "The id provided does not exist");
        }
        let n = s.refunds.len() + 1;
        let status = if s.refund_mode == RefundMode::Processed { "processed" } else { "pending" };
        let r = json!({
            "id": format!("rfnd_Mock{n:08}"), "entity": "refund", "amount": body["amount"], "currency": "INR",
            "payment_id": id, "receipt": body["receipt"], "notes": body["notes"], "status": status,
            "speed_requested": body["speed"]
        });
        s.refunds.push(r.clone());
        Json(r).into_response()
    })
}

async fn mock_payment_refunds(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    require_auth!(m, headers);
    let items: Vec<Value> = m.with(|s| {
        s.refunds
            .iter()
            .filter(|r| r["payment_id"] == json!(id))
            .cloned()
            .collect()
    });
    Json(json!({ "entity": "collection", "count": items.len(), "items": items })).into_response()
}

async fn mock_order_payments(
    State(m): State<Mock>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    require_auth!(m, headers);
    let items: Vec<Value> = m.with(|s| {
        s.payments
            .values()
            .filter(|p| p["order_id"] == json!(id))
            .cloned()
            .collect()
    });
    Json(json!({ "entity": "collection", "count": items.len(), "items": items })).into_response()
}

async fn start_mock() -> (Mock, String) {
    let mock = Mock(Arc::new(Mutex::new(MockState {
        orders_created: 0,
        payments: HashMap::new(),
        refunds: Vec::new(),
        refund_mode: RefundMode::Processed,
        capture_calls: 0,
        refund_calls: 0,
        unauthorized: 0,
    })));
    let app = Router::new()
        .route("/v1/orders", post(mock_create_order))
        .route("/v1/orders/{id}/payments", get(mock_order_payments))
        .route("/v1/payments/{id}", get(mock_fetch_payment))
        .route("/v1/payments/{id}/capture", post(mock_capture))
        .route("/v1/payments/{id}/refund", post(mock_refund))
        .route("/v1/payments/{id}/refunds", get(mock_payment_refunds))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (mock, format!("http://{addr}"))
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
        razorpay_key_id: KEY_ID.into(),
        razorpay_key_secret: SecretString::from(KEY_SECRET),
        razorpay_webhook_secret: SecretString::from(WEBHOOK_SECRET),
        razorpay_api_base: api_base,
        payment_expiry: Duration::from_secs(30 * 60),
        store_name: "Zhi-Mart Fireworks".into(),
        order_notifications_enabled: false,
        order_notification_emails: Vec::new(),
        public_site_url: None,
        customer_order_emails_enabled: false,
        pricing: Default::default(),
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

static ORDER_SEQ: AtomicU32 = AtomicU32::new(5001);

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

    async fn verify(&self, o: &Seeded, oid: &str, pid: &str, sig: &str) -> Resp {
        let body =
            json!({ "razorpayOrderId": oid, "razorpayPaymentId": pid, "razorpaySignature": sig });
        self.guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay/verify", o.number),
            &o.token,
            Some(body),
        )
        .await
    }

    async fn verify_signed(&self, o: &Seeded, oid: &str, pid: &str) -> Resp {
        let sig = signature::checkout_signature(KEY_SECRET.as_bytes(), oid, pid);
        self.verify(o, oid, pid, &sig).await
    }

    async fn webhook(&self, event_id: Option<&str>, body: &Value) -> Resp {
        let bytes = body.to_string().into_bytes();
        let sig = signature::webhook_signature(WEBHOOK_SECRET.as_bytes(), &bytes);
        let mut headers = vec![("x-razorpay-signature", sig)];
        if let Some(id) = event_id {
            headers.push(("x-razorpay-event-id", id.to_owned()));
        }
        self.raw(
            Method::POST,
            "/api/v1/webhooks/razorpay",
            &headers,
            Some(bytes),
        )
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

    /// Seed a `pending_payment` order for QTY units of PRODUCT (stock reserved).
    async fn seed_order(&self) -> Seeded {
        let id = Uuid::now_v7();
        let number = format!("ZM-261013-{}", ORDER_SEQ.fetch_add(1, Ordering::Relaxed));
        let mut tx = self.pool.begin().await.expect("tx");
        sqlx::query(
            "INSERT INTO orders (id, order_number, idempotency_key, customer_name, customer_phone, customer_email, \
             address_line, city, state, pincode, item_count, mrp_total_paise, total_paise, status, payment_method, \
             payment_status, payment_expires_at) \
             VALUES ($1, $2, $3, 'Test Customer', '+919840012345', NULL, '12 Test Street', 'Chennai', 'Tamil Nadu', \
             '600028', $4, $5, $6, 'pending_payment', 'razorpay', 'pending', now() + interval '30 minutes')",
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

    /// `pay` must succeed; returns the Razorpay order id.
    async fn rzp_order(&self, o: &Seeded) -> String {
        let r = self.pay(o).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body["razorpayOrderId"]
            .as_str()
            .expect("razorpayOrderId")
            .to_owned()
    }

    /// Seed + pay + captured payment + verify ⇒ paid order. Returns (order, payment id).
    async fn paid_order(&self, pid: &str) -> Seeded {
        let o = self.seed_order().await;
        let oid = self.rzp_order(&o).await;
        self.mock.add_payment(pid, &oid, TOTAL, "captured");
        let r = self.verify_signed(&o, &oid, pid).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        o
    }

    async fn row(&self, id: Uuid) -> DbOrder {
        sqlx::query_as::<_, DbOrder>(
            "SELECT status, payment_status, payment_method, razorpay_order_id, razorpay_payment_id, payment_instrument, \
             paid_at, amount_refunded_paise, payment_attention, version, refunded_at FROM orders WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .expect("order row")
    }

    async fn outbox_rows(&self, id: Uuid) -> i64 {
        self.scalar_i64(
            "SELECT count(*) FROM email_outbox WHERE order_id = $1 AND kind = 'admin_order_notification'",
            id,
        )
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

    /// Expire the order through the real sweep (as jobs.rs does).
    async fn expire(&self, o: &Seeded) {
        sqlx::query(
            "UPDATE orders SET payment_expires_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(o.id)
        .execute(&self.pool)
        .await
        .expect("backdate");
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
    razorpay_order_id: Option<String>,
    razorpay_payment_id: Option<String>,
    payment_instrument: Option<String>,
    paid_at: Option<DateTime<Utc>>,
    amount_refunded_paise: i64,
    payment_attention: Option<String>,
    version: i32,
    refunded_at: Option<DateTime<Utc>>,
}

fn payment_event(event: &str, payment: Value) -> Value {
    json!({
        "entity": "event", "account_id": "acc_TestAccount01", "event": event, "contains": ["payment"],
        "payload": { "payment": { "entity": payment } }, "created_at": 1_760_000_000
    })
}

fn refund_event(event: &str, refund: Value) -> Value {
    json!({
        "entity": "event", "account_id": "acc_TestAccount01", "event": event, "contains": ["refund", "payment"],
        "payload": { "refund": { "entity": refund } }, "created_at": 1_760_000_000
    })
}

// ---------------------------------------------------------------------------
// Tests
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
        json!({ "testMode": false, "onlinePaymentsEnabled": true, "razorpayKeyId": KEY_ID, "currency": "INR",
                "paymentExpiryMinutes": 30, "storeName": "Zhi-Mart Fireworks",
                "pricing": { "bulkMinOrder": 10000, "bulkDiscountPercent": 85, "standardDiscountPercent": 70 } })
    );
    assert_eq!(r.headers[header::CACHE_CONTROL], "public, max-age=60");
}

#[sqlx::test(migrations = "./migrations")]
async fn pay_creates_razorpay_order_once_and_reuses_it(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;

    let r1 = h.pay(&o).await;
    assert_eq!(r1.status, StatusCode::OK, "{}", r1.body);
    assert_eq!(r1.headers[header::CACHE_CONTROL], "no-store");
    let b = &r1.body;
    assert_eq!(b["keyId"], KEY_ID);
    assert_eq!(b["amountPaise"], TOTAL);
    assert_eq!(b["currency"], "INR");
    assert_eq!(b["name"], "Zhi-Mart Fireworks");
    assert_eq!(b["description"], format!("Order {}", o.number));
    assert_eq!(
        b["prefill"],
        json!({ "name": "Test Customer", "email": null, "contact": "+919840012345" })
    );
    assert_eq!(b["notes"], json!({ "orderNumber": o.number }));
    assert_eq!(b["testMode"], false);
    assert!(b["paymentExpiresAt"].is_string());
    let oid = b["razorpayOrderId"].as_str().expect("id").to_owned();
    assert!(signature::is_order_id(&oid));

    let r2 = h.pay(&o).await;
    assert_eq!(r2.status, StatusCode::OK);
    assert_eq!(r2.body["razorpayOrderId"], oid.as_str());

    // Concurrent double click: still one stored id, and both answers agree.
    let (a, c) = tokio::join!(h.pay(&o), h.pay(&o));
    assert_eq!(a.body["razorpayOrderId"], oid.as_str());
    assert_eq!(c.body["razorpayOrderId"], oid.as_str());

    assert_eq!(h.mock.with(|s| s.orders_created), 1);
    assert_eq!(h.mock.with(|s| s.unauthorized), 0);
    assert_eq!(
        h.row(o.id).await.razorpay_order_id.as_deref(),
        Some(oid.as_str())
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_first_pay_calls_agree_on_one_razorpay_order(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let (a, b, c) = tokio::join!(h.pay(&o), h.pay(&o), h.pay(&o));
    for r in [&a, &b, &c] {
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    }
    let stored = h.row(o.id).await.razorpay_order_id.expect("stored");
    for r in [&a, &b, &c] {
        assert_eq!(r.body["razorpayOrderId"], stored.as_str());
    }
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
                Some(json!({ "razorpayOrderId": "order_AAAAAAAAAA", "razorpayPaymentId": "pay_AAAAAAAAAA", "razorpaySignature": "00" })),
            )
            .await;
        assert_eq!(v.status, StatusCode::NOT_FOUND);
    }
    // No header at all.
    let r = h
        .raw(
            Method::GET,
            &format!("/api/v1/orders/{}", o.number),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // Unknown and malformed order numbers look the same.
    let r = h
        .guest(
            Method::GET,
            "/api/v1/orders/ZM-261013-999999",
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
        h.mock.with(|s| s.orders_created),
        0,
        "nothing created without a valid token"
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
    assert_eq!(g.body["city"], "Chennai");
    assert_eq!(g.body["totalAmount"], 1650);
    // Seeded with plain SQL: the column default (pre-migration orders were all bulk).
    assert_eq!(g.body["pricingTier"], "bulk");
    assert_eq!(
        g.body["payment"],
        json!({ "method": "razorpay", "status": "pending", "instrument": null, "paidAt": null })
    );
    let text = g.body.to_string();
    assert!(
        !text.contains("9840012345") && !text.contains("Test Street") && !text.contains("600028")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn verify_happy_path_marks_paid_and_is_idempotent(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    h.mock
        .add_payment("pay_Happy000001", &oid, TOTAL, "captured");

    let r = h.verify_signed(&o, &oid, "pay_Happy000001").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["status"], "pending_pack");
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["payment"]["instrument"], "upi");
    assert_eq!(r.body["payment"]["method"], "razorpay");
    assert!(r.body["payment"]["paidAt"].is_string());
    assert!(r.body["paymentExpiresAt"].is_null());

    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert_eq!(row.payment_status, "paid");
    assert_eq!(row.payment_method, "razorpay");
    assert_eq!(row.razorpay_payment_id.as_deref(), Some("pay_Happy000001"));
    assert_eq!(row.payment_instrument.as_deref(), Some("upi"));

    // Retried verify: 200, no second transition.
    let again = h.verify_signed(&o, &oid, "pay_Happy000001").await;
    assert_eq!(again.status, StatusCode::OK);
    let row2 = h.row(o.id).await;
    assert_eq!(row2.version, row.version);
    assert_eq!(row2.paid_at, row.paid_at);

    // Pay on a paid order.
    let p = h.pay(&o).await;
    assert_eq!(p.status, StatusCode::CONFLICT);
    assert_eq!(p.code(), "order_already_paid");
}

#[sqlx::test(migrations = "./migrations")]
async fn bad_signature_is_rejected_and_order_unchanged(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    h.mock
        .add_payment("pay_BadSig00001", &oid, TOTAL, "captured");
    let before = h.row(o.id).await;

    let wrong_key = {
        let sig =
            signature::checkout_signature(b"some_other_secret_123456", &oid, "pay_BadSig00001");
        h.verify(&o, &oid, "pay_BadSig00001", &sig).await
    };
    let garbage = h.verify(&o, &oid, "pay_BadSig00001", "deadbeef").await;
    // Valid signature, but for a Razorpay order that is not this order's.
    let foreign = h
        .verify_signed(&o, "order_SomeoneElse1", "pay_BadSig00001")
        .await;
    for r in [&wrong_key, &garbage, &foreign] {
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
        assert_eq!(r.code(), "payment_verification_failed");
    }
    let unknown_field = h
        .guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay/verify", o.number),
            &o.token,
            Some(json!({ "razorpayOrderId": oid, "razorpayPaymentId": "pay_BadSig00001", "razorpaySignature": "00", "paid": true })),
        )
        .await;
    assert_eq!(unknown_field.status, StatusCode::BAD_REQUEST);

    let after = h.row(o.id).await;
    assert_eq!(after.status, "pending_payment");
    assert_eq!(after.payment_status, "pending");
    assert_eq!(after.version, before.version);
}

#[sqlx::test(migrations = "./migrations")]
async fn amount_mismatch_from_api_is_rejected(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    h.mock.add_payment("pay_Cheap000001", &oid, 100, "captured");

    let r = h.verify_signed(&o, &oid, "pay_Cheap000001").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.code(), "payment_verification_failed");
    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_payment");
    assert_eq!(row.payment_status, "pending");
    assert!(row.razorpay_payment_id.is_none());
    assert!(
        row.payment_attention.is_some(),
        "captured money that does not match is flagged"
    );

    // A payment Razorpay has no record of.
    let r = h.verify_signed(&o, &oid, "pay_Unknown00001").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn authorized_payment_is_captured(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    h.mock
        .add_payment("pay_Auth0000001", &oid, TOTAL, "authorized");

    let r = h.verify_signed(&o, &oid, "pay_Auth0000001").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(h.mock.with(|s| s.capture_calls), 1);
    assert_eq!(
        h.mock
            .with(|s| s.payments["pay_Auth0000001"]["status"].clone()),
        json!("captured")
    );

    // A failed payment is never settled.
    let o2 = h.seed_order().await;
    let oid2 = h.rzp_order(&o2).await;
    h.mock
        .add_payment("pay_Failed00001", &oid2, TOTAL, "failed");
    let r = h.verify_signed(&o2, &oid2, "pay_Failed00001").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(h.row(o2.id).await.status, "pending_payment");
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_verify_and_webhook_transition_once(pool: PgPool) {
    let h = setup_with(pool, |c| c.order_notifications_enabled = true).await;
    for i in 0..5 {
        let o = h.seed_order().await;
        let oid = h.rzp_order(&o).await;
        let pid = format!("pay_Race{i:07}");
        let entity = h.mock.add_payment(&pid, &oid, TOTAL, "captured");
        let before = h.row(o.id).await;
        assert_eq!(h.outbox_rows(o.id).await, 0, "no email before payment");

        let event = payment_event("payment.captured", entity.clone());
        let order_paid = json!({
            "entity": "event", "account_id": "acc_TestAccount01", "event": "order.paid", "contains": ["payment", "order"],
            "payload": { "payment": { "entity": entity }, "order": { "entity": { "id": oid, "amount": TOTAL, "status": "paid" } } },
            "created_at": 1_760_000_000
        });
        let ev1 = format!("evt_RaceCaptured{i}");
        let ev2 = format!("evt_RaceOrderPaid{i}");
        let (v, w1, w2) = tokio::join!(
            h.verify_signed(&o, &oid, &pid),
            h.webhook(Some(&ev1), &event),
            h.webhook(Some(&ev2), &order_paid),
        );
        assert_eq!(v.status, StatusCode::OK, "{}", v.body);
        assert_eq!(w1.status, StatusCode::OK, "{}", w1.body);
        assert_eq!(w2.status, StatusCode::OK, "{}", w2.body);

        let after = h.row(o.id).await;
        assert_eq!(after.status, "pending_pack");
        assert_eq!(after.payment_status, "paid");
        assert_eq!(after.razorpay_payment_id.as_deref(), Some(pid.as_str()));
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
        let outcomes: Vec<String> = sqlx::query_scalar(
            "SELECT outcome FROM payment_events WHERE order_id = $1 ORDER BY id",
        )
        .bind(o.id)
        .fetch_all(&h.pool)
        .await
        .expect("outcomes");
        assert_eq!(outcomes.len(), 2);
        assert!(
            outcomes.iter().all(|x| x == "paid" || x == "already_paid"),
            "{outcomes:?}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_rejects_bad_or_missing_signature(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    let entity = h
        .mock
        .add_payment("pay_Forged00001", &oid, TOTAL, "captured");
    let body = payment_event("payment.captured", entity)
        .to_string()
        .into_bytes();

    let bad = signature::webhook_signature(b"not_the_webhook_secret_0", &body);
    let r = h
        .raw(
            Method::POST,
            "/api/v1/webhooks/razorpay",
            &[
                ("x-razorpay-signature", bad),
                ("x-razorpay-event-id", "evt_Forged1".into()),
            ],
            Some(body.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Signed with the KEY secret instead of the webhook secret.
    let wrong_secret = signature::webhook_signature(KEY_SECRET.as_bytes(), &body);
    let r = h
        .raw(
            Method::POST,
            "/api/v1/webhooks/razorpay",
            &[("x-razorpay-signature", wrong_secret)],
            Some(body.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .raw(
            Method::POST,
            "/api/v1/webhooks/razorpay",
            &[],
            Some(body.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Body changed after signing.
    let good = signature::webhook_signature(WEBHOOK_SECRET.as_bytes(), &body);
    let mut tampered = body.clone();
    tampered.push(b' ');
    let r = h
        .raw(
            Method::POST,
            "/api/v1/webhooks/razorpay",
            &[("x-razorpay-signature", good)],
            Some(tampered),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_payment");
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM payment_events")
        .fetch_one(&h.pool)
        .await
        .expect("count");
    assert_eq!(events, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_duplicate_event_is_processed_once(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    let entity = h
        .mock
        .add_payment("pay_Webhook0001", &oid, TOTAL, "captured");
    let event = payment_event("payment.captured", entity);

    let r1 = h.webhook(Some("evt_Dup00000001"), &event).await;
    assert_eq!(r1.status, StatusCode::OK, "{}", r1.body);
    let paid = h.row(o.id).await;
    assert_eq!(paid.status, "pending_pack");
    assert_eq!(paid.payment_status, "paid");

    let r2 = h.webhook(Some("evt_Dup00000001"), &event).await;
    assert_eq!(r2.status, StatusCode::OK);
    let (count, outcome): (i64, Option<String>) = sqlx::query_as(
        "SELECT count(*), max(outcome) FROM payment_events WHERE id = 'evt_Dup00000001'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("event");
    assert_eq!(count, 1);
    assert_eq!(
        outcome.as_deref(),
        Some("paid"),
        "redelivery was not reprocessed"
    );
    assert_eq!(h.row(o.id).await.version, paid.version);

    // Stored payload is redacted (no contact details).
    let payload: Value =
        sqlx::query_scalar("SELECT payload FROM payment_events WHERE id = 'evt_Dup00000001'")
            .fetch_one(&h.pool)
            .await
            .expect("payload");
    let text = payload.to_string();
    assert!(
        !text.contains("customer@example.com")
            && !text.contains("9840012345")
            && !text.contains("customer@upi")
    );

    // Without an event-id header the id is derived from the body: still deduplicated.
    let r3 = h.webhook(None, &event).await;
    let r4 = h.webhook(None, &event).await;
    assert_eq!((r3.status, r4.status), (StatusCode::OK, StatusCode::OK));
    let derived: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payment_events WHERE id LIKE 'body_%'")
            .fetch_one(&h.pool)
            .await
            .expect("count");
    assert_eq!(derived, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn payment_failed_keeps_order_payable(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    let mut entity = payment_entity("pay_Declined001", &oid, TOTAL, "failed");
    entity["error_code"] = json!("BAD_REQUEST_ERROR");
    entity["error_reason"] = json!("payment_declined");

    let r = h
        .webhook(
            Some("evt_Failed000001"),
            &payment_event("payment.failed", entity),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let outcome: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM payment_events WHERE id = 'evt_Failed000001'")
            .fetch_one(&h.pool)
            .await
            .expect("outcome");
    assert_eq!(
        outcome.as_deref(),
        Some("payment_failed: BAD_REQUEST_ERROR")
    );

    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_payment");
    assert_eq!(row.payment_status, "pending");
    let again = h.pay(&o).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body["razorpayOrderId"], oid.as_str());

    // The retry succeeds.
    h.mock
        .add_payment("pay_Retry000001", &oid, TOTAL, "captured");
    let v = h.verify_signed(&o, &oid, "pay_Retry000001").await;
    assert_eq!(v.status, StatusCode::OK);
    assert_eq!(v.body["status"], "pending_pack");
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_order_cannot_start_payment(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.seed_order().await;
    sqlx::query("UPDATE orders SET payment_expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(o.id)
        .execute(&h.pool)
        .await
        .expect("backdate");
    let r = h.pay(&o).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "order_expired");
    repo::orders::expire_unpaid_orders(&h.pool, 10)
        .await
        .expect("sweep");
    let r = h.pay(&o).await;
    assert_eq!(r.code(), "order_expired");
    assert_eq!(h.mock.with(|s| s.orders_created), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn late_payment_with_stock_revives_order(pool: PgPool) {
    let h = setup_with(pool, |c| c.order_notifications_enabled = true).await;
    let o = h.seed_order().await;
    let oid = h.rzp_order(&o).await;
    let reserved_stock = h.stock().await;
    h.expire(&o).await;
    assert_eq!(
        h.stock().await,
        reserved_stock + QTY,
        "expiry released the stock"
    );

    h.mock
        .add_payment("pay_Late0000001", &oid, TOTAL, "captured");
    let r = h.verify_signed(&o, &oid, "pay_Late0000001").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["status"], "pending_pack");
    assert_eq!(r.body["payment"]["status"], "paid");
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
    let oid = h.rzp_order(&o).await;
    h.expire(&o).await;
    sqlx::query("UPDATE products SET stock = 0 WHERE id = $1")
        .bind(PRODUCT)
        .execute(&h.pool)
        .await
        .expect("sell out");

    h.mock
        .add_payment("pay_LateNoStock1", &oid, TOTAL, "captured");
    let r = h.verify_signed(&o, &oid, "pay_LateNoStock1").await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "order_expired");

    let row = h.row(o.id).await;
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.razorpay_payment_id.as_deref(), Some("pay_LateNoStock1"));
    assert_eq!(row.payment_status, "refunded");
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert!(row.payment_attention.is_some());
    assert_eq!(h.stock().await, 0, "no stock taken");
    assert_eq!(h.mock.with(|s| s.refund_calls), 1);
    let (status, rid): (String, Option<String>) =
        sqlx::query_as("SELECT status, razorpay_refund_id FROM refunds WHERE order_id = $1")
            .bind(o.id)
            .fetch_one(&h.pool)
            .await
            .expect("refund row");
    assert_eq!(status, "processed");
    assert!(rid.is_some());

    // The webhook for the same payment does not refund again.
    let entity = h.mock.with(|s| s.payments["pay_LateNoStock1"].clone());
    let w = h
        .webhook(
            Some("evt_LateNoStock1"),
            &payment_event("payment.captured", entity),
        )
        .await;
    assert_eq!(w.status, StatusCode::OK);
    assert_eq!(h.mock.with(|s| s.refund_calls), 1);
    assert_eq!(
        h.scalar_i64("SELECT count(*) FROM refunds WHERE order_id = $1", o.id)
            .await,
        1
    );
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
    let oid = h.rzp_order(&o).await;
    sqlx::query("UPDATE orders SET status = 'cancelled', cancel_reason = 'Customer request', cancelled_at = now() WHERE id = $1")
        .bind(o.id)
        .execute(&h.pool)
        .await
        .expect("cancel");
    h.mock
        .add_payment("pay_Cancelled01", &oid, TOTAL, "captured");
    let r = h.verify_signed(&o, &oid, "pay_Cancelled01").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "order_not_payable");
    let row = h.row(o.id).await;
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.payment_status, "refunded");
    assert!(row.payment_attention.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn duplicate_payment_is_refunded_automatically(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.paid_order("pay_First000001").await;
    let oid = h.row(o.id).await.razorpay_order_id.expect("rzp order");
    let second = h
        .mock
        .add_payment("pay_Second00001", &oid, TOTAL, "captured");

    let w = h
        .webhook(
            Some("evt_Second000001"),
            &payment_event("payment.captured", second),
        )
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert_eq!(
        row.payment_status, "paid",
        "the order's own payment is untouched"
    );
    assert_eq!(row.razorpay_payment_id.as_deref(), Some("pay_First000001"));
    assert_eq!(row.amount_refunded_paise, 0);
    assert!(
        row.payment_attention
            .as_deref()
            .is_some_and(|a| a.contains("pay_Second00001"))
    );
    let refunded: String =
        sqlx::query_scalar("SELECT razorpay_payment_id FROM refunds WHERE order_id = $1")
            .bind(o.id)
            .fetch_one(&h.pool)
            .await
            .expect("refund");
    assert_eq!(refunded, "pay_Second00001");

    // Verify of the second payment (customer's browser) is 200 and refunds nothing more.
    let v = h.verify_signed(&o, &oid, "pay_Second00001").await;
    assert_eq!(v.status, StatusCode::OK);
    assert_eq!(h.mock.with(|s| s.refund_calls), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_full_refund(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.paid_order("pay_FullRefund1").await;
    let v = h.row(o.id).await.version;

    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "amount": null, "reason": "Customer request", "expectedVersion": v })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");
    assert_eq!(r.body["payment"]["amountRefunded"], 1650);
    let row = h.row(o.id).await;
    assert_eq!(row.payment_status, "refunded");
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert!(row.refunded_at.is_some());
    let receipts: Vec<Value> = h
        .mock
        .with(|s| s.refunds.iter().map(|r| r["receipt"].clone()).collect());
    let refund_id: Uuid = sqlx::query_scalar("SELECT id FROM refunds WHERE order_id = $1")
        .bind(o.id)
        .fetch_one(&h.pool)
        .await
        .expect("refund id");
    assert_eq!(
        receipts,
        vec![json!(refund_id.to_string())],
        "receipt = our refund id"
    );

    // Nothing left.
    let again = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "reason": "again", "expectedVersion": row.version })),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "invalid_transition");

    // Events endpoint lists the refund with the admin's e-mail.
    let ev = h
        .admin(
            Method::GET,
            &format!("/api/v1/admin/orders/{}/payment-events", o.number),
            None,
        )
        .await;
    assert_eq!(ev.status, StatusCode::OK, "{}", ev.body);
    let refunds = ev.body["refunds"].as_array().expect("refunds");
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds[0]["adminEmail"], "ops@zhimart.com");
    assert_eq!(refunds[0]["amount"], 1650);
    assert_eq!(refunds[0]["status"], "processed");
    assert_eq!(refunds[0]["reason"], "Customer request");
    assert!(ev.body["events"].is_array());

    // Unauthenticated.
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
    let o = h.paid_order("pay_Partial0001").await;
    let uri = format!("/api/v1/admin/orders/{}/refund", o.id);
    let v0 = h.row(o.id).await.version;

    let r = h
        .admin(
            Method::POST,
            &uri,
            Some(json!({ "amount": 500, "reason": "Damaged box", "expectedVersion": v0 })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["payment"]["amountRefunded"], 500);

    // Stale version (double click).
    let stale = h
        .admin(
            Method::POST,
            &uri,
            Some(json!({ "amount": 100, "reason": "Damaged box", "expectedVersion": v0 })),
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.code(), "version_conflict");

    let v1 = h.row(o.id).await.version;
    let over = h
        .admin(
            Method::POST,
            &uri,
            Some(json!({ "amount": 1200, "reason": "Too much", "expectedVersion": v1 })),
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
        let r = h.admin(Method::POST, &uri, Some(bad)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    }

    let rest = h
        .admin(
            Method::POST,
            &uri,
            Some(json!({ "amount": 1150, "reason": "Rest", "expectedVersion": v1 })),
        )
        .await;
    assert_eq!(rest.status, StatusCode::OK, "{}", rest.body);
    assert_eq!(rest.body["payment"]["status"], "refunded");
    assert_eq!(h.row(o.id).await.amount_refunded_paise, TOTAL);

    // An unpaid order cannot be refunded.
    let unpaid = h.seed_order().await;
    let v = h.row(unpaid.id).await.version;
    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", unpaid.id),
            Some(json!({ "reason": "x", "expectedVersion": v })),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_refund_gateway_failure_is_502_and_recorded(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.paid_order("pay_RefundFail1").await;
    h.mock.with(|s| s.refund_mode = RefundMode::Reject);
    let v = h.row(o.id).await.version;

    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "reason": "Customer request", "expectedVersion": v })),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    assert_eq!(r.code(), "payment_gateway_error");
    let (status, failure): (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM refunds WHERE order_id = $1")
            .bind(o.id)
            .fetch_one(&h.pool)
            .await
            .expect("refund row");
    assert_eq!(status, "failed");
    assert!(failure.is_some());
    let row = h.row(o.id).await;
    assert_eq!(row.payment_status, "paid");
    assert_eq!(row.amount_refunded_paise, 0);

    // The failed refund does not block a retry.
    h.mock.with(|s| s.refund_mode = RefundMode::Processed);
    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "reason": "Customer request", "expectedVersion": row.version })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");
}

#[sqlx::test(migrations = "./migrations")]
async fn refund_webhooks_update_the_ledger(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.paid_order("pay_RefundHook1").await;
    h.mock.with(|s| s.refund_mode = RefundMode::Pending);
    let v = h.row(o.id).await.version;

    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "amount": 650, "reason": "Partial", "expectedVersion": v })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let status: String = sqlx::query_scalar("SELECT status FROM refunds WHERE order_id = $1")
        .bind(o.id)
        .fetch_one(&h.pool)
        .await
        .expect("status");
    assert_eq!(status, "pending");
    assert_eq!(h.row(o.id).await.amount_refunded_paise, 65_000);

    let entity = h.mock.with(|s| s.refunds[0].clone());
    let mut processed = entity.clone();
    processed["status"] = json!("processed");
    let w = h
        .webhook(
            Some("evt_RefundDone01"),
            &refund_event("refund.processed", processed),
        )
        .await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    let status: String = sqlx::query_scalar("SELECT status FROM refunds WHERE order_id = $1")
        .bind(o.id)
        .fetch_one(&h.pool)
        .await
        .expect("status");
    assert_eq!(status, "processed");
    let row = h.row(o.id).await;
    assert_eq!(row.amount_refunded_paise, 65_000);
    assert_eq!(row.payment_status, "paid");

    // A refund made in the Razorpay Dashboard for the rest is recorded too.
    let dashboard = json!({ "id": "rfnd_Dashboard01", "entity": "refund", "amount": 100_000, "payment_id": "pay_RefundHook1",
                            "status": "processed", "receipt": null, "notes": [] });
    let w = h
        .webhook(
            Some("evt_RefundDash01"),
            &refund_event("refund.processed", dashboard),
        )
        .await;
    assert_eq!(w.status, StatusCode::OK);
    let row = h.row(o.id).await;
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert_eq!(row.payment_status, "refunded");
    assert_eq!(
        h.scalar_i64("SELECT count(*) FROM refunds WHERE order_id = $1", o.id)
            .await,
        2
    );

    // A refund.failed later on rolls the totals back and flags the order.
    let mut failed = entity;
    failed["status"] = json!("failed");
    let w = h
        .webhook(
            Some("evt_RefundFail01"),
            &refund_event("refund.failed", failed),
        )
        .await;
    assert_eq!(w.status, StatusCode::OK);
    let row = h.row(o.id).await;
    // processed refunds never regress
    assert_eq!(row.amount_refunded_paise, TOTAL);
    assert_eq!(row.payment_status, "refunded");
    assert!(row.payment_attention.is_none());

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
            .any(|e| e["id"] == "evt_RefundDone01" && e["outcome"] == "refund_processed")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn notifications_disabled_queue_nothing(pool: PgPool) {
    let h = setup(pool).await;
    let o = h.paid_order("pay_NoNotify001").await;
    assert_eq!(h.row(o.id).await.payment_status, "paid");
    assert_eq!(h.outbox_rows(o.id).await, 0);
}

fn payments_off(c: &mut Config) {
    c.payments_enabled = false;
    c.razorpay_key_id = String::new();
    c.razorpay_key_secret = SecretString::from("");
    c.razorpay_webhook_secret = SecretString::from("");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_storefront_config_hides_razorpay(pool: PgPool) {
    let h = setup_with(pool, payments_off).await;
    let r = h
        .raw(Method::GET, "/api/v1/storefront/config", &[], None)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["onlinePaymentsEnabled"], false);
    assert!(r.body["razorpayKeyId"].is_null(), "{}", r.body);
    assert_eq!(r.body["currency"], "INR");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_pay_verify_webhook_are_refused(pool: PgPool) {
    let h = setup_with(pool, payments_off).await;
    let o = h.seed_order().await;
    // A pay-later order as checkout creates it in this mode.
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
    let v = h
        .verify(&o, "order_Mock00000001", "pay_Disabled0001", "00")
        .await;
    assert_eq!(v.status, StatusCode::CONFLICT, "{}", v.body);
    assert_eq!(v.code(), "online_payments_disabled");
    // The token is still checked first: a wrong token stays a 404.
    let other = h.seed_order().await;
    let r = h
        .guest(
            Method::POST,
            &format!("/api/v1/orders/{}/pay", o.number),
            &other.token,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Webhook: 404 even with a well-formed signed body; nothing recorded.
    let entity = payment_entity("pay_Disabled0001", "order_Mock00000001", TOTAL, "captured");
    let w = h
        .webhook(
            Some("evt_Disabled0001"),
            &payment_event("payment.captured", entity),
        )
        .await;
    assert_eq!(w.status, StatusCode::NOT_FOUND, "{}", w.body);
    assert_eq!(w.code(), "not_found");
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM payment_events")
        .fetch_one(&h.pool)
        .await
        .expect("count");
    assert_eq!(events, 0);

    // The order page keeps working.
    let g = h
        .guest(
            Method::GET,
            &format!("/api/v1/orders/{}", o.number),
            &o.token,
            None,
        )
        .await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert_eq!(g.body["status"], "pending_pack");
    assert_eq!(
        g.body["payment"],
        json!({ "method": "pay_later", "status": "pending", "instrument": null, "paidAt": null })
    );
    assert!(g.body["paymentExpiresAt"].is_null());

    // Admin refund of a pay-later order: unchanged (nothing to refund online).
    let version = h.row(o.id).await.version;
    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "reason": "x", "expectedVersion": version })),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "invalid_transition");

    let row = h.row(o.id).await;
    assert_eq!(row.status, "pending_pack");
    assert!(row.razorpay_order_id.is_none());
    let calls = h
        .mock
        .with(|s| s.orders_created + s.capture_calls + s.refund_calls + s.unauthorized);
    assert_eq!(calls, 0, "Razorpay is never called while disabled");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_mode_does_not_refund_old_razorpay_orders_online(pool: PgPool) {
    let h = setup_with(pool, payments_off).await;
    let o = h.seed_order().await;
    // Paid online before the switch was turned off.
    sqlx::query(
        "UPDATE orders SET status = 'pending_pack', payment_status = 'paid', payment_expires_at = NULL, \
         razorpay_order_id = 'order_Before00001', razorpay_payment_id = 'pay_Before000001', paid_at = now() \
         WHERE id = $1",
    )
    .bind(o.id)
    .execute(&h.pool)
    .await
    .expect("paid before");
    let version = h.row(o.id).await.version;
    let r = h
        .admin(
            Method::POST,
            &format!("/api/v1/admin/orders/{}/refund", o.id),
            Some(json!({ "reason": "Customer request", "expectedVersion": version })),
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
