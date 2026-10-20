//! End-to-end tests for the public catalog, admin product management and
//! inventory APIs against a real PostgreSQL database (a fresh database per test
//! via `#[sqlx::test]`, with all migrations — including the catalog seed — applied).

use std::{sync::Arc, time::Duration};

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
    email::{EmailSender, log::LogEmailSender},
    routes,
    state::AppState,
};

const ADMIN_EMAIL: &str = "ops@zhimart.com";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

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
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Resp {
    fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("<none>")
    }
    fn message(&self) -> &str {
        self.body["error"]["message"].as_str().unwrap_or("")
    }
}

async fn setup(pool: PgPool) -> Harness {
    setup_with(pool, false).await
}

/// The behaviour tests run against a small, stable fixture catalog (the original
/// 15-product starter seed, `zm-01`..`zm-15`) instead of the production price
/// list, so catalog changes never break them. `seed_2026_catalog_is_present`
/// checks the real production seed.
const FIXTURE_CATALOG: &str = include_str!("../migrations/20261013000004_seed_catalog.sql");

async fn load_fixture_catalog(pool: &PgPool) {
    sqlx::query("DELETE FROM products")
        .execute(pool)
        .await
        .expect("clear products");
    sqlx::raw_sql(FIXTURE_CATALOG)
        .execute(pool)
        .await
        .expect("load fixture catalog");
}

async fn setup_with(pool: PgPool, payment_test_mode: bool) -> Harness {
    load_fixture_catalog(&pool).await;
    setup_inner(pool, payment_test_mode).await
}

async fn setup_inner(pool: PgPool, payment_test_mode: bool) -> Harness {
    let sender: Arc<dyn EmailSender> = Arc::new(LogEmailSender);
    let mut config = test_config();
    config.payment_test_mode = payment_test_mode;
    let state = AppState::new(Arc::new(config), pool.clone(), sender).expect("state");

    // An admin with a live session, created directly so these tests do not
    // depend on the login flow.
    let admin_id = Uuid::now_v7();
    let session_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO admins (id, email, password_hash, display_name) VALUES ($1, $2, 'not-a-real-hash', 'Ops')",
    )
    .bind(admin_id)
    .bind(ADMIN_EMAIL)
    .execute(&pool)
    .await
    .expect("insert admin");
    let expires_at = sqlx::query_scalar::<_, DateTime<Utc>>(
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
        .issue(admin_id, session_id, expires_at)
        .expect("issue token")
        .token;

    let app = Router::new()
        .nest(
            "/api/v1",
            routes::catalog::public_routes().nest(
                "/admin",
                routes::admin::products::routes().merge(routes::admin::inventory::routes()),
            ),
        )
        .with_state(state);
    Harness { app, pool, token }
}

