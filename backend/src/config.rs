//! Process configuration, loaded once from the environment and validated at
//! startup. Anything security-relevant fails closed: a weak secret or an unsafe
//! production setting aborts boot instead of running degraded.

use std::{env, net::SocketAddr, str::FromStr, time::Duration};

use anyhow::{Context, bail, ensure};
use axum::http::HeaderValue;
use ipnetwork::IpNetwork;
use secrecy::{ExposeSecret, SecretString};

/// Hard ceiling for an admin session. Rotation never extends past this.
pub const MAX_SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

impl AppEnv {
    pub fn is_production(self) -> bool {
        matches!(self, AppEnv::Production)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailProvider {
    /// AWS SES v2. Credentials come from the standard AWS provider chain.
    Ses,
    /// Writes the OTP to the server log. Refused when APP_ENV=production.
    Log,
}

#[derive(Debug)]
pub struct Config {
    pub app_env: AppEnv,
    pub bind_addr: SocketAddr,

    // Database
    pub database_url: SecretString,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout: Duration,

    // Tokens
    pub jwt_secret: SecretString,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub access_token_ttl: Duration,
    pub session_ttl: Duration,

    // OTP / login hardening
    pub otp_pepper: SecretString,
    pub otp_ttl: Duration,
    pub otp_max_attempts: i32,
    pub otp_resend_cooldown: Duration,
    pub otp_max_resends: i32,
    pub login_max_failures: i32,
    pub login_lockout: Duration,

    // Cookies / browser security
    pub cookie_secure: bool,
    pub allowed_origins: Vec<HeaderValue>,

    // Network edge
    pub trusted_proxies: Vec<IpNetwork>,
    pub rate_limit_global_per_minute: u32,
    pub rate_limit_auth_per_minute: u32,
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
    pub max_concurrent_requests: usize,

    // Email
    pub email_provider: EmailProvider,
    pub ses_from_address: String,
    pub ses_configuration_set: Option<String>,

    // Payments (Cashfree)
    /// Master switch for online payments (`CASHFREE_ENABLED`, default false).
    /// Off ⇒ checkout places `pay_later` orders straight into `pending_pack`
    /// (stock deducted, no payment window) and the admin collects payment
    /// offline; the Cashfree keys are then optional.
    pub payments_enabled: bool,
    /// When true every product is charged at [`TEST_MODE_UNIT_PRICE_PAISE`]
    /// (₹1) per unit, enforced server-side, and orders are flagged `is_test`.
    pub payment_test_mode: bool,
    /// `CASHFREE_ENV`: picks the API base and is the storefront SDK `mode`.
    pub cashfree_env: CashfreeEnv,
    /// `CASHFREE_APP_ID` (the "client id"; not secret).
    pub cashfree_app_id: String,
    /// `CASHFREE_SECRET_KEY`: API secret, also the webhook signing key.
    pub cashfree_secret_key: SecretString,
    /// Base URL of the Cashfree PG API: [`CashfreeEnv::api_base`] unless
    /// `CASHFREE_API_BASE` overrides it (tests/mocks only; refused in production).
    pub cashfree_api_base: String,
    /// `CASHFREE_NOTIFY_URL`: optional https webhook URL sent per order as
    /// `order_meta.notify_url` (the dashboard webhook endpoint works without it).
    pub cashfree_notify_url: Option<String>,
    /// Unpaid orders hold their stock reservation this long, then expire.
    pub payment_expiry: Duration,
    /// Store name shown on the storefront and in emails.
    pub store_name: String,

    // Order notifications
    /// Email the admins about every new order (`ORDER_NOTIFICATION_ENABLED`, default true).
    pub order_notifications_enabled: bool,
    /// Explicit recipients (`ORDER_NOTIFICATION_EMAILS`, comma-separated). Empty ⇒
    /// every active admin's login email.
    pub order_notification_emails: Vec<String>,
    /// Public storefront origin (`PUBLIC_SITE_URL`, e.g. `https://zhimart.com`),
    /// used for the "open in admin console" link in notification emails and the
    /// customer's tracking link. Required in production when customer emails are on.
    pub public_site_url: Option<String>,
    /// Email the customer an order confirmation with their private tracking link
    /// (`CUSTOMER_ORDER_EMAIL_ENABLED`, default true).
    pub customer_order_emails_enabled: bool,

    // Pricing tiers + delivery charge
    /// Two-tier pricing (`BULK_MIN_ORDER_RUPEES`, `STANDARD_DISCOUNT_PERCENT`,
    /// `BULK_DISCOUNT_PERCENT`) and the delivery charge (`DELIVERY_FEE_RUPEES`,
    /// `FREE_DELIVERY_MIN_RUPEES`).
    pub pricing: Pricing,

    // Google Sheets order sync
    /// `Some` when `GOOGLE_SHEETS_ENABLED=true` (all settings then validated
    /// at startup); `None` ⇒ the sync worker is not started.
    pub google_sheets: Option<GoogleSheetsConfig>,
}

/// Two-tier pricing. An order whose total at price-list prices
/// (`products.price_paise`) reaches `bulk_min_order_paise` is charged those
/// prices ("bulk"); a smaller order pays the "standard" price on every line
/// (MRP less `standard_discount_percent`, see [`crate::money::standard_price_paise`]).
///
/// On top of the goods ("subtotal", after tier pricing) every order pays a flat
/// `delivery_fee_paise` unless its subtotal reaches `free_delivery_min_paise`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pricing {
    /// Minimum order value, at price-list prices, for bulk pricing (paise).
    pub bulk_min_order_paise: i64,
    /// Discount off MRP for orders below the bulk minimum (e.g. 80 ⇒ pay 20 % of MRP).
    pub standard_discount_percent: u32,
    /// Typical discount of the price list; a label for the storefront only.
    pub bulk_discount_percent: u32,
    /// Flat delivery charge for an order whose subtotal is below
    /// `free_delivery_min_paise` (paise).
    pub delivery_fee_paise: i64,
    /// Subtotal (after tier pricing) from which delivery is free, inclusive
    /// (paise). 0 ⇒ delivery is always free.
    pub free_delivery_min_paise: i64,
}

impl Default for Pricing {
    fn default() -> Self {
        Pricing {
            bulk_min_order_paise: 5_000 * 100,
            standard_discount_percent: 80,
            bulk_discount_percent: 85,
            delivery_fee_paise: 500 * 100,
            free_delivery_min_paise: 5_000 * 100,
        }
    }
}

impl Pricing {
    /// Largest accepted `BULK_MIN_ORDER_RUPEES` (₹1 crore).
    pub const MAX_BULK_MIN_ORDER_RUPEES: i64 = 10_000_000;
    /// Largest accepted `DELIVERY_FEE_RUPEES` (₹10,000).
    pub const MAX_DELIVERY_FEE_RUPEES: i64 = 10_000;

