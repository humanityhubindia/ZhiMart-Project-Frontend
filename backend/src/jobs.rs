//! Background jobs: admin order-notification outbox, Google Sheets order sync,
//! unpaid-order expiry, database retention purge, and rate-limiter memory GC. All loops exit
//! promptly when the shutdown signal flips to `true`, and all are safe to run
//! on every replica (`SKIP LOCKED` + leases in the repo functions).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use rand::Rng;
use sqlx::PgPool;
use tokio::{sync::watch, task::JoinHandle, time::MissedTickBehavior};
use uuid::Uuid;

use crate::{
    config::Config,
    email::{EmailSender, OutboundEmail, templates},
    middleware::rate_limit::RateLimiters,
    payments,
    repo::{
        self,
        orders::{Order, OrderRef},
        outbox::{KIND_ADMIN_ORDER_NOTIFICATION, KIND_CUSTOMER_ORDER_CONFIRMATION, OutboxItem},
        sheet_sync::SheetSyncItem,
    },
    sheets::{SheetsClient, SheetsError, row},
    state::AppState,
};

/// Outbox worker cadence. Notifications are near-real-time but not urgent.
const OUTBOX_INTERVAL: Duration = Duration::from_secs(5);
const OUTBOX_JITTER: Duration = Duration::from_secs(1);
const OUTBOX_INITIAL_DELAY: Duration = Duration::from_secs(3);
pub const OUTBOX_BATCH: i64 = 20;
/// Batches per tick before yielding to the next tick (200 emails).
const OUTBOX_MAX_BATCHES_PER_TICK: u32 = 10;
/// Upper bound on one provider call. Well inside the 120 s claim lease, so a
/// slow send never lets another worker claim the same row concurrently.
const OUTBOX_SEND_TIMEOUT: Duration = Duration::from_secs(15);

/// Google Sheets sync cadence: a new order reaches the sheet within ~10 s.
/// One batch per tick costs at most 3 Sheets requests whatever its size (read
/// column A, one batch update, one append), so even a backlog stays far below
/// Google's per-user quota (60 reads + 60 writes per minute).
const SHEETS_INTERVAL: Duration = Duration::from_secs(5);
const SHEETS_JITTER: Duration = Duration::from_secs(2);
const SHEETS_INITIAL_DELAY: Duration = Duration::from_secs(10);
pub const SHEETS_BATCH: i64 = 100;
/// Upper bound on one batch (token refresh + Sheets calls, 20 s each at most).
/// Well inside the 120 s claim lease, so a slow batch never lets another
/// worker claim the same orders concurrently (which could append twice).
const SHEETS_BATCH_TIMEOUT: Duration = Duration::from_secs(90);

/// Maintenance cadence (plus up to [`MAINTENANCE_JITTER`] so replicas spread out).
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(10 * 60);
const MAINTENANCE_JITTER: Duration = Duration::from_secs(60);
/// First run shortly after boot, once the server is taking traffic.
const MAINTENANCE_INITIAL_DELAY: Duration = Duration::from_secs(30);
/// A purge that takes longer than this is abandoned and retried next cycle.
const MAINTENANCE_TIMEOUT: Duration = Duration::from_secs(120);

/// Unpaid-order expiry sweep: releases stock reserved by `pending_payment`
/// orders whose payment window closed. Frequent, so stock comes back soon
/// after `PAYMENT_EXPIRY_MINUTES`; the repo function uses
/// `FOR UPDATE SKIP LOCKED`, so replicas never double-process an order.
const EXPIRY_INTERVAL: Duration = Duration::from_secs(30);
const EXPIRY_JITTER: Duration = Duration::from_secs(5);
const EXPIRY_INITIAL_DELAY: Duration = Duration::from_secs(5);
const EXPIRY_BATCH: i64 = 200;
/// Upper bound on batches per tick (10 000 orders), so a misbehaving repo
/// function can never spin this loop; the backlog continues next tick.
const EXPIRY_MAX_BATCHES_PER_TICK: u32 = 50;
const EXPIRY_BATCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Before cancelling, ask Cashfree about at most this many expired orders per
/// tick, within this much wall time (each check is one or two GETs of ≤ 10 s).
/// Anything beyond is cancelled unchecked: a late payment still revives or
/// refunds it, and the webhook re-triggers that.
const EXPIRY_RECONCILE_LIMIT: i64 = 100;
const EXPIRY_RECONCILE_BUDGET: Duration = Duration::from_secs(20);