impl Harness {
    async fn call(&self, method: Method, path: &str, body: Option<Value>, auth: bool) -> Resp {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"));
        if auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .expect("request");
        let res = self.app.clone().oneshot(req).await.expect("infallible");
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

    async fn public(&self, path: &str) -> Resp {
        self.call(Method::GET, path, None, false).await
    }
    async fn get(&self, path: &str) -> Resp {
        self.call(Method::GET, path, None, true).await
    }
    async fn post(&self, path: &str, body: Value) -> Resp {
        self.call(Method::POST, path, Some(body), true).await
    }
    async fn put(&self, path: &str, body: Value) -> Resp {
        self.call(Method::PUT, path, Some(body), true).await
    }
    async fn delete(&self, path: &str) -> Resp {
        self.call(Method::DELETE, path, None, true).await
    }

    async fn stock(&self, id: &str) -> i32 {
        sqlx::query_scalar::<_, i32>("SELECT stock FROM products WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("stock")
    }

    async fn movement_count(&self, id: &str, reason: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM stock_movements WHERE product_id = $1 AND reason = $2",
        )
        .bind(id)
        .bind(reason)
        .fetch_one(&self.pool)
        .await
        .expect("count")
    }
}

fn product_input() -> Value {
    json!({
        "name": "Test Rocket 5\"",
        "tamilName": "  ",
        "category": "AERIAL REPEATERS",
        "brand": "Zhi-mart Fireworks",
        "packInfo": "1 Box",
        "piecesPerPack": 5,
        "price": 99.5,
        "originalPrice": 199,
        "image": "/assets/products/aerial_box.jpg",
        "gallery": ["https://cdn.example.com/rocket.jpg"],
        "description": "  Bright.  ",
        "soundLevel": "Low Sound",
        "isPopular": true,
        "lowStockThreshold": 10,
        "bayLocation": "Bay A-2",
        "sortOrder": 100
    })
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn seed_2026_catalog_is_present(pool: PgPool) {
    let h = setup_inner(pool, false).await;
    let r = h.public("/products").await;
    assert_eq!(r.status, StatusCode::OK);
    let products = r.body["products"].as_array().expect("products");
    assert_eq!(
        products.len(),
        135,
        "130 price-list products + 5 gift boxes"
    );
    assert!(
        products
            .iter()
            .all(|p| p["id"].as_str().is_some_and(|id| id.starts_with("zm26-")))
    );

    let first = &products[0];
    assert_eq!(first["id"], "zm26-001");
    assert_eq!(first["name"], "10CM Electric Sparklers");
    assert_eq!(first["packInfo"], "10 Pieces");
    assert_eq!(first["category"], "SPARKLERS");
    assert_eq!(num(&first["price"]), 32.0);
    assert_eq!(num(&first["originalPrice"]), 210.0);
    assert_eq!(first["discountPercent"], 85);
    assert_eq!(num(&first["standardPrice"]), 42.0, "80% off MRP ₹210 = ₹42");
    assert_eq!(first["standardDiscountPercent"], 80);
    assert_eq!(first["inStock"], true);
    assert_eq!(
        r.body["pricing"],
        json!({
            "bulkMinOrder": 5000, "bulkDiscountPercent": 85, "standardDiscountPercent": 80,
            "deliveryFee": 500, "freeDeliveryMin": 5000
        })
    );

    let big = products
        .iter()
        .find(|p| p["id"] == "zm26-130")
        .expect("zm26-130");
    assert_eq!(big["name"], "Shubh Vivah 10 X 10 Multi Colour");
    assert_eq!(num(&big["price"]), 6000.0);
    assert_eq!(num(&big["originalPrice"]), 40000.0);
    assert_eq!(num(&big["standardPrice"]), 8000.0);

    let gift = products.last().expect("gift box");
    assert_eq!(gift["id"], "zm26-gift-60");
    assert_eq!(gift["category"], "GIFT BOXES");
    assert_eq!(num(&gift["price"]), 1250.0);
    assert_eq!(num(&gift["originalPrice"]), 1250.0);
    assert_eq!(gift["discountPercent"], 0);
    assert_eq!(
        num(&gift["standardPrice"]),
        1250.0,
        "not discounted: standard = list price"
    );
    assert_eq!(gift["standardDiscountPercent"], 0);

    // Every 85%-list product is within ₹1 of 15% of its list price.
    for p in products.iter().filter(|p| p["category"] != "GIFT BOXES") {
        let (price, mrp) = (num(&p["price"]), num(&p["originalPrice"]));
        assert!((price - (mrp * 0.15).round()).abs() <= 1.0, "{}", p["id"]);
    }
    let initial = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM stock_movements WHERE reason = 'initial' AND note = '2026 price list import'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("count");
    assert_eq!(initial, 135);
}

#[sqlx::test(migrations = "./migrations")]
async fn fixture_catalog_is_present(pool: PgPool) {
    let h = setup(pool).await;
    let r = h.public("/products").await;
    assert_eq!(r.status, StatusCode::OK);
    let products = r.body["products"].as_array().expect("products");
    assert_eq!(products.len(), 15);
    assert_eq!(products[0]["id"], "zm-01");
    assert_eq!(products[14]["id"], "zm-15");

    let zm01 = &products[0];
    assert_eq!(zm01["name"], "2.75\" Bird");
    assert_eq!(num(&zm01["price"]), 205.0);
    assert_eq!(num(&zm01["originalPrice"]), 373.0);
    assert_eq!(zm01["discountPercent"], 45);
    assert_eq!(zm01["image"], "/assets/products/lakshmi_box.jpg");
    assert_eq!(zm01["soundLevel"], "Single Loud Blast");
    assert_eq!(zm01["isBestSelling"], true);
    assert_eq!(zm01["availableQuantity"], 120);
    assert_eq!(zm01["inStock"], true);
    assert!(
        zm01.get("stock").is_none(),
        "exact stock must not be public"
    );

    // Stock / bay follow the frontend's former defaults; one `initial` movement each.
    let zm02 = h.get("/admin/products").await.body["products"][1].clone();
    assert_eq!(zm02["stock"], 105);
    assert_eq!(zm02["bayLocation"], "Bay A-2 (Aerial Shells)");
    assert_eq!(zm02["sortOrder"], 1);
    let initial = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM stock_movements m JOIN products p ON p.id = m.product_id \
         WHERE m.reason = 'initial' AND m.stock_after = p.stock",
    )
    .fetch_one(&h.pool)
    .await
    .expect("count");
    assert_eq!(initial, 15);
}

#[sqlx::test(migrations = "./migrations")]
async fn public_catalog_hides_archived_and_caps_quantity(pool: PgPool) {
    let h = setup(pool).await;
    sqlx::query("UPDATE products SET stock = 0 WHERE id = 'zm-02'")
        .execute(&h.pool)
        .await
        .expect("update");
    sqlx::query("UPDATE products SET stock = 5000 WHERE id = 'zm-04'")
        .execute(&h.pool)
        .await
        .expect("update");

    let r = h.public("/products/zm-02").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("public, max-age=10")
    );
    assert_eq!(r.body["inStock"], false);
    assert_eq!(r.body["availableQuantity"], 0);
    assert_eq!(
        h.public("/products/zm-04").await.body["availableQuantity"],
        1000
    );