    /// Unit price for an order that does not qualify for bulk pricing.
    pub fn standard_price_paise(&self, price_paise: i64, mrp_paise: i64) -> i64 {
        crate::money::standard_price_paise(price_paise, mrp_paise, self.standard_discount_percent)
    }

    /// Whether an order worth `list_total_paise` at price-list prices gets them.
    pub fn is_bulk(&self, list_total_paise: i64) -> bool {
        list_total_paise >= self.bulk_min_order_paise
    }

    /// Delivery charge for an order whose goods come to `subtotal_paise`
    /// (after tier pricing): free from `free_delivery_min_paise` (inclusive),
    /// else the flat fee.
    pub fn delivery_fee_for(&self, subtotal_paise: i64) -> i64 {
        if subtotal_paise >= self.free_delivery_min_paise {
            0
        } else {
            self.delivery_fee_paise
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            (0..=Self::MAX_BULK_MIN_ORDER_RUPEES * 100).contains(&self.bulk_min_order_paise),
            "BULK_MIN_ORDER_RUPEES must be between 0 and {}",
            Self::MAX_BULK_MIN_ORDER_RUPEES
        );
        ensure!(
            self.standard_discount_percent <= 99,
            "STANDARD_DISCOUNT_PERCENT must be between 0 and 99"
        );
        ensure!(
            self.bulk_discount_percent <= 99,
            "BULK_DISCOUNT_PERCENT must be between 0 and 99"
        );
        ensure!(
            self.standard_discount_percent <= self.bulk_discount_percent,
            "STANDARD_DISCOUNT_PERCENT must not exceed BULK_DISCOUNT_PERCENT (bulk orders get the bigger discount)"
        );
        ensure!(
            (0..=Self::MAX_DELIVERY_FEE_RUPEES * 100).contains(&self.delivery_fee_paise),
            "DELIVERY_FEE_RUPEES must be between 0 and {}",
            Self::MAX_DELIVERY_FEE_RUPEES
        );
        ensure!(
            (0..=Self::MAX_BULK_MIN_ORDER_RUPEES * 100).contains(&self.free_delivery_min_paise),
            "FREE_DELIVERY_MIN_RUPEES must be between 0 and {}",
            Self::MAX_BULK_MIN_ORDER_RUPEES
        );
        Ok(())
    }
}

/// Google OAuth "client" credentials (Cloud Console → APIs & Services →
/// Credentials). Needed by the sync worker and by the `google-auth` CLI.
#[derive(Clone)]
pub struct GoogleOAuthClientConfig {
    /// `GOOGLE_OAUTH_CLIENT_ID` (`….apps.googleusercontent.com`; not secret).
    pub client_id: String,
    /// `GOOGLE_OAUTH_CLIENT_SECRET`.
    pub client_secret: SecretString,
    /// Token endpoint (`GOOGLE_OAUTH_TOKEN_URL`; overridable only for tests).
    pub token_url: String,
}

impl std::fmt::Debug for GoogleOAuthClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleOAuthClientConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[redacted]")
            .field("token_url", &self.token_url)
            .finish()
    }
}

impl GoogleOAuthClientConfig {
    /// Client ID + secret only (all `google-auth` needs).
    pub fn from_env() -> anyhow::Result<Self> {
        let missing: Vec<&str> = ["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET"]
            .into_iter()
            .filter(|k| var_or(k, "").trim().is_empty())
            .collect();
        ensure!(
            missing.is_empty(),
            "missing {} (Google Cloud Console → APIs & Services → Credentials → your OAuth 2.0 client)",
            missing.join(", ")
        );
        let cfg = GoogleOAuthClientConfig {
            client_id: var_or("GOOGLE_OAUTH_CLIENT_ID", "").trim().to_owned(),
            client_secret: SecretString::from(
                var_or("GOOGLE_OAUTH_CLIENT_SECRET", "").trim().to_owned(),
            ),
            token_url: var_or("GOOGLE_OAUTH_TOKEN_URL", GOOGLE_TOKEN_URL)
                .trim()
                .to_owned(),
        };
        ensure!(
            cfg.client_id.ends_with(".apps.googleusercontent.com")
                && !cfg
                    .client_id
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control()),
            "GOOGLE_OAUTH_CLIENT_ID must be an OAuth client ID ending in .apps.googleusercontent.com"
        );
        ensure!(
            !cfg.client_secret
                .expose_secret()
                .chars()
                .any(|c| c.is_whitespace() || c.is_control()),
            "GOOGLE_OAUTH_CLIENT_SECRET must not contain spaces"
        );
        ensure!(
            cfg.token_url.starts_with("https://") || cfg.token_url.starts_with("http://"),
            "GOOGLE_OAUTH_TOKEN_URL must be an http(s) URL"
        );
        Ok(cfg)
    }
}

