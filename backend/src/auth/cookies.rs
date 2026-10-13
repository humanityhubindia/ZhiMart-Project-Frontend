//! Auth cookies, both scoped to `/api/v1/auth` so they are only ever sent to
//! the auth endpoints:
//!
//! * the refresh-token cookie, which expires with the session's hard limit;
//! * the trusted-device cookie (set after a full password + OTP login), which
//!   exempts this browser from the account-wide failure ceiling.

use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};

use crate::config::Config;

pub const REFRESH_COOKIE_PATH: &str = "/api/v1/auth";

/// Lifetime of a trusted-device cookie (and its database row).
pub const DEVICE_TTL: std::time::Duration = std::time::Duration::from_secs(180 * 24 * 60 * 60);

/// `__Secure-zm_dev`, or `zm_dev` when `COOKIE_SECURE=false` (local http).
pub fn device_cookie_name(config: &Config) -> &'static str {
    if config.cookie_secure {
        "__Secure-zm_dev"
    } else {
        "zm_dev"
    }
}

fn base(config: &Config, value: String, max_age: time::Duration) -> Cookie<'static> {
    with_name(config, config.refresh_cookie_name(), value, max_age)
}

fn with_name(
    config: &Config,
    name: &'static str,
    value: String,
    max_age: time::Duration,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .secure(config.cookie_secure)
        .same_site(SameSite::Strict)
        .path(REFRESH_COOKIE_PATH)
        .max_age(max_age)
        .build()
}

/// Whole seconds from `now` until `session_expires_at` (never negative).
pub fn remaining_secs(session_expires_at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (session_expires_at - now).num_seconds().max(0)
}

/// Refresh cookie whose Max-Age ends at the session's absolute expiry.
pub fn refresh_cookie(
    config: &Config,
    token: &SecretString,
    session_expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Cookie<'static> {
    base(
        config,
        token.expose_secret().to_owned(),
        time::Duration::seconds(remaining_secs(session_expires_at, now)),
    )
}

/// Same name/path/attributes, empty value, Max-Age=0: makes the browser drop it.
/// (The `__Secure-` prefix requires the clearing cookie to carry `Secure` too.)
pub fn clear_refresh_cookie(config: &Config) -> Cookie<'static> {
    base(config, String::new(), time::Duration::ZERO)
}

/// Trusted-device cookie, valid for [`DEVICE_TTL`].
pub fn device_cookie(config: &Config, token: &SecretString) -> Cookie<'static> {
    let secs = i64::try_from(DEVICE_TTL.as_secs()).unwrap_or(i64::MAX);
    with_name(
        config,
        device_cookie_name(config),
        token.expose_secret().to_owned(),
        time::Duration::seconds(secs),
    )
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::auth::jwt::tests::test_config;

    #[test]
    fn refresh_cookie_attributes() {
        let config = test_config();
        let now = Utc::now();
        let token = SecretString::from("tok");
        let c = refresh_cookie(&config, &token, now + Duration::days(30), now);
        assert_eq!(c.name(), "__Secure-zm_rt");
        assert_eq!(c.value(), "tok");
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Strict));
        assert_eq!(c.path(), Some("/api/v1/auth"));
        assert_eq!(c.max_age(), Some(time::Duration::seconds(30 * 24 * 3600)));
        assert!(c.domain().is_none());

        let s = c.to_string();
        assert!(s.contains("HttpOnly"));
        assert!(s.contains("Secure"));
        assert!(s.contains("SameSite=Strict"));
        assert!(s.contains("Path=/api/v1/auth"));
        assert!(s.contains("Max-Age=2592000"));
    }

    #[test]
    fn insecure_dev_cookie_uses_plain_name() {
        let mut config = test_config();
        config.cookie_secure = false;
        let now = Utc::now();
        let c = refresh_cookie(
            &config,
            &SecretString::from("tok"),
            now + Duration::seconds(90),
            now,
        );
        assert_eq!(c.name(), "zm_rt");
        assert_eq!(c.secure(), Some(false));
        assert_eq!(c.max_age(), Some(time::Duration::seconds(90)));
    }

    #[test]
    fn past_expiry_gives_zero_max_age() {
        let config = test_config();
        let now = Utc::now();
        let c = refresh_cookie(
            &config,
            &SecretString::from("tok"),
            now - Duration::seconds(5),
            now,
        );
        assert_eq!(c.max_age(), Some(time::Duration::ZERO));
    }

    #[test]
    fn device_cookie_attributes() {
        let mut config = test_config();
        let c = device_cookie(&config, &SecretString::from("dev"));
        assert_eq!(c.name(), "__Secure-zm_dev");
        assert_eq!(c.value(), "dev");
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Strict));
        assert_eq!(c.path(), Some("/api/v1/auth"));
        assert_eq!(c.max_age(), Some(time::Duration::days(180)));

        config.cookie_secure = false;
        let c = device_cookie(&config, &SecretString::from("dev"));
        assert_eq!(c.name(), "zm_dev");
        assert_eq!(c.secure(), Some(false));
    }

    #[test]
    fn clear_cookie_matches_attributes() {
        let config = test_config();
        let c = clear_refresh_cookie(&config);
        assert_eq!(c.name(), "__Secure-zm_rt");
        assert_eq!(c.value(), "");
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Strict));
        assert_eq!(c.path(), Some("/api/v1/auth"));
        assert_eq!(c.max_age(), Some(time::Duration::ZERO));
    }
}
