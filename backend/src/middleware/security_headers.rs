//! Defensive response headers for a JSON-only API. Applied to every response,
//! including errors produced by the edge layers. Headers a handler already set
//! are left alone.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, header},
    middleware::Next,
    response::Response,
};

const PERMISSIONS_POLICY: &str =
    "accelerometer=(), camera=(), geolocation=(), gyroscope=(), microphone=(), payment=(), usb=()";
/// Two years, as required for HSTS preload lists.
const HSTS: &str = "max-age=63072000; includeSubDomains";

#[derive(Debug, Clone, Copy)]
pub struct SecurityHeaders {
    /// Send `Strict-Transport-Security`. Production only: in development the
    /// API is plain http on localhost and HSTS would poison the browser.
    pub hsts: bool,
}

impl SecurityHeaders {
    pub fn new(production: bool) -> Self {
        Self { hsts: production }
    }

    pub fn apply(self, path_is_api: bool, headers: &mut HeaderMap) {
        let set = |headers: &mut HeaderMap, name: HeaderName, value: &'static str| {
            headers
                .entry(name)
                .or_insert_with(|| HeaderValue::from_static(value));
        };
        set(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
        set(headers, header::X_FRAME_OPTIONS, "DENY");
        set(headers, header::REFERRER_POLICY, "no-referrer");
        set(
            headers,
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; frame-ancestors 'none'",
        );
        set(
            headers,
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        );
        set(
            headers,
            HeaderName::from_static("cross-origin-resource-policy"),
            "same-site",
        );
        set(
            headers,
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS_POLICY,
        );
        set(
            headers,
            HeaderName::from_static("x-permitted-cross-domain-policies"),
            "none",
        );
        if self.hsts {
            set(headers, header::STRICT_TRANSPORT_SECURITY, HSTS);
        }
        if path_is_api {
            set(headers, header::CACHE_CONTROL, "no-store");
        }
    }
}

/// Axum middleware: `from_fn_with_state(SecurityHeaders::new(prod), security_headers)`.
pub async fn security_headers(
    State(cfg): State<SecurityHeaders>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let is_api = path == "/api" || path.starts_with("/api/");
    let mut res = next.run(req).await;
    cfg.apply(is_api, res.headers_mut());
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_headers() {
        let mut h = HeaderMap::new();
        SecurityHeaders::new(true).apply(true, &mut h);
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["x-frame-options"], "DENY");
        assert_eq!(h["referrer-policy"], "no-referrer");
        assert_eq!(
            h["content-security-policy"],
            "default-src 'none'; frame-ancestors 'none'"
        );
        assert_eq!(h["cross-origin-opener-policy"], "same-origin");
        assert_eq!(h["cross-origin-resource-policy"], "same-site");
        assert!(h.contains_key("permissions-policy"));
        assert_eq!(h["strict-transport-security"], HSTS);
        assert_eq!(h["cache-control"], "no-store");
    }

    #[test]
    fn development_has_no_hsts_and_non_api_is_cacheable() {
        let mut h = HeaderMap::new();
        SecurityHeaders::new(false).apply(false, &mut h);
        assert!(!h.contains_key("strict-transport-security"));
        assert!(!h.contains_key("cache-control"));
        assert_eq!(h["x-frame-options"], "DENY");
    }

    #[test]
    fn existing_values_are_preserved() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=5"),
        );
        SecurityHeaders::new(true).apply(true, &mut h);
        assert_eq!(h["cache-control"], "private, max-age=5");
    }
}
