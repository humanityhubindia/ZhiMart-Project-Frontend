//! Admin order-notification outbox worker (`jobs::process_outbox_once`)
//! against a real PostgreSQL database. Each test gets a fresh database from
//! `#[sqlx::test]` (requires `DATABASE_URL` with CREATEDB rights).

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;
use zhimart_backend::{
    config::{AppEnv, Config, EmailProvider},
    email::{BoxFuture, EmailSender, OtpEmail, OutboundEmail},
    jobs::{OutboxRun, process_outbox_once},
    payments::order_token,
    repo::outbox::{
        self, KIND_ADMIN_ORDER_NOTIFICATION, KIND_CUSTOMER_ORDER_CONFIRMATION, MAX_ATTEMPTS,
    },
};

/// (to, subject, html, text) of one captured email.
type Sent = (Vec<String>, String, String, String);

#[derive(Default)]
struct Capture {
    sent: Mutex<Vec<Sent>>,
    fail: bool,
}

impl EmailSender for Capture {
    fn send_otp<'a>(&'a self, _email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn send_email<'a>(&'a self, email: &'a OutboundEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            if self.fail {
                anyhow::bail!("simulated provider outage for admin@example.com");
            }
            self.sent.lock().unwrap_or_else(|e| e.into_inner()).push((
                email.to.clone(),
                email.subject.clone(),
                email.html.clone(),
                email.text.clone(),
            ));
            Ok(())
        })
    }
}

impl Capture {
    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn config(enabled: bool, explicit: &[&str]) -> Config {
    Config {
        app_env: AppEnv::Development,
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        database_url: SecretString::from("postgres://unused"),
        db_max_connections: 5,
        db_min_connections: 0,
        db_acquire_timeout: Duration::from_secs(5),
        jwt_secret: SecretString::from("outbox-test-jwt-secret-0123456789abcdef"),
        jwt_issuer: "zhimart-api".into(),
        jwt_audience: "zhimart-admin".into(),
        access_token_ttl: Duration::from_secs(900),
        session_ttl: Duration::from_secs(86_400),
        otp_pepper: SecretString::from("outbox-test-otp-pepper-0123456789abcdef"),
        otp_ttl: Duration::from_secs(300),
        otp_max_attempts: 5,
        otp_resend_cooldown: Duration::from_secs(60),
        otp_max_resends: 3,
        login_max_failures: 5,
        login_lockout: Duration::from_secs(900),
        cookie_secure: false,
        allowed_origins: vec![],
        trusted_proxies: vec![],
        rate_limit_global_per_minute: 300,
        rate_limit_auth_per_minute: 10,
        request_timeout: Duration::from_secs(15),
        max_body_bytes: 65_536,
        max_concurrent_requests: 1024,
        email_provider: EmailProvider::Log,
        ses_from_address: String::new(),
        ses_configuration_set: None,
        payments_enabled: false,
        payment_test_mode: false,
        cashfree_env: zhimart_backend::config::CashfreeEnv::Sandbox,
        cashfree_app_id: String::new(),
        cashfree_secret_key: SecretString::from(""),
        cashfree_api_base: "https://sandbox.cashfree.com/pg".into(),
        cashfree_notify_url: None,
        payment_expiry: Duration::from_secs(30 * 60),
        store_name: "Zhi-Mart Fireworks".into(),
        order_notifications_enabled: enabled,
        order_notification_emails: explicit.iter().map(|s| (*s).to_owned()).collect(),
        public_site_url: Some("https://zhimart.example".into()),
        customer_order_emails_enabled: false,
        pricing: Default::default(),
        google_sheets: None,
    }
}

/// Two admins (one inactive), one product, one order with a hostile customer
/// name, and its queued notification. Returns the order id.
async fn fixture(pool: &PgPool) -> Uuid {
    for (email, active) in [
        ("owner@zhimart.example", true),
        ("former@zhimart.example", false),
    ] {
        sqlx::query(
            "INSERT INTO admins (id, email, password_hash, display_name, is_active) VALUES ($1, $2, 'x', 'A', $3)",
        )
        .bind(Uuid::now_v7())
        .bind(email)
        .bind(active)
        .execute(pool)
        .await
        .expect("admin");
    }
    sqlx::query(
        "INSERT INTO products (id, name, category, brand, pack_info, price_paise, original_price_paise, image_url, stock) \
         VALUES ('outbox-test-1', 'Flower Pots', 'GROUND', 'Zhi-mart', 'Box of 10', 16500, 30000, '/x.jpg', 100)",
    )
    .execute(pool)
    .await
    .expect("product");

    let order_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO orders (id, order_number, idempotency_key, customer_name, customer_phone, address_line, \
                             city, state, pincode, item_count, mrp_total_paise, total_paise) \
         VALUES ($1, 'ZM-TEST-0001', $2, '<script>x</script> Ravi', '+919840012345', '12 Gandhi Street', \
                 'Sivakasi', 'Tamil Nadu', '626123', 2, 60000, 33000)",
    )
    .bind(order_id)
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .expect("order");
    sqlx::query(
        "INSERT INTO order_items (order_id, product_id, product_name, pack_info, unit_price_paise, unit_mrp_paise, \
                                  quantity, line_total_paise) \
         VALUES ($1, 'outbox-test-1', 'Flower Pots', 'Box of 10', 16500, 30000, 2, 33000)",
    )
    .bind(order_id)
    .execute(pool)
    .await
    .expect("item");

