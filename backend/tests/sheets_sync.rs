//! Google Sheets order sync (`jobs::process_sheet_sync_once`) end to end:
//! real PostgreSQL (`#[sqlx::test]`, the queue is filled by the migration's
//! triggers), the real checkout/admin routes, and a mock Google (OAuth token
//! endpoint + the Sheets v4 calls we use) served by axum on 127.0.0.1:0. No
//! traffic ever goes to Google.

use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use zhimart_backend::{
    config::{AppEnv, Config, EmailProvider, GoogleOAuthClientConfig, GoogleSheetsConfig},
    email::{BoxFuture, EmailSender, OtpEmail},
    jobs::{SheetSyncRun, process_sheet_sync_once},
    repo, routes,
    sheets::{SheetsClient, row::HEADER},
    state::AppState,
};

const SPREADSHEET_ID: &str = "1TestSpreadsheetId_abcdefghijklmnop";
const CLIENT_ID: &str = "1234-test.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "test-client-secret";
const REFRESH_TOKEN: &str = "test-refresh-token";
const SHEETS_MIGRATION: i64 = 20261014000012;

// ---------------------------------------------------------------------------
// Mock Google
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MockState {
    tab_exists: bool,
    /// `rows[0]` is sheet row 1.
    rows: Vec<Vec<Value>>,
    /// Answer every Sheets call with this status.
    fail_status: Option<u16>,
    /// Answer the token endpoint with `{"error": …}` (HTTP 400).
    token_error: Option<&'static str>,
    tokens_issued: u32,
    /// The only access token currently accepted.
    valid_token: Option<String>,
    appends: u32,
    batch_updates: u32,
    unauthorized: u32,
    /// Any write without valueInputOption=RAW (or an append without INSERT_ROWS).
    bad_write_options: u32,
}

#[derive(Clone)]
struct Mock(Arc<Mutex<MockState>>);

impl Mock {
    fn with<R>(&self, f: impl FnOnce(&mut MockState) -> R) -> R {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut g)
    }

    /// Non-empty data rows (header excluded), as order numbers.
    fn order_numbers(&self) -> Vec<String> {
        self.with(|m| {
            m.rows
                .iter()
                .skip(1)
                .filter_map(|r| r.first().and_then(Value::as_str).map(str::to_owned))
                .filter(|s| !s.is_empty())
                .collect()
        })
    }

    fn row_of(&self, order_number: &str) -> Vec<Value> {
        self.with(|m| {
            m.rows
                .iter()
                .find(|r| r.first().and_then(Value::as_str) == Some(order_number))
                .cloned()
                .unwrap_or_else(|| panic!("{order_number} not in sheet"))
        })
    }
}

fn cell(row: &[Value], name: &str) -> Value {
    let i = HEADER.iter().position(|h| *h == name).expect("column");
    row.get(i).cloned().unwrap_or(Value::Null)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).expect("utf8")
}

fn query_pairs(uri: &Uri) -> Vec<(String, String)> {
    uri.query()
        .unwrap_or_default()
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(&v.replace('+', " ")))
        })
        .collect()
}

/// `'Orders'!A5:AA5` → 5.
fn range_row(range: &str) -> usize {
    let a1 = range.rsplit('!').next().expect("a1");
    let digits: String = a1
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().expect("row number")
}

fn google_error(status: u16, message: &str) -> Response {
    let code = StatusCode::from_u16(status).expect("status");
    (
        code,
        axum::Json(json!({"error": {"code": status, "message": message, "status": "MOCK"}})),
    )
        .into_response()
}