const LIMITER_GC_INTERVAL: Duration = Duration::from_secs(60);
/// Log loudly if a limiter is tracking this many distinct keys after GC: it
/// means a wide distributed flood (or a broken TRUSTED_PROXIES setup).
const LIMITER_KEYS_WARN: usize = 250_000;

/// Spawn all background jobs. Await the returned handles after flipping the
/// shutdown sender to `true`.
pub fn spawn(
    state: &AppState,
    limiters: RateLimiters,
    shutdown: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let db = state.db.clone();
    let mut handles = vec![
        tokio::spawn(expiry_loop(state.clone(), shutdown.clone())),
        tokio::spawn(maintenance_loop(db.clone(), shutdown.clone())),
        tokio::spawn(limiter_gc_loop(limiters, shutdown.clone())),
    ];
    if state.config.order_notifications_enabled || state.config.customer_order_emails_enabled {
        handles.push(tokio::spawn(outbox_loop(
            db,
            Arc::clone(&state.config),
            Arc::clone(&state.email),
            shutdown.clone(),
        )));
    } else {
        tracing::info!("order emails disabled (admin and customer): outbox worker not started");
    }
    match &state.sheets {
        Some(sheets) => handles.push(tokio::spawn(sheet_sync_loop(
            state.db.clone(),
            Arc::clone(sheets),
            shutdown,
        ))),
        None => tracing::info!(
            "GOOGLE_SHEETS_ENABLED=false: Google Sheets sync worker not started (orders stay queued in sheet_sync_queue)"
        ),
    }
    handles
}

// ---------------------------------------------------------------------------
// Google Sheets order sync
// ---------------------------------------------------------------------------

/// What one [`process_sheet_sync_once`] call did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SheetSyncRun {
    pub claimed: usize,
    pub appended: usize,
    pub updated: usize,
    /// Rows left queued after a failed attempt (retried with backoff).
    pub retried: usize,
    /// Synced, but the order changed mid-sync: queued again for an immediate re-sync.
    pub requeued: usize,
}

/// Delay before retrying a failed batch. Transient errors (network, 429, 5xx)
/// back off 30 s × 2^(n-1) up to 30 min; errors that need a human (revoked
/// token, no access, bad request) start at 10 min and cap at 1 h, so a broken
/// setup costs a few requests and log lines per hour. Rows are never dropped.
pub fn sheets_retry_backoff(attempts: i32, err: &SheetsError) -> Duration {
    let n = u32::try_from((attempts - 1).clamp(0, 10)).unwrap_or(0);
    let secs = if err.is_transient() {
        (30u64 << n).min(30 * 60)
    } else {
        (600u64 << n).min(60 * 60)
    };
    Duration::from_secs(secs)
}

async fn sheet_sync_loop(
    db: PgPool,
    sheets: Arc<SheetsClient>,
    mut shutdown: watch::Receiver<bool>,
) {
    tracing::info!(
        spreadsheet_id = sheets.spreadsheet_id(),
        tab = sheets.tab(),
        "Google Sheets order sync enabled"
    );
    let mut delay = SHEETS_INITIAL_DELAY + jitter(SHEETS_JITTER);
    loop {
        tokio::select! {
            _ = shutdown_requested(&mut shutdown) => break,
            _ = tokio::time::sleep(delay) => {}
        }
        match process_sheet_sync_once(&db, &sheets).await {
            Ok(run) => {
                if run.appended + run.updated > 0 {
                    tracing::info!(
                        appended = run.appended,
                        updated = run.updated,
                        requeued = run.requeued,
                        "google sheets: orders synced"
                    );
                }
            }
            Err(e) => tracing::error!(error = %e, "google sheets: queue query failed"),
        }
        delay = SHEETS_INTERVAL + jitter(SHEETS_JITTER);
    }
    tracing::debug!("google sheets sync job stopped");
}