/// Google Sheets order sync (`GOOGLE_SHEETS_*`, `GOOGLE_OAUTH_*`). See README
/// "Google Sheets order sync".
#[derive(Clone)]
pub struct GoogleSheetsConfig {
    pub oauth: GoogleOAuthClientConfig,
    /// Long-lived OAuth refresh token from `zhimart-backend google-auth`
    /// (`GOOGLE_OAUTH_REFRESH_TOKEN`).
    pub refresh_token: SecretString,
    /// The ID from the sheet URL `…/spreadsheets/d/<ID>/edit`
    /// (`GOOGLE_SHEETS_SPREADSHEET_ID`; a full URL is accepted too).
    pub spreadsheet_id: String,
    /// Tab (sheet) name; created if missing (`GOOGLE_SHEETS_TAB`, default `Orders`).
    pub tab: String,
    /// Sheets REST API origin (`GOOGLE_SHEETS_API_BASE`; overridable only for tests).
    pub api_base: String,
}

impl std::fmt::Debug for GoogleSheetsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleSheetsConfig")
            .field("oauth", &self.oauth)
            .field("refresh_token", &"[redacted]")
            .field("spreadsheet_id", &self.spreadsheet_id)
            .field("tab", &self.tab)
            .field("api_base", &self.api_base)
            .finish()
    }
}

/// Cashfree environment (`CASHFREE_ENV`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CashfreeEnv {
    /// Test credentials; no real money moves.
    Sandbox,
    /// Live credentials; payments are real.
    Production,
}

pub const CASHFREE_SANDBOX_API: &str = "https://sandbox.cashfree.com/pg";
pub const CASHFREE_PRODUCTION_API: &str = "https://api.cashfree.com/pg";

impl CashfreeEnv {
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sandbox" | "test" => Ok(CashfreeEnv::Sandbox),
            "production" | "prod" | "live" => Ok(CashfreeEnv::Production),
            other => bail!("CASHFREE_ENV must be 'sandbox' or 'production', got '{other}'"),
        }
    }

    /// Also the JS SDK `mode` (`Cashfree({ mode })`).
    pub fn as_str(self) -> &'static str {
        match self {
            CashfreeEnv::Sandbox => "sandbox",
            CashfreeEnv::Production => "production",
        }
    }

    pub fn api_base(self) -> &'static str {
        match self {
            CashfreeEnv::Sandbox => CASHFREE_SANDBOX_API,
            CashfreeEnv::Production => CASHFREE_PRODUCTION_API,
        }
    }
}

/// Names of `RAZORPAY_*` variables still present in the environment. Razorpay
/// support was removed; they are ignored (main logs a warning naming them).
pub fn legacy_razorpay_vars() -> Vec<String> {
    let mut v: Vec<String> = env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| k.starts_with("RAZORPAY_"))
        .collect();
    v.sort();
    v
}

pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_SHEETS_API: &str = "https://sheets.googleapis.com";
pub const GOOGLE_SHEETS_DEFAULT_TAB: &str = "Orders";

impl GoogleSheetsConfig {
    /// `None` unless `GOOGLE_SHEETS_ENABLED=true`; when enabled every setting
    /// is required and validated (fail fast at boot).
    pub fn from_env(app_env: AppEnv) -> anyhow::Result<Option<Self>> {
        if !parse_or::<bool>("GOOGLE_SHEETS_ENABLED", "false")? {
            return Ok(None);
        }
        Self::load(app_env)
            .context("GOOGLE_SHEETS_ENABLED=true but the Google Sheets settings are incomplete")
            .map(Some)
    }

    /// Read and validate the settings regardless of `GOOGLE_SHEETS_ENABLED`
    /// (used by `google-sheets-check`).
    pub fn load(app_env: AppEnv) -> anyhow::Result<Self> {
        let missing: Vec<&str> = [
            "GOOGLE_OAUTH_CLIENT_ID",
            "GOOGLE_OAUTH_CLIENT_SECRET",
            "GOOGLE_OAUTH_REFRESH_TOKEN",
            "GOOGLE_SHEETS_SPREADSHEET_ID",
        ]
        .into_iter()
        .filter(|k| var_or(k, "").trim().is_empty())
        .collect();
        ensure!(
            missing.is_empty(),
            "missing {} (see README \"Google Sheets order sync\"; the refresh token comes from `zhimart-backend google-auth`)",
            missing.join(", ")
        );
        let cfg = GoogleSheetsConfig {
            oauth: GoogleOAuthClientConfig::from_env()?,
            refresh_token: SecretString::from(
                var_or("GOOGLE_OAUTH_REFRESH_TOKEN", "").trim().to_owned(),
            ),
            spreadsheet_id: normalize_spreadsheet_id(&var_or("GOOGLE_SHEETS_SPREADSHEET_ID", "")),
            tab: var_or("GOOGLE_SHEETS_TAB", GOOGLE_SHEETS_DEFAULT_TAB)
                .trim()
                .to_owned(),
            api_base: var_or("GOOGLE_SHEETS_API_BASE", GOOGLE_SHEETS_API)
                .trim()
                .trim_end_matches('/')
                .to_owned(),
        };
        cfg.validate(app_env)?;
        Ok(cfg)
    }