async fn mock_handler(
    State(mock): State<Mock>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = percent_decode(uri.path());

    if path == "/token" {
        let form: Vec<(String, String)> = String::from_utf8_lossy(&body)
            .split('&')
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (percent_decode(k), percent_decode(v))
            })
            .collect();
        let get = |k: &str| form.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("grant_type"), Some("refresh_token"));
        assert_eq!(get("client_id"), Some(CLIENT_ID));
        assert_eq!(get("client_secret"), Some(CLIENT_SECRET));
        assert_eq!(get("refresh_token"), Some(REFRESH_TOKEN));
        return mock.with(|m| {
            if let Some(err) = m.token_error {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(json!({"error": err, "error_description": "Token has been expired or revoked."})),
                )
                    .into_response();
            }
            m.tokens_issued += 1;
            let token = format!("ya29.mock-{}", m.tokens_issued);
            m.valid_token = Some(token.clone());
            axum::Json(json!({"access_token": token, "expires_in": 3599, "token_type": "Bearer"}))
                .into_response()
        });
    }

    let prefix = format!("/v4/spreadsheets/{SPREADSHEET_ID}");
    let Some(rest) = path.strip_prefix(&prefix) else {
        return google_error(404, "Requested entity was not found.");
    };
    let rest = rest.to_owned();
    let query = query_pairs(&uri);
    let q = |k: &str| query.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone());
    let json_body: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).expect("json body")
    };

    mock.with(|m| {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        if bearer.is_none() || bearer != m.valid_token {
            m.unauthorized += 1;
            return google_error(401, "Request had invalid authentication credentials.");
        }
        if let Some(status) = m.fail_status {
            return google_error(status, "mock failure");
        }

        match (method.clone(), rest.as_str()) {
            (Method::GET, "") => {
                let sheets: Vec<Value> = if m.tab_exists {
                    vec![json!({"properties": {"sheetId": 7, "title": "Orders"}})]
                } else {
                    vec![json!({"properties": {"sheetId": 0, "title": "Sheet1"}})]
                };
                axum::Json(
                    json!({"properties": {"title": "Crackers-Ordering-Leads"}, "sheets": sheets}),
                )
                .into_response()
            }
            (Method::POST, ":batchUpdate") => {
                let mut replies = Vec::new();
                for r in json_body["requests"].as_array().expect("requests") {
                    if r.get("addSheet").is_some() {
                        assert_eq!(r["addSheet"]["properties"]["title"], "Orders");
                        if m.tab_exists {
                            return google_error(400, "A sheet with this name already exists.");
                        }
                        m.tab_exists = true;
                        replies.push(
                            json!({"addSheet": {"properties": {"sheetId": 7, "title": "Orders"}}}),
                        );
                    } else {
                        replies.push(json!({}));
                    }
                }
                axum::Json(json!({"replies": replies})).into_response()
            }
            (Method::GET, "/values:batchGet") => {
                if !m.tab_exists {
                    return google_error(400, "Unable to parse range: 'Orders'!A1:AA1");
                }
                let ranges: Vec<String> = query
                    .iter()
                    .filter(|(k, _)| k == "ranges")
                    .map(|(_, v)| v.clone())
                    .collect();
                assert_eq!(
                    ranges,
                    vec!["'Orders'!A1:AA1".to_owned(), "'Orders'!A:A".to_owned()]
                );
                let header: Vec<Vec<Value>> = m
                    .rows
                    .first()
                    .filter(|r| !r.is_empty())
                    .map(|r| vec![r.clone()])
                    .unwrap_or_default();
                // Column A, trailing empty rows trimmed; empty rows as [].
                let mut col: Vec<Vec<Value>> = m
                    .rows
                    .iter()
                    .map(|r| match r.first() {
                        Some(v) if v.as_str() != Some("") => vec![v.clone()],
                        _ => vec![],
                    })
                    .collect();
                while col.last().is_some_and(Vec::is_empty) {
                    col.pop();
                }
                let mut vr0 = json!({"range": "'Orders'!A1:AA1", "majorDimension": "ROWS"});
                if !header.is_empty() {
                    vr0["values"] = json!(header);
                }
                let mut vr1 = json!({"range": "'Orders'!A1:A1000", "majorDimension": "ROWS"});
                if !col.is_empty() {
                    vr1["values"] = json!(col);
                }
                axum::Json(json!({"spreadsheetId": SPREADSHEET_ID, "valueRanges": [vr0, vr1]}))
                    .into_response()
            }
            (Method::PUT, r) if r.starts_with("/values/") => {
                if q("valueInputOption").as_deref() != Some("RAW") {
                    m.bad_write_options += 1;
                }
                let range = r.trim_start_matches("/values/");
                assert_eq!(range, "'Orders'!A1:AA1", "only the header is PUT");
                let row = json_body["values"][0].as_array().expect("row").clone();
                if m.rows.is_empty() {
                    m.rows.push(row);
                } else {
                    m.rows[0] = row;
                }
                axum::Json(json!({"updatedRows": 1})).into_response()
            }
            (Method::POST, "/values:batchUpdate") => {
                if json_body["valueInputOption"] != "RAW" {
                    m.bad_write_options += 1;
                }
                m.batch_updates += 1;
                for d in json_body["data"].as_array().expect("data") {
                    let n = range_row(d["range"].as_str().expect("range"));
                    let row = d["values"][0].as_array().expect("row").clone();
                    while m.rows.len() < n {
                        m.rows.push(vec![]);
                    }
                    m.rows[n - 1] = row;
                }
                axum::Json(json!({"totalUpdatedRows": 1})).into_response()
            }
            (Method::POST, r) if r.starts_with("/values/") && r.ends_with(":append") => {
                if q("valueInputOption").as_deref() != Some("RAW")
                    || q("insertDataOption").as_deref() != Some("INSERT_ROWS")
                {
                    m.bad_write_options += 1;
                }
                assert_eq!(r, "/values/'Orders'!A1:append");
                m.appends += 1;
                // Append after the last non-empty row.
                while m
                    .rows
                    .last()
                    .is_some_and(|r| r.iter().all(|c| c.as_str() == Some("")))
                {
                    m.rows.pop();
                }
                for row in json_body["values"].as_array().expect("values") {
                    m.rows.push(row.as_array().expect("row").clone());
                }
                axum::Json(json!({"updates": {"updatedRows": 1}})).into_response()
            }
            other => panic!("unexpected Sheets call {other:?}"),
        }
    })
}