/// Claim one batch of dirty orders and write their current state to the
/// sheet (update in place, or append when the order number is not in column
/// A). On success each row is cleared only if the order did not change
/// meanwhile; on failure every claimed row is pushed back with backoff.
/// Delivery is at-least-once and idempotent: a repeat finds the row by
/// order number and rewrites it.
pub async fn process_sheet_sync_once(
    db: &PgPool,
    sheets: &SheetsClient,
) -> sqlx::Result<SheetSyncRun> {
    let mut run = SheetSyncRun::default();
    let items = repo::sheet_sync::claim_due(db, SHEETS_BATCH).await?;
    run.claimed = items.len();
    if items.is_empty() {
        return Ok(run);
    }

    let ids: Vec<Uuid> = items.iter().map(|i| i.order_id).collect();
    let mut orders = {
        // Released before any Google call.
        let mut conn = db.acquire().await?;
        repo::orders::load_many(&mut conn, &ids).await?
    };
    let found: HashSet<Uuid> = orders.iter().map(|o| o.id).collect();
    let gone: Vec<Uuid> = ids
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect();
    repo::sheet_sync::forget(db, &gone).await?;
    let items: Vec<&SheetSyncItem> = items
        .iter()
        .filter(|i| found.contains(&i.order_id))
        .collect();
    if items.is_empty() {
        return Ok(run);
    }
    // New rows land in the order the orders were placed.
    orders.sort_by(|a, b| (a.created_at, &a.order_number).cmp(&(b.created_at, &b.order_number)));
    let rows: Vec<_> = orders.iter().map(row::order_row).collect();

    let result = match tokio::time::timeout(SHEETS_BATCH_TIMEOUT, sheets.upsert_rows(&rows)).await {
        Ok(r) => r,
        Err(_) => Err(SheetsError::Transient("sync batch timed out".into())),
    };
    match result {
        Ok(outcome) => {
            run.appended = outcome.appended;
            run.updated = outcome.updated;
            for item in items {
                match repo::sheet_sync::complete(db, item).await {
                    Ok(true) => {}
                    Ok(false) => run.requeued += 1,
                    // The lease expires and the order is re-synced (an in-place update).
                    Err(e) => {
                        tracing::error!(order_id = %item.order_id, error = %e, "google sheets: clearing queue row failed")
                    }
                }
            }
        }
        Err(err) => {
            run.retried = items.len();
            let attempt = items.iter().map(|i| i.attempts).max().unwrap_or(1);
            if err.is_transient() {
                tracing::warn!(orders = items.len(), attempt, error = %err, hint = err.hint(), "google sheets: sync failed; will retry");
            } else {
                tracing::error!(orders = items.len(), attempt, error = %err, hint = err.hint(), "google sheets: sync failed; will retry");
            }
            let mut by_backoff: HashMap<Duration, Vec<Uuid>> = HashMap::new();
            for item in &items {
                by_backoff
                    .entry(sheets_retry_backoff(item.attempts, &err))
                    .or_default()
                    .push(item.order_id);
            }
            let summary = err.summary();
            for (backoff, ids) in by_backoff {
                repo::sheet_sync::mark_retry(db, &ids, backoff, &summary).await?;
            }
        }
    }
    Ok(run)
}

// ---------------------------------------------------------------------------
// Order email outbox (admin notification + customer confirmation)
// ---------------------------------------------------------------------------

/// A claimed row whose kind is switched off is pushed back by this much, with
/// the claim not counted as an attempt, so turning a kind back on resumes it.
const OUTBOX_DISABLED_DEFER_SECS: f64 = 15.0 * 60.0;

/// What one [`process_outbox_once`] call did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OutboxRun {
    pub claimed: usize,
    pub sent: usize,
    pub retried: usize,
    pub failed: usize,
    /// Rows of a kind that is currently disabled, released untouched.
    pub deferred: usize,
}

