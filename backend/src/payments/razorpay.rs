//! Minimal Razorpay REST client (orders, payments, captures, refunds).
//!
//! * HTTP Basic auth with `key_id:key_secret` over rustls; connect timeout 3 s,
//!   total timeout 10 s; redirects are never followed.
//! * Only idempotent GETs are retried (once, on a transient failure). POSTs are
//!   never retried blindly:
//!   - create order: if the response is lost we may leave an orphan Razorpay
//!     order behind. That is harmless — no money can move on an order id we
//!     never handed to a customer — so the caller simply stores the id only
//!     after a successful response (see `service::start_payment`).
//!   - capture: the caller re-fetches the payment after any failure.
//!   - refund: sent with `receipt` = our refund row id and the
//!     `X-Refund-Idempotency` header, so a deliberate retry with the same id
//!     cannot double-refund; after an ambiguous failure the caller reconciles by
//!     listing the payment's refunds.
//! * Logs carry the operation, HTTP status and Razorpay's `error.code` /
//!   `error.description` only — never auth headers or response bodies (which
//!   contain customer contact details).

use std::time::Duration;

use reqwest::{Method, StatusCode, redirect};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{config::Config, error::AppError};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const GET_RETRY_DELAY: Duration = Duration::from_millis(300);
/// Responses larger than this are treated as garbage.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_LOGGED_DESCRIPTION: usize = 200;

/// Why a Razorpay call did not produce the expected entity.
#[derive(Debug, Clone)]
pub enum GatewayError {
    /// Transport error, timeout, 429/5xx or an unreadable 2xx body. The request
    /// may or may not have taken effect at Razorpay.
    Unavailable,
    /// Razorpay answered with a 4xx error: the request did not take effect.
    Rejected {
        status: u16,
        code: String,
        description: String,
    },
}

impl GatewayError {
    /// Client-safe one-line summary for the admin console / `refunds.failure_reason`.
    pub fn summary(&self) -> String {
        match self {
            GatewayError::Unavailable => "Razorpay did not respond".to_owned(),
            GatewayError::Rejected {
                code, description, ..
            } => truncate(
                &format!("Razorpay rejected the request: {code}: {description}"),
                300,
            ),
        }
    }
}

impl From<GatewayError> for AppError {
    fn from(_: GatewayError) -> Self {
        AppError::PaymentGateway
    }
}

/// `POST /v1/orders` body.
#[derive(Debug, Serialize)]
struct CreateOrderBody<'a> {
    amount: i64,
    currency: &'static str,
    receipt: &'a str,
    partial_payment: bool,
    notes: OrderNotes<'a>,
}

#[derive(Debug, Serialize)]
struct OrderNotes<'a> {
    order_id: String,
    order_number: &'a str,
}

#[derive(Debug, Serialize)]
struct CaptureBody {
    amount: i64,
    currency: &'static str,
}

#[derive(Debug, Serialize)]
struct RefundBody<'a> {
    amount: i64,
    speed: &'static str,
    receipt: String,
    notes: RefundNotes<'a>,
}

#[derive(Debug, Serialize)]
struct RefundNotes<'a> {
    refund_id: String,
    order_number: &'a str,
}

/// Razorpay order entity (fields we use).
#[derive(Debug, Clone, Deserialize)]
pub struct RzpOrder {
    pub id: String,
    pub amount: i64,
    pub currency: String,
    #[serde(default)]
    pub receipt: Option<String>,
    #[serde(default)]
    pub status: String,
}

/// Razorpay payment entity (fields we use). Also the shape of
/// `payload.payment.entity` in webhooks.
#[derive(Debug, Clone, Deserialize)]
pub struct RzpPayment {
    pub id: String,
    pub amount: i64,
    pub currency: String,
    /// created | authorized | captured | refunded | failed
    pub status: String,
    #[serde(default)]
    pub order_id: Option<String>,
    /// upi | card | netbanking | wallet | emi | paylater | …
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub amount_refunded: i64,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_reason: Option<String>,
}

/// Razorpay refund entity (fields we use). Also `payload.refund.entity`.
#[derive(Debug, Clone, Deserialize)]
pub struct RzpRefund {
    pub id: String,
    pub amount: i64,
    pub payment_id: String,
    /// pending | processed | failed
    pub status: String,
    #[serde(default)]
    pub receipt: Option<String>,
    /// An object, or `[]` when empty.
    #[serde(default)]
    pub notes: serde_json::Value,
}