    fn validate(&self, app_env: AppEnv) -> anyhow::Result<()> {
        ensure!(
            !self
                .refresh_token
                .expose_secret()
                .chars()
                .any(|c| c.is_whitespace() || c.is_control()),
            "GOOGLE_OAUTH_REFRESH_TOKEN must not contain spaces"
        );
        ensure!(
            (20..=128).contains(&self.spreadsheet_id.len())
                && self
                    .spreadsheet_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "GOOGLE_SHEETS_SPREADSHEET_ID must be the ID from the sheet URL: https://docs.google.com/spreadsheets/d/<ID>/edit"
        );
        ensure!(
            (1..=100).contains(&self.tab.chars().count())
                && !self.tab.chars().any(char::is_control),
            "GOOGLE_SHEETS_TAB must be 1-100 characters"
        );
        ensure!(
            self.api_base.starts_with("https://") || self.api_base.starts_with("http://"),
            "GOOGLE_SHEETS_API_BASE must be an http(s) URL"
        );
        if app_env.is_production() {
            ensure!(
                self.oauth.token_url == GOOGLE_TOKEN_URL,
                "GOOGLE_OAUTH_TOKEN_URL cannot be overridden in production"
            );
            ensure!(
                self.api_base == GOOGLE_SHEETS_API,
                "GOOGLE_SHEETS_API_BASE cannot be overridden in production"
            );
        }
        Ok(())
    }
}

/// Accept either the bare ID or a pasted sheet URL
/// (`https://docs.google.com/spreadsheets/d/<ID>/edit#gid=0`).
pub fn normalize_spreadsheet_id(raw: &str) -> String {
    let raw = raw.trim();
    match raw.split_once("/spreadsheets/d/") {
        Some((_, rest)) => rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .to_owned(),
        None => raw.to_owned(),
    }
}

/// Storefront origin assumed in development when `PUBLIC_SITE_URL` is unset (Vite dev server).
const DEV_SITE_URL: &str = "http://localhost:5173";