enum Outcome {
    Sent,
    /// Transient: retry with backoff (`mark_retry`; parked after MAX_ATTEMPTS).
    Retry(&'static str),
    /// Permanent: will never succeed.
    Failed(&'static str),
    /// Kind disabled by config: release the claim without spending an attempt.
    Deferred,
}

async fn outbox_loop(
    db: PgPool,
    config: Arc<Config>,
    email: Arc<dyn EmailSender>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut delay = OUTBOX_INITIAL_DELAY + jitter(OUTBOX_JITTER);
    loop {
        tokio::select! {
            _ = shutdown_requested(&mut shutdown) => break,
            _ = tokio::time::sleep(delay) => {}
        }
        for _ in 0..OUTBOX_MAX_BATCHES_PER_TICK {
            if *shutdown.borrow() {
                break;
            }
            match process_outbox_once(&db, &config, email.as_ref()).await {
                Ok(run) => {
                    if run.sent + run.failed + run.retried > 0 {
                        tracing::info!(
                            sent = run.sent,
                            retried = run.retried,
                            failed = run.failed,
                            deferred = run.deferred,
                            "order emails processed"
                        );
                    }
                    if run.claimed < usize::try_from(OUTBOX_BATCH).unwrap_or(usize::MAX) {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "order notifications: outbox query failed");
                    break;
                }
            }
        }
        delay = OUTBOX_INTERVAL + jitter(OUTBOX_JITTER);
    }
    tracing::debug!("order notification job stopped");
}

/// Claim and process one batch of due outbox rows. A no-op when both
/// `ORDER_NOTIFICATION_ENABLED` and `CUSTOMER_ORDER_EMAIL_ENABLED` are false;
/// rows of a single disabled kind are left pending (deferred). Delivery is
/// at-least-once: a crash between a successful send and `mark_sent` resends
/// after the claim lease.
pub async fn process_outbox_once(
    db: &PgPool,
    config: &Config,
    email: &dyn EmailSender,
) -> sqlx::Result<OutboxRun> {
    let mut run = OutboxRun::default();
    if !config.order_notifications_enabled && !config.customer_order_emails_enabled {
        return Ok(run);
    }
    let items = repo::outbox::claim_due(db, OUTBOX_BATCH).await?;
    run.claimed = items.len();

    // Admin recipients: resolved lazily, once per batch.
    let mut admin_recipients: Option<Result<Vec<String>, ()>> = None;

    for item in &items {
        let outcome = match item.kind.as_str() {
            KIND_ADMIN_ORDER_NOTIFICATION if !config.order_notifications_enabled => {
                Outcome::Deferred
            }
            KIND_CUSTOMER_ORDER_CONFIRMATION if !config.customer_order_emails_enabled => {
                Outcome::Deferred
            }
            KIND_ADMIN_ORDER_NOTIFICATION => {
                if admin_recipients.is_none() {
                    admin_recipients = Some(resolve_admin_recipients(db, config).await);
                }
                match admin_recipients.as_ref() {
                    Some(Ok(to)) => deliver_admin(db, config, email, item, to).await,
                    _ => Outcome::Retry("database error loading recipients"),
                }
            }
            KIND_CUSTOMER_ORDER_CONFIRMATION => deliver_customer(db, config, email, item).await,
            _ => Outcome::Failed("unknown outbox kind"),
        };
        let marked = match outcome {
            Outcome::Sent => {
                run.sent += 1;
                repo::outbox::mark_sent(db, item.id).await
            }
            Outcome::Retry(reason) => {
                run.retried += 1;
                repo::outbox::mark_retry(db, item, reason).await
            }
            Outcome::Failed(reason) => {
                run.failed += 1;
                tracing::warn!(outbox_id = %item.id, kind = %item.kind, reason, "order email permanently failed");
                repo::outbox::mark_failed(db, item.id, reason).await
            }
            Outcome::Deferred => {
                run.deferred += 1;
                defer_claim(db, item).await
            }
        };
        if let Err(e) = marked {
            // The lease expires and the row is retried; worst case a duplicate email.
            tracing::error!(outbox_id = %item.id, error = %e, "order emails: updating outbox row failed");
        }
    }
    Ok(run)
}

/// Undo a claim on a row whose kind is disabled: the attempt is not counted
/// and the row is looked at again later (e.g. after the flag is re-enabled).
async fn defer_claim(db: &PgPool, item: &OutboxItem) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE email_outbox SET attempts = GREATEST(attempts - 1, 0), \
                next_attempt_at = now() + make_interval(secs => $2) \
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(item.id)
    .bind(OUTBOX_DISABLED_DEFER_SECS)
    .execute(db)
    .await?;
    Ok(())
}

async fn resolve_admin_recipients(db: &PgPool, config: &Config) -> Result<Vec<String>, ()> {
    if !config.order_notification_emails.is_empty() {
        return Ok(config.order_notification_emails.clone());
    }
    repo::outbox::active_admin_emails(db).await.map_err(|e| {
        tracing::warn!(error = %e, "order emails: loading admin recipients failed");
    })
}

/// Load the row's order; the connection is released before any provider call.
async fn load_order(db: &PgPool, item: &OutboxItem) -> Result<Order, Outcome> {
    let Some(order_id) = item.order_id else {
        return Err(Outcome::Failed("outbox row has no order"));
    };
    let mut conn = db.acquire().await.map_err(|e| {
        tracing::warn!(error = %e, "order emails: no database connection");
        Outcome::Retry("database unavailable")
    })?;
    match repo::orders::load_full(&mut conn, &OrderRef::Id(order_id)).await {
        Ok(Some(order)) => Ok(order),
        Ok(None) => Err(Outcome::Failed("order not found")),
        Err(e) => {
            tracing::warn!(error = %e, %order_id, "order emails: loading order failed");
            Err(Outcome::Retry("database error loading order"))
        }
    }
}

async fn deliver_admin(
    db: &PgPool,
    config: &Config,
    email: &dyn EmailSender,
    item: &OutboxItem,
    recipients: &[String],
) -> Outcome {
    if recipients.is_empty() {
        return Outcome::Retry(
            "no recipients (no active admins and ORDER_NOTIFICATION_EMAILS unset)",
        );
    }
    let order = match load_order(db, item).await {
        Ok(o) => o,
        Err(outcome) => return outcome,
    };
    let message = templates::order_notification(
        &order,
        recipients.to_vec(),
        config.public_site_url.as_deref(),
        config.pricing.bulk_min_order_paise,
    );
    send_logged(email, &message, &order.order_number, item, false).await
}

async fn deliver_customer(
    db: &PgPool,
    config: &Config,
    email: &dyn EmailSender,
    item: &OutboxItem,
) -> Outcome {
    let order = match load_order(db, item).await {
        Ok(o) => o,
        Err(outcome) => return outcome,
    };
    // Exactly one recipient (the customer); renders the private tracking link.
    let Some(message) = templates::customer_order_confirmation(&order, config) else {
        return Outcome::Failed("no customer email");
    };
    send_logged(email, &message, &order.order_number, item, true).await
}

/// Send with a timeout and log by order number + kind only. Never logs the
/// body (customer emails contain the tracking token) and, for customer mail,
/// scrubs addresses out of provider errors.
async fn send_logged(
    email: &dyn EmailSender,
    message: &OutboundEmail,
    order_number: &str,
    item: &OutboxItem,
    customer: bool,
) -> Outcome {
    match tokio::time::timeout(OUTBOX_SEND_TIMEOUT, email.send_email(message)).await {
        Ok(Ok(())) => {
            tracing::info!(
                order_number,
                kind = %item.kind,
                recipients = message.to.len(),
                attempt = item.attempts,
                "order email sent"
            );
            Outcome::Sent
        }
        Ok(Err(e)) => {
            let error = format!("{e:#}");
            let error = if customer {
                redact_addresses(&error)
            } else {
                error
            };
            tracing::warn!(
                order_number,
                kind = %item.kind,
                attempt = item.attempts,
                error,
                "order email send failed; will retry"
            );
            Outcome::Retry("email provider error (see server logs)")
        }
        Err(_) => {
            tracing::warn!(order_number, kind = %item.kind, attempt = item.attempts, "order email send timed out");
            Outcome::Retry("email provider timed out")
        }
    }
}

/// Replace every whitespace-separated token containing `@` (an address,
/// possibly quoted) with `[redacted]`.
fn redact_addresses(s: &str) -> String {
    s.split_inclusive(char::is_whitespace)
        .map(|tok| {
            if tok.contains('@') {
                let trailing: String = tok.chars().filter(|c| c.is_whitespace()).collect();
                format!("[redacted]{trailing}")
            } else {
                tok.to_owned()
            }
        })
        .collect()
}

/// Resolves once shutdown has been requested (or the sender is gone).
async fn shutdown_requested(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|stop| *stop).await;
}

