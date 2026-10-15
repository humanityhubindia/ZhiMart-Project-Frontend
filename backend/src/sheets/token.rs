//! OAuth 2.0 access tokens for the Sheets API, minted from the long-lived
//! refresh token (`grant_type=refresh_token`) and cached in memory until
//! shortly before they expire.
//!
//! Tokens, the client secret and the refresh token are never logged; errors
//! carry Google's `error` code only (e.g. `invalid_grant`), never the body.

use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::{sync::Mutex, time::Instant};

use crate::config::GoogleOAuthClientConfig;

use super::SheetsError;

/// Refresh this long before Google's stated expiry, so a token never expires
/// mid-request (also absorbs clock skew and slow requests).
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(5 * 60);
/// Upper bound on a cached token's life whatever `expires_in` says.
const MAX_TOKEN_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// A cached access token.
pub struct CachedToken {
    value: SecretString,
    expires_at: Instant,
}

impl CachedToken {
    /// A token Google says is valid for `expires_in_secs` from `now`.
    pub fn new(value: SecretString, expires_in_secs: u64, now: Instant) -> Self {
        let lifetime = Duration::from_secs(expires_in_secs).min(MAX_TOKEN_LIFETIME);
        Self {
            value,
            expires_at: now + lifetime,
        }
    }

    /// Usable at `now` only while more than [`EXPIRY_MARGIN`] remains.
    pub fn is_fresh(&self, now: Instant) -> bool {
        now + EXPIRY_MARGIN < self.expires_at
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize, Default)]
struct TokenError {
    #[serde(default)]
    error: String,
}

/// Single-flight token cache: concurrent callers wait for one refresh.
pub struct TokenSource {
    http: reqwest::Client,
    oauth: GoogleOAuthClientConfig,
    refresh_token: SecretString,
    cache: Mutex<Option<CachedToken>>,
}

impl std::fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSource")
            .field("client_id", &self.oauth.client_id)
            .field("token_url", &self.oauth.token_url)
            .finish_non_exhaustive()
    }
}

impl TokenSource {
    pub fn new(
        http: reqwest::Client,
        oauth: GoogleOAuthClientConfig,
        refresh_token: SecretString,
    ) -> Self {
        Self {
            http,
            oauth,
            refresh_token,
            cache: Mutex::new(None),
        }
    }

    /// A fresh access token (cached, or refreshed now).
    pub async fn access_token(&self) -> Result<SecretString, SheetsError> {
        let mut cache = self.cache.lock().await;
        if let Some(t) = cache.as_ref().filter(|t| t.is_fresh(Instant::now())) {
            return Ok(t.value.clone());
        }
        let token = self.refresh().await?;
        let value = token.value.clone();
        *cache = Some(token);
        Ok(value)
    }

    /// Drop the cached token (after a 401), so the next call refreshes.
    pub async fn invalidate(&self) {
        *self.cache.lock().await = None;
    }

    async fn refresh(&self) -> Result<CachedToken, SheetsError> {
        let started = Instant::now();
        let form = [
            ("grant_type", "refresh_token"),
            ("client_id", self.oauth.client_id.as_str()),
            ("client_secret", self.oauth.client_secret.expose_secret()),
            ("refresh_token", self.refresh_token.expose_secret()),
        ];
        let res = self
            .http
            .post(&self.oauth.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| SheetsError::Transient(transport_summary("token refresh", &e)))?;
        let status = res.status();
        let body = read_capped(res).await?;
        if status.is_success() {
            let parsed: TokenResponse = serde_json::from_slice(&body).map_err(|_| {
                SheetsError::Transient("token endpoint returned an unreadable response".into())
            })?;
            if parsed.access_token.is_empty() {
                return Err(SheetsError::Transient(
                    "token endpoint returned no access token".into(),
                ));
            }
            let token = CachedToken::new(
                SecretString::from(parsed.access_token),
                parsed.expires_in.unwrap_or(3600),
                started,
            );
            tracing::debug!(
                expires_in = parsed.expires_in,
                "google sheets: access token refreshed"
            );
            return Ok(token);
        }
        let code = serde_json::from_slice::<TokenError>(&body)
            .unwrap_or_default()
            .error;
        Err(classify_token_error(status.as_u16(), &code))
    }
}

/// Map a token-endpoint failure to an error class.
pub fn classify_token_error(status: u16, code: &str) -> SheetsError {
    match code {
        "invalid_grant" => SheetsError::AuthRevoked,
        "invalid_client" | "unauthorized_client" | "invalid_request" | "invalid_scope" => {
            SheetsError::AuthConfig(format!("token endpoint rejected the OAuth client: {code}"))
        }
        _ if status == 429 || status >= 500 => {
            SheetsError::Transient(format!("token endpoint returned HTTP {status}"))
        }
        _ => SheetsError::AuthConfig(format!(
            "token endpoint returned HTTP {status} {}",
            code.chars().take(60).collect::<String>()
        )),
    }
}

/// A transport error without the URL (which never carries secrets here, but
/// keep logs short and uniform).
pub(super) fn transport_summary(what: &str, e: &reqwest::Error) -> String {
    let kind = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "connection failed"
    } else {
        "request failed"
    };
    format!("{what}: {kind}")
}

pub(super) async fn read_capped(res: reqwest::Response) -> Result<Vec<u8>, SheetsError> {
    read_body(res, MAX_RESPONSE_BYTES).await
}

/// Read a response body, refusing anything larger than `cap`.
pub(super) async fn read_body(
    mut res: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, SheetsError> {
    let mut out = Vec::new();
    loop {
        match res.chunk().await {
            Ok(Some(chunk)) => {
                if out.len() + chunk.len() > cap {
                    return Err(SheetsError::Transient("response too large".into()));
                }
                out.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(out),
            Err(e) => {
                return Err(SheetsError::Transient(transport_summary(
                    "reading response",
                    &e,
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_fresh_until_margin_before_expiry() {
        let now = Instant::now();
        let t = CachedToken::new(SecretString::from("x"), 3600, now);
        assert!(t.is_fresh(now));
        assert!(t.is_fresh(now + Duration::from_secs(3600 - 301)));
        assert!(!t.is_fresh(now + Duration::from_secs(3600 - 300)), "margin");
        assert!(!t.is_fresh(now + Duration::from_secs(3600)));
        assert!(!t.is_fresh(now + Duration::from_secs(7200)));
    }

    #[test]
    fn short_or_huge_lifetimes() {
        let now = Instant::now();
        // Shorter than the margin: never served from cache.
        assert!(!CachedToken::new(SecretString::from("x"), 120, now).is_fresh(now));
        assert!(!CachedToken::new(SecretString::from("x"), 0, now).is_fresh(now));
        // Absurd expires_in is capped at 24 h.
        let t = CachedToken::new(SecretString::from("x"), u64::MAX / 4, now);
        assert!(!t.is_fresh(now + MAX_TOKEN_LIFETIME));
    }

    #[test]
    fn token_errors_classified() {
        assert!(matches!(
            classify_token_error(400, "invalid_grant"),
            SheetsError::AuthRevoked
        ));
        assert!(matches!(
            classify_token_error(401, "invalid_client"),
            SheetsError::AuthConfig(_)
        ));
        assert!(matches!(
            classify_token_error(503, ""),
            SheetsError::Transient(_)
        ));
        assert!(matches!(
            classify_token_error(429, ""),
            SheetsError::Transient(_)
        ));
        assert!(matches!(
            classify_token_error(403, "access_denied"),
            SheetsError::AuthConfig(_)
        ));
    }
}