/// Unit price charged for every product while `PAYMENT_TEST_MODE=true`.
pub const TEST_MODE_UNIT_PRICE_PAISE: i64 = 100;

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let app_env = app_env_from_env()?;

        let email_provider = match var_or("EMAIL_PROVIDER", "ses")
            .to_ascii_lowercase()
            .as_str()
        {
            "ses" => EmailProvider::Ses,
            "log" => EmailProvider::Log,
            other => bail!("EMAIL_PROVIDER must be 'ses' or 'log', got '{other}'"),
        };

        let allowed_origins = var_or("ALLOWED_ORIGINS", "")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|o| {
                ensure!(
                    o.starts_with("https://") || o.starts_with("http://"),
                    "ALLOWED_ORIGINS entry '{o}' must include a scheme"
                );
                ensure!(
                    !o.ends_with('/'),
                    "ALLOWED_ORIGINS entry '{o}' must not end with '/'"
                );
                HeaderValue::from_str(o).with_context(|| format!("invalid origin '{o}'"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let trusted_proxies = var_or("TRUSTED_PROXIES", "")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|p| {
                let net = IpNetwork::from_str(p).with_context(|| format!("invalid TRUSTED_PROXIES entry '{p}'"))?;
                // Trusting a huge range makes X-Forwarded-For spoofable by anyone in it.
                let min_prefix = if net.is_ipv4() { 8 } else { 32 };
                ensure!(
                    net.prefix() >= min_prefix,
                    "TRUSTED_PROXIES entry '{p}' is too broad (minimum /{min_prefix}); list your proxy/LB ranges only"
                );
                Ok(net)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let cashfree_env = CashfreeEnv::parse(&var_or("CASHFREE_ENV", "sandbox"))?;

        let cfg = Config {
            app_env,
            bind_addr: parse_or("BIND_ADDR", "0.0.0.0:8080")?,

            database_url: SecretString::from(required("DATABASE_URL")?),
            db_max_connections: parse_or("DB_MAX_CONNECTIONS", "20")?,
            db_min_connections: parse_or("DB_MIN_CONNECTIONS", "2")?,
            db_acquire_timeout: Duration::from_secs(parse_or("DB_ACQUIRE_TIMEOUT_SECS", "5")?),

            jwt_secret: SecretString::from(required("JWT_SECRET")?),
            jwt_issuer: var_or("JWT_ISSUER", "zhimart-api"),
            jwt_audience: var_or("JWT_AUDIENCE", "zhimart-admin"),
            access_token_ttl: Duration::from_secs(parse_or("ACCESS_TOKEN_TTL_SECS", "900")?),
            session_ttl: Duration::from_secs(parse_or("SESSION_TTL_SECS", "2592000")?),

            otp_pepper: SecretString::from(required("OTP_PEPPER")?),
            otp_ttl: Duration::from_secs(parse_or("OTP_TTL_SECS", "300")?),
            otp_max_attempts: parse_or("OTP_MAX_ATTEMPTS", "5")?,
            otp_resend_cooldown: Duration::from_secs(parse_or("OTP_RESEND_COOLDOWN_SECS", "60")?),
            otp_max_resends: parse_or("OTP_MAX_RESENDS", "3")?,
            login_max_failures: parse_or("LOGIN_MAX_FAILURES", "5")?,
            login_lockout: Duration::from_secs(parse_or("LOGIN_LOCKOUT_SECS", "900")?),

            cookie_secure: parse_or("COOKIE_SECURE", "true")?,
            allowed_origins,

            trusted_proxies,
            rate_limit_global_per_minute: parse_or("RATE_LIMIT_GLOBAL_PER_MINUTE", "300")?,
            rate_limit_auth_per_minute: parse_or("RATE_LIMIT_AUTH_PER_MINUTE", "10")?,
            request_timeout: Duration::from_secs(parse_or("REQUEST_TIMEOUT_SECS", "15")?),
            max_body_bytes: parse_or("MAX_BODY_BYTES", "65536")?,
            max_concurrent_requests: parse_or("MAX_CONCURRENT_REQUESTS", "1024")?,

            email_provider,
            ses_from_address: var_or("SES_FROM_ADDRESS", ""),
            ses_configuration_set: env::var("SES_CONFIGURATION_SET")
                .ok()
                .filter(|s| !s.trim().is_empty()),

            payments_enabled: parse_or("CASHFREE_ENABLED", "false")?,
            payment_test_mode: parse_or("PAYMENT_TEST_MODE", "false")?,
            cashfree_env,
            cashfree_app_id: var_or("CASHFREE_APP_ID", "").trim().to_owned(),
            cashfree_secret_key: SecretString::from(
                var_or("CASHFREE_SECRET_KEY", "").trim().to_owned(),
            ),
            cashfree_api_base: var_or("CASHFREE_API_BASE", cashfree_env.api_base())
                .trim()
                .trim_end_matches('/')
                .to_owned(),
            cashfree_notify_url: env::var("CASHFREE_NOTIFY_URL")
                .ok()
                .map(|u| u.trim().to_owned())
                .filter(|u| !u.is_empty()),
            payment_expiry: Duration::from_secs(
                60 * parse_or::<u64>("PAYMENT_EXPIRY_MINUTES", "30")?,
            ),
            store_name: var_or("STORE_NAME", "Zhi-Mart Fireworks"),

            order_notifications_enabled: parse_or("ORDER_NOTIFICATION_ENABLED", "true")?,
            order_notification_emails: var_or("ORDER_NOTIFICATION_EMAILS", "")
                .split(',')
                .map(|e| e.trim().to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
            public_site_url: env::var("PUBLIC_SITE_URL")
                .ok()
                .map(|u| u.trim().trim_end_matches('/').to_owned())
                .filter(|u| !u.is_empty()),
            customer_order_emails_enabled: parse_or("CUSTOMER_ORDER_EMAIL_ENABLED", "true")?,

            pricing: Pricing {
                bulk_min_order_paise: parse_or::<i64>("BULK_MIN_ORDER_RUPEES", "5000")?
                    .checked_mul(100)
                    .context("BULK_MIN_ORDER_RUPEES is out of range")?,
                standard_discount_percent: parse_or("STANDARD_DISCOUNT_PERCENT", "80")?,
                bulk_discount_percent: parse_or("BULK_DISCOUNT_PERCENT", "85")?,
                delivery_fee_paise: parse_or::<i64>("DELIVERY_FEE_RUPEES", "500")?
                    .checked_mul(100)
                    .context("DELIVERY_FEE_RUPEES is out of range")?,
                free_delivery_min_paise: parse_or::<i64>("FREE_DELIVERY_MIN_RUPEES", "5000")?
                    .checked_mul(100)
                    .context("FREE_DELIVERY_MIN_RUPEES is out of range")?,
            },

            google_sheets: GoogleSheetsConfig::from_env(app_env)?,
        };

        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.jwt_secret.expose_secret().len() >= 32,
            "JWT_SECRET must be at least 32 bytes (generate with: openssl rand -base64 48)"
        );
        ensure!(
            self.otp_pepper.expose_secret().len() >= 32,
            "OTP_PEPPER must be at least 32 bytes (generate with: openssl rand -base64 48)"
        );
        ensure!(
            self.jwt_secret.expose_secret() != self.otp_pepper.expose_secret(),
            "JWT_SECRET and OTP_PEPPER must be different secrets"
        );
        ensure!(
            (60..=3600).contains(&self.access_token_ttl.as_secs()),
            "ACCESS_TOKEN_TTL_SECS must be between 60 and 3600"
        );
        ensure!(
            self.session_ttl > self.access_token_ttl && self.session_ttl <= MAX_SESSION_TTL,
            "SESSION_TTL_SECS must be greater than the access token TTL and at most 30 days"
        );
        ensure!(
            (60..=900).contains(&self.otp_ttl.as_secs()),
            "OTP_TTL_SECS must be between 60 and 900"
        );
        ensure!(
            (1..=10).contains(&self.otp_max_attempts),
            "OTP_MAX_ATTEMPTS must be between 1 and 10"
        );
        ensure!(
            (0..=10).contains(&self.otp_max_resends),
            "OTP_MAX_RESENDS must be between 0 and 10"
        );
        ensure!(
            (1..=20).contains(&self.login_max_failures),
            "LOGIN_MAX_FAILURES must be between 1 and 20"
        );
        ensure!(
            self.db_min_connections <= self.db_max_connections,
            "DB_MIN_CONNECTIONS > DB_MAX_CONNECTIONS"
        );
        ensure!(
            self.rate_limit_auth_per_minute > 0,
            "RATE_LIMIT_AUTH_PER_MINUTE must be > 0"
        );
        ensure!(
            self.rate_limit_global_per_minute > 0,
            "RATE_LIMIT_GLOBAL_PER_MINUTE must be > 0"
        );
        ensure!(
            self.max_body_bytes >= 1024,
            "MAX_BODY_BYTES must be at least 1024"
        );
        ensure!(
            self.max_concurrent_requests > 0,
            "MAX_CONCURRENT_REQUESTS must be > 0"
        );
        self.pricing.validate()?;

        if self.email_provider == EmailProvider::Ses {
            ensure!(
                self.ses_from_address.contains('@'),
                "SES_FROM_ADDRESS must be set to a verified SES identity when EMAIL_PROVIDER=ses"
            );
        }

        self.validate_cashfree()?;
        for e in &self.order_notification_emails {
            ensure!(
                e.len() <= 254
                    && e.contains('@')
                    && !e.chars().any(|c| c.is_whitespace() || c.is_control()),
                "ORDER_NOTIFICATION_EMAILS contains an invalid address '{e}'"
            );
        }
        ensure!(
            self.order_notification_emails.len() <= 20,
            "ORDER_NOTIFICATION_EMAILS: at most 20 recipients"
        );
        if let Some(url) = &self.public_site_url {
            ensure!(
                url.starts_with("https://")
                    || (!self.app_env.is_production() && url.starts_with("http://")),
                "PUBLIC_SITE_URL must be an https:// URL"
            );
        }
        ensure!(
            (10..=1440).contains(&(self.payment_expiry.as_secs() / 60)),
            "PAYMENT_EXPIRY_MINUTES must be between 10 and 1440"
        );
        ensure!(
            !self.store_name.trim().is_empty() && self.store_name.len() <= 80,
            "STORE_NAME must be 1-80 characters"
        );

        if self.app_env.is_production() {
            ensure!(
                !self.customer_order_emails_enabled || self.public_site_url.is_some(),
                "PUBLIC_SITE_URL must be set in production when CUSTOMER_ORDER_EMAIL_ENABLED=true (it builds the customer's tracking link)"
            );
            ensure!(
                self.cookie_secure,
                "COOKIE_SECURE must be true in production"
            );
            ensure!(
                self.email_provider == EmailProvider::Ses,
                "EMAIL_PROVIDER=log is not allowed in production"
            );
            ensure!(
                !self.allowed_origins.is_empty(),
                "ALLOWED_ORIGINS must be set in production"
            );
            ensure!(
                self.allowed_origins
                    .iter()
                    .all(|o| o.as_bytes().starts_with(b"https://")),
                "ALLOWED_ORIGINS must all be https:// in production"
            );
        }
        Ok(())
    }

    /// Cashfree settings. The API base / notify URL are checked whatever the
    /// switch says (they are cheap to get right); credentials only when enabled.
    fn validate_cashfree(&self) -> anyhow::Result<()> {
        let prod = self.app_env.is_production();
        ensure!(
            self.cashfree_api_base.starts_with("https://")
                || (!prod && self.cashfree_api_base.starts_with("http://")),
            "CASHFREE_API_BASE must be an https:// URL"
        );
        if let Some(u) = &self.cashfree_notify_url {
            ensure!(
                u.starts_with("https://")
                    && u.len() <= 250
                    && !u.chars().any(|c| c.is_whitespace() || c.is_control()),
                "CASHFREE_NOTIFY_URL must be an https:// URL of at most 250 characters (Cashfree requirement)"
            );
        }
        if prod {
            ensure!(
                self.cashfree_api_base == self.cashfree_env.api_base(),
                "CASHFREE_API_BASE cannot be overridden in production (set CASHFREE_ENV instead)"
            );
        }
        if !self.payments_enabled {
            return Ok(());
        }
        let app_id = self.cashfree_app_id.as_str();
        ensure!(
            (4..=100).contains(&app_id.len())
                && app_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "CASHFREE_ENABLED=true requires CASHFREE_APP_ID (Cashfree Merchant Dashboard → Developers → API Keys)"
        );
        let secret = self.cashfree_secret_key.expose_secret();
        ensure!(
            secret.len() >= 16
                && secret.len() <= 200
                && !secret.chars().any(|c| c.is_whitespace() || c.is_control()),
            "CASHFREE_ENABLED=true requires CASHFREE_SECRET_KEY (min 16 chars, no spaces)"
        );
        ensure!(
            secret != app_id,
            "CASHFREE_SECRET_KEY must not equal CASHFREE_APP_ID"
        );
        if prod {
            ensure!(
                self.payment_test_mode || self.cashfree_env == CashfreeEnv::Production,
                "APP_ENV=production with CASHFREE_ENABLED=true and PAYMENT_TEST_MODE=false requires CASHFREE_ENV=production (live keys)"
            );
            ensure!(
                self.public_site_url.is_some(),
                "PUBLIC_SITE_URL must be set in production when CASHFREE_ENABLED=true (it builds the Cashfree return_url; whitelist its domain in the Cashfree dashboard)"
            );
        }
        ensure!(
            self.cashfree_return_url("ZM-000000-0000000000000000000")
                .len()
                <= 250,
            "PUBLIC_SITE_URL is too long for a Cashfree return_url (max 250 characters)"
        );
        Ok(())
    }

    /// Where Cashfree sends the customer after checkout (redirect flows). It
    /// deliberately carries NO order token (Cashfree would see and log it):
    /// the storefront recovers the token from sessionStorage.
    pub fn cashfree_return_url(&self, order_number: &str) -> String {
        format!(
            "{}/?page=order&order={order_number}&cf_return=1",
            self.site_url()
        )
    }

    /// `true` when Cashfree runs against the sandbox (no real money moves).
    pub fn cashfree_is_sandbox(&self) -> bool {
        self.cashfree_env == CashfreeEnv::Sandbox
    }

    /// Name of the refresh-token cookie. The `__Secure-` prefix makes browsers
    /// refuse the cookie unless it was set over HTTPS with the Secure flag.
    pub fn refresh_cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Secure-zm_rt"
        } else {
            "zm_rt"
        }
    }

    /// Origin used to build links in emails: `PUBLIC_SITE_URL`, or the Vite dev
    /// server in development. (Production always has `PUBLIC_SITE_URL` when it matters — validated.)
    pub fn site_url(&self) -> &str {
        self.public_site_url.as_deref().unwrap_or(DEV_SITE_URL)
    }

    /// Customer tracking page for an order. The token sits in the URL *fragment*,
    /// so it is never sent to any server or leaked via `Referer`.
    pub fn order_tracking_url(&self, order_number: &str, order_token: &str) -> String {
        format!(
            "{}/?page=order&order={order_number}#t={order_token}",
            self.site_url()
        )
    }

    /// The pricing actually charged (and published to the storefront). Under
    /// `PAYMENT_TEST_MODE` the delivery charge is capped at
    /// [`TEST_MODE_UNIT_PRICE_PAISE`] (₹1) like every unit price, so test
    /// payments stay cheap but still exercise the delivery line.
    pub fn effective_pricing(&self) -> Pricing {
        let mut p = self.pricing;
        if self.payment_test_mode {
            p.delivery_fee_paise = p.delivery_fee_paise.min(TEST_MODE_UNIT_PRICE_PAISE);
        }
        p
    }

    pub fn is_allowed_origin(&self, origin: &[u8]) -> bool {
        self.allowed_origins.iter().any(|o| o.as_bytes() == origin)
    }
}