fn jitter(max: Duration) -> Duration {
    let max_ms = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
    Duration::from_millis(rand::thread_rng().gen_range(0..=max_ms))
}

async fn expiry_loop(state: AppState, mut shutdown: watch::Receiver<bool>) {
    let mut delay = EXPIRY_INITIAL_DELAY + jitter(EXPIRY_JITTER);
    loop {
        tokio::select! {
            _ = shutdown_requested(&mut shutdown) => break,
            _ = tokio::time::sleep(delay) => {}
        }
        reconcile_before_expiry(&state).await;
        let expired = expire_backlog(&state.db, &shutdown).await;
        if expired > 0 {
            tracing::info!(
                expired,
                "payment expiry: cancelled unpaid orders and released their stock"
            );
        }
        delay = EXPIRY_INTERVAL + jitter(EXPIRY_JITTER);
    }
    tracing::debug!("payment expiry job stopped");
}

/// Best-effort: settle expired orders that Cashfree says were paid, so the
/// sweep does not cancel (and later revive or refund) them. Never holds a row
/// lock across the HTTP calls; Cashfree being down just means "cancel anyway".
pub async fn reconcile_before_expiry(state: &AppState) -> payments::service::SweepReconcile {
    let run = payments::service::reconcile_expiring_orders(
        state,
        EXPIRY_RECONCILE_LIMIT,
        EXPIRY_RECONCILE_BUDGET,
    )
    .await;
    if run.paid > 0 {
        tracing::info!(
            checked = run.checked,
            paid = run.paid,
            "payment expiry: orders paid at Cashfree were settled instead of cancelled"
        );
    }
    if run.gateway_unavailable {
        tracing::warn!(
            checked = run.checked,
            "payment expiry: Cashfree unavailable; expiring the remaining orders unchecked"
        );
    }
    run
}

