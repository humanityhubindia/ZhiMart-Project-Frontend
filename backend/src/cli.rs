//! Command-line interface and the operational subcommands.
//!
//! `migrate`, `create-admin` and `reset-password` only need `DATABASE_URL`, so
//! operators can run them without JWT/OTP/SES secrets being present. The
//! Google Sheets commands (`google-auth`, `google-sheets-check`, implemented in
//! `sheets::setup`) need only the `GOOGLE_*` settings and no database.

use std::{
    io::{BufRead, IsTerminal},
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, anyhow, bail, ensure};
use clap::{Parser, Subcommand};
use secrecy::SecretString;
use sqlx::{
    Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use zeroize::Zeroizing;

use crate::{auth::password, repo::admins};

/// Embedded at compile time; the binary needs no migrations directory at runtime.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

const MAX_EMAIL_LEN: usize = 254;
const MAX_DISPLAY_NAME_CHARS: usize = 100;
const CLI_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const APPLICATION_NAME: &str = "zhimart-backend";

#[derive(Debug, Parser)]
#[command(
    name = "zhimart-backend",
    version,
    about = "Zhi-Mart admin API server and operations CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the HTTP API server (default when no subcommand is given).
    Serve,
    /// Apply pending database migrations and exit. Needs only DATABASE_URL.
    Migrate,
    /// Create an admin account. Prompts for the password twice on a terminal;
    /// otherwise reads a single line from stdin (for automation).
    CreateAdmin {
        /// Login email (stored trimmed + lowercased).
        #[arg(long)]
        email: String,
        /// Display name shown in the admin UI and in sign-in emails.
        #[arg(long)]
        name: String,
    },
    /// Set a new password for an admin, clear any lockout and revoke all of
    /// their sessions. Password input works as for `create-admin`.
    ResetPassword {
        #[arg(long)]
        email: String,
    },
    /// One-time Google OAuth consent for the Sheets order sync: prints the
    /// refresh token for GOOGLE_OAUTH_REFRESH_TOKEN. Needs only
    /// GOOGLE_OAUTH_CLIENT_ID and GOOGLE_OAUTH_CLIENT_SECRET; listens on
    /// http://127.0.0.1:53682/ (GOOGLE_OAUTH_REDIRECT_PORT) for the redirect.
    GoogleAuth {
        /// Only print the consent URL; do not try to open a browser.
        #[arg(long)]
        no_browser: bool,
    },
    /// Verify the Google Sheets settings: refresh a token, create the tab and
    /// header row if missing, and print the spreadsheet title.
    GoogleSheetsCheck,
}

/// Parse `DATABASE_URL` into connect options (no statement timeout; the
/// server adds its own on top).
pub fn pg_connect_options(database_url: &str) -> anyhow::Result<PgConnectOptions> {
    // Never include the URL itself in errors: it carries the DB password.
    let opts = PgConnectOptions::from_str(database_url)
        .map_err(|e| anyhow!("DATABASE_URL is not a valid PostgreSQL URL: {e}"))?;
    Ok(opts.application_name(APPLICATION_NAME))
}

/// Apply all pending migrations on a dedicated connection (no statement
/// timeout, so long-running DDL is not cut off). sqlx takes a Postgres
/// advisory lock, so concurrent replicas booting together are safe.
pub async fn run_migrations(opts: &PgConnectOptions) -> anyhow::Result<()> {
    let mut conn = tokio::time::timeout(CLI_CONNECT_TIMEOUT, PgConnection::connect_with(opts))
        .await
        .context("timed out connecting to the database for migrations")?
        .context("failed to connect to the database for migrations")?;
    MIGRATOR
        .run(&mut conn)
        .await
        .context("failed to apply database migrations")?;
    if let Err(e) = conn.close().await {
        tracing::debug!(error = %e, "closing migration connection failed");
    }
    Ok(())
}

fn database_url_from_env() -> anyhow::Result<String> {
    match std::env::var("DATABASE_URL") {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => bail!("DATABASE_URL must be set (environment or .env)"),
    }
}

async fn cli_pool() -> anyhow::Result<PgPool> {
    let opts = pg_connect_options(&database_url_from_env()?)?;
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(CLI_CONNECT_TIMEOUT)
        .connect_with(opts)
        .await
        .context(
            "failed to connect to the database (is DATABASE_URL correct and the server running?)",
        )
}

pub async fn migrate() -> anyhow::Result<()> {
    let opts = pg_connect_options(&database_url_from_env()?)?;
    run_migrations(&opts).await?;
    println!("Migrations are up to date.");
    Ok(())
}

pub async fn create_admin(email: &str, name: &str) -> anyhow::Result<()> {
    let email = normalize_email(email)?;
    let name = normalize_display_name(name)?;
    // Connect first so the operator does not type a password for nothing.
    let pool = cli_pool().await?;

    let password = read_new_password()?;
    let hash = password::hash_password(password)
        .await
        .context("failed to hash password")?;

    let result = admins::insert_admin(&pool, &email, &name, &hash).await;
    pool.close().await;
    match result {
        Ok(id) => {
            println!("Created admin {email} (id {id}).");
            Ok(())
        }
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            bail!(
                "an admin with email {email} already exists (use reset-password to change its password)"
            )
        }
        Err(e) => {
            Err(anyhow::Error::new(e)
                .context("failed to create admin (have migrations been applied?)"))
        }
    }
}