impl RzpRefund {
    /// Our refund row id, if this refund was created by us (`receipt` or `notes.refund_id`).
    pub fn our_refund_id(&self) -> Option<Uuid> {
        self.receipt
            .as_deref()
            .and_then(|r| Uuid::parse_str(r).ok())
            .or_else(|| {
                self.notes
                    .get("refund_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
            })
    }
}

#[derive(Debug, Deserialize)]
struct Collection<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

#[derive(Debug, Default, Deserialize)]
struct ErrorEnvelope {
    #[serde(default)]
    error: ErrorDetail,
}

#[derive(Debug, Default, Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Razorpay REST client (orders, payments, refunds). Cheap to share via `Arc`.
pub struct RazorpayClient {
    http: reqwest::Client,
    base: String,
    key_id: String,
    key_secret: SecretString,
}

impl std::fmt::Debug for RazorpayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RazorpayClient")
            .field("base", &self.base)
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl RazorpayClient {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let base = config.razorpay_api_base.trim_end_matches('/').to_owned();
        anyhow::ensure!(
            base.starts_with("https://") || base.starts_with("http://"),
            "RAZORPAY_API_BASE must be an http(s) URL"
        );
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .redirect(redirect::Policy::none())
            .https_only(base.starts_with("https://"))
            .user_agent(concat!("zhimart-backend/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            base,
            key_id: config.razorpay_key_id.clone(),
            key_secret: config.razorpay_key_secret.clone(),
        })
    }

    /// `POST /v1/orders`. `receipt` is our order number (cut to Razorpay's 40-char limit).
    pub async fn create_order(
        &self,
        amount_paise: i64,
        order_id: Uuid,
        order_number: &str,
    ) -> Result<RzpOrder, GatewayError> {
        let receipt: String = order_number.chars().take(40).collect();
        let body = CreateOrderBody {
            amount: amount_paise,
            currency: "INR",
            receipt: &receipt,
            partial_payment: false,
            notes: OrderNotes {
                order_id: order_id.to_string(),
                order_number,
            },
        };
        self.send(
            Method::POST,
            "/v1/orders".to_owned(),
            Some(&body),
            &[],
            "create_order",
        )
        .await
    }

    /// `GET /v1/payments/{id}` (retried once on a transient failure).
    pub async fn fetch_payment(&self, payment_id: &str) -> Result<RzpPayment, GatewayError> {
        self.get(format!("/v1/payments/{payment_id}"), "fetch_payment")
            .await
    }

    /// `GET /v1/orders/{id}/payments`, for reconciliation.
    pub async fn fetch_order_payments(
        &self,
        order_id: &str,
    ) -> Result<Vec<RzpPayment>, GatewayError> {
        let c: Collection<RzpPayment> = self
            .get(
                format!("/v1/orders/{order_id}/payments"),
                "fetch_order_payments",
            )
            .await?;
        Ok(c.items)
    }

    /// `GET /v1/payments/{id}/refunds`, to reconcile an ambiguous refund call.
    pub async fn fetch_payment_refunds(
        &self,
        payment_id: &str,
    ) -> Result<Vec<RzpRefund>, GatewayError> {
        let c: Collection<RzpRefund> = self
            .get(
                format!("/v1/payments/{payment_id}/refunds?count=100"),
                "fetch_payment_refunds",
            )
            .await?;
        Ok(c.items)
    }

    /// `POST /v1/payments/{id}/capture` for an `authorized` payment.
    pub async fn capture_payment(
        &self,
        payment_id: &str,
        amount_paise: i64,
    ) -> Result<RzpPayment, GatewayError> {
        let body = CaptureBody {
            amount: amount_paise,
            currency: "INR",
        };
        self.send(
            Method::POST,
            format!("/v1/payments/{payment_id}/capture"),
            Some(&body),
            &[],
            "capture_payment",
        )
        .await
    }