/// Expire batches until one comes back short (backlog drained), an error
/// occurs, the per-tick cap is hit, or shutdown is requested. Returns the
/// number of orders expired.
async fn expire_backlog(db: &PgPool, shutdown: &watch::Receiver<bool>) -> u64 {
    let full_batch = u64::try_from(EXPIRY_BATCH).unwrap_or(u64::MAX);
    let mut total = 0u64;
    for _ in 0..EXPIRY_MAX_BATCHES_PER_TICK {
        if *shutdown.borrow() {
            break;
        }
        let batch = tokio::time::timeout(
            EXPIRY_BATCH_TIMEOUT,
            repo::orders::expire_unpaid_orders(db, EXPIRY_BATCH),
        )
        .await;
        match batch {
            Ok(Ok(n)) => {
                total += n;
                if n < full_batch {
                    break;
                }
            }
            Ok(Err(e)) => {
                tracing::error!(error = %e, "payment expiry: sweep failed");
                break;
            }
            Err(_) => {
                tracing::error!(
                    timeout_secs = EXPIRY_BATCH_TIMEOUT.as_secs(),
                    "payment expiry: sweep timed out"
                );
                break;
            }
        }
    }
    total
}

async fn maintenance_loop(db: PgPool, mut shutdown: watch::Receiver<bool>) {
    let mut delay = MAINTENANCE_INITIAL_DELAY + jitter(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = shutdown_requested(&mut shutdown) => break,
            _ = tokio::time::sleep(delay) => {}
        }

        let started = std::time::Instant::now();
        match tokio::time::timeout(MAINTENANCE_TIMEOUT, repo::maintenance::purge_expired(&db)).await
        {
            Ok(Ok(stats)) => {
                let total = stats.otp_challenges
                    + stats.refresh_tokens
                    + stats.sessions
                    + stats.audit_rows
                    + stats.login_throttles
                    + stats.trusted_devices;
                let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                if total > 0 {
                    tracing::info!(
                        otp_challenges = stats.otp_challenges,
                        refresh_tokens = stats.refresh_tokens,
                        sessions = stats.sessions,
                        audit_rows = stats.audit_rows,
                        login_throttles = stats.login_throttles,
                        trusted_devices = stats.trusted_devices,
                        elapsed_ms,
                        "maintenance: purged expired rows"
                    );
                } else {
                    tracing::debug!(elapsed_ms, "maintenance: nothing to purge");
                }
            }
            Ok(Err(e)) => tracing::error!(error = %e, "maintenance: purge failed"),
            Err(_) => tracing::error!(
                timeout_secs = MAINTENANCE_TIMEOUT.as_secs(),
                "maintenance: purge timed out"
            ),
        }

        match tokio::time::timeout(MAINTENANCE_TIMEOUT, repo::outbox::purge_old(&db)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(n)) => {
                tracing::info!(email_outbox_rows = n, "maintenance: purged old outbox rows")
            }
            Ok(Err(e)) => tracing::error!(error = %e, "maintenance: outbox purge failed"),
            Err(_) => tracing::error!("maintenance: outbox purge timed out"),
        }

        delay = MAINTENANCE_INTERVAL + jitter(MAINTENANCE_JITTER);
    }
    tracing::debug!("maintenance job stopped");
}