pub async fn reset_password(email: &str) -> anyhow::Result<()> {
    let email = normalize_email(email)?;
    let pool = cli_pool().await?;

    let password = read_new_password()?;
    let hash = password::hash_password(password)
        .await
        .context("failed to hash password")?;

    let result = admins::reset_password(&pool, &email, &hash).await;
    pool.close().await;
    match result.context("failed to reset password")? {
        true => {
            println!("Password updated for {email}; all of their sessions were revoked.");
            Ok(())
        }
        false => bail!("no admin with email {email}"),
    }
}

/// Trim + lowercase, then a conservative shape check (the DB enforces the
/// lowercase/length constraints too).
pub fn normalize_email(raw: &str) -> anyhow::Result<String> {
    let email = raw.trim().to_lowercase();
    ensure!(
        (3..=MAX_EMAIL_LEN).contains(&email.len()),
        "email must be 3-{MAX_EMAIL_LEN} characters"
    );
    ensure!(
        !email.chars().any(|c| c.is_whitespace() || c.is_control()),
        "email must not contain spaces or control characters"
    );
    let (local, domain) = email
        .split_once('@')
        .ok_or_else(|| anyhow!("email must contain '@'"))?;
    ensure!(
        !local.is_empty() && !domain.is_empty() && !domain.contains('@'),
        "email must look like name@domain"
    );
    ensure!(
        domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'),
        "email domain looks invalid"
    );
    Ok(email)
}

pub fn normalize_display_name(raw: &str) -> anyhow::Result<String> {
    let name = raw.trim();
    let chars = name.chars().count();
    ensure!(
        (1..=MAX_DISPLAY_NAME_CHARS).contains(&chars),
        "name must be 1-{MAX_DISPLAY_NAME_CHARS} characters"
    );
    ensure!(
        !name.chars().any(char::is_control),
        "name must not contain control characters"
    );
    Ok(name.to_owned())
}

/// Read a new password: two hidden prompts on a TTY, else one line of stdin.
/// The password is never echoed or logged and is wiped from memory after use.
fn read_new_password() -> anyhow::Result<SecretString> {
    let mut pw = if std::io::stdin().is_terminal() {
        let first = Zeroizing::new(
            rpassword::prompt_password("New password: ").context("failed to read password")?,
        );
        password::check_password_policy(&first).map_err(|m| anyhow!("password rejected: {m}"))?;
        let second = Zeroizing::new(
            rpassword::prompt_password("Repeat password: ").context("failed to read password")?,
        );
        ensure!(*first == *second, "passwords do not match");
        first
    } else {
        let mut line = Zeroizing::new(String::new());
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .context("failed to read password from stdin")?;
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        ensure!(!line.is_empty(), "no password provided on stdin");
        line
    };
    password::check_password_policy(&pw).map_err(|m| anyhow!("password rejected: {m}"))?;
    Ok(SecretString::from(std::mem::take(&mut *pw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalisation() {
        assert_eq!(
            normalize_email("  Admin@ZhiMart.COM ").expect("valid"),
            "admin@zhimart.com"
        );
        assert!(normalize_email("nope").is_err());
        assert!(normalize_email("a@b").is_err());
        assert!(normalize_email("@zhimart.com").is_err());
        assert!(normalize_email("a b@zhimart.com").is_err());
        assert!(normalize_email("a@@zhimart.com").is_err());
        assert!(normalize_email(&format!("{}@x.io", "a".repeat(260))).is_err());
    }

    #[test]
    fn display_name_validation() {
        assert_eq!(
            normalize_display_name("  Ops Team ").expect("valid"),
            "Ops Team"
        );
        assert!(normalize_display_name("   ").is_err());
        assert!(normalize_display_name("a\nb").is_err());
        assert!(normalize_display_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn cli_parses_subcommands() {
        let cli = Cli::try_parse_from(["zhimart-backend"]).expect("parse");
        assert!(cli.command.is_none());
        let cli = Cli::try_parse_from([
            "zhimart-backend",
            "create-admin",
            "--email",
            "a@b.co",
            "--name",
            "A",
        ])
        .expect("parse");
        assert!(matches!(cli.command, Some(Command::CreateAdmin { .. })));
        assert!(Cli::try_parse_from(["zhimart-backend", "reset-password"]).is_err());
        let cli = Cli::try_parse_from(["zhimart-backend", "google-auth"]).expect("parse");
        assert!(matches!(
            cli.command,
            Some(Command::GoogleAuth { no_browser: false })
        ));
        let cli =
            Cli::try_parse_from(["zhimart-backend", "google-auth", "--no-browser"]).expect("parse");
        assert!(matches!(
            cli.command,
            Some(Command::GoogleAuth { no_browser: true })
        ));
        let cli = Cli::try_parse_from(["zhimart-backend", "google-sheets-check"]).expect("parse");
        assert!(matches!(cli.command, Some(Command::GoogleSheetsCheck)));
    }

    #[test]
    fn database_url_errors_do_not_echo_the_url() {
        let err =
            pg_connect_options("postgres://user:s3cret@host:notaport/db").expect_err("bad port");
        assert!(!format!("{err:#}").contains("s3cret"));
    }
}
