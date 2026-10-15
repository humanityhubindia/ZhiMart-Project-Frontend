//! Google Sheets order sync: one row per order in the client's spreadsheet,
//! appended when the order is placed and rewritten in place on every change.
//!
//! * [`row`]: order → row cells, header, finding rows by order number.
//! * [`token`]: OAuth access tokens from the refresh token (cached).
//! * [`client`]: the few Sheets REST calls we make.
//! * [`setup`]: the `google-auth` and `google-sheets-check` CLI commands.
//!
//! The durable queue (`sheet_sync_queue`, filled by DB triggers) and the
//! worker live in `repo::sheet_sync` and `jobs.rs`. Nothing here is ever on
//! the checkout path.

pub mod client;
pub mod row;
pub mod setup;
pub mod token;

pub use client::{SheetsClient, UpsertOutcome};

/// Why a Sheets sync attempt failed. Every variant is retried later by the
/// worker; they differ in backoff and in how loudly they are logged.
#[derive(Debug, Clone)]
pub enum SheetsError {
    /// Network error, timeout, 429 or 5xx: retry with short exponential backoff.
    Transient(String),
    /// The token endpoint answered `invalid_grant`: the refresh token was
    /// revoked or expired. Needs `zhimart-backend google-auth` again.
    AuthRevoked,
    /// The OAuth client (ID/secret) was rejected, or Google rejected a fresh
    /// access token.
    AuthConfig(String),
    /// 403/404 from Sheets: the spreadsheet ID is wrong, the authorising
    /// Google account cannot edit it, or the Sheets API is not enabled.
    Access { status: u16, message: String },
    /// Any other 4xx (should not happen; logged with the request kind).
    Rejected { status: u16, message: String },
}

impl SheetsError {
    pub fn is_transient(&self) -> bool {
        matches!(self, SheetsError::Transient(_))
    }

    /// Short, PII-free description for `sheet_sync_queue.last_error`.
    pub fn summary(&self) -> String {
        match self {
            SheetsError::Transient(m) => format!("transient: {m}"),
            SheetsError::AuthRevoked => {
                "refresh token revoked or expired (invalid_grant): run google-auth".to_owned()
            }
            SheetsError::AuthConfig(m) => format!("oauth client rejected: {m}"),
            SheetsError::Access { message, .. } => format!("no access: {message}"),
            SheetsError::Rejected { message, .. } => format!("rejected: {message}"),
        }
    }

    /// What the operator should do, for the log line.
    pub fn hint(&self) -> &'static str {
        match self {
            SheetsError::Transient(_) => {
                "Google is unreachable or rate-limiting; rows stay queued and are retried"
            }
            SheetsError::AuthRevoked => {
                "GOOGLE_OAUTH_REFRESH_TOKEN was revoked or has expired. Regenerate it with \
                 `zhimart-backend google-auth` (README \"Google Sheets order sync\"), update \
                 backend/.env and restart. Rows stay queued. If this happens every 7 days, publish \
                 the OAuth consent screen to \"In production\"."
            }
            SheetsError::AuthConfig(_) => {
                "check GOOGLE_OAUTH_CLIENT_ID / GOOGLE_OAUTH_CLIENT_SECRET (a rotated secret needs a \
                 new refresh token from `zhimart-backend google-auth`). Rows stay queued."
            }
            SheetsError::Access { .. } => {
                "check GOOGLE_SHEETS_SPREADSHEET_ID (the ID from the sheet URL), that the Google \
                 account used for google-auth can EDIT the sheet, and that the Google Sheets API is \
                 enabled in the Cloud project. Rows stay queued."
            }
            SheetsError::Rejected { .. } => "unexpected request error; rows stay queued",
        }
    }
}

impl std::fmt::Display for SheetsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

impl std::error::Error for SheetsError {}