    /// `POST /v1/payments/{id}/refund` (speed `normal`). `refund_id` (our
    /// `refunds.id`) is sent as `receipt`, in `notes`, and as the idempotency key.
    pub async fn refund_payment(
        &self,
        payment_id: &str,
        amount_paise: i64,
        refund_id: Uuid,
        order_number: &str,
    ) -> Result<RzpRefund, GatewayError> {
        let body = RefundBody {
            amount: amount_paise,
            speed: "normal",
            receipt: refund_id.to_string(),
            notes: RefundNotes {
                refund_id: refund_id.to_string(),
                order_number,
            },
        };
        let key = refund_id.to_string();
        self.send(
            Method::POST,
            format!("/v1/payments/{payment_id}/refund"),
            Some(&body),
            &[("X-Refund-Idempotency", key.as_str())],
            "refund_payment",
        )
        .await
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: String,
        op: &'static str,
    ) -> Result<T, GatewayError> {
        match self
            .send::<T, ()>(Method::GET, path.clone(), None, &[], op)
            .await
        {
            Err(GatewayError::Unavailable) => {
                tokio::time::sleep(GET_RETRY_DELAY).await;
                self.send::<T, ()>(Method::GET, path, None, &[], op).await
            }
            other => other,
        }
    }

    async fn send<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        path: String,
        body: Option<&B>,
        headers: &[(&str, &str)],
        op: &'static str,
    ) -> Result<T, GatewayError> {
        let mut req = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .basic_auth(&self.key_id, Some(self.key_secret.expose_secret()))
            .header(reqwest::header::ACCEPT, "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(b) = body {
            req = req.json(b);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    op,
                    timeout = e.is_timeout(),
                    connect = e.is_connect(),
                    "razorpay request failed"
                );
                return Err(GatewayError::Unavailable);
            }
        };
        let status = resp.status();
        let bytes = match resp.bytes().await {
            Ok(b) if b.len() <= MAX_RESPONSE_BYTES => b,
            Ok(_) => {
                tracing::warn!(op, status = status.as_u16(), "razorpay response too large");
                return Err(GatewayError::Unavailable);
            }
            Err(e) => {
                tracing::warn!(
                    op,
                    status = status.as_u16(),
                    timeout = e.is_timeout(),
                    "razorpay response read failed"
                );
                return Err(GatewayError::Unavailable);
            }
        };

        if status.is_success() {
            return serde_json::from_slice::<T>(&bytes).map_err(|e| {
                // serde's message names the field/position, never the values.
                tracing::error!(op, status = status.as_u16(), error = %e, "unexpected razorpay response shape");
                GatewayError::Unavailable
            });
        }

        let detail = serde_json::from_slice::<ErrorEnvelope>(&bytes)
            .unwrap_or_default()
            .error;
        let code = truncate(detail.code.as_deref().unwrap_or("UNKNOWN"), 60);
        let description = truncate(
            detail.description.as_deref().unwrap_or(""),
            MAX_LOGGED_DESCRIPTION,
        );
        if status.is_server_error()
            || status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::REQUEST_TIMEOUT
        {
            tracing::warn!(op, status = status.as_u16(), code = %code, description = %description, "razorpay unavailable");
            return Err(GatewayError::Unavailable);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            tracing::error!(op, status = status.as_u16(), code = %code, "razorpay rejected our API credentials");
        } else {
            tracing::warn!(op, status = status.as_u16(), code = %code, description = %description, "razorpay rejected request");
        }
        Err(GatewayError::Rejected {
            status: status.as_u16(),
            code,
            description,
        })
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refund_id_from_receipt_or_notes() {
        let id = Uuid::now_v7();
        let r: RzpRefund = serde_json::from_value(serde_json::json!({
            "id": "rfnd_ABCDEF123", "amount": 100, "payment_id": "pay_ABCDEF123", "status": "processed",
            "receipt": id.to_string(), "notes": []
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.our_refund_id(), Some(id));

        let r: RzpRefund = serde_json::from_value(serde_json::json!({
            "id": "rfnd_ABCDEF123", "amount": 100, "payment_id": "pay_ABCDEF123", "status": "processed",
            "receipt": null, "notes": { "refund_id": id.to_string() }
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.our_refund_id(), Some(id));
    }

    #[test]
    fn summary_is_bounded() {
        let e = GatewayError::Rejected {
            status: 400,
            code: "BAD_REQUEST_ERROR".into(),
            description: "x".repeat(1000),
        };
        assert!(e.summary().chars().count() <= 300);
    }
}
