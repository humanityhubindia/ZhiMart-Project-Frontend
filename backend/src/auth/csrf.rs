//! CSRF defence for the cookie-authenticated endpoints (`/auth/refresh`,
//! `/auth/logout`). `SameSite=Strict` already blocks cross-site cookie sends in
//! modern browsers; this is defence in depth:
//!
//! * `X-Requested-With: XMLHttpRequest` is required. A custom header cannot be
//!   attached to a cross-origin request without a CORS preflight, which our
//!   CORS policy only grants to `ALLOWED_ORIGINS`.
//! * If `Origin` is present it must be one of `ALLOWED_ORIGINS`.
//! * `Sec-Fetch-Site: cross-site` is rejected outright.

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header, request::Parts},
};

use crate::{config::Config, error::AppError, state::AppState};

const X_REQUESTED_WITH: &str = "x-requested-with";
const SEC_FETCH_SITE: &str = "sec-fetch-site";

/// Zero-sized proof that the request passed the CSRF checks.
#[derive(Debug, Clone, Copy)]
pub struct CsrfGuard;

/// The checks, independent of axum plumbing (unit-testable).
pub fn check(headers: &HeaderMap, config: &Config) -> Result<(), AppError> {
    let mut xrw = headers.get_all(X_REQUESTED_WITH).iter();
    match (xrw.next(), xrw.next()) {
        (Some(v), None) if v.as_bytes().eq_ignore_ascii_case(b"XMLHttpRequest") => {}
        _ => {
            tracing::debug!("csrf: missing or invalid X-Requested-With");
            return Err(AppError::Forbidden);
        }
    }

    let mut origins = headers.get_all(header::ORIGIN).iter();
    match (origins.next(), origins.next()) {
        (None, _) => {}
        (Some(origin), None) if config.is_allowed_origin(origin.as_bytes()) => {}
        _ => {
            tracing::debug!("csrf: origin not allowed");
            return Err(AppError::Forbidden);
        }
    }

    for site in headers.get_all(SEC_FETCH_SITE) {
        if site.as_bytes().eq_ignore_ascii_case(b"cross-site") {
            tracing::debug!("csrf: Sec-Fetch-Site is cross-site");
            return Err(AppError::Forbidden);
        }
    }
    Ok(())
}

impl FromRequestParts<AppState> for CsrfGuard {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        check(&parts.headers, &state.config).map(|()| CsrfGuard)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::auth::jwt::tests::test_config;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_static(v));
        }
        h
    }

    #[test]
    fn accepts_xhr_from_allowed_origin() {
        let c = test_config();
        assert!(check(&headers(&[("x-requested-with", "XMLHttpRequest")]), &c).is_ok());
        assert!(check(&headers(&[("x-requested-with", "xmlhttprequest")]), &c).is_ok());
        assert!(
            check(
                &headers(&[
                    ("x-requested-with", "XMLHttpRequest"),
                    ("origin", "https://admin.zhimart.com"),
                    ("sec-fetch-site", "same-site"),
                ]),
                &c
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_missing_or_wrong_marker() {
        let c = test_config();
        assert!(matches!(
            check(&HeaderMap::new(), &c),
            Err(AppError::Forbidden)
        ));
        assert!(check(&headers(&[("x-requested-with", "fetch")]), &c).is_err());
        assert!(
            check(
                &headers(&[
                    ("x-requested-with", "XMLHttpRequest"),
                    ("x-requested-with", "XMLHttpRequest")
                ]),
                &c
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_foreign_origin_and_cross_site() {
        let c = test_config();
        assert!(
            check(
                &headers(&[
                    ("x-requested-with", "XMLHttpRequest"),
                    ("origin", "https://evil.example")
                ]),
                &c
            )
            .is_err()
        );
        assert!(
            check(
                &headers(&[("x-requested-with", "XMLHttpRequest"), ("origin", "null")]),
                &c
            )
            .is_err()
        );
        assert!(
            check(
                &headers(&[
                    ("x-requested-with", "XMLHttpRequest"),
                    ("origin", "https://admin.zhimart.com/")
                ]),
                &c
            )
            .is_err()
        );
        assert!(
            check(
                &headers(&[
                    ("x-requested-with", "XMLHttpRequest"),
                    ("sec-fetch-site", "cross-site")
                ]),
                &c
            )
            .is_err()
        );
    }
}
