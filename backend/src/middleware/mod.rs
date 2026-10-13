//! HTTP edge middleware. The stack itself is assembled in `app.rs`.

pub mod client_ip;
pub mod conn_limit;
pub mod rate_limit;
pub mod security_headers;

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use serde_json::json;

/// Build an error response in the API's `{"error":{"code","message"}}` shape
/// for failures that do not originate from an `AppError` (tower layers, axum
/// rejections, panics).
pub fn json_error(status: StatusCode, code: &str, message: &str) -> Response {
    let body = json!({ "error": { "code": code, "message": message } }).to_string();
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = status;
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// Code + message for a bare error status. Messages match `error.rs` where the
/// two overlap so the frontend sees one vocabulary.
fn describe(status: StatusCode) -> (&'static str, &'static str) {
    match status {
        StatusCode::BAD_REQUEST => ("validation_error", "The request is malformed."),
        StatusCode::UNAUTHORIZED => ("unauthorized", "Authentication required."),
        StatusCode::FORBIDDEN => ("forbidden", "You do not have access to this resource."),
        StatusCode::NOT_FOUND => ("not_found", "Resource not found."),
        StatusCode::METHOD_NOT_ALLOWED => ("method_not_allowed", "Method not allowed."),
        StatusCode::REQUEST_TIMEOUT => ("request_timeout", "The request took too long."),
        StatusCode::PAYLOAD_TOO_LARGE => ("payload_too_large", "The request body is too large."),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            "unsupported_media_type",
            "Content-Type must be application/json.",
        ),
        StatusCode::UNPROCESSABLE_ENTITY => ("validation_error", "The request body is invalid."),
        StatusCode::TOO_MANY_REQUESTS => (
            "too_many_requests",
            "Too many requests. Please wait and try again later.",
        ),
        StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => (
            "service_unavailable",
            "The service is temporarily unavailable. Please try again.",
        ),
        s if s.is_server_error() => ("internal_error", "An unexpected error occurred."),
        _ => ("bad_request", "The request could not be processed."),
    }
}

fn is_json(res: &Response) -> bool {
    res.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| {
            ct.trim_start()
                .to_ascii_lowercase()
                .starts_with("application/json")
        })
}

/// Rewrite any 4xx/5xx response that is not already JSON (axum extractor
/// rejections, the default 405, tower-http's 413, ...) into the API error
/// shape. Status and other headers (`Allow`, `Retry-After`, CORS) are kept;
/// the original body is discarded so framework error text never leaks.
pub async fn json_errors(req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    let status = res.status();
    if !(status.is_client_error() || status.is_server_error()) || is_json(&res) {
        return res;
    }

    let (code, message) = describe(status);
    let (mut parts, _discarded) = res.into_parts();
    let replacement = json_error(status, code, message);
    let (rparts, body) = replacement.into_parts();
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.remove(header::CONTENT_ENCODING);
    for (name, value) in rparts.headers {
        if let Some(name) = name {
            parts.headers.insert(name, value);
        }
    }
    Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use axum::{Router, http::Method, routing::post};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    async fn body_json(res: Response) -> serde_json::Value {
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn rewrites_plain_text_errors_and_keeps_headers() {
        let app = Router::new()
            .route("/x", post(|| async { "ok" }))
            .layer(axum::middleware::from_fn(json_errors));

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/x")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert!(res.headers().contains_key(header::ALLOW));
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
        let v = body_json(res).await;
        assert_eq!(v["error"]["code"], "method_not_allowed");
    }

    #[tokio::test]
    async fn leaves_success_and_json_errors_untouched() {
        let app = Router::new()
            .route("/ok", post(|| async { "fine" }))
            .route("/err", post(|| async { crate::error::AppError::Forbidden }))
            .layer(axum::middleware::from_fn(json_errors));

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/ok")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.into_body().collect().await.expect("body").to_bytes(),
            "fine"
        );

        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/err")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let v = body_json(res).await;
        assert_eq!(
            v["error"]["message"],
            "You do not have access to this resource."
        );
    }
}