    assert_eq!(
        h.delete("/admin/products/zm-03").await.status,
        StatusCode::NO_CONTENT
    );
    let list = h.public("/products").await;
    assert_eq!(
        list.headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("public, max-age=10")
    );
    let ids: Vec<&str> = list.body["products"]
        .as_array()
        .expect("products")
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert_eq!(ids.len(), 14);
    assert!(!ids.contains(&"zm-03"));

    let r = h.public("/products/zm-03").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.code(), "not_found");
    assert_eq!(
        h.public("/products/nope-404").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.public("/products/BAD%20ID").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.public(&format!("/products/{}", "a".repeat(41)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_create_validates_and_rejects_duplicates(pool: PgPool) {
    let h = setup(pool).await;

    let mut input = product_input();
    input["id"] = json!("zm-21");
    input["initialStock"] = json!(50);
    let r = h.post("/admin/products", input.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let p = &r.body;
    assert_eq!(p["id"], "zm-21");
    assert_eq!(p["name"], "Test Rocket 5\"");
    assert_eq!(p["tamilName"], Value::Null);
    assert_eq!(p["description"], "Bright.");
    assert_eq!(num(&p["price"]), 99.5);
    assert_eq!(p["discountPercent"], 50);
    assert_eq!(p["stock"], 50);
    assert_eq!(p["lowStockThreshold"], 10);
    assert_eq!(p["version"], 1);
    assert_eq!(p["isActive"], true);
    assert_eq!(p["gallery"], json!(["https://cdn.example.com/rocket.jpg"]));
    assert_eq!(h.movement_count("zm-21", "initial").await, 1);
    let mv = h.get("/admin/inventory/zm-21/movements").await;
    assert_eq!(mv.body["movements"][0]["adminEmail"], ADMIN_EMAIL);
    assert_eq!(mv.body["movements"][0]["delta"], 50);

    // Duplicate id.
    let r = h.post("/admin/products", input).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "duplicate_id");
    let mut seeded = product_input();
    seeded["id"] = json!("zm-01");
    assert_eq!(
        h.post("/admin/products", seeded).await.code(),
        "duplicate_id"
    );

    // Generated id: slug of the name + 4 base32 chars; no stock ⇒ no movement.
    let r = h.post("/admin/products", product_input()).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let id = r.body["id"].as_str().expect("id").to_owned();
    assert!(
        id.starts_with("test-rocket-5-") && id.len() == "test-rocket-5-".len() + 4,
        "{id}"
    );
    assert_eq!(r.body["stock"], 0);
    assert_eq!(h.movement_count(&id, "initial").await, 0);

    // Field-specific validation errors.
    let cases: Vec<(&str, Value, &str)> = vec![
        ("name", json!("   "), "name"),
        ("price", json!(0), "price"),
        ("price", json!(10.123), "price"),
        ("price", json!(500), "originalPrice"),
        ("image", json!("/assets/../etc/passwd"), "image"),
        ("image", json!("http://example.com/x.jpg"), "image"),
        (
            "gallery",
            json!(vec!["/assets/products/x.jpg"; 11]),
            "gallery",
        ),
        ("gallery", json!(["javascript:alert(1)"]), "gallery"),
        ("soundLevel", json!("Deafening"), "soundLevel"),
        ("lowStockThreshold", json!(100_001), "lowStockThreshold"),
        ("sortOrder", json!(5_000_000), "sortOrder"),
        ("initialStock", json!(-1), "initialStock"),
        ("piecesPerPack", json!(0), "piecesPerPack"),
        ("packInfo", json!("x".repeat(61)), "packInfo"),
        ("id", json!("Bad_ID"), "id"),
    ];
    for (field, value, expect) in cases {
        let mut body = product_input();
        body[field] = value;
        let r = h.post("/admin/products", body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{field}: {:?}", r.body);
        assert_eq!(r.code(), "validation_error");
        assert!(r.message().contains(expect), "{field}: {}", r.message());
    }
    let mut body = product_input();
    body["unknownField"] = json!(1);
    assert_eq!(
        h.post("/admin/products", body).await.status,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_update_uses_optimistic_concurrency(pool: PgPool) {
    let h = setup(pool).await;
    let before = h.stock("zm-05").await;

    let mut body = product_input();
    body["expectedVersion"] = json!(1);
    let r = h.put("/admin/products/zm-05", body.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["version"], 2);
    assert_eq!(r.body["name"], "Test Rocket 5\"");
    assert_eq!(r.body["stock"], before, "PUT must not change stock");

    // Same (now stale) version again.
    let r = h.put("/admin/products/zm-05", body.clone()).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "version_conflict");

    assert_eq!(
        h.put("/admin/products/zm-99", body.clone()).await.status,
        StatusCode::NOT_FOUND
    );

    let mut with_stock = body.clone();
    with_stock["expectedVersion"] = json!(2);
    with_stock["initialStock"] = json!(5);
    let r = h.put("/admin/products/zm-05", with_stock).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.message().contains("initialStock"));

    let mut no_version = body.clone();
    no_version
        .as_object_mut()
        .expect("object")
        .remove("expectedVersion");
    let r = h.put("/admin/products/zm-05", no_version).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.message().contains("expectedVersion"));

    let mut other_id = body;
    other_id["expectedVersion"] = json!(2);
    other_id["id"] = json!("zm-06");
    assert_eq!(
        h.put("/admin/products/zm-05", other_id).await.status,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn archive_and_restore(pool: PgPool) {
    let h = setup(pool).await;
    assert_eq!(
        h.delete("/admin/products/zm-07").await.status,
        StatusCode::NO_CONTENT
    );
    // Idempotent.
    assert_eq!(
        h.delete("/admin/products/zm-07").await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.delete("/admin/products/zm-99").await.status,
        StatusCode::NOT_FOUND
    );

    let active = h.get("/admin/products").await;
    assert_eq!(active.body["products"].as_array().map(Vec::len), Some(14));
    let all = h.get("/admin/products?includeArchived=true").await;
    let archived = all.body["products"]
        .as_array()
        .expect("products")
        .iter()
        .find(|p| p["id"] == "zm-07")
        .cloned()
        .expect("archived product listed");
    assert_eq!(archived["isActive"], false);
    assert_eq!(archived["version"], 2);
    assert_eq!(
        h.get("/admin/products?includeArchived=maybe").await.status,
        StatusCode::BAD_REQUEST
    );

    let r = h.post("/admin/products/zm-07/restore", json!({})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["isActive"], true);
    assert_eq!(r.body["version"], 3);
    assert_eq!(h.public("/products/zm-07").await.status, StatusCode::OK);
    assert_eq!(
        h.post("/admin/products/zm-99/restore", json!({}))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn inventory_adjustments_and_ledger(pool: PgPool) {
    let h = setup(pool).await;
    let id = "zm-01"; // seeded with 120
    let path = format!("/admin/inventory/{id}/adjust");

    // `set` needs the stock the admin saw.
    let r = h
        .post(&path, json!({ "mode": "set", "quantity": 80 }))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.message().contains("expectedStock"));
    let r = h
        .post(
            &path,
            json!({ "mode": "set", "quantity": 80, "expectedStock": 119 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "version_conflict");

    let r = h
        .post(&path, json!({ "mode": "set", "quantity": 80, "expectedStock": 120, "note": "  Stock take  " }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["stock"], 80);
    assert_eq!(r.body["version"], 2);

    // No-op set: no version bump, no movement.
    let r = h
        .post(
            &path,
            json!({ "mode": "set", "quantity": 80, "expectedStock": 80 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["version"], 2);
    assert_eq!(h.movement_count(id, "manual_set").await, 1);

    let r = h
        .post(
            &path,
            json!({ "mode": "add", "quantity": 20, "note": "Batch #12" }),
        )
        .await;
    assert_eq!(r.body["stock"], 100);

    let r = h
        .post(&path, json!({ "mode": "remove", "quantity": 101 }))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.code(), "out_of_stock");
    assert_eq!(
        r.body["error"]["details"],
        json!([{ "productId": id, "requested": 101, "available": 100 }])
    );
    assert_eq!(h.stock(id).await, 100);

    let r = h
        .post(&path, json!({ "mode": "remove", "quantity": 100 }))
        .await;
    assert_eq!(r.body["stock"], 0);
    assert_eq!(r.body["inStock"], false);

    // Input validation.
    for bad in [
        json!({ "mode": "add", "quantity": 0 }),
        json!({ "mode": "add", "quantity": 1_000_001 }),
        json!({ "mode": "explode", "quantity": 1 }),
        json!({ "mode": "add", "quantity": 1, "note": "x".repeat(301) }),
        json!({ "mode": "set", "quantity": 10_000_001, "expectedStock": 0 }),
        json!({ "mode": "add", "quantity": 1, "extra": true }),
    ] {
        assert_eq!(h.post(&path, bad).await.status, StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        h.post(
            "/admin/inventory/zm-99/adjust",
            json!({ "mode": "add", "quantity": 1 })
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Ledger: newest first, with admin email and notes.
    let r = h.get(&format!("/admin/inventory/{id}/movements")).await;
    assert_eq!(r.status, StatusCode::OK);
    let mv = r.body["movements"].as_array().expect("movements");
    let reasons: Vec<&str> = mv.iter().filter_map(|m| m["reason"].as_str()).collect();
    assert_eq!(
        reasons,
        ["manual_adjust", "restock", "manual_set", "initial"]
    );
    let deltas: Vec<i64> = mv.iter().filter_map(|m| m["delta"].as_i64()).collect();
    assert_eq!(deltas, [-100, 20, -40, 120]);
    let after: Vec<i64> = mv.iter().filter_map(|m| m["stockAfter"].as_i64()).collect();
    assert_eq!(after, [0, 100, 80, 120]);
    assert_eq!(mv[0]["adminEmail"], ADMIN_EMAIL);
    assert_eq!(mv[1]["note"], "Batch #12");
    assert_eq!(mv[2]["note"], "Stock take");
    assert_eq!(mv[3]["adminEmail"], Value::Null);
    assert_eq!(mv[0]["orderNumber"], Value::Null);

    let r = h
        .get(&format!("/admin/inventory/{id}/movements?limit=2"))
        .await;
    assert_eq!(r.body["movements"].as_array().map(Vec::len), Some(2));
    for bad in ["0", "201", "abc"] {
        let r = h
            .get(&format!("/admin/inventory/{id}/movements?limit={bad}"))
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "limit={bad}");
    }
    assert_eq!(
        h.get("/admin/inventory/zm-99/movements").await.status,
        StatusCode::NOT_FOUND
    );

    // Overview reflects the adjustments.
    let r = h.get("/admin/inventory").await;
    assert_eq!(r.status, StatusCode::OK);
    let items = r.body["items"].as_array().expect("items");
    assert_eq!(items.len(), 15);
    let item = items.iter().find(|i| i["productId"] == id).expect("item");
    assert_eq!(item["status"], "out_of_stock");
    assert_eq!(item["stock"], 0);
    let summary = &r.body["summary"];
    assert_eq!(summary["outOfStock"], 1);
    let total: i64 = items.iter().filter_map(|i| i["stock"].as_i64()).sum();
    assert_eq!(summary["totalUnits"], total);
    let counted = summary["inStock"].as_i64().unwrap_or(0)
        + summary["lowStock"].as_i64().unwrap_or(0)
        + summary["outOfStock"].as_i64().unwrap_or(0);
    assert_eq!(counted, 15);
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_restocks_are_serialised(pool: PgPool) {
    let h = setup(pool).await;
    let id = "zm-06";
    let start = h.stock(id).await;

    let tasks: Vec<_> = (0..20)
        .map(|_| {
            let h = h.clone();
            tokio::spawn(async move {
                h.post(
                    &format!("/admin/inventory/{id}/adjust"),
                    json!({ "mode": "add", "quantity": 5 }),
                )
                .await
                .status
            })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.expect("join"), StatusCode::OK);
    }

    assert_eq!(h.stock(id).await, start + 100);
    assert_eq!(h.movement_count(id, "restock").await, 20);
    // The ledger is consistent: stock_after values are exactly start+5 … start+100.
    let afters = sqlx::query_scalar::<_, i32>(
        "SELECT stock_after FROM stock_movements WHERE product_id = $1 AND reason = 'restock' ORDER BY stock_after",
    )
    .bind(id)
    .fetch_all(&h.pool)
    .await
    .expect("afters");
    let expected: Vec<i32> = (1..=20).map(|i| start + 5 * i).collect();
    assert_eq!(afters, expected);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_endpoints_require_authentication(pool: PgPool) {
    let h = setup(pool).await;
    let cases = [
        (Method::GET, "/admin/products", None),
        (Method::POST, "/admin/products", Some(product_input())),
        (Method::PUT, "/admin/products/zm-01", Some(product_input())),
        (Method::DELETE, "/admin/products/zm-01", None),
        (
            Method::POST,
            "/admin/products/zm-01/restore",
            Some(json!({})),
        ),
        (Method::GET, "/admin/inventory", None),
        (
            Method::POST,
            "/admin/inventory/zm-01/adjust",
            Some(json!({ "mode": "add", "quantity": 1 })),
        ),
        (Method::GET, "/admin/inventory/zm-01/movements", None),
    ];
    for (method, path, body) in cases {
        let r = h.call(method.clone(), path, body, false).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(r.code(), "unauthorized");
    }

    let mut bad = h.clone();
    bad.token = "not.a.jwt".into();
    assert_eq!(
        bad.get("/admin/products").await.status,
        StatusCode::UNAUTHORIZED
    );

    // A revoked session stops working immediately.
    sqlx::query("UPDATE sessions SET revoked_at = now()")
        .execute(&h.pool)
        .await
        .expect("revoke");
    assert_eq!(
        h.get("/admin/inventory").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(h.stock("zm-01").await, 120);
}

#[sqlx::test(migrations = "./migrations")]
async fn live_mode_shows_real_prices(pool: PgPool) {
    let h = setup_with(pool, false).await;
    let r = h.public("/products").await;
    assert_eq!(r.body["testMode"], false);
    assert_eq!(num(&r.body["products"][0]["price"]), 205.0);
    let one = h.public("/products/zm-03").await;
    assert_eq!(num(&one.body["price"]), 165.0);
    assert_eq!(one.body["discountPercent"], 45);
    // Only 45 % off MRP: the standard price is the list price.
    assert_eq!(num(&one.body["standardPrice"]), 165.0);
    assert_eq!(one.body["standardDiscountPercent"], 45);
    assert_eq!(r.body["pricing"]["bulkMinOrder"], 5000);
    assert_eq!(r.body["pricing"]["deliveryFee"], 500);
    assert_eq!(r.body["pricing"]["freeDeliveryMin"], 5000);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_mode_prices_storefront_at_one_rupee_but_not_admin(pool: PgPool) {
    let h = setup_with(pool, true).await;

    let r = h.public("/products").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["testMode"], true);
    let products = r.body["products"].as_array().expect("products");
    assert_eq!(products.len(), 15);
    for p in products {
        assert_eq!(num(&p["price"]), 1.0, "{}", p["id"]);
        assert_eq!(num(&p["standardPrice"]), 1.0, "{}", p["id"]);
        assert_eq!(
            p["standardDiscountPercent"], p["discountPercent"],
            "{}",
            p["id"]
        );
        let mrp = num(&p["originalPrice"]);
        let expected = (((mrp - 1.0) / mrp) * 100.0).round() as i64;
        assert_eq!(p["discountPercent"].as_i64(), Some(expected), "{}", p["id"]);
    }
    assert_eq!(num(&products[0]["originalPrice"]), 373.0);
    assert_eq!(products[0]["discountPercent"], 100);
    // Test orders pay ₹1 for delivery (when not free), like every unit.
    assert_eq!(r.body["pricing"]["deliveryFee"], 1);
    assert_eq!(r.body["pricing"]["freeDeliveryMin"], 5000);

    let one = h.public("/products/zm-03").await;
    assert_eq!(num(&one.body["price"]), 1.0);
    assert_eq!(num(&one.body["originalPrice"]), 300.0);
    assert_eq!(one.body["discountPercent"], 100);

    // Admin views keep the real prices, including after an edit.
    let admin = h.get("/admin/products").await;
    assert_eq!(num(&admin.body["products"][0]["price"]), 205.0);
    assert_eq!(admin.body["products"][0]["discountPercent"], 45);
    let r = h
        .post(
            "/admin/inventory/zm-03/adjust",
            json!({ "mode": "add", "quantity": 1 }),
        )
        .await;
    assert_eq!(num(&r.body["price"]), 165.0);
    let mut input = product_input();
    input["id"] = json!("zm-30");
    let r = h.post("/admin/products", input).await;
    assert_eq!(num(&r.body["price"]), 99.5);
    assert_eq!(num(&h.public("/products/zm-30").await.body["price"]), 1.0);

    // The database keeps the real price; test mode is presentation-only here.
    let paise = sqlx::query_scalar::<_, i64>("SELECT price_paise FROM products WHERE id = 'zm-01'")
        .fetch_one(&h.pool)
        .await
        .expect("price");
    assert_eq!(paise, 20_500);
}
