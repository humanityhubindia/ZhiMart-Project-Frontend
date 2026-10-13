//! Uniform API error type. Every failure leaves the server as
//! `{"error": {"code": "...", "message": "..."}}`; internal details are logged,
//! never returned to the client.

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

/// One line of an order that cannot be fulfilled from current stock.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockShortage {
    pub product_id: String,
    pub requested: i32,
    pub available: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("validation failed: {0}")]
    Validation(&'static str),

    /// Validation failure with a dynamic, client-safe message (e.g. naming the
    /// offending field). Never put user-supplied text in the message verbatim.
    #[error("validation failed: {0}")]
    InvalidField(String),

    /// State conflict: optimistic-concurrency version mismatch, illegal status
    /// transition, duplicate id, etc. `code` is a stable machine-readable slug.
    #[error("conflict: {code}")]
    Conflict {
        code: &'static str,
        message: &'static str,
    },

    /// One or more order lines exceed available stock.
    #[error("insufficient stock")]
    OutOfStock { items: Vec<StockShortage> },

    /// Wrong email or password. Deliberately does not say which.
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("invalid or expired verification code")]
    OtpInvalid { attempts_remaining: i32 },

    /// The OTP challenge is expired, consumed, invalidated or out of attempts.
    /// The client must restart from the password step.
    #[error("verification challenge expired")]
    OtpChallengeExpired,

    /// Missing/invalid/expired access token.
    #[error("unauthorized")]
    Unauthorized,

    /// Refresh token missing, expired, revoked, or the 30-day session ended.
    #[error("session expired")]
    SessionExpired,

    #[error("forbidden")]
    Forbidden,

    #[error("not found")]
    NotFound,

    #[error("too many requests")]
    TooManyRequests { retry_after_secs: u64 },

    #[error("account temporarily locked")]
    AccountLocked { retry_after_secs: u64 },

    #[error("service unavailable")]
    ServiceUnavailable,

    /// The payment gateway (Razorpay) failed or timed out. Details are logged.
    #[error("payment gateway error")]
    PaymentGateway,

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut => AppError::ServiceUnavailable,
            other => AppError::Internal(other.into()),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempts_remaining: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a [StockShortage]>,
}

impl AppError {
    fn parts(&self) -> (StatusCode, &'static str, &'static str) {
        match self {
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, "validation_error", msg),
            AppError::InvalidField(_) => (
                StatusCode::BAD_REQUEST,
                "validation_error",
                "The request is invalid.",
            ),
            AppError::Conflict { code, message } => (StatusCode::CONFLICT, *code, *message),
            AppError::OutOfStock { .. } => (
                StatusCode::CONFLICT,
                "out_of_stock",
                "Some items in your order are no longer available in the requested quantity.",
            ),
            AppError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "Invalid email or password.",
            ),
            AppError::OtpInvalid { .. } => (
                StatusCode::UNAUTHORIZED,
                "otp_invalid",
                "The verification code is incorrect.",
            ),
            AppError::OtpChallengeExpired => (
                StatusCode::UNAUTHORIZED,
                "otp_challenge_expired",
                "This verification code has expired. Please sign in again.",
            ),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required.",
            ),
            AppError::SessionExpired => (
                StatusCode::UNAUTHORIZED,
                "session_expired",
                "Your session has ended. Please sign in again.",
            ),
            AppError::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "You do not have access to this resource.",
            ),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found", "Resource not found."),
            AppError::TooManyRequests { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                "Too many requests. Please wait and try again later.",
            ),
            AppError::AccountLocked { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "account_locked",
                "Too many failed attempts. Please wait before trying again.",
            ),
            AppError::ServiceUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "The service is temporarily unavailable. Please try again.",
            ),
            AppError::PaymentGateway => (
                StatusCode::BAD_GATEWAY,
                "payment_gateway_error",
                "The payment service is not responding. Please try again in a moment.",
            ),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An unexpected error occurred.",
            ),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();

        match &self {
            AppError::Internal(e) => tracing::error!(error = ?e, "internal error"),
            AppError::ServiceUnavailable => tracing::warn!("responding 503 service_unavailable"),
            AppError::PaymentGateway => tracing::warn!("responding 502 payment_gateway_error"),
            _ => {}
        }

        let (attempts_remaining, retry_after) = match &self {
            AppError::OtpInvalid { attempts_remaining } => (Some(*attempts_remaining), None),
            AppError::TooManyRequests { retry_after_secs }
            | AppError::AccountLocked { retry_after_secs } => (None, Some(*retry_after_secs)),
            _ => (None, None),
        };

        let message = match &self {
            AppError::InvalidField(m) => m.as_str(),
            _ => message,
        };
        let details = match &self {
            AppError::OutOfStock { items } => Some(items.as_slice()),
            _ => None,
        };
        let body = ErrorBody {
            error: ErrorDetail {
                code,
                message,
                attempts_remaining,
                retry_after,
                details,
            },
        };
        let mut res = (status, Json(body)).into_response();
        let headers = res.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(secs) = retry_after {
            headers.insert(header::RETRY_AFTER, HeaderValue::from(secs.max(1)));
        }
        if status == StatusCode::UNAUTHORIZED {
            headers.insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        res
    }
}