async fn start_mock() -> (Mock, String) {
    let mock = Mock(Arc::new(Mutex::new(MockState::default())));
    let app = Router::new()
        .fallback(mock_handler)
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server");
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

fn sheets_config(base: &str) -> GoogleSheetsConfig {
    GoogleSheetsConfig {
        oauth: GoogleOAuthClientConfig {
            client_id: CLIENT_ID.into(),
            client_secret: SecretString::from(CLIENT_SECRET),
            token_url: format!("{base}/token"),
        },
        refresh_token: SecretString::from(REFRESH_TOKEN),
        spreadsheet_id: SPREADSHEET_ID.into(),
        tab: "Orders".into(),
        api_base: base.to_owned(),
    }
}

fn test_config(base: &str) -> Config {
    Config {
        app_env: AppEnv::Development,
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        database_url: SecretString::from("postgres://unused"),
        db_max_connections: 5,
        db_min_connections: 0,
        db_acquire_timeout: Duration::from_secs(5),
        jwt_secret: SecretString::from("sheets-test-jwt-secret-0123456789abcdef"),
        jwt_issuer: "zhimart-api".into(),
        jwt_audience: "zhimart-admin".into(),
        access_token_ttl: Duration::from_secs(900),
        session_ttl: Duration::from_secs(86_400),
        otp_pepper: SecretString::from("sheets-test-otp-pepper-0123456789abcdef"),
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
        // Pay-later checkout: orders go straight to pending_pack.
        payments_enabled: false,
        payment_test_mode: false,
        razorpay_key_id: String::new(),
        razorpay_key_secret: SecretString::from(""),
        razorpay_webhook_secret: SecretString::from(""),
        razorpay_api_base: "https://api.razorpay.com".into(),
        payment_expiry: Duration::from_secs(30 * 60),
        store_name: "Zhi-Mart Fireworks".into(),
        order_notifications_enabled: false,
        order_notification_emails: Vec::new(),
        public_site_url: None,
        customer_order_emails_enabled: false,
        pricing: Default::default(),
        google_sheets: Some(sheets_config(base)),
    }
}

struct H {
    app: Router,
    pool: PgPool,
    token: String,
    state: AppState,
    mock: Mock,
    base: String,
}

async fn setup(pool: PgPool) -> H {
    let (mock, base) = start_mock().await;
    let config = Arc::new(test_config(&base));
    let state = AppState::new(config, pool.clone(), Arc::new(NoopSender)).expect("state");
    assert!(state.sheets.is_some(), "sheets client built from config");

    sqlx::query("DELETE FROM products")
        .execute(&pool)
        .await
        .expect("clear catalogue");
    sqlx::query(
        "INSERT INTO products (id, name, category, brand, pack_info, price_paise, original_price_paise, image_url, stock) \
         VALUES ('sheet-pots', 'Flower Pots Special', 'GROUND', 'Zhi-mart', 'Box of 10', 3200, 21000, '/x.jpg', 1000), \
                ('sheet-wala', '1000 Wala', 'GARLAND', 'Zhi-mart', '1 Pc', 7200, 48000, '/y.jpg', 1000)",
    )
    .execute(&pool)
    .await
    .expect("products");

    let admin_id = Uuid::now_v7();
    sqlx::query("INSERT INTO admins (id, email, password_hash, display_name) VALUES ($1, 'ops@zhimart.com', 'unused', 'Ops')")
        .bind(admin_id)
        .execute(&pool)
        .await
        .expect("admin");
    let session_id = Uuid::now_v7();
    let expires: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO sessions (id, admin_id, absolute_expires_at) VALUES ($1, $2, now() + interval '1 day') \
         RETURNING absolute_expires_at",
    )
    .bind(session_id)
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .expect("session");
    let token = state
        .jwt
        .issue(admin_id, session_id, expires)
        .expect("jwt")
        .token;

    let app = Router::new()
        .nest(
            "/api/v1",
            routes::orders::public_routes().nest("/admin", routes::admin::orders::routes()),
        )
        .with_state(state.clone());
    H {
        app,
        pool,
        token,
        state,
        mock,
        base,
    }
}