    outbox::enqueue_admin_order_notification(pool, order_id)
        .await
        .expect("enqueue");
    // Idempotent per order.
    outbox::enqueue_admin_order_notification(pool, order_id)
        .await
        .expect("enqueue again");
    order_id
}

async fn row(pool: &PgPool, order_id: Uuid) -> (String, i32, Option<String>) {
    sqlx::query_as("SELECT status, attempts, last_error FROM email_outbox WHERE order_id = $1")
        .bind(order_id)
        .fetch_one(pool)
        .await
        .expect("outbox row")
}

#[sqlx::test(migrations = "./migrations")]
async fn sends_once_to_active_admins(pool: PgPool) {
    let order_id = fixture(&pool).await;
    let sender = Capture::default();
    let cfg = config(true, &[]);

    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!(
        run,
        OutboxRun {
            claimed: 1,
            sent: 1,
            retried: 0,
            failed: 0,
            deferred: 0
        }
    );

    let sent = sender.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].0,
        vec!["owner@zhimart.example".to_owned()],
        "inactive admin excluded"
    );
    assert!(sent[0].1.starts_with("New order ZM-TEST-0001 — ₹330 from "));
    assert!(!sent[0].2.contains("<script>"), "customer data escaped");
    assert_eq!(row(&pool, order_id).await.0, "sent");

    // Nothing left to send.
    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!(run.claimed, 0);
    assert_eq!(sender.sent().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn explicit_recipients_override_admins(pool: PgPool) {
    fixture(&pool).await;
    let sender = Capture::default();
    let cfg = config(true, &["orders@zhimart.example", "owner2@zhimart.example"]);
    process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!(
        sender.sent()[0].0,
        vec![
            "orders@zhimart.example".to_owned(),
            "owner2@zhimart.example".to_owned()
        ]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn provider_failure_backs_off_then_parks_as_failed(pool: PgPool) {
    let order_id = fixture(&pool).await;
    let sender = Capture {
        fail: true,
        ..Capture::default()
    };
    let cfg = config(true, &[]);

    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!(run.retried, 1);
    let (status, attempts, error) = row(&pool, order_id).await;
    assert_eq!((status.as_str(), attempts), ("pending", 1));
    let error = error.expect("last_error");
    assert!(!error.contains('@'), "no PII in last_error: {error}");

    // Backoff: not due again immediately.
    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!(run.claimed, 0, "row is backing off");
    let backoff: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM next_attempt_at - now())::float8 FROM email_outbox WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_one(&pool)
    .await
    .expect("backoff");
    assert!(
        (20.0..=31.0).contains(&backoff),
        "first backoff ≈30 s, got {backoff}"
    );

    for _ in 1..MAX_ATTEMPTS {
        sqlx::query("UPDATE email_outbox SET next_attempt_at = now() WHERE order_id = $1")
            .bind(order_id)
            .execute(&pool)
            .await
            .expect("make due");
        process_outbox_once(&pool, &cfg, &sender)
            .await
            .expect("run");
    }
    let (status, attempts, _) = row(&pool, order_id).await;
    assert_eq!((status.as_str(), attempts), ("failed", MAX_ATTEMPTS));
    assert!(sender.sent().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn no_recipients_retries(pool: PgPool) {
    let order_id = fixture(&pool).await;
    sqlx::query("UPDATE admins SET is_active = FALSE")
        .execute(&pool)
        .await
        .expect("deactivate");
    let sender = Capture::default();
    let run = process_outbox_once(&pool, &config(true, &[]), &sender)
        .await
        .expect("run");
    assert_eq!(run.retried, 1);
    let (status, _, error) = row(&pool, order_id).await;
    assert_eq!(status, "pending");
    assert!(error.unwrap_or_default().contains("no recipients"));
    assert!(sender.sent().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_leaves_outbox_untouched(pool: PgPool) {
    let order_id = fixture(&pool).await;
    let sender = Capture::default();
    let run = process_outbox_once(&pool, &config(false, &[]), &sender)
        .await
        .expect("run");
    assert_eq!(run, OutboxRun::default());
    assert_eq!(row(&pool, order_id).await, ("pending".to_owned(), 0, None));
    assert!(sender.sent().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn deleted_order_is_marked_failed(pool: PgPool) {
    let order_id = fixture(&pool).await;
    // Simulate a row whose order vanished (FK is ON DELETE CASCADE, so detach it instead).
    let orphan = Uuid::now_v7();
    sqlx::query("INSERT INTO email_outbox (id, kind, order_id) VALUES ($1, $2, NULL)")
        .bind(orphan)
        .bind(outbox::KIND_ADMIN_ORDER_NOTIFICATION)
        .execute(&pool)
        .await
        .expect("orphan row");
    let sender = Capture::default();
    let run = process_outbox_once(&pool, &config(true, &[]), &sender)
        .await
        .expect("run");
    assert_eq!((run.sent, run.failed), (1, 1));
    let status: String = sqlx::query_scalar("SELECT status FROM email_outbox WHERE id = $1")
        .bind(orphan)
        .fetch_one(&pool)
        .await
        .expect("status");
    assert_eq!(status, "failed");
    assert_eq!(row(&pool, order_id).await.0, "sent");
}

/// Two workers racing on the same rows never double-send (SKIP LOCKED + lease).
#[sqlx::test(migrations = "./migrations")]
async fn concurrent_workers_do_not_double_send(pool: PgPool) {
    fixture(&pool).await;
    let sender = Arc::new(Capture::default());
    let cfg = Arc::new(config(true, &[]));
    let runs = futures_join(&pool, &cfg, &sender).await;
    assert_eq!(runs.iter().map(|r| r.sent).sum::<usize>(), 1);
    assert_eq!(sender.sent().len(), 1);
}

async fn futures_join(pool: &PgPool, cfg: &Arc<Config>, sender: &Arc<Capture>) -> Vec<OutboxRun> {
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (pool, cfg, sender) = (pool.clone(), Arc::clone(cfg), Arc::clone(sender));
        handles.push(tokio::spawn(async move {
            process_outbox_once(&pool, &cfg, sender.as_ref())
                .await
                .expect("run")
        }));
    }
    let mut runs = Vec::new();
    for h in handles {
        runs.push(h.await.expect("join"));
    }
    runs
}

// ---------------------------------------------------------------------------
// Customer order confirmation
// ---------------------------------------------------------------------------

fn both_enabled() -> Config {
    let mut cfg = config(true, &[]);
    cfg.customer_order_emails_enabled = true;
    cfg
}

/// Fixture order with a customer email and BOTH kinds queued (as producers do).
async fn fixture_with_customer(pool: &PgPool, cfg: &Config, email: Option<&str>) -> Uuid {
    let order_id = fixture(pool).await;
    sqlx::query("UPDATE orders SET customer_email = $2 WHERE id = $1")
        .bind(order_id)
        .bind(email)
        .execute(pool)
        .await
        .expect("set email");
    outbox::enqueue_order_emails(pool, cfg, order_id)
        .await
        .expect("enqueue both");
    order_id
}

async fn kind_status(pool: &PgPool, order_id: Uuid, kind: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT status, last_error FROM email_outbox WHERE order_id = $1 AND kind = $2")
        .bind(order_id)
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("row")
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_and_customer_emails_go_to_the_right_people(pool: PgPool) {
    let cfg = both_enabled();
    let order_id = fixture_with_customer(&pool, &cfg, Some("ravi@customer.example")).await;
    let sender = Capture::default();

    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!((run.claimed, run.sent), (2, 2));

    let sent = sender.sent();
    let admin = sent
        .iter()
        .find(|m| m.1.starts_with("New order"))
        .expect("admin email");
    assert_eq!(admin.0, vec!["owner@zhimart.example".to_owned()]);
    let customer = sent
        .iter()
        .find(|m| m.1.starts_with("Your Zhi-Mart order"))
        .expect("customer email");
    assert_eq!(customer.1, "Your Zhi-Mart order ZM-TEST-0001 is confirmed");
    assert_eq!(
        customer.0,
        vec!["ravi@customer.example".to_owned()],
        "customer only"
    );
    assert!(!customer.2.contains("<script>"));

    // The tracking link carries a token that verifies for this order.
    let prefix = "https://zhimart.example/?page=order&order=ZM-TEST-0001#t=";
    let start = customer.3.find(prefix).expect("tracking link in text") + prefix.len();
    let token: String = customer.3[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    assert!(
        order_token::verify(&cfg, order_id, &token),
        "token verifies"
    );

    assert_eq!(
        kind_status(&pool, order_id, KIND_ADMIN_ORDER_NOTIFICATION)
            .await
            .0,
        "sent"
    );
    assert_eq!(
        kind_status(&pool, order_id, KIND_CUSTOMER_ORDER_CONFIRMATION)
            .await
            .0,
        "sent"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_without_email_is_failed(pool: PgPool) {
    let cfg = both_enabled();
    let order_id = fixture_with_customer(&pool, &cfg, None).await;
    let sender = Capture::default();
    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!((run.sent, run.failed), (1, 1));
    let (status, error) = kind_status(&pool, order_id, KIND_CUSTOMER_ORDER_CONFIRMATION).await;
    assert_eq!(status, "failed");
    assert_eq!(error.as_deref(), Some("no customer email"));
    assert_eq!(sender.sent().len(), 1, "admin email still sent");
}

#[sqlx::test(migrations = "./migrations")]
async fn disabled_kind_is_deferred_not_spent(pool: PgPool) {
    let cfg = both_enabled();
    let order_id = fixture_with_customer(&pool, &cfg, Some("ravi@customer.example")).await;
    // Customer emails switched off after the rows were queued.
    let mut off = both_enabled();
    off.customer_order_emails_enabled = false;
    let sender = Capture::default();
    let run = process_outbox_once(&pool, &off, &sender)
        .await
        .expect("run");
    assert_eq!((run.sent, run.deferred), (1, 1));

    let (status, attempts, _): (String, i32, Option<String>) = sqlx::query_as(
        "SELECT status, attempts, last_error FROM email_outbox WHERE order_id = $1 AND kind = $2",
    )
    .bind(order_id)
    .bind(KIND_CUSTOMER_ORDER_CONFIRMATION)
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(
        (status.as_str(), attempts),
        ("pending", 0),
        "attempt not spent"
    );

    // Re-enabled: it goes out once it is due again.
    sqlx::query("UPDATE email_outbox SET next_attempt_at = now() WHERE order_id = $1")
        .bind(order_id)
        .execute(&pool)
        .await
        .expect("make due");
    let run = process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    assert_eq!(run.sent, 1);
    assert_eq!(
        sender.sent().last().expect("sent").0,
        vec!["ravi@customer.example".to_owned()]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_send_errors_never_store_addresses(pool: PgPool) {
    let cfg = both_enabled();
    let order_id = fixture_with_customer(&pool, &cfg, Some("ravi@customer.example")).await;
    let sender = Capture {
        fail: true,
        ..Capture::default()
    };
    process_outbox_once(&pool, &cfg, &sender)
        .await
        .expect("run");
    let (status, error) = kind_status(&pool, order_id, KIND_CUSTOMER_ORDER_CONFIRMATION).await;
    assert_eq!(status, "pending");
    assert!(!error.unwrap_or_default().contains('@'));
}