/// `APP_ENV` (default `production`).
pub fn app_env_from_env() -> anyhow::Result<AppEnv> {
    match var_or("APP_ENV", "production")
        .to_ascii_lowercase()
        .as_str()
    {
        "production" | "prod" => Ok(AppEnv::Production),
        "development" | "dev" | "local" => Ok(AppEnv::Development),
        other => bail!("APP_ENV must be 'production' or 'development', got '{other}'"),
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    match env::var(key) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => bail!("missing required environment variable {key}"),
    }
}

fn var_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn parse_or<T>(key: &str, default: &str) -> anyhow::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let raw = var_or(key, default);
    raw.trim()
        .parse::<T>()
        .map_err(|e| anyhow::anyhow!("invalid value for {key}: {e}"))
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::{
        AppEnv, CASHFREE_PRODUCTION_API, CASHFREE_SANDBOX_API, CashfreeEnv, Config, EmailProvider,
        Pricing, normalize_spreadsheet_id, test_support,
    };

    type Tweak = Box<dyn Fn(&mut Config)>;

    /// A production config that passes validation, Cashfree live.
    fn prod() -> Config {
        let mut c = test_support::config();
        c.app_env = AppEnv::Production;
        c.email_provider = EmailProvider::Ses;
        c.ses_from_address = "no-reply@zhimart.in".into();
        c.public_site_url = Some("https://zhimart.in".into());
        c.cashfree_env = CashfreeEnv::Production;
        c.cashfree_api_base = CASHFREE_PRODUCTION_API.into();
        c
    }

    #[test]
    fn cashfree_env_parsing_and_bases() {
        assert_eq!(
            CashfreeEnv::parse("sandbox").ok(),
            Some(CashfreeEnv::Sandbox)
        );
        assert_eq!(
            CashfreeEnv::parse(" PRODUCTION ").ok(),
            Some(CashfreeEnv::Production)
        );
        assert!(CashfreeEnv::parse("staging").is_err());
        assert_eq!(CashfreeEnv::Sandbox.api_base(), CASHFREE_SANDBOX_API);
        assert_eq!(CashfreeEnv::Production.as_str(), "production");
    }

    #[test]
    fn cashfree_validation() {
        assert!(
            test_support::config().validate().is_ok(),
            "dev sandbox config"
        );
        assert!(prod().validate().is_ok(), "{:?}", prod().validate().err());

        let bad: Vec<(&str, Tweak)> = vec![
            ("missing app id", Box::new(|c| c.cashfree_app_id.clear())),
            (
                "app id with spaces",
                Box::new(|c| c.cashfree_app_id = "TEST 123".into()),
            ),
            (
                "short secret",
                Box::new(|c| c.cashfree_secret_key = SecretString::from("short")),
            ),
            (
                "secret with spaces",
                Box::new(|c| c.cashfree_secret_key = SecretString::from("cfsk ma prod 0123456789")),
            ),
            (
                "sandbox in production",
                Box::new(|c| {
                    c.cashfree_env = CashfreeEnv::Sandbox;
                    c.cashfree_api_base = CASHFREE_SANDBOX_API.into();
                }),
            ),
            (
                "api base override",
                Box::new(|c| c.cashfree_api_base = "https://evil.example.com/pg".into()),
            ),
            ("no site url", Box::new(|c| c.public_site_url = None)),
            (
                "http notify url",
                Box::new(|c| {
                    c.cashfree_notify_url =
                        Some("http://api.zhimart.in/api/v1/webhooks/cashfree".into())
                }),
            ),
        ];
        for (why, tweak) in bad {
            let mut c = prod();
            tweak(&mut c);
            assert!(c.validate().is_err(), "{why} must be refused");
        }

        // Sandbox is fine in production while PAYMENT_TEST_MODE=true (₹1 orders).
        let mut c = prod();
        c.payment_test_mode = true;
        c.cashfree_env = CashfreeEnv::Sandbox;
        c.cashfree_api_base = CASHFREE_SANDBOX_API.into();
        assert!(c.validate().is_ok());

        // Disabled: no credentials needed, but the API base is still pinned in production.
        let mut c = prod();
        c.payments_enabled = false;
        c.cashfree_app_id.clear();
        c.cashfree_secret_key = SecretString::from("");
        c.public_site_url = None;
        assert!(c.validate().is_ok(), "{:?}", c.validate().err());
        c.cashfree_api_base = "http://127.0.0.1:9/pg".into();
        assert!(c.validate().is_err());

        // Development may point at a local mock over plain http.
        let mut c = test_support::config();
        c.cashfree_api_base = "http://127.0.0.1:9/pg".into();
        assert!(c.validate().is_ok());
        c.cashfree_notify_url = Some("https://api.zhimart.in/api/v1/webhooks/cashfree".into());
        assert!(c.validate().is_ok());
    }

    #[test]
    fn return_url_has_no_token() {
        let c = prod();
        assert_eq!(
            c.cashfree_return_url("ZM-261020-1001"),
            "https://zhimart.in/?page=order&order=ZM-261020-1001&cf_return=1"
        );
        assert!(!c.cashfree_return_url("ZM-261020-1001").contains('#'));
    }

    #[test]
    fn spreadsheet_id_from_url_or_bare() {
        let id = "1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-abcd";
        assert_eq!(normalize_spreadsheet_id(id), id);
        assert_eq!(normalize_spreadsheet_id(&format!("  {id} ")), id);
        assert_eq!(
            normalize_spreadsheet_id(&format!(
                "https://docs.google.com/spreadsheets/d/{id}/edit#gid=0"
            )),
            id
        );
        assert_eq!(
            normalize_spreadsheet_id(&format!("https://docs.google.com/spreadsheets/d/{id}")),
            id
        );
    }

    #[test]
    fn pricing_defaults_and_tier() {
        let p = Pricing::default();
        assert!(p.validate().is_ok());
        assert_eq!(p.bulk_min_order_paise, 500_000);
        assert!(p.is_bulk(500_000), "exactly ₹5,000 is bulk");
        assert!(!p.is_bulk(499_999));
        // 80 % off MRP ₹210 = ₹42.
        assert_eq!(p.standard_discount_percent, 80);
        assert_eq!(p.standard_price_paise(3_200, 21_000), 4_200);
    }

    #[test]
    fn delivery_fee_below_free_minimum_only() {
        let p = Pricing::default();
        assert_eq!(p.delivery_fee_paise, 50_000);
        assert_eq!(p.free_delivery_min_paise, 500_000);
        assert_eq!(p.delivery_fee_for(100), 50_000);
        assert_eq!(p.delivery_fee_for(499_999), 50_000);
        assert_eq!(p.delivery_fee_for(500_000), 0, "exactly ₹5,000 ships free");
        assert_eq!(p.delivery_fee_for(1_000_000), 0);
        let always_free = Pricing {
            free_delivery_min_paise: 0,
            ..p
        };
        assert_eq!(always_free.delivery_fee_for(100), 0);
    }

    #[test]
    fn pricing_validation() {
        let ok = Pricing::default();
        let bad = [
            Pricing {
                bulk_min_order_paise: -100,
                ..ok
            },
            Pricing {
                bulk_min_order_paise: (Pricing::MAX_BULK_MIN_ORDER_RUPEES + 1) * 100,
                ..ok
            },
            Pricing {
                standard_discount_percent: 100,
                bulk_discount_percent: 100,
                ..ok
            },
            Pricing {
                bulk_discount_percent: 100,
                ..ok
            },
            Pricing {
                standard_discount_percent: 90,
                ..ok
            },
            Pricing {
                delivery_fee_paise: -1,
                ..ok
            },
            Pricing {
                delivery_fee_paise: (Pricing::MAX_DELIVERY_FEE_RUPEES + 1) * 100,
                ..ok
            },
            Pricing {
                free_delivery_min_paise: -100,
                ..ok
            },
            Pricing {
                free_delivery_min_paise: (Pricing::MAX_BULK_MIN_ORDER_RUPEES + 1) * 100,
                ..ok
            },
        ];
        for p in bad {
            assert!(p.validate().is_err(), "{p:?}");
        }
        assert!(
            Pricing {
                bulk_min_order_paise: 0,
                ..ok
            }
            .validate()
            .is_ok(),
            "0 = always bulk"
        );
        assert!(
            Pricing {
                delivery_fee_paise: 0,
                free_delivery_min_paise: 0,
                ..ok
            }
            .validate()
            .is_ok(),
            "no delivery charge"
        );
    }
}

/// Shared `Config` fixture for unit tests across modules.
#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) fn config() -> super::Config {
        crate::auth::jwt::tests::test_config()
    }
}
