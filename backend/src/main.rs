//! zhimart-backend binary: `serve` (default), `migrate`, `create-admin`,
//! `reset-password`, `google-auth`, `google-sheets-check`.

use std::{process::ExitCode, sync::Arc, time::Duration};

use anyhow::Context;
use clap::Parser;
use secrecy::ExposeSecret;
use sqlx::postgres::PgPoolOptions;
use tokio::{net::TcpListener, sync::watch};
use zhimart_backend::{
    app,
    cli::{self, Cli, Command},
    config::Config,
    email, jobs, routes, sheets,
    state::AppState,
    telemetry::{self, LogFormat},
};

const DB_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DB_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);
/// Server-side cap on any single statement issued by the API.
const DB_STATEMENT_TIMEOUT_MS: &str = "5000";
/// Abort an idle-in-transaction session holding locks. Must exceed the longest
/// external call made inside a transaction (OTP resend holds one across an
/// SES send with a 9 s budget).
const DB_IDLE_IN_TX_TIMEOUT_MS: &str = "30000";
const JOBS_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    // .env is optional (production injects real env vars, which always win).
    if let Err(e) = dotenvy::dotenv()
        && !e.not_found()
    {
        eprintln!("error: failed to load .env: {e}");
        return ExitCode::FAILURE;
    }

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Serve);
    let is_serve = matches!(command, Command::Serve);

    let log_format = if is_serve {
        LogFormat::from_env()
    } else {
        LogFormat::Pretty
    };
    if let Err(e) = telemetry::init(log_format) {
        eprintln!("error: {e:#}");
        return ExitCode::FAILURE;
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async move {
        match command {
            Command::Serve => serve().await,
            Command::Migrate => cli::migrate().await,
            Command::CreateAdmin { email, name } => cli::create_admin(&email, &name).await,
            Command::ResetPassword { email } => cli::reset_password(&email).await,
            Command::GoogleAuth { no_browser } => sheets::setup::google_auth(!no_browser).await,
            Command::GoogleSheetsCheck => sheets::setup::sheets_check().await,
        }
    });
    // Do not wait on stray blocking tasks (e.g. a hash in flight) at exit.
    runtime.shutdown_timeout(Duration::from_secs(1));

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if is_serve {
                tracing::error!(error = format!("{e:#}"), "server exited with an error");
            } else {
                eprintln!("error: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn auto_migrate_enabled() -> bool {
    !matches!(
        std::env::var("AUTO_MIGRATE")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("false" | "0" | "no" | "off")
    )
}

async fn serve() -> anyhow::Result<()> {
    let config = Arc::new(Config::from_env().context("invalid configuration")?);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        env = ?config.app_env,
        bind = %config.bind_addr,
        email_provider = ?config.email_provider,
        trusted_proxies = config.trusted_proxies.len(),
        "starting zhimart-backend"
    );

    if config.payment_test_mode {
        tracing::warn!(
            "PAYMENT TEST MODE: all products charged ₹1 per unit (PAYMENT_TEST_MODE=true). \
             Orders are flagged isTest and excluded from revenue."
        );
    }
    let legacy = zhimart_backend::config::legacy_razorpay_vars();
    if !legacy.is_empty() {
        tracing::warn!(
            vars = %legacy.join(","),
            "RAZORPAY_* settings are ignored: online payments now use Cashfree (CASHFREE_*); remove them"
        );
    }
    if !config.payments_enabled {
        tracing::info!(
            online_payments = false,
            "CASHFREE_ENABLED=false: orders are pay-later; admins record payments manually"
        );
    } else if config.cashfree_is_sandbox() {
        tracing::info!(
            online_payments = true,
            cashfree_env = "sandbox",
            "Cashfree online payments enabled against the SANDBOX: no real money moves"
        );
    } else {
        tracing::warn!(
            online_payments = true,
            cashfree_env = "production",
            test_mode = config.payment_test_mode,
            "Cashfree online payments enabled in PRODUCTION: payments are real"
        );
    }
    if !config.order_notifications_enabled {
        tracing::info!(
            order_notifications = false,
            "admin new-order emails disabled"
        );
    } else if config.order_notification_emails.is_empty() {
        tracing::info!(
            order_notifications = true,
            recipients = "all active admins",
            "admin new-order emails enabled"
        );
    } else {
        tracing::info!(
            order_notifications = true,
            recipients = config.order_notification_emails.len(),
            "admin new-order emails enabled (ORDER_NOTIFICATION_EMAILS)"
        );
    }

    if config.customer_order_emails_enabled {
        tracing::info!(
            customer_order_emails = true,
            site_url = config.site_url(),
            "customer order-confirmation emails enabled (tracking links use site_url)"
        );
    } else {
        tracing::info!(
            customer_order_emails = false,
            "customer order-confirmation emails disabled"
        );
    }

    let base_opts = cli::pg_connect_options(config.database_url.expose_secret())?;

    if auto_migrate_enabled() {
        cli::run_migrations(&base_opts).await?;
        tracing::info!("database migrations applied");
    } else {
        tracing::info!("AUTO_MIGRATE=false: skipping migrations");
    }

    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .acquire_timeout(config.db_acquire_timeout)
        .idle_timeout(Some(DB_IDLE_TIMEOUT))
        .max_lifetime(Some(DB_MAX_LIFETIME))
        .connect_with(base_opts.options([
            ("statement_timeout", DB_STATEMENT_TIMEOUT_MS),
            (
                "idle_in_transaction_session_timeout",
                DB_IDLE_IN_TX_TIMEOUT_MS,
            ),
        ]))
        .await
        .context("failed to connect to the database")?;

    let email = email::build_sender(&config)
        .await
        .context("failed to initialise email provider")?;
    let state = AppState::new(Arc::clone(&config), pool.clone(), email)
        .context("failed to build app state")?;
    let app = app::build_app(state.clone());

    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_tx = Arc::new(shutdown_tx);
    {
        let shutdown_tx = Arc::clone(&shutdown_tx);
        tokio::spawn(async move {
            shutdown_signal().await;
            tracing::info!("shutdown signal received");
            routes::health::begin_shutdown();
            let _ = shutdown_tx.send(true);
        });
    }

    let job_handles = jobs::spawn(&state, app.limiters.clone(), shutdown_rx.clone());
    let served = app::serve(
        listener,
        app.router,
        shutdown_rx,
        app::ServeOptions::new(&config.trusted_proxies),
    )
    .await;

    // Whether we got here by signal or by error, stop the jobs too.
    let _ = shutdown_tx.send(true);
    for handle in job_handles {
        if tokio::time::timeout(JOBS_STOP_TIMEOUT, handle)
            .await
            .is_err()
        {
            tracing::warn!("background job did not stop in time");
        }
    }
    if tokio::time::timeout(POOL_CLOSE_TIMEOUT, pool.close())
        .await
        .is_err()
    {
        tracing::warn!("database pool did not close in time");
    }
    tracing::info!("shutdown complete");
    served
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "failed to listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