impl H {
    async fn call(
        &self,
        method: Method,
        uri: &str,
        body: Value,
        auth: bool,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        }
        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).expect("request"))
            .await
            .expect("response");
        let status = res.status();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json")
        };
        (status, body)
    }

    /// Checkout (pay later). Returns (order id, order number).
    async fn place(&self, phone: &str, notes: Option<&str>) -> (Uuid, String) {
        let (status, body) = self
            .call(
                Method::POST,
                "/api/v1/orders",
                json!({
                    "idempotencyKey": Uuid::new_v4().to_string(),
                    "customer": {
                        "name": "Ravi Kumar",
                        "phone": phone,
                        "email": "ravi@example.com",
                        "address": "12, Gandhi Street, Mylapore",
                        "city": "Chennai",
                        "state": "Tamil Nadu",
                        "pincode": "600004"
                    },
                    "notes": notes,
                    "items": [
                        {"productId": "sheet-pots", "quantity": 2},
                        {"productId": "sheet-wala", "quantity": 1}
                    ]
                }),
                false,
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let number = body["order"]["orderNumber"]
            .as_str()
            .expect("number")
            .to_owned();
        let id = sqlx::query_scalar("SELECT id FROM orders WHERE order_number = $1")
            .bind(&number)
            .fetch_one(&self.pool)
            .await
            .expect("id");
        (id, number)
    }

    async fn version(&self, id: Uuid) -> i32 {
        sqlx::query_scalar("SELECT version FROM orders WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("version")
    }

    async fn admin_post(&self, uri: &str, body: Value) {
        let (status, body) = self.call(Method::POST, uri, body, true).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    }

    async fn sync(&self) -> SheetSyncRun {
        let client = self.state.sheets.as_ref().expect("client");
        process_sheet_sync_once(&self.pool, client)
            .await
            .expect("sync")
    }

    async fn queued(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM sheet_sync_queue")
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }

    /// Make every queued row due now (skip backoff in tests).
    async fn make_due(&self) {
        sqlx::query("UPDATE sheet_sync_queue SET next_attempt_at = now()")
            .execute(&self.pool)
            .await
            .expect("due");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn order_lifecycle_appends_once_then_updates_in_place(pool: PgPool) {
    let h = setup(pool).await;

    // Place → one append (tab + header created on first use).
    let (id, number) = h
        .place("9840011111", Some("=HYPERLINK(\"http://x\",\"y\")"))
        .await;
    assert_eq!(h.queued().await, 1, "trigger queued the new order");
    let run = h.sync().await;
    assert_eq!(
        run,
        SheetSyncRun {
            claimed: 1,
            appended: 1,
            updated: 0,
            retried: 0,
            requeued: 0
        }
    );
    assert_eq!(h.queued().await, 0);
    assert_eq!(h.mock.order_numbers(), vec![number.clone()]);
    h.mock.with(|m| {
        assert!(m.tab_exists);
        assert_eq!(m.rows[0][0], "Order No", "header row written");
        assert_eq!(m.rows[0].len(), HEADER.len());
        assert_eq!(m.appends, 1);
        assert_eq!(m.batch_updates, 0);
    });
    let row = h.mock.row_of(&number);
    assert_eq!(row.len(), HEADER.len());
    assert_eq!(cell(&row, "Customer Name"), "Ravi Kumar");
    assert_eq!(cell(&row, "Phone"), "+919840011111");
    assert_eq!(cell(&row, "WhatsApp"), "https://wa.me/919840011111");
    // Standard tier (list ₹32 / ₹72): MRP 210 × 80 % off → ₹42; MRP 480 → ₹96.
    assert_eq!(
        cell(&row, "Items"),
        "Flower Pots Special × 2 @ ₹42; 1000 Wala × 1 @ ₹96"
    );
    assert_eq!(cell(&row, "Total Units"), 3);
    assert_eq!(cell(&row, "Subtotal"), 180);
    assert_eq!(cell(&row, "Delivery Charge"), 500);
    assert_eq!(cell(&row, "Total Payable"), 680);
    assert_eq!(cell(&row, "Payment Status"), "Unpaid");
    assert_eq!(cell(&row, "Amount Paid"), 0);
    assert_eq!(cell(&row, "Order Status"), "Pending Pack");
    assert_eq!(
        cell(&row, "Customer Notes"),
        "=HYPERLINK(\"http://x\",\"y\")",
        "stored as literal text (RAW)"
    );
    assert_eq!(cell(&row, "Test Order"), "No");

    // Nothing queued ⇒ a tick makes no Google calls at all.
    let tokens = h.mock.with(|m| m.tokens_issued);
    assert_eq!(h.sync().await, SheetSyncRun::default());
    assert_eq!(h.mock.with(|m| m.tokens_issued), tokens);

    // Record a payment → the same row is updated, not appended.
    let v = h.version(id).await;
    h.admin_post(
        &format!("/api/v1/admin/orders/{id}/payment"),
        json!({"action": "mark_paid", "method": "upi", "reference": "UTR123", "expectedVersion": v}),
    )
    .await;
    let run = h.sync().await;
    assert_eq!((run.appended, run.updated), (0, 1));
    assert_eq!(h.mock.order_numbers(), vec![number.clone()]);
    let row = h.mock.row_of(&number);
    assert_eq!(cell(&row, "Payment Status"), "Paid");
    assert_eq!(cell(&row, "Amount Paid"), 680);
    assert_eq!(cell(&row, "Payment Method / Reference"), "UPI · Ref UTR123");

    // Dispatch with courier + LR → updated in place.
    let v = h.version(id).await;
    h.admin_post(
        &format!("/api/v1/admin/orders/{id}/status"),
        json!({"status": "dispatched", "transportPartner": "KPN Parcel", "docketNumber": "LR-7781", "expectedVersion": v}),
    )
    .await;
    let run = h.sync().await;
    assert_eq!((run.appended, run.updated), (0, 1));
    let row = h.mock.row_of(&number);
    assert_eq!(cell(&row, "Order Status"), "Dispatched");
    assert_eq!(cell(&row, "Courier"), "KPN Parcel");
    assert_eq!(cell(&row, "LR / Docket No"), "LR-7781");

    // A second order, cancelled → appended once, then updated in place.
    let (id2, number2) = h.place("9840022222", None).await;
    assert_eq!(h.sync().await.appended, 1);
    h.admin_post(
        &format!("/api/v1/admin/orders/{id2}/cancel"),
        json!({"expectedVersion": 1, "reason": "Customer changed mind"}),
    )
    .await;
    let run = h.sync().await;
    assert_eq!((run.appended, run.updated), (0, 1));
    assert_eq!(
        h.mock.order_numbers(),
        vec![number.clone(), number2.clone()]
    );
    assert!(
        cell(&h.mock.row_of(&number2), "Order Status")
            .as_str()
            .expect("status")
            .starts_with("Cancelled"),
    );

    // Someone sorts the sheet: rows are found by order number, not position.
    h.mock.with(|m| m.rows.swap(1, 2));
    let v = h.version(id).await;
    h.admin_post(
        &format!("/api/v1/admin/orders/{id}/status"),
        json!({"status": "in_transit", "transportPartner": "KPN Parcel", "docketNumber": "LR-7781", "expectedVersion": v}),
    )
    .await;
    assert_eq!(h.sync().await.updated, 1);
    assert_eq!(
        h.mock.order_numbers(),
        vec![number2.clone(), number.clone()]
    );
    assert_eq!(cell(&h.mock.row_of(&number), "Order Status"), "In Transit");
    assert!(
        cell(&h.mock.row_of(&number2), "Order Status")
            .as_str()
            .expect("status")
            .starts_with("Cancelled"),
        "the other order's row is untouched"
    );

    // Someone deletes the order's row → the next change re-appends it.
    h.mock.with(|m| {
        m.rows.remove(2);
    });
    assert_eq!(h.mock.order_numbers(), vec![number2.clone()]);
    let v = h.version(id).await;
    h.admin_post(
        &format!("/api/v1/admin/orders/{id}/status"),
        json!({"status": "delivered", "transportPartner": "KPN Parcel", "docketNumber": "LR-7781", "expectedVersion": v}),
    )
    .await;
    let run = h.sync().await;
    assert_eq!((run.appended, run.updated), (1, 0));
    assert_eq!(h.mock.order_numbers(), vec![number2, number.clone()]);
    assert_eq!(cell(&h.mock.row_of(&number), "Order Status"), "Delivered");

    h.mock.with(|m| {
        assert_eq!(
            m.bad_write_options, 0,
            "every write RAW, appends INSERT_ROWS"
        );
        assert_eq!(m.tokens_issued, 1, "access token cached across batches");
        assert_eq!(m.unauthorized, 0);
    });
}

#[sqlx::test(migrations = "./migrations")]
async fn google_down_never_affects_checkout_and_rows_are_retried(pool: PgPool) {
    let h = setup(pool).await;
    h.mock.with(|m| m.fail_status = Some(500));

    // Checkout succeeds (it never talks to Google) and the order is queued.
    let started = std::time::Instant::now();
    let (_, number) = h.place("9840033333", None).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(h.queued().await, 1);

    let run = h.sync().await;
    assert_eq!((run.claimed, run.retried, run.appended), (1, 1, 0));
    let (attempts, wait_secs, error): (i32, f64, Option<String>) = sqlx::query_as(
        "SELECT attempts, EXTRACT(EPOCH FROM next_attempt_at - now())::float8, last_error FROM sheet_sync_queue",
    )
    .fetch_one(&h.pool)
    .await
    .expect("queue row");
    assert_eq!(attempts, 1);
    assert!(
        (20.0..=31.0).contains(&wait_secs),
        "30 s backoff, got {wait_secs}"
    );
    assert!(error.unwrap_or_default().starts_with("transient:"));

    // Not due yet: nothing is claimed.
    assert_eq!(h.sync().await.claimed, 0);

    // Second failure doubles the backoff.
    h.make_due().await;
    h.sync().await;
    let wait_secs: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM next_attempt_at - now())::float8 FROM sheet_sync_queue",
    )
    .fetch_one(&h.pool)
    .await
    .expect("queue row");
    assert!(
        (50.0..=61.0).contains(&wait_secs),
        "60 s backoff, got {wait_secs}"
    );

    // Google recovers → the order is appended and the queue drains.
    h.mock.with(|m| m.fail_status = None);
    h.make_due().await;
    let run = h.sync().await;
    assert_eq!((run.appended, run.retried), (1, 0));
    assert_eq!(h.queued().await, 0);
    assert_eq!(h.mock.order_numbers(), vec![number]);
}

#[sqlx::test(migrations = "./migrations")]
async fn revoked_refresh_token_keeps_rows_queued(pool: PgPool) {
    let h = setup(pool).await;
    h.mock.with(|m| m.token_error = Some("invalid_grant"));
    let (_, number) = h.place("9840044444", None).await;

    let run = h.sync().await;
    assert_eq!(run.retried, 1);
    let (wait_secs, error): (f64, Option<String>) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM next_attempt_at - now())::float8, last_error FROM sheet_sync_queue",
    )
    .fetch_one(&h.pool)
    .await
    .expect("still queued");
    assert!(
        wait_secs > 9.0 * 60.0,
        "long backoff for errors that need a human"
    );
    assert!(error.unwrap_or_default().contains("invalid_grant"));
    assert_eq!(h.mock.with(|m| m.tokens_issued), 0);

    // A new refresh token (simulated: the endpoint accepts again) drains it.
    h.mock.with(|m| m.token_error = None);
    h.make_due().await;
    assert_eq!(h.sync().await.appended, 1);
    assert_eq!(h.mock.order_numbers(), vec![number]);
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_access_token_is_refreshed_once(pool: PgPool) {
    let h = setup(pool).await;
    h.place("9840055555", None).await;
    assert_eq!(h.sync().await.appended, 1);
    assert_eq!(h.mock.with(|m| m.tokens_issued), 1);

    // Google stops accepting the cached token (expired/revoked early).
    h.mock
        .with(|m| m.valid_token = Some("something-else".into()));
    h.place("9840066666", None).await;
    let run = h.sync().await;
    assert_eq!(run.appended, 1, "{run:?}");
    h.mock.with(|m| {
        assert_eq!(m.unauthorized, 1, "one 401, then a refreshed token");
        assert_eq!(m.tokens_issued, 2);
    });
}