async fn limiter_gc_loop(limiters: RateLimiters, mut shutdown: watch::Receiver<bool>) {
    let mut ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + LIMITER_GC_INTERVAL,
        LIMITER_GC_INTERVAL,
    );
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shutdown_requested(&mut shutdown) => break,
            _ = ticker.tick() => {}
        }
        // Sweeping a large keyed store is synchronous work; keep it off the
        // async workers so request latency is unaffected under a flood.
        let limiters = limiters.clone();
        let swept = tokio::task::spawn_blocking(move || {
            limiters.all().map(|limiter| (limiter.name(), limiter.gc()))
        })
        .await;
        match swept {
            Ok(results) => {
                for (name, remaining) in results {
                    if remaining >= LIMITER_KEYS_WARN {
                        tracing::warn!(
                            limiter = name,
                            keys = remaining,
                            "rate limiter tracking an unusually large number of clients"
                        );
                    } else {
                        tracing::trace!(limiter = name, keys = remaining, "rate limiter gc");
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "rate limiter gc task failed"),
        }
    }
    tracing::debug!("rate limiter gc job stopped");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{redact_addresses, sheets_retry_backoff};
    use crate::sheets::SheetsError;

    #[test]
    fn sheets_backoff_grows_and_caps() {
        let t = SheetsError::Transient("x".into());
        assert_eq!(sheets_retry_backoff(1, &t), Duration::from_secs(30));
        assert_eq!(sheets_retry_backoff(2, &t), Duration::from_secs(60));
        assert_eq!(sheets_retry_backoff(6, &t), Duration::from_secs(960));
        assert_eq!(sheets_retry_backoff(7, &t), Duration::from_secs(1800));
        assert_eq!(sheets_retry_backoff(500, &t), Duration::from_secs(1800));
        assert_eq!(sheets_retry_backoff(0, &t), Duration::from_secs(30));
        let p = SheetsError::AuthRevoked;
        assert_eq!(sheets_retry_backoff(1, &p), Duration::from_secs(600));
        assert_eq!(sheets_retry_backoff(2, &p), Duration::from_secs(1200));
        assert_eq!(sheets_retry_backoff(3, &p), Duration::from_secs(2400));
        assert_eq!(sheets_retry_backoff(4, &p), Duration::from_secs(3600));
        assert_eq!(sheets_retry_backoff(40, &p), Duration::from_secs(3600));
    }

    #[test]
    fn redacts_email_addresses_from_errors() {
        assert_eq!(
            redact_addresses(
                "MessageRejected: Email address is not verified: <ravi@example.com>, other"
            ),
            "MessageRejected: Email address is not verified: [redacted] other"
        );
        assert_eq!(redact_addresses("timeout\nretry"), "timeout\nretry");
    }
}
