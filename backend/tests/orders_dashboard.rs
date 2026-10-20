//! End-to-end tests for checkout, admin order management, the dashboard and
//! the sync marker, against a real PostgreSQL database.
//!
//! Each test gets a fresh database from `#[sqlx::test]` (requires
//! `DATABASE_URL` pointing at a server where the user may create databases).
//! Products are created here with plain SQL so the tests never depend on the
//! catalogue seed migration.

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
};
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
    net::ResolvedClientIp,
    payments::order_token,
    repo::orders::{ReviveOutcome, expire_unpaid_orders, revive_expired_order},
    routes,
    state::AppState,
};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct NoopSender;

impl EmailSender for NoopSender {
    fn send_otp<'a>(&'a self, _email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { Ok(()) })
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
    token: String,
    admin_id: Uuid,
    config: Arc<Config>,
}

async fn setup(pool: PgPool) -> Harness {
    setup_with(pool, |_| {}).await
}

async fn setup_with(pool: PgPool, tweak: impl FnOnce(&mut Config)) -> Harness {
    let mut config = test_config();
    tweak(&mut config);
    let config = Arc::new(config);
    let state = AppState::new(config.clone(), pool.clone(), Arc::new(NoopSender)).expect("state");

    // Start from an empty catalogue so fixtures (and dashboard totals) never
    // depend on whatever the seed migration ships. Cascades to `initial` movements.
    sqlx::query("DELETE FROM products")
        .execute(&pool)
        .await
        .expect("clear catalogue");

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
    let token = state
        .jwt
        .issue(admin_id, session_id, expires)
        .expect("token")
        .token;

    let app = Router::new()
        .nest(
            "/api/v1",
            routes::orders::public_routes().nest(
                "/admin",
                routes::admin::orders::routes().merge(routes::admin::dashboard::routes()),
            ),
        )
        .with_state(state);
    Harness {
        app,
        pool,
        token,
        admin_id,
        config,
    }
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

impl Harness {
    async fn call(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        auth: bool,
        ip: Option<IpAddr>,
    ) -> Resp {
        let mut req = Request::builder().method(method).uri(uri);
        if auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        }
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let mut req = req.body(body).expect("request");
        if let Some(ip) = ip {
            req.extensions_mut().insert(ResolvedClientIp(ip));
        }
        let res = self.app.clone().oneshot(req).await.expect("response");
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

    async fn order(&self, body: Value) -> Resp {
        self.call(Method::POST, "/api/v1/orders", Some(body), false, None)
            .await
    }

    async fn order_from(&self, body: Value, ip: IpAddr) -> Resp {
        self.call(Method::POST, "/api/v1/orders", Some(body), false, Some(ip))
            .await
    }

    async fn admin_get(&self, uri: &str) -> Resp {
        self.call(Method::GET, uri, None, true, None).await
    }

    async fn admin_post(&self, uri: &str, body: Value) -> Resp {
        self.call(Method::POST, uri, Some(body), true, None).await
    }

    async fn admin_put(&self, uri: &str, body: Value) -> Resp {
        self.call(Method::PUT, uri, Some(body), true, None).await
    }

    async fn stock(&self, id: &str) -> i32 {
        sqlx::query_scalar("SELECT stock FROM products WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("stock")
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }

    async fn order_id(&self, order_number: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM orders WHERE order_number = $1")
            .bind(order_number)
            .fetch_one(&self.pool)
            .await
            .expect("order id")
    }

    /// Place an order and simulate a verified Cashfree payment (what the
    /// payments service does): `pending_pack` + `paid`, version 2.
    async fn place(&self, phone: &str, items: &[(&str, i64)]) -> Value {
        let o = self.place_unpaid(phone, items).await;
        let id: Uuid = o["id"].as_str().expect("id").parse().expect("uuid");
        self.pay_sql(id).await;
        self.get_order(id).await
    }

    async fn pay_sql(&self, id: Uuid) {
        sqlx::query(
            r#"
            UPDATE orders
            SET status = 'pending_pack', payment_status = 'paid', paid_at = now(),
                gateway_order_id = order_number || '_' || substr(md5(id::text), 1, 12),
                gateway_payment_id = (1000000000 + (('x' || substr(md5(id::text), 15, 7))::bit(28)::int))::text,
                payment_instrument = 'upi', version = version + 1, updated_at = now()
            WHERE id = $1 AND status = 'pending_payment'
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .expect("pay");
    }

    async fn get_order(&self, id: Uuid) -> Value {
        let g = self.admin_get(&format!("/api/v1/admin/orders/{id}")).await;
        assert_eq!(g.status, StatusCode::OK, "{}", g.body);
        g.body
    }

    async fn status_of(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM orders WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("status")
    }

    /// Place an order (must succeed) and return its admin `Order` JSON
    /// (`pending_payment`, version 1).
    async fn place_unpaid(&self, phone: &str, items: &[(&str, i64)]) -> Value {
        let r = self.order(order_body(Uuid::new_v4(), phone, items)).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
        let number = r.body["order"]["orderNumber"]
            .as_str()
            .expect("orderNumber")
            .to_owned();
        let id = self.order_id(&number).await;
        let g = self.admin_get(&format!("/api/v1/admin/orders/{id}")).await;
        assert_eq!(g.status, StatusCode::OK, "{}", g.body);
        g.body
    }
}

async fn add_product(
    pool: &PgPool,
    id: &str,
    price_paise: i64,
    mrp_paise: i64,
    stock: i32,
    active: bool,
) {
    sqlx::query(
        r#"
        INSERT INTO products (id, name, category, brand, pack_info, price_paise, original_price_paise,
                              image_url, stock, low_stock_threshold, is_active)
        VALUES ($1, 'Product ' || $1, 'SPARKLERS', 'Zhi-mart Fireworks', '10 Packet(s)', $2, $3,
                '/assets/products/x.jpg', $4, 20, $5)
        "#,
    )
    .bind(id)
    .bind(price_paise)
    .bind(mrp_paise)
    .bind(stock)
    .bind(active)
    .execute(pool)
    .await
    .expect("insert product");
}

fn order_body(key: Uuid, phone: &str, items: &[(&str, i64)]) -> Value {
    json!({
        "idempotencyKey": key.to_string(),
        "customer": {
            "name": "  Ravi   Kumar ",
            "phone": phone,
            "email": " Ravi.Kumar@Example.com ",
            "address": "12, Gandhi Street, Mylapore",
            "city": "Chennai",
            "state": "Tamil Nadu",
            "pincode": "600004"
        },
        "notes": null,
        "items": items.iter().map(|(id, q)| json!({ "productId": id, "quantity": q })).collect::<Vec<_>>()
    })
}

/// `(product_id, delta, stock_after, order_id, admin_id)`
type CancelMove = (String, i32, i32, Option<Uuid>, Option<Uuid>);

fn phone(n: u32) -> String {
    format!("98400{n:05}")
}

fn ip(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, last))
}

// ---------------------------------------------------------------------------
// Checkout
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn order_decrements_stock_and_uses_db_prices(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-03", 16_500, 30_000, 100, true).await;
    add_product(&h.pool, "zm-07", 5_050, 10_000, 20, true).await;

    let r = h
        .order(order_body(
            Uuid::new_v4(),
            "+91 98400-12345",
            &[("zm-07", 3), ("zm-03", 10)],
        ))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(
        r.headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let o = &r.body["order"];
    let number = o["orderNumber"].as_str().expect("number");
    let parts: Vec<&str> = number.split('-').collect();
    assert_eq!(parts.len(), 3, "{number}");
    assert_eq!(parts[0], "ZM");
    assert_eq!(parts[1].len(), 6);
    assert!(parts[2].parse::<u64>().expect("seq") >= 1001);
    assert_eq!(o["status"], "pending_payment");
    assert_eq!(o["isTest"], false);
    let expires: DateTime<Utc> = o["paymentExpiresAt"]
        .as_str()
        .expect("paymentExpiresAt")
        .parse()
        .expect("rfc3339");
    let created: DateTime<Utc> = o["createdAt"]
        .as_str()
        .expect("createdAt")
        .parse()
        .expect("rfc3339");
    assert_eq!(
        (expires - created).num_seconds(),
        30 * 60,
        "expiry = now() + PAYMENT_EXPIRY on the DB clock"
    );
    assert_eq!(o["itemCount"], 13);
    assert_eq!(o["subtotal"].as_f64(), Some(1801.5)); // 10 × 165 + 3 × 50.50
    assert_eq!(o["deliveryFee"], 500, "below ₹5,000 ⇒ delivery charged");
    assert_eq!(o["totalAmount"].as_f64(), Some(2301.5));
    assert_eq!(o["mrpTotal"].as_f64(), Some(3300.0));
    let items = o["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["productId"], "zm-03");
    assert_eq!(items[0]["productName"], "Product zm-03");
    assert_eq!(items[0]["unitPrice"].as_f64(), Some(165.0));
    assert_eq!(items[0]["lineTotal"].as_f64(), Some(1650.0));
    assert_eq!(items[1]["unitMrp"].as_f64(), Some(100.0));
    assert!(
        o.get("customer").is_none(),
        "public receipt must not echo PII"
    );

    assert_eq!(h.stock("zm-03").await, 90);
    assert_eq!(h.stock("zm-07").await, 17);
    assert_eq!(
        h.count("SELECT version::bigint FROM products WHERE id = 'zm-03'")
            .await,
        2
    );

    let id = h.order_id(number).await;
    let moves: Vec<(String, i32, i32, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT product_id, delta, stock_after, reason, order_id FROM stock_movements ORDER BY product_id",
    )
    .fetch_all(&h.pool)
    .await
    .expect("movements");
    assert_eq!(
        moves,
        vec![
            ("zm-03".into(), -10, 90, "order_placed".into(), Some(id)),
            ("zm-07".into(), -3, 17, "order_placed".into(), Some(id)),
        ]
    );

    let (name, phone, item_count, total): (String, String, i32, i64) = sqlx::query_as(
        "SELECT customer_name, customer_phone, item_count, total_paise FROM orders WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&h.pool)
    .await
    .expect("order row");
    assert_eq!(name, "Ravi Kumar");
    assert_eq!(phone, "+919840012345");
    assert_eq!(item_count, 13);
    assert_eq!(total, 230_150, "goods + delivery");
}

#[sqlx::test(migrations = "./migrations")]
async fn client_supplied_prices_are_rejected(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-03", 16_500, 30_000, 100, true).await;

    let mut body = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
    body["items"][0]["price"] = json!(1);
    let r = h.order(body).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.code(), "validation_error");

    let mut body = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
    body["totalAmount"] = json!(1);
    assert_eq!(h.order(body).await.status, StatusCode::BAD_REQUEST);

    assert_eq!(h.stock("zm-03").await, 100);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn input_validation(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-03", 16_500, 30_000, 100, true).await;

    let cases: Vec<(Value, &str)> = vec![
        (
            order_body(Uuid::new_v4(), "12345", &[("zm-03", 1)]),
            "customer.phone",
        ),
        (
            order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 0)]),
            "quantity",
        ),
        (
            order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1001)]),
            "quantity",
        ),
        (
            order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1), ("zm-03", 2)]),
            "more than once",
        ),
        (
            order_body(Uuid::new_v4(), "9840012345", &[("ZM-03; DROP", 1)]),
            "productId",
        ),
        (order_body(Uuid::new_v4(), "9840012345", &[]), "items"),
        (
            {
                let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
                b["idempotencyKey"] = json!("not-a-uuid");
                b
            },
            "idempotencyKey",
        ),
        (
            {
                let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
                b["customer"]["pincode"] = json!("060004");
                b
            },
            "pincode",
        ),
        (
            {
                let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
                b["customer"]["name"] = json!("R\u{0000}avi");
                b
            },
            "customer.name",
        ),
        (
            {
                let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
                b["customer"]["email"] = json!("not-an-email");
                b
            },
            "customer.email",
        ),
        (
            {
                let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-03", 1)]);
                b["notes"] = json!("x".repeat(501));
                b
            },
            "notes",
        ),
    ];
    for (body, needle) in cases {
        let r = h.order(body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{needle}: {}", r.body);
        let msg = r.body["error"]["message"].as_str().unwrap_or("");
        assert!(msg.contains(needle), "expected '{needle}' in '{msg}'");
        assert!(!msg.contains("DROP"), "user text must never be echoed");
    }

    let many: Vec<(String, i64)> = (0..51).map(|i| (format!("zm-{i:02}"), 1)).collect();
    let many_ref: Vec<(&str, i64)> = many.iter().map(|(s, q)| (s.as_str(), *q)).collect();
    let r = h
        .order(order_body(Uuid::new_v4(), "9840012345", &many_ref))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn out_of_stock_lists_every_short_line_and_changes_nothing(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 5, true).await;
    add_product(&h.pool, "zm-02", 1_000, 2_000, 2, true).await;
    add_product(&h.pool, "zm-04", 1_000, 2_000, 100, true).await;

    let r = h
        .order(order_body(
            Uuid::new_v4(),
            "9840012345",
            &[("zm-04", 1), ("zm-02", 3), ("zm-01", 6)],
        ))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.code(), "out_of_stock");
    assert_eq!(
        r.body["error"]["details"],
        json!([
            { "productId": "zm-01", "requested": 6, "available": 5 },
            { "productId": "zm-02", "requested": 3, "available": 2 },
        ])
    );
    assert_eq!(h.stock("zm-01").await, 5);
    assert_eq!(h.stock("zm-02").await, 2);
    assert_eq!(h.stock("zm-04").await, 100);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 0);
    assert_eq!(h.count("SELECT count(*) FROM stock_movements").await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn archived_or_unknown_products_are_unavailable(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    add_product(&h.pool, "zm-09", 1_000, 2_000, 50, false).await;

    for items in [
        vec![("zm-01", 1), ("zm-09", 1)],
        vec![("zm-01", 1), ("zm-99", 1)],
    ] {
        let r = h
            .order(order_body(Uuid::new_v4(), "9840012345", &items))
            .await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
        assert_eq!(r.code(), "product_unavailable");
    }
    assert_eq!(h.stock("zm-01").await, 50);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn idempotent_replay_and_conflict(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    add_product(&h.pool, "zm-02", 1_000, 2_000, 50, true).await;
    let key = Uuid::new_v4();

    let first = h
        .order(order_body(key, "9840012345", &[("zm-01", 4), ("zm-02", 1)]))
        .await;
    assert_eq!(first.status, StatusCode::CREATED);
    // Same checkout, different item order and phone formatting.
    let again = h
        .order(order_body(
            key,
            "+91 98400 12345",
            &[("zm-02", 1), ("zm-01", 4)],
        ))
        .await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.body);
    assert_eq!(again.body, first.body, "same receipt and same orderToken");
    let token = first.body["orderToken"].as_str().expect("orderToken");
    let number = first.body["order"]["orderNumber"].as_str().expect("n");
    assert!(order_token::verify(
        &h.config,
        h.order_id(number).await,
        token
    ));
    assert_eq!(h.stock("zm-01").await, 46);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 1);
    assert_eq!(h.count("SELECT count(*) FROM stock_movements").await, 2);

    let changed_qty = h
        .order(order_body(key, "9840012345", &[("zm-01", 5), ("zm-02", 1)]))
        .await;
    assert_eq!(changed_qty.status, StatusCode::CONFLICT);
    assert_eq!(changed_qty.code(), "idempotency_conflict");
    let changed_phone = h
        .order(order_body(key, "9840099999", &[("zm-01", 4), ("zm-02", 1)]))
        .await;
    assert_eq!(changed_phone.code(), "idempotency_conflict");
    assert_eq!(h.stock("zm-01").await, 46);
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_same_key_submits_create_one_order(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 100, true).await;
    let key = Uuid::new_v4();

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let h = h.clone();
        tasks.push(tokio::spawn(async move {
            h.order(order_body(key, "9840012345", &[("zm-01", 3)]))
                .await
                .status
        }));
    }
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.expect("join"));
    }
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CREATED)
            .count(),
        1,
        "{statuses:?}"
    );
    assert!(
        statuses
            .iter()
            .all(|s| *s == StatusCode::CREATED || *s == StatusCode::OK),
        "{statuses:?}"
    );
    assert_eq!(h.stock("zm-01").await, 97);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_orders_never_oversell(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;

    let mut tasks = Vec::new();
    for i in 0..20 {
        let h = h.clone();
        tasks.push(tokio::spawn(async move {
            let r = h
                .order(order_body(Uuid::new_v4(), &phone(i), &[("zm-01", 5)]))
                .await;
            (r.status, r.code().to_owned())
        }));
    }
    let mut ok = 0;
    let mut out = 0;
    for t in tasks {
        match t.await.expect("join") {
            (StatusCode::CREATED, _) => ok += 1,
            (StatusCode::CONFLICT, code) if code == "out_of_stock" => out += 1,
            other => panic!("unexpected response {other:?}"),
        }
    }
    assert_eq!((ok, out), (10, 10));
    assert_eq!(h.stock("zm-01").await, 0);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 10);
    assert_eq!(
        h.count("SELECT COALESCE(sum(delta), 0)::bigint FROM stock_movements")
            .await,
        -50
    );
    assert_eq!(
        h.count("SELECT count(*) FROM stock_movements WHERE stock_after < 0")
            .await,
        0
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn per_ip_hourly_cap(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 1_000, true).await;

    for i in 0..10 {
        let r = h
            .order_from(
                order_body(Uuid::new_v4(), &phone(i), &[("zm-01", 1)]),
                ip(7),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "order {i}: {}", r.body);
    }
    let r = h
        .order_from(
            order_body(Uuid::new_v4(), &phone(99), &[("zm-01", 1)]),
            ip(7),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.code(), "too_many_requests");
    let retry: u64 = r
        .headers
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .expect("Retry-After");
    assert!((3000..=3600).contains(&retry), "{retry}");
    assert_eq!(h.stock("zm-01").await, 990);

    // Another client is unaffected.
    let r = h
        .order_from(
            order_body(Uuid::new_v4(), &phone(99), &[("zm-01", 1)]),
            ip(8),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
}

#[sqlx::test(migrations = "./migrations")]
async fn per_phone_pending_cap(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 1_000, true).await;

    for i in 0..5 {
        let r = h
            .order_from(
                order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]),
                ip(i),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED);
    }
    let r = h
        .order_from(
            order_body(Uuid::new_v4(), "09840012345", &[("zm-01", 1)]),
            ip(50),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);

    // Orders that moved on (e.g. cancelled) no longer count.
    sqlx::query("UPDATE orders SET status = 'cancelled' WHERE id = (SELECT id FROM orders ORDER BY created_at LIMIT 1)")
        .execute(&h.pool)
        .await
        .expect("cancel one");
    let r = h
        .order_from(
            order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]),
            ip(51),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
}