#[sqlx::test(migrations = "./migrations")]
async fn sheet_access_errors_back_off_long(pool: PgPool) {
    let h = setup(pool).await;
    h.mock.with(|m| m.fail_status = Some(403));
    h.place("9840077777", None).await;
    assert_eq!(h.sync().await.retried, 1);
    let (wait_secs, error): (f64, Option<String>) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM next_attempt_at - now())::float8, last_error FROM sheet_sync_queue",
    )
    .fetch_one(&h.pool)
    .await
    .expect("queued");
    assert!(wait_secs > 9.0 * 60.0, "{wait_secs}");
    assert!(error.unwrap_or_default().starts_with("no access:"));
}

#[sqlx::test(migrations = "./migrations")]
async fn change_during_sync_is_not_lost(pool: PgPool) {
    let h = setup(pool).await;
    let (id, _) = h.place("9840088888", None).await;

    // Worker claims revision 1, then the order changes before it finishes.
    let items = repo::sheet_sync::claim_due(&h.pool, 10)
        .await
        .expect("claim");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].revision, 1);
    // Re-dirtying a leased row must not make it claimable by another worker.
    sqlx::query("UPDATE orders SET payment_status = 'paid', paid_at = now() WHERE id = $1")
        .bind(id)
        .execute(&h.pool)
        .await
        .expect("change");
    assert!(
        repo::sheet_sync::claim_due(&h.pool, 10)
            .await
            .expect("claim")
            .is_empty(),
        "still leased"
    );

    // Compare-and-clear fails: the row stays queued and is due immediately.
    assert!(
        !repo::sheet_sync::complete(&h.pool, &items[0])
            .await
            .expect("complete")
    );
    let (revision, attempts): (i64, i32) =
        sqlx::query_as("SELECT revision, attempts FROM sheet_sync_queue WHERE order_id = $1")
            .bind(id)
            .fetch_one(&h.pool)
            .await
            .expect("still queued");
    assert_eq!((revision, attempts), (2, 0));
    let run = h.sync().await;
    assert_eq!((run.claimed, run.appended), (1, 1));
    assert_eq!(
        cell(&h.mock.row_of(&h.mock.order_numbers()[0]), "Payment Status"),
        "Paid"
    );
    assert_eq!(h.queued().await, 0);

    // A no-op UPDATE does not re-queue.
    sqlx::query("UPDATE orders SET notes = notes WHERE id = $1")
        .bind(id)
        .execute(&h.pool)
        .await
        .expect("noop");
    assert_eq!(h.queued().await, 0);
}

/// Orders that exist when the migration runs are never synced, even when
/// they change later. Migrations are applied in two steps around them.
#[sqlx::test(migrations = false)]
async fn orders_before_the_migration_are_not_backfilled(pool: PgPool) {
    let mut before = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .expect("migrator");
    before.migrations = Cow::Owned(
        before
            .migrations
            .iter()
            .filter(|m| m.version < SHEETS_MIGRATION)
            .cloned()
            .collect(),
    );
    before
        .run(&pool)
        .await
        .expect("migrations before the sheet sync");

    sqlx::query(
        "INSERT INTO products (id, name, category, brand, pack_info, price_paise, original_price_paise, image_url, stock) \
         VALUES ('old-prod', 'Old Product', 'GROUND', 'Zhi-mart', 'Box', 10000, 20000, '/x.jpg', 10)",
    )
    .execute(&pool)
    .await
    .expect("product");
    let old_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO orders (id, order_number, idempotency_key, customer_name, customer_phone, address_line, \
                             city, state, pincode, item_count, mrp_total_paise, total_paise) \
         VALUES ($1, 'ZM-OLD-0001', $2, 'Old Customer', '+919840099999', '1 Old Street', 'Sivakasi', \
                 'Tamil Nadu', '626123', 1, 20000, 10000)",
    )
    .bind(old_id)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .expect("old order");

    zhimart_backend::cli::MIGRATOR
        .run(&pool)
        .await
        .expect("remaining migrations");

    let h = setup(pool).await;
    // Changing the old order does not queue it.
    sqlx::query("UPDATE orders SET payment_status = 'paid', paid_at = now(), version = version + 1 WHERE id = $1")
        .bind(old_id)
        .execute(&h.pool)
        .await
        .expect("update old");
    assert_eq!(h.queued().await, 0);
    let flag: bool = sqlx::query_scalar("SELECT sync_to_sheet FROM orders WHERE id = $1")
        .bind(old_id)
        .fetch_one(&h.pool)
        .await
        .expect("flag");
    assert!(!flag);

    // New orders are.
    let (_, number) = h.place("9840010101", None).await;
    assert_eq!(h.queued().await, 1);
    assert_eq!(h.sync().await.appended, 1);
    assert_eq!(h.mock.order_numbers(), vec![number]);
    assert!(!h.base.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn sheets_check_creates_tab_and_header(pool: PgPool) {
    let h = setup(pool).await;
    let client = SheetsClient::new(&sheets_config(&h.base)).expect("client");
    let report = client.ensure_ready().await.expect("ready");
    assert_eq!(report.spreadsheet_title, "Crackers-Ordering-Leads");
    assert!(report.tab_created && report.header_written);
    assert_eq!(report.order_rows, 0);
    let again = client.ensure_ready().await.expect("ready");
    assert!(!again.tab_created && !again.header_written);
    h.mock.with(|m| {
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.bad_write_options, 0);
    });
}