// ---------------------------------------------------------------------------
// Admin: auth, list, get
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn admin_endpoints_require_a_token(pool: PgPool) {
    let h = setup(pool).await;
    let id = Uuid::now_v7();
    let calls: Vec<(Method, String, Option<Value>)> = vec![
        (Method::GET, "/api/v1/admin/orders".into(), None),
        (Method::GET, format!("/api/v1/admin/orders/{id}"), None),
        (
            Method::POST,
            format!("/api/v1/admin/orders/{id}/status"),
            Some(json!({"status": "dispatched", "expectedVersion": 1})),
        ),
        (
            Method::PUT,
            format!("/api/v1/admin/orders/{id}/shipping"),
            Some(json!({"expectedVersion": 1})),
        ),
        (
            Method::POST,
            format!("/api/v1/admin/orders/{id}/cancel"),
            Some(json!({"expectedVersion": 1})),
        ),
        (
            Method::POST,
            format!("/api/v1/admin/orders/{id}/payment"),
            Some(json!({"action": "mark_paid", "expectedVersion": 1})),
        ),
        (Method::GET, "/api/v1/admin/dashboard".into(), None),
        (Method::GET, "/api/v1/admin/sync".into(), None),
    ];
    for (method, uri, body) in calls {
        let r = h
            .call(method.clone(), &uri, body.clone(), false, None)
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(r.code(), "unauthorized");

        let mut req = Request::builder()
            .method(method.clone())
            .uri(&uri)
            .header(header::AUTHORIZATION, "Bearer forged.token.value");
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/json");
        }
        let req = req
            .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
            .expect("req");
        let res = h.app.clone().oneshot(req).await.expect("res");
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "forged token {method} {uri}"
        );
    }

    // A revoked session invalidates its still-unexpired token.
    sqlx::query("UPDATE sessions SET revoked_at = now()")
        .execute(&h.pool)
        .await
        .expect("revoke");
    assert_eq!(
        h.admin_get("/api/v1/admin/sync").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn list_pagination_and_filters(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 1_000, true).await;
    let mut numbers = Vec::new();
    for i in 0..5 {
        let o = h.place(&phone(i), &[("zm-01", 1)]).await;
        numbers.push(o["orderNumber"].as_str().expect("n").to_owned());
    }
    // Two orders share a timestamp: the id tiebreaker must keep pages disjoint.
    sqlx::query("UPDATE orders SET created_at = (SELECT min(created_at) FROM orders) WHERE order_number = ANY($1)")
        .bind(&numbers[1..3])
        .execute(&h.pool)
        .await
        .expect("tie");

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let uri = match &cursor {
            Some(c) => format!("/api/v1/admin/orders?limit=2&cursor={c}"),
            None => "/api/v1/admin/orders?limit=2&status=&q=".to_owned(),
        };
        let r = h.admin_get(&uri).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        let orders = r.body["orders"].as_array().expect("orders");
        assert!(orders.len() <= 2);
        for o in orders {
            assert_eq!(
                o["items"].as_array().map(Vec::len),
                Some(1),
                "items attached"
            );
            assert_eq!(
                o["customer"]["phone"]
                    .as_str()
                    .map(|p| p.starts_with("+91")),
                Some(true)
            );
            seen.push((
                o["createdAt"].as_str().expect("ts").to_owned(),
                o["id"].as_str().expect("id").to_owned(),
            ));
        }
        pages += 1;
        match r.body["nextCursor"].as_str() {
            Some(c) => cursor = Some(c.to_owned()),
            None => break,
        }
        assert!(pages < 10, "pagination did not terminate");
    }
    assert_eq!(pages, 3);
    assert_eq!(seen.len(), 5);
    let mut uniq = seen.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), 5);
    let mut sorted = seen.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(seen, sorted, "newest first");

    for bad in [
        "cursor=abc!",
        "cursor=MTIzOmFiYw",
        "limit=0",
        "limit=101",
        "limit=x",
        "status=shipped",
        "paymentStatus=done",
    ] {
        let r = h.admin_get(&format!("/api/v1/admin/orders?{bad}")).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
    }

    let r = h.admin_get("/api/v1/admin/orders?status=cancelled").await;
    assert_eq!(r.body["orders"].as_array().map(Vec::len), Some(0));
    let r = h
        .admin_get("/api/v1/admin/orders?paymentStatus=paid&status=pending_pack")
        .await;
    assert_eq!(r.body["orders"].as_array().map(Vec::len), Some(5));
    let r = h.admin_get("/api/v1/admin/orders?q=98400%2000003").await;
    assert_eq!(
        r.body["orders"].as_array().map(Vec::len),
        Some(1),
        "{}",
        r.body
    );
    let r = h
        .admin_get(&format!("/api/v1/admin/orders?q={}", numbers[4]))
        .await;
    assert_eq!(r.body["orders"][0]["orderNumber"], numbers[4].as_str());
    let r = h.admin_get("/api/v1/admin/orders?q=chEnnAi").await;
    assert_eq!(r.body["orders"].as_array().map(Vec::len), Some(5));
    // LIKE metacharacters are literals.
    let r = h.admin_get("/api/v1/admin/orders?q=%25").await;
    assert_eq!(r.body["orders"].as_array().map(Vec::len), Some(0));
    let r = h.admin_get("/api/v1/admin/orders?q=_").await;
    assert_eq!(r.body["orders"].as_array().map(Vec::len), Some(0));
    let r = h
        .admin_get(&format!("/api/v1/admin/orders?q={}", "x".repeat(101)))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_by_id_or_number(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 16_500, 30_000, 100, true).await;
    let o = h.place("9840012345", &[("zm-01", 10)]).await;

    assert_eq!(o["version"], 2);
    assert_eq!(o["status"], "pending_pack");
    assert_eq!(o["isTest"], false);
    assert!(o["paymentExpiresAt"].is_string());
    assert_eq!(o["customer"]["name"], "Ravi Kumar");
    assert_eq!(o["customer"]["address"], "12, Gandhi Street, Mylapore");
    assert_eq!(o["customer"]["pincode"], "600004");
    assert_eq!(
        o["payment"],
        json!({
            "method": "cashfree", "status": "paid", "reference": null,
            "paidAt": o["payment"]["paidAt"].clone(), "refundedAt": null,
            "instrument": "upi",
            "gatewayOrderId": o["payment"]["gatewayOrderId"].clone(),
            "gatewayPaymentId": o["payment"]["gatewayPaymentId"].clone(),
            "amountRefunded": 0, "attention": null
        })
    );
    assert_eq!(
        o["shipping"],
        json!({"transportPartner": null, "docketNumber": null, "dispatchedAt": null, "deliveredAt": null})
    );
    assert_eq!(o["subtotal"].as_f64(), Some(1650.0));
    assert_eq!(o["deliveryFee"].as_f64(), Some(500.0));
    assert_eq!(o["totalAmount"].as_f64(), Some(2150.0));
    assert_eq!(o["mrpTotal"].as_f64(), Some(3000.0));
    assert_eq!(o["customer"]["state"], "Tamil Nadu");
    assert_eq!(o["cancelledAt"], Value::Null);

    let by_number = h
        .admin_get(&format!(
            "/api/v1/admin/orders/{}",
            o["orderNumber"].as_str().expect("n")
        ))
        .await;
    assert_eq!(by_number.body, o);
    assert_eq!(
        h.admin_get(&format!("/api/v1/admin/orders/{}", Uuid::now_v7()))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.admin_get("/api/v1/admin/orders/nonsense").await.status,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// Admin: mutations
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn status_transitions(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 100, true).await;
    let o = h.place("9840012345", &[("zm-01", 2)]).await;
    let base = format!("/api/v1/admin/orders/{}", o["id"].as_str().expect("id"));

    // Dispatch needs transport partner + docket.
    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "dispatched", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);

    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "teleported", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "dispatched", "transportPartner": " VRL  Logistics ", "docketNumber": "LR-1", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["status"], "dispatched");
    assert_eq!(r.body["version"], 3);
    assert_eq!(r.body["shipping"]["transportPartner"], "VRL Logistics");
    assert!(r.body["shipping"]["dispatchedAt"].is_string());
    assert!(r.body["shipping"]["deliveredAt"].is_null());

    // Stale version.
    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "in_transit", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "version_conflict");

    // Backwards and same-state moves are illegal.
    for s in ["pending_pack", "dispatched"] {
        let r = h
            .admin_post(
                &format!("{base}/status"),
                json!({"status": s, "expectedVersion": 3}),
            )
            .await;
        assert_eq!(r.code(), "invalid_transition", "{s}");
    }
    // Cancelling via status is refused (and cancel only works before dispatch).
    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "cancelled", "expectedVersion": 3}),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");
    let r = h
        .admin_post(&format!("{base}/cancel"), json!({"expectedVersion": 3}))
        .await;
    assert_eq!(r.code(), "invalid_transition");

    // Shipping edit keeps existing values required while dispatched.
    let r = h
        .admin_put(
            &format!("{base}/shipping"),
            json!({"transportPartner": "KPN", "docketNumber": null, "expectedVersion": 3}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .admin_put(
            &format!("{base}/shipping"),
            json!({"transportPartner": "KPN", "docketNumber": "LR-2", "expectedVersion": 3}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["shipping"]["docketNumber"], "LR-2");
    assert_eq!(r.body["version"], 4);

    // Skip ahead to delivered (existing shipping details carry over).
    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "delivered", "expectedVersion": 4}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body["shipping"]["deliveredAt"].is_string());
    assert_eq!(r.body["shipping"]["transportPartner"], "KPN");

    // Terminal.
    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "out_for_delivery", "expectedVersion": 5}),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");
    let r = h
        .admin_put(
            &format!("{base}/shipping"),
            json!({"transportPartner": "X", "docketNumber": "Y", "expectedVersion": 5}),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");

    // Unknown order / unknown fields.
    let r = h
        .admin_post(
            &format!("/api/v1/admin/orders/{}/status", Uuid::now_v7()),
            json!({"status": "dispatched", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = h
        .admin_post(
            &format!("{base}/status"),
            json!({"status": "delivered", "expectedVersion": 5, "force": true}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn cancel_restocks_every_line(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    add_product(&h.pool, "zm-02", 1_000, 2_000, 30, true).await;
    let o = h.place("9840012345", &[("zm-01", 10), ("zm-02", 5)]).await;
    let id: Uuid = o["id"].as_str().expect("id").parse().expect("uuid");
    let base = format!("/api/v1/admin/orders/{id}");
    assert_eq!(h.stock("zm-01").await, 40);

    let r = h
        .admin_post(
            &format!("{base}/cancel"),
            json!({"reason": "Customer request", "expectedVersion": 8}),
        )
        .await;
    assert_eq!(r.code(), "version_conflict");

    let r = h
        .admin_post(
            &format!("{base}/cancel"),
            json!({"reason": "Customer request", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["status"], "cancelled");
    assert_eq!(r.body["cancelReason"], "Customer request");
    assert!(r.body["cancelledAt"].is_string());
    assert_eq!(r.body["version"], 3);
    assert_eq!(r.body["items"].as_array().map(Vec::len), Some(2));

    assert_eq!(h.stock("zm-01").await, 50);
    assert_eq!(h.stock("zm-02").await, 30);
    let moves: Vec<CancelMove> = sqlx::query_as(
        "SELECT product_id, delta, stock_after, order_id, admin_id FROM stock_movements \
         WHERE reason = 'order_cancelled' ORDER BY product_id",
    )
    .fetch_all(&h.pool)
    .await
    .expect("moves");
    assert_eq!(
        moves,
        vec![
            ("zm-01".into(), 10, 50, Some(id), Some(h.admin_id)),
            ("zm-02".into(), 5, 30, Some(id), Some(h.admin_id)),
        ]
    );

    // Already cancelled.
    let r = h
        .admin_post(&format!("{base}/cancel"), json!({"expectedVersion": 3}))
        .await;
    assert_eq!(r.code(), "invalid_transition");
    assert_eq!(h.stock("zm-01").await, 50);
    let r = h.admin_post(&format!("{base}/status"), json!({"status": "dispatched", "transportPartner": "A", "docketNumber": "B", "expectedVersion": 3})).await;
    assert_eq!(r.code(), "invalid_transition");
}

#[sqlx::test(migrations = "./migrations")]
async fn payment_flow(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;

    // Cashfree orders: manual payment actions are refused, paid or not.
    let paid = h.place(&phone(1), &[("zm-01", 1)]).await;
    let unpaid = h.place_unpaid(&phone(2), &[("zm-01", 1)]).await;
    for (o, v) in [(&paid, 2), (&unpaid, 1)] {
        for body in [
            json!({"action": "mark_refunded", "expectedVersion": v}),
            json!({"action": "mark_paid", "method": "upi", "expectedVersion": v}),
            json!({"action": "mark_failed", "expectedVersion": v}),
        ] {
            let r = h
                .admin_post(
                    &format!(
                        "/api/v1/admin/orders/{}/payment",
                        o["id"].as_str().expect("id")
                    ),
                    body,
                )
                .await;
            assert_eq!(r.code(), "invalid_transition", "{}", r.body);
        }
    }

    // Legacy offline order (pay_later, already confirmed): manual flow applies.
    let o = h.place_unpaid("9840012345", &[("zm-01", 1)]).await;
    sqlx::query(
        "UPDATE orders SET status = 'pending_pack', payment_method = 'pay_later' WHERE id = $1",
    )
    .bind(o["id"].as_str().expect("id").parse::<Uuid>().expect("uuid"))
    .execute(&h.pool)
    .await
    .expect("legacy");
    let uri = format!(
        "/api/v1/admin/orders/{}/payment",
        o["id"].as_str().expect("id")
    );

    let r = h
        .admin_post(
            &uri,
            json!({"action": "mark_refunded", "expectedVersion": 1}),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");
    let r = h
        .admin_post(&uri, json!({"action": "mark_paid", "expectedVersion": 1}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .admin_post(
            &uri,
            json!({"action": "mark_paid", "method": "bitcoin", "expectedVersion": 1}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h.admin_post(&uri, json!({"action": "mark_paid", "method": "upi", "reference": "x".repeat(121), "expectedVersion": 1})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .admin_post(&uri, json!({"action": "explode", "expectedVersion": 1}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = h
        .admin_post(&uri, json!({"action": "mark_failed", "expectedVersion": 1}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "failed");

    let r = h.admin_post(&uri, json!({"action": "mark_paid", "method": "upi", "reference": "UTR123", "expectedVersion": 2})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["payment"]["method"], "upi");
    assert_eq!(r.body["payment"]["reference"], "UTR123");
    assert!(r.body["payment"]["paidAt"].is_string());
    assert_eq!(r.body["version"], 3);

    let r = h
        .admin_post(
            &uri,
            json!({"action": "mark_paid", "method": "card", "expectedVersion": 3}),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");
    let r = h
        .admin_post(&uri, json!({"action": "mark_failed", "expectedVersion": 3}))
        .await;
    assert_eq!(r.code(), "invalid_transition");
    let r = h
        .admin_post(
            &uri,
            json!({"action": "mark_refunded", "expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.code(), "version_conflict");

    let r = h
        .admin_post(
            &uri,
            json!({"action": "mark_refunded", "expectedVersion": 3}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "refunded");
    assert!(r.body["payment"]["refundedAt"].is_string());
    assert!(r.body["payment"]["paidAt"].is_string());
    assert_eq!(r.body["payment"]["reference"], "UTR123");
    let r = h
        .admin_post(
            &uri,
            json!({"action": "mark_refunded", "expectedVersion": 4}),
        )
        .await;
    assert_eq!(r.code(), "invalid_transition");
}

// ---------------------------------------------------------------------------
// Dashboard & sync
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn dashboard_numbers(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 16_500, 30_000, 100, true).await; // ₹165
    add_product(&h.pool, "zm-02", 5_050, 10_000, 50, true).await; // ₹50.50
    add_product(&h.pool, "zm-low", 1_000, 2_000, 3, true).await;
    add_product(&h.pool, "zm-out", 1_000, 2_000, 0, true).await;
    add_product(&h.pool, "zm-old", 1_000, 2_000, 0, false).await;

    // Every order here is below ₹5,000, so each total includes the ₹500
    // delivery charge (`FEE`): revenue is what was charged, delivery included.
    const FEE: f64 = 500.0;
    let o1 = h.place(&phone(1), &[("zm-01", 10)]).await; // today, paid: 1650 + FEE
    let o2 = h.place(&phone(2), &[("zm-02", 2)]).await; // yesterday 23:30 IST, offline pending: 101 + FEE
    let o3 = h.place(&phone(3), &[("zm-01", 1)]).await; // 10 days ago, paid: 165 + FEE
    let o4 = h.place(&phone(4), &[("zm-01", 2)]).await; // 40 days ago, paid: 330 + FEE
    let o5 = h.place_unpaid(&phone(5), &[("zm-02", 1)]).await; // cancelled (excluded)
    let o6 = h.place(&phone(6), &[("zm-01", 1)]).await; // 3 days ago, fully refunded: 0
    let o7 = h.place(&phone(7), &[("zm-01", 2)]).await; // 5 days ago, ₹100 refunded: 230 + FEE net
    let o8 = h.place(&phone(8), &[("zm-02", 1)]).await; // test order: 50.5 + FEE (test block only)
    let _o9 = h.place_unpaid(&phone(9), &[("zm-02", 3)]).await; // unpaid: 151.5 + FEE (unpaid block only)

    let r = h
        .admin_post(
            &format!(
                "/api/v1/admin/orders/{}/cancel",
                o5["id"].as_str().expect("id")
            ),
            json!({"expectedVersion": 1}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    let today_start: DateTime<Utc> = sqlx::query_scalar(
        "SELECT date_trunc('day', now() AT TIME ZONE 'Asia/Kolkata') AT TIME ZONE 'Asia/Kolkata'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("today");
    let id_of = |o: &Value| o["id"].as_str().expect("id").parse::<Uuid>().expect("uuid");
    let exec = |sql: &'static str, id: Uuid| {
        let pool = h.pool.clone();
        async move {
            sqlx::query(sql)
                .bind(id)
                .execute(&pool)
                .await
                .expect("fixture update");
        }
    };
    let set_created = |id: Uuid, at: DateTime<Utc>| {
        let pool = h.pool.clone();
        async move {
            sqlx::query("UPDATE orders SET created_at = $2 WHERE id = $1")
                .bind(id)
                .bind(at)
                .execute(&pool)
                .await
                .expect("move");
        }
    };
    // 00:05 IST today is 18:35 UTC *yesterday*: must still count as today.
    set_created(id_of(&o1), today_start + chrono::Duration::minutes(5)).await;
    // 23:30 IST yesterday is 18:00 UTC yesterday: must count as yesterday.
    set_created(id_of(&o2), today_start - chrono::Duration::minutes(30)).await;
    set_created(
        id_of(&o3),
        today_start - chrono::Duration::days(10) + chrono::Duration::hours(12),
    )
    .await;
    set_created(id_of(&o4), today_start - chrono::Duration::days(40)).await;
    set_created(
        id_of(&o6),
        today_start - chrono::Duration::days(3) + chrono::Duration::hours(12),
    )
    .await;
    set_created(
        id_of(&o7),
        today_start - chrono::Duration::days(5) + chrono::Duration::hours(12),
    )
    .await;
    exec(
        "UPDATE orders SET payment_method = 'pay_later', payment_status = 'pending', paid_at = NULL, \
         gateway_order_id = NULL, gateway_payment_id = NULL WHERE id = $1",
        id_of(&o2),
    )
    .await;
    exec(
        "UPDATE orders SET payment_status = 'refunded', refunded_at = now(), amount_refunded_paise = total_paise WHERE id = $1",
        id_of(&o6),
    )
    .await;
    exec(
        "UPDATE orders SET amount_refunded_paise = 10000 WHERE id = $1",
        id_of(&o7),
    )
    .await;
    exec("UPDATE orders SET is_test = TRUE WHERE id = $1", id_of(&o8)).await;

    let r = h.admin_get("/api/v1/admin/dashboard").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let d = &r.body;
    let rupees = |v: &Value| v.as_f64().expect("number");

    let last7 = (1650.0 + FEE) + (101.0 + FEE) + (230.0 + FEE);
    let last30 = last7 + (165.0 + FEE);
    assert_eq!(rupees(&d["revenue"]["today"]), 1650.0 + FEE);
    assert_eq!(rupees(&d["revenue"]["last7Days"]), last7);
    assert_eq!(rupees(&d["revenue"]["last30Days"]), last30);
    assert_eq!(rupees(&d["revenue"]["allTime"]), last30 + (330.0 + FEE));
    assert_eq!(
        rupees(&d["revenue"]["paid"]),
        1650.0 + 165.0 + 330.0 + 230.0 + 4.0 * FEE
    );
    assert_eq!(rupees(&d["revenue"]["pendingCollection"]), 101.0 + FEE);
    assert_eq!(d["test"]["orders"], 1);
    assert_eq!(rupees(&d["test"]["revenue"]), 50.5 + FEE);
    assert_eq!(d["unpaid"]["orders"], 1);
    assert_eq!(rupees(&d["unpaid"]["amount"]), 151.5 + FEE);

    assert_eq!(d["orders"]["total"], 9);
    assert_eq!(d["orders"]["today"], 4); // o1, cancelled o5, test o8, unpaid o9
    assert_eq!(
        d["orders"]["byStatus"],
        json!({"pending_payment": 1, "pending_pack": 7, "dispatched": 0, "in_transit": 0,
               "out_for_delivery": 0, "delivered": 0, "cancelled": 1})
    );

    // zm-01: 100 - 10 - 1 - 2 - 1 - 2 = 84; zm-02: 50 - 2 - 1 + 1 (restocked) - 1 - 3 = 44.
    assert_eq!(
        d["inventory"],
        json!({
            "activeProducts": 4, "totalUnits": 84 + 44 + 3, "lowStockCount": 1, "outOfStockCount": 1,
            "lowStock": [
                {"productId": "zm-out", "name": "Product zm-out", "stock": 0, "lowStockThreshold": 20},
                {"productId": "zm-low", "name": "Product zm-low", "stock": 3, "lowStockThreshold": 20},
            ]
        })
    );

    let today: chrono::NaiveDate =
        sqlx::query_scalar("SELECT (now() AT TIME ZONE 'Asia/Kolkata')::date")
            .fetch_one(&h.pool)
            .await
            .expect("today date");
    let days = d["salesByDay"].as_array().expect("salesByDay");
    assert_eq!(days.len(), 14);
    assert_eq!(
        days[0]["date"],
        (today - chrono::Duration::days(13)).to_string()
    );
    let day = |offset: i64| {
        let want = (today - chrono::Duration::days(offset)).to_string();
        days.iter()
            .find(|p| p["date"] == want.as_str())
            .cloned()
            .expect("day present")
    };
    assert_eq!(
        day(0)["orders"],
        1,
        "test, unpaid and cancelled orders are not sales"
    );
    assert_eq!(rupees(&day(0)["revenue"]), 1650.0 + FEE);
    assert_eq!(day(1)["orders"], 1);
    assert_eq!(rupees(&day(1)["revenue"]), 101.0 + FEE);
    assert_eq!(day(3)["orders"], 1); // refunded order still counts as an order...
    assert_eq!(rupees(&day(3)["revenue"]), 0.0); // ...but not as revenue
    assert_eq!(rupees(&day(5)["revenue"]), 230.0 + FEE); // net of the partial refund
    assert_eq!(rupees(&day(10)["revenue"]), 165.0 + FEE);
    assert_eq!(day(2)["orders"], 0);
    let total_rev: f64 = days.iter().map(|p| rupees(&p["revenue"])).sum();
    assert_eq!(total_rev, last30);

    assert_eq!(
        d["topProducts"],
        json!([
            // Line totals: goods only, no delivery charge.
            {"productId": "zm-01", "name": "Product zm-01", "unitsSold": 13, "revenue": 2145},
            {"productId": "zm-02", "name": "Product zm-02", "unitsSold": 2, "revenue": 101},
        ])
    );

    let recent = d["recentOrders"].as_array().expect("recent");
    assert_eq!(recent.len(), 5);
    assert!(
        recent.iter().all(|o| o["status"] != "pending_payment"),
        "unpaid checkouts are not recent orders"
    );
    assert!(recent.iter().all(|o| o["customer"]["phone"].is_string()
        && o["items"].as_array().is_some_and(|i| !i.is_empty())));
    let stamps: Vec<&str> = recent
        .iter()
        .map(|o| o["createdAt"].as_str().expect("ts"))
        .collect();
    let mut sorted = stamps.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(stamps, sorted);
    assert!(
        !recent.iter().any(|o| o["id"] == o4["id"]),
        "oldest order is not recent"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn dashboard_on_empty_database(pool: PgPool) {
    let h = setup(pool).await;
    let r = h.admin_get("/api/v1/admin/dashboard").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["orders"]["total"], 0);
    assert_eq!(r.body["revenue"]["allTime"].as_f64(), Some(0.0));
    assert_eq!(r.body["salesByDay"].as_array().map(Vec::len), Some(14));
    assert_eq!(r.body["inventory"]["lowStock"], json!([]));
    assert_eq!(r.body["recentOrders"], json!([]));
    assert_eq!(r.body["topProducts"], json!([]));
}

#[sqlx::test(migrations = "./migrations")]
async fn sync_marker_moves_on_every_change(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;

    let s0 = h.admin_get("/api/v1/admin/sync").await;
    assert_eq!(s0.status, StatusCode::OK, "{}", s0.body);
    assert_eq!(s0.body["pendingOrders"], 0);
    assert_eq!(s0.body["latestOrderNumber"], Value::Null);
    assert!(s0.body["ordersVersion"].is_string() && s0.body["productsVersion"].is_string());
    let again = h.admin_get("/api/v1/admin/sync").await;
    assert_eq!(again.body, s0.body, "stable when nothing changed");

    let o = h.place("9840012345", &[("zm-01", 1)]).await;
    let s1 = h.admin_get("/api/v1/admin/sync").await;
    assert_ne!(s1.body["ordersVersion"], s0.body["ordersVersion"]);
    assert_ne!(s1.body["productsVersion"], s0.body["productsVersion"]);
    assert_eq!(s1.body["pendingOrders"], 1);
    assert_eq!(s1.body["latestOrderNumber"], o["orderNumber"]);

    // A status change (no new rows) must also move the orders version, even
    // when its updated_at is not newer than the current max.
    sqlx::query("UPDATE orders SET version = version + 1, payment_status = 'failed', updated_at = updated_at")
        .execute(&h.pool)
        .await
        .expect("touch");
    let s2 = h.admin_get("/api/v1/admin/sync").await;
    assert_ne!(s2.body["ordersVersion"], s1.body["ordersVersion"]);
    assert_eq!(s2.body["productsVersion"], s1.body["productsVersion"]);
}

// ---------------------------------------------------------------------------
// Payments lifecycle: test mode, unpaid orders, expiry sweep, revival
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_mode_charges_one_rupee_per_unit(pool: PgPool) {
    let h = setup_with(pool, |c| c.payment_test_mode = true).await;
    add_product(&h.pool, "zm-03", 16_500, 30_000, 100, true).await;
    add_product(&h.pool, "zm-cheap", 50, 80, 100, true).await; // MRP below ₹1

    let r = h
        .order(order_body(
            Uuid::new_v4(),
            "9840012345",
            &[("zm-03", 10), ("zm-cheap", 2)],
        ))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let o = &r.body["order"];
    assert_eq!(o["isTest"], true);
    assert_eq!(o["status"], "pending_payment");
    assert_eq!(o["subtotal"], 12); // ₹1 × 12 units
    assert_eq!(o["deliveryFee"], 1, "test mode: delivery charged at ₹1 too");
    assert_eq!(o["totalAmount"], 13);
    assert_eq!(o["mrpTotal"], 3000 + 2); // real MRP kept, floored at ₹1
    assert_eq!(o["items"][0]["unitPrice"], 1);
    assert_eq!(o["items"][0]["unitMrp"], 300);
    assert_eq!(o["items"][0]["lineTotal"], 10);
    assert_eq!(o["items"][1]["unitMrp"], 1);
    assert_eq!(
        h.stock("zm-03").await,
        90,
        "stock is reserved exactly as in live mode"
    );
    let (is_test, total): (bool, i64) = sqlx::query_as("SELECT is_test, total_paise FROM orders")
        .fetch_one(&h.pool)
        .await
        .expect("row");
    assert!(is_test);
    assert_eq!(total, 1_300);
}

// ---------------------------------------------------------------------------
// Two-tier pricing (bulk ≥ ₹5,000 at list prices, else standard = 80 % off MRP)
// ---------------------------------------------------------------------------

/// ₹32 list / ₹210 MRP (standard ₹42) and ₹1,000 list / ₹6,000 MRP (standard ₹1,200).
async fn add_tier_products(pool: &PgPool) {
    add_product(pool, "zm-a", 3_200, 21_000, 1_000, true).await;
    add_product(pool, "zm-b", 100_000, 600_000, 100, true).await;
}

/// Place an order (must succeed); returns the public receipt and the admin `Order`.
async fn place_priced(h: &Harness, key: Uuid, items: &[(&str, i64)]) -> (Value, Value) {
    let r = h.order(order_body(key, "9840012345", items)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let receipt = r.body["order"].clone();
    let id = h
        .order_id(receipt["orderNumber"].as_str().expect("orderNumber"))
        .await;
    (receipt, h.get_order(id).await)
}

async fn stored_tier(h: &Harness, order_number: &str) -> (String, i64) {
    sqlx::query_as("SELECT pricing_tier, total_paise FROM orders WHERE order_number = $1")
        .bind(order_number)
        .fetch_one(&h.pool)
        .await
        .expect("pricing tier")
}

#[sqlx::test(migrations = "./migrations")]
async fn small_order_is_charged_standard_prices(pool: PgPool) {
    let h = setup(pool).await;
    add_tier_products(&h.pool).await;

    // List total ₹32 × 10 + ₹1,000 × 2 = ₹2,320 < ₹5,000.
    let (o, admin) = place_priced(&h, Uuid::new_v4(), &[("zm-a", 10), ("zm-b", 2)]).await;
    assert_eq!(o["pricingTier"], "standard");
    assert_eq!(o["items"][0]["productId"], "zm-a");
    // 20 % of MRP ₹210 = ₹42; 20 % of ₹6,000 = ₹1,200.
    assert_eq!(o["items"][0]["unitPrice"], 42);
    assert_eq!(o["items"][0]["unitMrp"], 210);
    assert_eq!(o["items"][0]["lineTotal"], 420);
    assert_eq!(o["items"][1]["unitPrice"], 1200);
    assert_eq!(o["items"][1]["lineTotal"], 2400);
    assert_eq!(o["subtotal"], 2820);
    assert_eq!(o["deliveryFee"], 500);
    assert_eq!(o["totalAmount"], 3320);
    assert_eq!(o["mrpTotal"], 2100 + 12_000);

    assert_eq!(admin["pricingTier"], "standard");
    assert_eq!(admin["subtotal"], 2820);
    assert_eq!(admin["deliveryFee"], 500);
    assert_eq!(admin["totalAmount"], 3320);
    assert_eq!(admin["items"][0]["unitPrice"], 42);

    let number = o["orderNumber"].as_str().expect("number");
    assert_eq!(
        stored_tier(&h, number).await,
        ("standard".to_owned(), 332_000)
    );
    // total_paise = sum(line totals) + delivery_fee_paise.
    let line_sum = h
        .count("SELECT sum(line_total_paise)::bigint FROM order_items")
        .await;
    assert_eq!(line_sum, 282_000);
    assert_eq!(
        h.count("SELECT delivery_fee_paise FROM orders").await,
        50_000
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn large_order_is_charged_list_prices(pool: PgPool) {
    let h = setup(pool).await;
    add_tier_products(&h.pool).await;

    // ₹32 × 125 + ₹1,000 × 2 = ₹6,000 ≥ ₹5,000.
    let key = Uuid::new_v4();
    let items = [("zm-a", 125), ("zm-b", 2)];
    let (o, admin) = place_priced(&h, key, &items).await;
    assert_eq!(o["pricingTier"], "bulk");
    assert_eq!(o["items"][0]["unitPrice"], 32);
    assert_eq!(o["items"][1]["unitPrice"], 1000);
    assert_eq!(o["subtotal"], 6_000);
    assert_eq!(o["deliveryFee"], 0, "₹5,000+ ships free");
    assert_eq!(o["totalAmount"], 6_000);
    assert_eq!(o["mrpTotal"], 26_250 + 12_000);
    assert_eq!(admin["pricingTier"], "bulk");
    assert_eq!(admin["deliveryFee"], 0);
    let number = o["orderNumber"].as_str().expect("number");
    assert_eq!(stored_tier(&h, number).await, ("bulk".to_owned(), 600_000));

    // An idempotent replay returns the same tier and prices.
    let r = h.order(order_body(key, "9840012345", &items)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["order"]["orderNumber"], number);
    assert_eq!(r.body["order"]["pricingTier"], "bulk");
    assert_eq!(r.body["order"]["totalAmount"], 6_000);
    assert_eq!(r.body["order"]["subtotal"], 6_000);
    assert_eq!(r.body["order"]["deliveryFee"], 0);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn bulk_minimum_is_inclusive(pool: PgPool) {
    let h = setup(pool).await;
    add_tier_products(&h.pool).await;

    // Exactly ₹5,000 at list prices (₹32 × 125 + ₹1,000 × 1) → bulk.
    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-a", 125), ("zm-b", 1)]).await;
    assert_eq!(o["pricingTier"], "bulk");
    assert_eq!(o["totalAmount"], 5_000);
    assert_eq!(o["deliveryFee"], 0);

    // ₹32 short of it (₹4,968) → standard on every line: ₹42 × 124 + ₹1,200 × 1
    // (₹6,408, so delivery is free).
    let r = h
        .order(order_body(
            Uuid::new_v4(),
            "9840012346",
            &[("zm-a", 124), ("zm-b", 1)],
        ))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["pricingTier"], "standard");
    assert_eq!(r.body["order"]["totalAmount"], 5_208 + 1_200);
    assert_eq!(r.body["order"]["deliveryFee"], 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn bulk_minimum_is_configurable(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.pricing.bulk_min_order_paise = 200_000; // ₹2,000
        c.pricing.standard_discount_percent = 60;
    })
    .await;
    add_tier_products(&h.pool).await;

    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-b", 2)]).await;
    assert_eq!(o["pricingTier"], "bulk");
    assert_eq!(o["subtotal"], 2_000);
    assert_eq!(
        o["totalAmount"],
        2_000 + 500,
        "bulk but below ₹5,000: delivery charged"
    );

    let r = h
        .order(order_body(Uuid::new_v4(), "9840012346", &[("zm-a", 10)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["pricingTier"], "standard");
    assert_eq!(r.body["order"]["items"][0]["unitPrice"], 84); // 40 % of ₹210
}

// ---------------------------------------------------------------------------
// Delivery charge (₹500 below a ₹5,000 subtotal, free from ₹5,000 inclusive)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn delivery_is_charged_below_the_free_minimum_only(pool: PgPool) {
    let h = setup(pool).await;
    // ₹100, not discounted: the same price in both tiers.
    add_product(&h.pool, "zm-flat", 10_000, 10_000, 1_000, true).await;

    // ₹4,900 of goods → ₹500 delivery. The total (₹5,400) exceeds the goods'
    // MRP (₹4,900): allowed, the MRP check is on the goods only.
    let key = Uuid::new_v4();
    let (o, admin) = place_priced(&h, key, &[("zm-flat", 49)]).await;
    assert_eq!(o["pricingTier"], "standard");
    assert_eq!(o["subtotal"], 4_900);
    assert_eq!(o["deliveryFee"], 500);
    assert_eq!(o["totalAmount"], 5_400);
    assert_eq!(o["mrpTotal"], 4_900);
    assert_eq!(admin["subtotal"], 4_900);
    assert_eq!(admin["deliveryFee"], 500);
    assert_eq!(admin["totalAmount"], 5_400);
    let number = o["orderNumber"].as_str().expect("number").to_owned();
    assert_eq!(
        stored_tier(&h, &number).await,
        ("standard".to_owned(), 540_000)
    );

    // Idempotent replay: same body, same numbers.
    let r1 = h
        .order(order_body(key, "9840012345", &[("zm-flat", 49)]))
        .await;
    assert_eq!(r1.status, StatusCode::OK, "{}", r1.body);
    assert_eq!(r1.body["order"], o);

    // Exactly ₹5,000 → free.
    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-flat", 50)]).await;
    assert_eq!(o["subtotal"], 5_000);
    assert_eq!(o["deliveryFee"], 0);
    assert_eq!(o["totalAmount"], 5_000);

    // Above → free.
    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-flat", 51)]).await;
    assert_eq!(o["deliveryFee"], 0);
    assert_eq!(o["totalAmount"], 5_100);

    // A single small item: ₹100 + ₹500.
    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-flat", 1)]).await;
    assert_eq!(o["subtotal"], 100);
    assert_eq!(o["deliveryFee"], 500);
    assert_eq!(o["totalAmount"], 600);

    // Invariant for every order: total = line totals + fee.
    let mismatched = h
        .count(
            "SELECT count(*) FROM orders o WHERE o.total_paise <> \
             (SELECT sum(line_total_paise) FROM order_items i WHERE i.order_id = o.id) + o.delivery_fee_paise",
        )
        .await;
    assert_eq!(mismatched, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn delivery_charge_is_configurable(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.pricing.delivery_fee_paise = 9_950; // ₹99.50
        c.pricing.free_delivery_min_paise = 20_000; // ₹200
    })
    .await;
    add_product(&h.pool, "zm-flat", 10_000, 10_000, 1_000, true).await;
    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-flat", 1)]).await;
    assert_eq!(o["deliveryFee"].as_f64(), Some(99.5));
    assert_eq!(o["totalAmount"].as_f64(), Some(199.5));
    let (o, _) = place_priced(&h, Uuid::new_v4(), &[("zm-flat", 2)]).await;
    assert_eq!(o["deliveryFee"], 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn zero_free_delivery_minimum_means_always_free(pool: PgPool) {
    let h = setup_with(pool, |c| c.pricing.free_delivery_min_paise = 0).await;
    add_product(&h.pool, "zm-flat", 10_000, 10_000, 1_000, true).await;
    let r = h
        .order(order_body(Uuid::new_v4(), "9840012346", &[("zm-flat", 1)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["deliveryFee"], 0);
    assert_eq!(r.body["order"]["totalAmount"], 100);
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_state_is_optional_and_defaults_to_tamil_nadu(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-flat", 10_000, 10_000, 1_000, true).await;

    let mut body = order_body(Uuid::new_v4(), "9840012345", &[("zm-flat", 1)]);
    body["customer"]
        .as_object_mut()
        .expect("customer")
        .remove("state");
    let r = h.order(body).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let id = h
        .order_id(r.body["order"]["orderNumber"].as_str().expect("n"))
        .await;
    assert_eq!(h.get_order(id).await["customer"]["state"], "Tamil Nadu");

    let mut body = order_body(Uuid::new_v4(), "9840012346", &[("zm-flat", 1)]);
    body["customer"]["state"] = json!("  ");
    let r = h.order(body).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let id = h
        .order_id(r.body["order"]["orderNumber"].as_str().expect("n"))
        .await;
    assert_eq!(h.get_order(id).await["customer"]["state"], "Tamil Nadu");

    let mut body = order_body(Uuid::new_v4(), "9840012347", &[("zm-flat", 1)]);
    body["customer"]["state"] = json!("x");
    let r = h.order(body).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_mode_records_tier_from_list_prices_but_charges_one_rupee(pool: PgPool) {
    let h = setup_with(pool, |c| c.payment_test_mode = true).await;
    add_tier_products(&h.pool).await;

    let (small, _) = place_priced(&h, Uuid::new_v4(), &[("zm-a", 10)]).await;
    assert_eq!(small["pricingTier"], "standard");
    assert_eq!(small["subtotal"], 10);
    assert_eq!(small["deliveryFee"], 1);
    assert_eq!(small["totalAmount"], 11);
    assert_eq!(small["items"][0]["unitPrice"], 1);

    let r = h
        .order(order_body(
            Uuid::new_v4(),
            "9840012346",
            &[("zm-a", 250), ("zm-b", 2)],
        ))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["pricingTier"], "bulk");
    assert_eq!(r.body["order"]["totalAmount"], 252 + 1);
}

/// Move every `pending_payment` order's deadline into the past.
async fn expire_all(pool: &PgPool) {
    sqlx::query(
        "UPDATE orders SET payment_expires_at = now() - interval '1 minute' WHERE status = 'pending_payment'",
    )
    .execute(pool)
    .await
    .expect("expire");
}

#[sqlx::test(migrations = "./migrations")]
async fn expiry_sweep_restocks_once(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    add_product(&h.pool, "zm-02", 1_000, 2_000, 50, true).await;

    // Two unpaid orders sharing zm-01, plus a paid one that must never expire.
    let a = h
        .place_unpaid(&phone(1), &[("zm-01", 5), ("zm-02", 2)])
        .await;
    let b = h.place_unpaid(&phone(2), &[("zm-01", 3)]).await;
    let paid = h.place(&phone(3), &[("zm-01", 1)]).await;
    let fresh = h.place_unpaid(&phone(4), &[("zm-02", 4)]).await;
    assert_eq!(h.stock("zm-01").await, 41);
    assert_eq!(
        expire_unpaid_orders(&h.pool, 100).await.expect("sweep"),
        0,
        "nothing due yet"
    );

    for o in [&a, &b] {
        sqlx::query(
            "UPDATE orders SET payment_expires_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(o["id"].as_str().expect("id").parse::<Uuid>().expect("uuid"))
        .execute(&h.pool)
        .await
        .expect("due");
    }
    assert_eq!(expire_unpaid_orders(&h.pool, 100).await.expect("sweep"), 2);
    assert_eq!(
        expire_unpaid_orders(&h.pool, 100).await.expect("sweep"),
        0,
        "idempotent"
    );

    assert_eq!(h.stock("zm-01").await, 49); // only the paid order's unit is still out
    assert_eq!(h.stock("zm-02").await, 46); // fresh order still holds 4
    for o in [&a, &b] {
        let o = h
            .get_order(o["id"].as_str().expect("id").parse().expect("uuid"))
            .await;
        assert_eq!(o["status"], "cancelled");
        assert_eq!(o["cancelReason"], "Payment not completed");
        assert_eq!(o["payment"]["status"], "pending");
        assert_eq!(o["version"], 2);
        assert!(o["cancelledAt"].is_string());
    }
    assert_eq!(
        h.get_order(paid["id"].as_str().expect("id").parse().expect("uuid"))
            .await["status"],
        "pending_pack"
    );
    assert_eq!(
        h.get_order(fresh["id"].as_str().expect("id").parse().expect("uuid"))
            .await["status"],
        "pending_payment"
    );

    assert_eq!(
        h.count("SELECT count(*) FROM stock_movements WHERE reason = 'payment_expired'")
            .await,
        3
    );
    // Ledger stays consistent line by line even though both orders share zm-01.
    let ledger: Vec<(i32, i32)> = sqlx::query_as(
        "SELECT delta, stock_after FROM stock_movements WHERE product_id = 'zm-01' ORDER BY id",
    )
    .fetch_all(&h.pool)
    .await
    .expect("ledger");
    let mut running = 50;
    for (delta, after) in &ledger {
        running += delta;
        assert_eq!(running, *after, "{ledger:?}");
    }
    assert_eq!(running, h.stock("zm-01").await);
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_sweeps_expire_each_order_once(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 1_000, true).await;
    add_product(&h.pool, "zm-02", 1_000, 2_000, 1_000, true).await;
    for i in 0..30 {
        let items: &[(&str, i64)] = if i % 2 == 0 {
            &[("zm-01", 2), ("zm-02", 1)]
        } else {
            &[("zm-01", 1)]
        };
        h.place_unpaid(&phone(i), items).await;
    }
    expire_all(&h.pool).await;

    let sweeper = |pool: PgPool| async move {
        let mut total = 0;
        loop {
            let n = expire_unpaid_orders(&pool, 4).await.expect("sweep");
            if n == 0 {
                return total;
            }
            total += n;
        }
    };
    let (x, y) = tokio::join!(
        tokio::spawn(sweeper(h.pool.clone())),
        tokio::spawn(sweeper(h.pool.clone()))
    );
    assert_eq!(x.expect("join") + y.expect("join"), 30);
    assert_eq!(
        h.count("SELECT count(*) FROM orders WHERE status = 'cancelled'")
            .await,
        30
    );
    assert_eq!(
        h.count("SELECT count(*) FROM orders WHERE version <> 2")
            .await,
        0,
        "each order expired once"
    );
    assert_eq!(
        h.count("SELECT count(*) FROM stock_movements WHERE reason = 'payment_expired'")
            .await,
        45
    );
    assert_eq!(h.stock("zm-01").await, 1_000);
    assert_eq!(h.stock("zm-02").await, 1_000);
}

async fn revive(pool: &PgPool, id: Uuid) -> ReviveOutcome {
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SELECT 1 FROM orders WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *tx)
        .await
        .expect("lock order");
    let out = revive_expired_order(&mut tx, id).await.expect("revive");
    tx.commit().await.expect("commit");
    out
}

#[sqlx::test(migrations = "./migrations")]
async fn revive_expired_order_outcomes(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 10, true).await;
    add_product(&h.pool, "zm-02", 1_000, 2_000, 10, true).await;

    // 1. Expired, stock still there → revived and re-reserved.
    let a = h
        .place_unpaid(&phone(1), &[("zm-01", 4), ("zm-02", 1)])
        .await;
    let a_id: Uuid = a["id"].as_str().expect("id").parse().expect("uuid");
    assert_eq!(
        revive(&h.pool, a_id).await,
        ReviveOutcome::NotExpired,
        "still pending_payment"
    );
    expire_all(&h.pool).await;
    assert_eq!(expire_unpaid_orders(&h.pool, 10).await.expect("sweep"), 1);
    assert_eq!(h.stock("zm-01").await, 10);
    assert_eq!(revive(&h.pool, a_id).await, ReviveOutcome::Revived);
    let o = h.get_order(a_id).await;
    assert_eq!(o["status"], "pending_payment");
    assert_eq!(o["cancelledAt"], Value::Null);
    assert_eq!(o["cancelReason"], Value::Null);
    assert_eq!(o["version"], 3);
    assert_eq!(h.stock("zm-01").await, 6);
    assert_eq!(h.stock("zm-02").await, 9);
    assert_eq!(
        h.count("SELECT count(*) FROM stock_movements WHERE reason = 'payment_revived'")
            .await,
        2
    );
    assert_eq!(
        revive(&h.pool, a_id).await,
        ReviveOutcome::NotExpired,
        "not twice"
    );

    // 2. Expired, stock sold to someone else meanwhile → InsufficientStock, nothing changes.
    let b = h.place_unpaid(&phone(2), &[("zm-01", 5)]).await;
    let b_id: Uuid = b["id"].as_str().expect("id").parse().expect("uuid");
    h.pay_sql(a_id).await; // a is paid now, keeps its 4 units
    expire_all(&h.pool).await;
    assert_eq!(expire_unpaid_orders(&h.pool, 10).await.expect("sweep"), 1);
    assert_eq!(h.stock("zm-01").await, 6);
    h.place(&phone(3), &[("zm-01", 3)]).await; // leaves 3 < 5
    let movements_before = h.count("SELECT count(*) FROM stock_movements").await;
    assert_eq!(
        revive(&h.pool, b_id).await,
        ReviveOutcome::InsufficientStock
    );
    assert_eq!(h.stock("zm-01").await, 3);
    assert_eq!(h.status_of(b_id).await, "cancelled");
    assert_eq!(
        h.count("SELECT count(*) FROM stock_movements").await,
        movements_before
    );

    // 3. Cancelled by an admin (even with the same wording) → never revived.
    let c = h.place_unpaid(&phone(4), &[("zm-02", 1)]).await;
    let c_id: Uuid = c["id"].as_str().expect("id").parse().expect("uuid");
    let r = h
        .admin_post(
            &format!("/api/v1/admin/orders/{c_id}/cancel"),
            json!({"reason": "Payment not completed", "expectedVersion": 1}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(revive(&h.pool, c_id).await, ReviveOutcome::NotExpired);
    assert_eq!(
        revive(&h.pool, Uuid::now_v7()).await,
        ReviveOutcome::NotExpired
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unpaid_orders_are_hidden_and_guarded(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    let paid = h.place(&phone(1), &[("zm-01", 1)]).await;
    let unpaid = h.place_unpaid(&phone(2), &[("zm-01", 7)]).await;
    let unpaid_id = unpaid["id"].as_str().expect("id");

    let count = |r: &Resp| r.body["orders"].as_array().map(Vec::len);
    let r = h.admin_get("/api/v1/admin/orders").await;
    assert_eq!(count(&r), Some(1));
    assert_eq!(r.body["orders"][0]["id"], paid["id"]);
    assert_eq!(
        count(&h.admin_get("/api/v1/admin/orders?includeUnpaid=true").await),
        Some(2)
    );
    assert_eq!(
        count(
            &h.admin_get("/api/v1/admin/orders?includeUnpaid=false")
                .await
        ),
        Some(1)
    );
    let r = h
        .admin_get("/api/v1/admin/orders?status=pending_payment")
        .await;
    assert_eq!(count(&r), Some(1));
    assert_eq!(r.body["orders"][0]["id"], unpaid["id"]);
    assert_eq!(
        h.admin_get("/api/v1/admin/orders?includeUnpaid=yes")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // Direct lookup still works.
    assert_eq!(
        h.admin_get(&format!("/api/v1/admin/orders/{unpaid_id}"))
            .await
            .status,
        StatusCode::OK
    );

    // Cannot be advanced manually.
    for target in ["pending_pack", "dispatched", "delivered"] {
        let r = h
            .admin_post(
                &format!("/api/v1/admin/orders/{unpaid_id}/status"),
                json!({"status": target, "transportPartner": "VRL", "docketNumber": "LR-1", "expectedVersion": 1}),
            )
            .await;
        assert_eq!(r.code(), "invalid_transition", "{target}");
    }

    // But may be cancelled, releasing the reservation.
    assert_eq!(h.stock("zm-01").await, 42);
    let r = h
        .admin_post(
            &format!("/api/v1/admin/orders/{unpaid_id}/cancel"),
            json!({"reason": "Duplicate checkout", "expectedVersion": 1}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["status"], "cancelled");
    assert_eq!(r.body["payment"]["status"], "pending");
    assert_eq!(h.stock("zm-01").await, 49);

    // A paid order cancelled before dispatch keeps its payment as paid (refund is separate).
    let r = h
        .admin_post(
            &format!(
                "/api/v1/admin/orders/{}/cancel",
                paid["id"].as_str().expect("id")
            ),
            json!({"expectedVersion": 2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(h.stock("zm-01").await, 50);
}

// ---------------------------------------------------------------------------
// Payment switch (CASHFREE_ENABLED=false ⇒ pay later) & admin notifications
// ---------------------------------------------------------------------------

const OUTBOX_ROWS: &str =
    "SELECT count(*) FROM email_outbox WHERE kind = 'admin_order_notification'";

#[sqlx::test(migrations = "./migrations")]
async fn pay_later_mode_places_confirmed_orders_and_notifies_once(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.payments_enabled = false;
        c.order_notifications_enabled = true;
    })
    .await;
    add_product(&h.pool, "zm-01", 16_500, 30_000, 50, true).await;
    let key = Uuid::new_v4();

    let r = h
        .order(order_body(key, "9840012345", &[("zm-01", 2)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let o = &r.body["order"];
    assert_eq!(o["status"], "pending_pack");
    assert_eq!(o["paymentExpiresAt"], Value::Null);
    assert_eq!(o["isTest"], false);
    assert_eq!(o["totalAmount"], 330 + 500);
    assert!(r.body["orderToken"].is_string());
    assert_eq!(h.stock("zm-01").await, 48, "stock deducted at placement");

    let id = h.order_id(o["orderNumber"].as_str().expect("n")).await;
    let admin = h.get_order(id).await;
    assert_eq!(admin["payment"]["method"], "pay_later");
    assert_eq!(admin["payment"]["status"], "pending");
    // Visible in the default admin list right away.
    let list = h.admin_get("/api/v1/admin/orders").await;
    assert_eq!(list.body["orders"].as_array().map(Vec::len), Some(1));

    let outbox: (Option<Uuid>, String) = sqlx::query_as(
        "SELECT order_id, status FROM email_outbox WHERE kind = 'admin_order_notification'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("one outbox row");
    assert_eq!(outbox, (Some(id), "pending".to_owned()));
    assert_eq!(
        h.count("SELECT count(*) FROM email_outbox").await,
        1,
        "customer confirmations disabled ⇒ admin row only"
    );
    assert_eq!(
        admin["customer"]["email"], "ravi.kumar@example.com",
        "trimmed + lower-cased"
    );

    // Idempotent replay: same order, no second email.
    let again = h
        .order(order_body(key, "9840012345", &[("zm-01", 2)]))
        .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body, r.body);
    assert_eq!(h.count(OUTBOX_ROWS).await, 1);

    // Out of stock rolls back: no order, no email.
    let r = h
        .order(order_body(Uuid::new_v4(), &phone(7), &[("zm-01", 49)]))
        .await;
    assert_eq!(r.code(), "out_of_stock");
    assert_eq!(h.count(OUTBOX_ROWS).await, 1);
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 1);

    // The sweep never touches pay-later orders.
    sqlx::query("UPDATE orders SET created_at = now() - interval '2 days'")
        .execute(&h.pool)
        .await
        .expect("age");
    assert_eq!(expire_unpaid_orders(&h.pool, 100).await.expect("sweep"), 0);
    assert_eq!(h.status_of(id).await, "pending_pack");

    // Admin records the offline payment manually.
    let r = h
        .admin_post(
            &format!("/api/v1/admin/orders/{id}/payment"),
            json!({"action": "mark_paid", "method": "upi", "reference": "UTR42", "expectedVersion": 1}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["payment"]["status"], "paid");
    assert_eq!(r.body["payment"]["method"], "upi");
    assert_eq!(r.body["payment"]["reference"], "UTR42");
}

#[sqlx::test(migrations = "./migrations")]
async fn pay_later_mode_without_notifications_and_test_pricing(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.payments_enabled = false;
        c.order_notifications_enabled = false;
        c.payment_test_mode = true;
    })
    .await;
    add_product(&h.pool, "zm-01", 16_500, 30_000, 50, true).await;

    let r = h
        .order(order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 3)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["status"], "pending_pack");
    assert_eq!(r.body["order"]["isTest"], true);
    assert_eq!(
        r.body["order"]["totalAmount"],
        3 + 1,
        "₹1 per unit (and ₹1 delivery) in pay-later mode too"
    );
    assert_eq!(h.count(OUTBOX_ROWS).await, 0, "notifications disabled");

    // Unpaid pay-later orders show up as pending collection (test orders aside).
    sqlx::query("UPDATE orders SET is_test = FALSE")
        .execute(&h.pool)
        .await
        .expect("real");
    let d = h.admin_get("/api/v1/admin/dashboard").await;
    assert_eq!(d.body["revenue"]["pendingCollection"], 4);
    assert_eq!(
        d.body["unpaid"]["orders"], 0,
        "pay-later orders are not awaiting online payment"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn online_mode_does_not_notify_at_placement(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.payments_enabled = true;
        c.order_notifications_enabled = true;
    })
    .await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    let r = h
        .order(order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["status"], "pending_payment");
    assert!(r.body["order"]["paymentExpiresAt"].is_string());
    assert_eq!(
        h.count(OUTBOX_ROWS).await,
        0,
        "notified only once payment is confirmed"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_email_is_required_and_validated(pool: PgPool) {
    let h = setup(pool).await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;

    let mut cases = Vec::new();
    for email in [Value::Null, json!(""), json!("   ")] {
        let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]);
        b["customer"]["email"] = email;
        cases.push(b);
    }
    let mut missing = order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]);
    missing["customer"]
        .as_object_mut()
        .expect("customer")
        .remove("email");
    cases.push(missing);
    for body in cases {
        let r = h.order(body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
        assert_eq!(r.code(), "validation_error");
        let msg = r.body["error"]["message"].as_str().unwrap_or("");
        assert!(msg.starts_with("customer.email is required"), "{msg}");
    }

    for bad in [
        "ravi",
        "ravi@localhost",
        "a b@x.com",
        "<script>@x.com",
        &format!("{}@x.com", "a".repeat(250)),
    ] {
        let mut b = order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]);
        b["customer"]["email"] = json!(bad);
        let r = h.order(b).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
        let msg = r.body["error"]["message"].as_str().unwrap_or("");
        assert_eq!(msg, "customer.email is not a valid email address.");
        assert!(!msg.contains(bad));
    }
    assert_eq!(h.count("SELECT count(*) FROM orders").await, 0);
    assert_eq!(h.stock("zm-01").await, 50);
}

#[sqlx::test(migrations = "./migrations")]
async fn pay_later_mode_queues_admin_and_customer_emails_once(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.payments_enabled = false;
        c.order_notifications_enabled = true;
        c.customer_order_emails_enabled = true;
    })
    .await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 10, true).await;
    let key = Uuid::new_v4();

    let r = h
        .order(order_body(key, "9840012345", &[("zm-01", 2)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let id = h
        .order_id(r.body["order"]["orderNumber"].as_str().expect("n"))
        .await;
    let kinds: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT kind, order_id FROM email_outbox ORDER BY kind")
            .fetch_all(&h.pool)
            .await
            .expect("outbox");
    assert_eq!(
        kinds,
        vec![
            ("admin_order_notification".to_owned(), Some(id)),
            ("customer_order_confirmation".to_owned(), Some(id)),
        ]
    );

    let again = h
        .order(order_body(key, "9840012345", &[("zm-01", 2)]))
        .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(
        h.count("SELECT count(*) FROM email_outbox").await,
        2,
        "replay queues nothing"
    );

    let r = h
        .order(order_body(Uuid::new_v4(), &phone(5), &[("zm-01", 9)]))
        .await;
    assert_eq!(r.code(), "out_of_stock");
    assert_eq!(
        h.count("SELECT count(*) FROM email_outbox").await,
        2,
        "rolled-back order queues nothing"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_confirmation_without_admin_notification(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.payments_enabled = false;
        c.order_notifications_enabled = false;
        c.customer_order_emails_enabled = true;
    })
    .await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 10, true).await;
    let r = h
        .order(order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let id = h
        .order_id(r.body["order"]["orderNumber"].as_str().expect("n"))
        .await;
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM email_outbox WHERE order_id = $1")
            .bind(id)
            .fetch_all(&h.pool)
            .await
            .expect("outbox");
    assert_eq!(kinds, vec!["customer_order_confirmation".to_owned()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn online_mode_queues_no_email_at_placement(pool: PgPool) {
    let h = setup_with(pool, |c| {
        c.payments_enabled = true;
        c.order_notifications_enabled = true;
        c.customer_order_emails_enabled = true;
    })
    .await;
    add_product(&h.pool, "zm-01", 1_000, 2_000, 50, true).await;
    let r = h
        .order(order_body(Uuid::new_v4(), "9840012345", &[("zm-01", 1)]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["order"]["status"], "pending_payment");
    assert_eq!(h.count("SELECT count(*) FROM email_outbox").await, 0);
}
