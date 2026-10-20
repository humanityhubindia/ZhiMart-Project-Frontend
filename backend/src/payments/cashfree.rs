//! Minimal Cashfree Payment Gateway REST client (orders, payments, refunds),
//! API version [`API_VERSION`].
//!
//! * Every call carries `x-client-id` / `x-client-secret` / `x-api-version`
//!   and a fresh `x-request-id` (logged, so a failure can be traced with
//!   Cashfree support). rustls only; connect timeout 3 s, total timeout 10 s;
//!   redirects are never followed; an `https://` base forbids plain HTTP.
//! * Only idempotent GETs are retried (once, on a transient failure). POSTs are
//!   never retried blindly, and always carry an `x-idempotency-key`:
//!   - create order: our Cashfree order id is deterministic per order
//!     ([`gateway_order_id`]) and the idempotency key is derived from it, so a
//!     lost response, a second tab or a double click can only ever produce
//!     ONE Cashfree order; the caller answers "already exists" (409) by
//!     fetching it (see `service::ensure_gateway_order`).
//!   - refund: `refund_id` = our refund row id (simple UUID form) and the
//!     idempotency key = the same UUID, so a deliberate retry cannot
//!     double-refund; after an ambiguous failure the caller reconciles with
//!     `GET /orders/{id}/refunds/{refund_id}`.
//! * Logs carry the operation, HTTP status, our request id and Cashfree's
//!   `code` / `message` only — never auth headers or response bodies (which
//!   contain customer contact details).

use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{Method, StatusCode, redirect};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{config::Config, error::AppError, money, payments::signature};

/// `x-api-version` sent on every call. Pinned: response and webhook shapes
/// below are those of this version (set the dashboard webhook to the same one).
pub const API_VERSION: &str = "2023-08-01";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const GET_RETRY_DELAY: Duration = Duration::from_millis(300);
/// Responses larger than this are treated as garbage.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_LOGGED_MESSAGE: usize = 200;
/// Largest amount we ever accept from Cashfree (₹1 crore), in paise.
const MAX_AMOUNT_PAISE: i64 = 1_000_000_000;

/// Why a Cashfree call did not produce the expected entity.
#[derive(Debug, Clone)]
pub enum GatewayError {
    /// Transport error, timeout, 408/429/5xx or an unreadable 2xx body. The
    /// request may or may not have taken effect at Cashfree.
    Unavailable,
    /// Cashfree answered with a 4xx error: the request did not take effect.
    Rejected {
        status: u16,
        code: String,
        message: String,
    },
}

impl GatewayError {
    /// Client-safe one-line summary for the admin console / `refunds.failure_reason`.
    pub fn summary(&self) -> String {
        match self {
            GatewayError::Unavailable => "Cashfree did not respond".to_owned(),
            GatewayError::Rejected { code, message, .. } => truncate(
                &format!("Cashfree rejected the request: {code}: {message}"),
                300,
            ),
        }
    }

    /// 404: the order / refund does not exist at Cashfree.
    pub fn is_not_found(&self) -> bool {
        matches!(self, GatewayError::Rejected { status: 404, .. })
    }

    /// 409 (`order_already_exists`, or a refund id already used) or 422
    /// (idempotency key replayed with a different body): the entity exists.
    pub fn is_conflict(&self) -> bool {
        matches!(
            self,
            GatewayError::Rejected {
                status: 409 | 422,
                ..
            }
        )
    }
}

impl From<GatewayError> for AppError {
    fn from(_: GatewayError) -> Self {
        AppError::PaymentGateway
    }
}

// ---------------------------------------------------------------------------
// Ids and amounts
// ---------------------------------------------------------------------------

/// Our Cashfree `order_id` for an order: `{order_number}_{last 12 hex digits
/// of the order UUID}`, e.g. `ZM-261020-1001_8f3a2b1c9d0e` (≤ 45 chars,
/// `[A-Za-z0-9_-]`). Deterministic, so every pay attempt for the same order
/// addresses the same Cashfree order; the UUID suffix keeps it unique across
/// databases (dev / staging / prod) that share one Cashfree account.
pub fn gateway_order_id(order_number: &str, order_id: Uuid) -> String {
    let simple = order_id.simple().to_string();
    let suffix = &simple[simple.len().saturating_sub(12)..];
    format!("{order_number}_{suffix}")
}

/// Our `refund_id` at Cashfree: the refund row UUID in simple form (32 hex
/// digits, alphanumeric as Cashfree requires).
pub fn refund_ref(refund_id: Uuid) -> String {
    refund_id.simple().to_string()
}

/// Paise → the decimal rupee amount Cashfree expects (JSON number, ≤ 2 dp).
pub fn paise_to_rupees(paise: i64) -> f64 {
    money::to_rupees(paise)
}

/// Cashfree decimal rupees → paise. `None` for NaN/∞, negatives or absurd values.
pub fn rupees_to_paise(rupees: f64) -> Option<i64> {
    if !rupees.is_finite() || rupees < 0.0 {
        return None;
    }
    let paise = (rupees * 100.0).round();
    (paise <= MAX_AMOUNT_PAISE as f64).then_some(paise as i64)
}

/// Cashfree ids (`cf_payment_id`, `cf_refund_id`) are integers in some API
/// versions and strings in others; accept both and keep the digit string.
fn de_cf_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Num(u64),
        Str(String),
    }
    Ok(match Id::deserialize(d)? {
        Id::Num(n) => n.to_string(),
        Id::Str(s) => s,
    })
}

fn de_opt_cf_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Num(u64),
        Str(String),
    }
    Ok(match Option::<Id>::deserialize(d)? {
        Some(Id::Num(n)) => Some(n.to_string()),
        Some(Id::Str(s)) => Some(s),
        None => None,
    })
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// Cashfree order entity (fields we use).
#[derive(Debug, Clone, Deserialize)]
pub struct CfOrder {
    pub order_id: String,
    pub order_amount: f64,
    pub order_currency: String,
    /// ACTIVE | PAID | EXPIRED | TERMINATED | TERMINATION_REQUESTED
    #[serde(default)]
    pub order_status: String,
    /// Handed to the JS SDK (`checkout({ paymentSessionId })`); usable while ACTIVE.
    #[serde(default)]
    pub payment_session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CfErrorDetails {
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_reason: Option<String>,
}

/// Cashfree payment entity (fields we use), from `GET /orders/{id}/payments`.
#[derive(Debug, Clone, Deserialize)]
pub struct CfPayment {
    #[serde(deserialize_with = "de_cf_id")]
    pub cf_payment_id: String,
    #[serde(default)]
    pub order_id: Option<String>,
    /// SUCCESS | NOT_ATTEMPTED | FAILED | USER_DROPPED | VOID | CANCELLED | PENDING
    pub payment_status: String,
    pub payment_amount: f64,
    pub payment_currency: String,
    /// upi | credit_card | debit_card | net_banking | wallet | … (see [`instrument`]).
    #[serde(default)]
    pub payment_group: Option<String>,
    #[serde(default)]
    pub error_details: Option<CfErrorDetails>,
}

impl CfPayment {
    pub fn amount_paise(&self) -> Option<i64> {
        rupees_to_paise(self.payment_amount)
    }

    pub fn is_success(&self) -> bool {
        self.payment_status == "SUCCESS"
    }

    pub fn is_pending(&self) -> bool {
        self.payment_status == "PENDING"
    }
}

/// Cashfree refund entity (fields we use). Also `data.refund` /
/// `data.auto_refund` in refund webhooks.
#[derive(Debug, Clone, Deserialize)]
pub struct CfRefund {
    #[serde(deserialize_with = "de_cf_id")]
    pub cf_refund_id: String,
    #[serde(default, deserialize_with = "de_opt_cf_id")]
    pub cf_payment_id: Option<String>,
    /// Our refund id (simple UUID) for refunds we created; Cashfree's own id
    /// for auto-refunds / dashboard refunds.
    #[serde(default)]
    pub refund_id: Option<String>,
    #[serde(default)]
    pub order_id: Option<String>,
    pub refund_amount: f64,
    #[serde(default)]
    pub refund_currency: Option<String>,
    /// SUCCESS | PENDING | PENDING_APPROVAL | ONHOLD | CANCELLED | REJECTED (| FAILED)
    pub refund_status: String,
    /// MERCHANT_INITIATED | PAYMENT_AUTO_REFUND | UNRECONCILED_AUTO_REFUND
    #[serde(default)]
    pub refund_type: Option<String>,
}

impl CfRefund {
    pub fn amount_paise(&self) -> Option<i64> {
        rupees_to_paise(self.refund_amount)
    }

    /// Our refund row id, if this refund was created by us.
    pub fn our_refund_id(&self) -> Option<Uuid> {
        self.refund_id
            .as_deref()
            .filter(|s| s.len() == 32)
            .and_then(|s| Uuid::try_parse(s).ok())
    }
}

/// Instrument from Cashfree's `payment_group`, in our vocabulary
/// (`upi | card | netbanking | wallet | emi | paylater | bank_transfer`).
pub fn instrument(payment_group: Option<&str>) -> Option<String> {
    let g = payment_group?.trim().to_ascii_lowercase();
    let mapped = match g.as_str() {
        "credit_card" | "debit_card" | "prepaid_card" => "card",
        "credit_card_emi" | "debit_card_emi" | "cardless_emi" => "emi",
        "net_banking" => "netbanking",
        "pay_later" => "paylater",
        "upi" | "upi_credit_card" | "upi_ppi" | "upi_ppi_offline" => "upi",
        "wallet" => "wallet",
        "bank_transfer" => "bank_transfer",
        other
            if !other.is_empty()
                && other.len() <= 30
                && other
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
        {
            return Some(other.to_owned());
        }
        _ => return None,
    };
    Some(mapped.to_owned())
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// What `POST /orders` needs to know about our order.
#[derive(Debug, Clone)]
pub struct NewGatewayOrder<'a> {
    pub gateway_order_id: &'a str,
    pub order_id: Uuid,
    pub order_number: &'a str,
    pub amount_paise: i64,
    pub customer_name: &'a str,
    /// As stored (`+91XXXXXXXXXX`); Cashfree gets the 10 national digits.
    pub customer_phone: &'a str,
    pub customer_email: Option<&'a str>,
    pub return_url: &'a str,
    pub notify_url: Option<&'a str>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct CreateOrderBody<'a> {
    order_id: &'a str,
    order_amount: f64,
    order_currency: &'static str,
    customer_details: CustomerDetails,
    order_meta: OrderMeta<'a>,
    order_expiry_time: String,
    order_note: String,
    order_tags: OrderTags<'a>,
}

#[derive(Debug, Serialize)]
struct CustomerDetails {
    customer_id: String,
    customer_phone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    customer_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    customer_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OrderMeta<'a> {
    return_url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    notify_url: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct OrderTags<'a> {
    order_number: &'a str,
    order_uuid: String,
}

#[derive(Debug, Serialize)]
struct RefundBody<'a> {
    refund_amount: f64,
    refund_id: &'a str,
    refund_note: String,
    refund_speed: &'static str,
}

/// Cashfree's 10-digit `customer_phone` from our stored `+91XXXXXXXXXX`.
fn national_phone(stored: &str) -> String {
    let digits: String = stored.chars().filter(char::is_ascii_digit).collect();
    let start = digits.len().saturating_sub(10);
    digits[start..].to_owned()
}

/// `Some(trimmed)` when `s` has `min..=max` characters (Cashfree's limits).
fn bounded(s: &str, min: usize, max: usize) -> Option<String> {
    let s: String = s.trim().chars().filter(|c| !c.is_control()).collect();
    let n = s.chars().count();
    (n >= min).then(|| s.chars().take(max).collect())
}

/// Deterministic UUID-shaped idempotency key for creating `gateway_order_id`.
fn order_idempotency_key(gateway_order_id: &str) -> Uuid {
    let digest = Sha256::digest(format!("zhimart/cashfree-order/{gateway_order_id}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

#[derive(Debug, Default, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Cashfree PG REST client. Cheap to share via `Arc`.
pub struct CashfreeClient {
    http: reqwest::Client,
    base: String,
    app_id: String,
    secret_key: SecretString,
}

impl std::fmt::Debug for CashfreeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CashfreeClient")
            .field("base", &self.base)
            .field("app_id", &self.app_id)
            .finish_non_exhaustive()
    }
}

impl CashfreeClient {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let base = config.cashfree_api_base.trim_end_matches('/').to_owned();
        anyhow::ensure!(
            base.starts_with("https://") || base.starts_with("http://"),
            "CASHFREE_API_BASE must be an http(s) URL"
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
            app_id: config.cashfree_app_id.clone(),
            secret_key: config.cashfree_secret_key.clone(),
        })
    }

    /// `POST /orders`. A 409 means the (deterministic) order id already exists:
    /// the caller fetches it instead.
    pub async fn create_order(&self, o: &NewGatewayOrder<'_>) -> Result<CfOrder, GatewayError> {
        guard_id(o.gateway_order_id)?;
        // Cashfree: `order_expiry_time` ISO-8601; `customer_id` 3-50 alphanumerics.
        let body = CreateOrderBody {
            order_id: o.gateway_order_id,
            order_amount: paise_to_rupees(o.amount_paise),
            order_currency: "INR",
            customer_details: CustomerDetails {
                customer_id: o.order_id.simple().to_string(),
                customer_phone: national_phone(o.customer_phone),
                customer_email: o.customer_email.and_then(|e| bounded(e, 3, 100)),
                customer_name: bounded(o.customer_name, 3, 100),
            },
            order_meta: OrderMeta {
                return_url: o.return_url,
                notify_url: o.notify_url,
            },
            order_expiry_time: o.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            order_note: format!("Order {}", o.order_number),
            order_tags: OrderTags {
                order_number: o.order_number,
                order_uuid: o.order_id.to_string(),
            },
        };
        let key = order_idempotency_key(o.gateway_order_id).to_string();
        self.send(
            Method::POST,
            "/orders".to_owned(),
            Some(&body),
            Some(&key),
            "create_order",
        )
        .await
    }

    /// `GET /orders/{order_id}` (retried once on a transient failure).
    pub async fn fetch_order(&self, gateway_order_id: &str) -> Result<CfOrder, GatewayError> {
        guard_id(gateway_order_id)?;
        self.get(format!("/orders/{gateway_order_id}"), "fetch_order")
            .await
    }

    /// `GET /orders/{order_id}/payments`: every attempt, successful or not.
    pub async fn fetch_order_payments(
        &self,
        gateway_order_id: &str,
    ) -> Result<Vec<CfPayment>, GatewayError> {
        guard_id(gateway_order_id)?;
        self.get(
            format!("/orders/{gateway_order_id}/payments"),
            "fetch_order_payments",
        )
        .await
    }

    /// `POST /orders/{order_id}/refunds` (speed STANDARD). `refund_id` (our
    /// `refunds.id`) is sent as Cashfree's `refund_id` and as the idempotency key.
    pub async fn create_refund(
        &self,
        gateway_order_id: &str,
        refund_id: Uuid,
        amount_paise: i64,
        order_number: &str,
    ) -> Result<CfRefund, GatewayError> {
        guard_id(gateway_order_id)?;
        let reference = refund_ref(refund_id);
        let body = RefundBody {
            refund_amount: paise_to_rupees(amount_paise),
            refund_id: &reference,
            refund_note: format!("Refund for order {order_number}")
                .chars()
                .take(100)
                .collect(),
            refund_speed: "STANDARD",
        };
        let key = refund_id.to_string();
        self.send(
            Method::POST,
            format!("/orders/{gateway_order_id}/refunds"),
            Some(&body),
            Some(&key),
            "create_refund",
        )
        .await
    }

    /// `GET /orders/{order_id}/refunds/{refund_id}`; 404 ⇒ the refund request
    /// never reached Cashfree.
    pub async fn fetch_refund(
        &self,
        gateway_order_id: &str,
        refund_id: &str,
    ) -> Result<CfRefund, GatewayError> {
        guard_id(gateway_order_id)?;
        guard_id(refund_id)?;
        self.get(
            format!("/orders/{gateway_order_id}/refunds/{refund_id}"),
            "fetch_refund",
        )
        .await
    }

    /// `GET /orders/{order_id}/refunds`: every refund of the order (ours,
    /// dashboard refunds and Cashfree's automatic ones).
    pub async fn fetch_order_refunds(
        &self,
        gateway_order_id: &str,
    ) -> Result<Vec<CfRefund>, GatewayError> {
        guard_id(gateway_order_id)?;
        self.get(
            format!("/orders/{gateway_order_id}/refunds"),
            "fetch_order_refunds",
        )
        .await
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: String,
        op: &'static str,
    ) -> Result<T, GatewayError> {
        match self
            .send::<T, ()>(Method::GET, path.clone(), None, None, op)
            .await
        {
            Err(GatewayError::Unavailable) => {
                tokio::time::sleep(GET_RETRY_DELAY).await;
                self.send::<T, ()>(Method::GET, path, None, None, op).await
            }
            other => other,
        }
    }

    async fn send<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        path: String,
        body: Option<&B>,
        idempotency_key: Option<&str>,
        op: &'static str,
    ) -> Result<T, GatewayError> {
        let request_id = Uuid::now_v7().to_string();
        let mut req = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .header("x-client-id", &self.app_id)
            .header("x-client-secret", self.secret_key.expose_secret())
            .header("x-api-version", API_VERSION)
            .header("x-request-id", &request_id)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(k) = idempotency_key {
            req = req.header("x-idempotency-key", k);
        }
        if let Some(b) = body {
            req = req.json(b);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    op,
                    request_id = %request_id,
                    timeout = e.is_timeout(),
                    connect = e.is_connect(),
                    "cashfree request failed"
                );
                return Err(GatewayError::Unavailable);
            }
        };
        let status = resp.status();
        let bytes = match resp.bytes().await {
            Ok(b) if b.len() <= MAX_RESPONSE_BYTES => b,
            Ok(_) => {
                tracing::warn!(op, request_id = %request_id, status = status.as_u16(), "cashfree response too large");
                return Err(GatewayError::Unavailable);
            }
            Err(e) => {
                tracing::warn!(
                    op,
                    request_id = %request_id,
                    status = status.as_u16(),
                    timeout = e.is_timeout(),
                    "cashfree response read failed"
                );
                return Err(GatewayError::Unavailable);
            }
        };

        if status.is_success() {
            return serde_json::from_slice::<T>(&bytes).map_err(|e| {
                // serde's message names the field/position, never the values.
                tracing::error!(op, request_id = %request_id, status = status.as_u16(), error = %e, "unexpected cashfree response shape");
                GatewayError::Unavailable
            });
        }

        let detail = serde_json::from_slice::<ErrorBody>(&bytes).unwrap_or_default();
        let code = truncate(detail.code.as_deref().unwrap_or("unknown"), 60);
        let message = truncate(
            &scrub(detail.message.as_deref().unwrap_or("")),
            MAX_LOGGED_MESSAGE,
        );
        if status.is_server_error()
            || status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::REQUEST_TIMEOUT
        {
            tracing::warn!(op, request_id = %request_id, status = status.as_u16(), code = %code, message = %message, "cashfree unavailable");
            return Err(GatewayError::Unavailable);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            tracing::error!(op, request_id = %request_id, status = status.as_u16(), code = %code, "cashfree rejected our API credentials");
        } else if status == StatusCode::NOT_FOUND || status == StatusCode::CONFLICT {
            tracing::debug!(op, request_id = %request_id, status = status.as_u16(), code = %code, "cashfree: not found / already exists");
        } else {
            tracing::warn!(op, request_id = %request_id, status = status.as_u16(), code = %code, message = %message, "cashfree rejected request");
        }
        Err(GatewayError::Rejected {
            status: status.as_u16(),
            code,
            message,
        })
    }
}

/// Ids are interpolated into URL paths: refuse anything outside `[A-Za-z0-9_-]`
/// before a request is ever built.
fn guard_id(id: &str) -> Result<(), GatewayError> {
    if signature::is_gateway_id(id) {
        Ok(())
    } else {
        Err(GatewayError::Rejected {
            status: 400,
            code: "invalid_id".into(),
            message: "malformed id (not sent)".into(),
        })
    }
}

/// Cashfree error messages are free text and could echo a request value
/// (e.g. the customer's phone or e-mail): mask anything that looks like one
/// before it is logged or stored.
fn scrub(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let digits = w.bytes().filter(u8::is_ascii_digit).count();
            if w.contains('@') || digits >= 7 {
                "[redacted]"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    fn gateway_order_id_is_deterministic_and_well_formed() {
        let id = Uuid::parse_str("01890a5d-ac96-774b-bcce-b302099a8057").expect("uuid");
        let a = gateway_order_id("ZM-261020-1001", id);
        assert_eq!(a, "ZM-261020-1001_b302099a8057");
        assert_eq!(a, gateway_order_id("ZM-261020-1001", id));
        assert!(signature::is_gateway_order_id(&a));
        let longest = gateway_order_id(&format!("ZM-261020-{}", "9".repeat(19)), id);
        assert!(longest.len() <= 45, "{longest}");
        assert_ne!(a, gateway_order_id("ZM-261020-1001", Uuid::now_v7()));
    }

    #[test]
    fn amounts_round_trip() {
        assert_eq!(rupees_to_paise(1650.0), Some(165_000));
        assert_eq!(rupees_to_paise(1650.5), Some(165_050));
        assert_eq!(rupees_to_paise(0.29), Some(29));
        assert_eq!(rupees_to_paise(-1.0), None);
        assert_eq!(rupees_to_paise(f64::NAN), None);
        assert_eq!(rupees_to_paise(1e12), None);
        for p in [1, 29, 100, 165_000, 165_050, 99_999_999] {
            assert_eq!(rupees_to_paise(paise_to_rupees(p)), Some(p));
        }
        let body = serde_json::to_string(&serde_json::json!({ "a": paise_to_rupees(165_050) }))
            .unwrap_or_default();
        assert_eq!(body, r#"{"a":1650.5}"#);
    }

    #[test]
    fn ids_accept_numbers_or_strings() {
        let p: CfPayment = serde_json::from_value(serde_json::json!({
            "cf_payment_id": 12376123, "order_id": "ZM-1_abc", "payment_status": "SUCCESS",
            "payment_amount": 10.5, "payment_currency": "INR", "payment_group": "upi"
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.cf_payment_id, "12376123");
        assert_eq!(p.amount_paise(), Some(1050));
        let p: CfPayment = serde_json::from_value(serde_json::json!({
            "cf_payment_id": "1453002795", "payment_status": "FAILED",
            "payment_amount": 1, "payment_currency": "INR", "error_details": null
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.cf_payment_id, "1453002795");

        let id = Uuid::now_v7();
        let r: CfRefund = serde_json::from_value(serde_json::json!({
            "cf_refund_id": 11325632, "cf_payment_id": "789727431", "refund_id": refund_ref(id),
            "order_id": "ZM-1_abc", "refund_amount": 2.0, "refund_status": "SUCCESS"
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.cf_refund_id, "11325632");
        assert_eq!(r.cf_payment_id.as_deref(), Some("789727431"));
        assert_eq!(r.our_refund_id(), Some(id));
        assert_eq!(r.amount_paise(), Some(200));
    }

    #[test]
    fn instrument_mapping() {
        let m = |g: &str| instrument(Some(g));
        assert_eq!(m("upi").as_deref(), Some("upi"));
        assert_eq!(m("credit_card").as_deref(), Some("card"));
        assert_eq!(m("debit_card").as_deref(), Some("card"));
        assert_eq!(m("prepaid_card").as_deref(), Some("card"));
        assert_eq!(m("credit_card_emi").as_deref(), Some("emi"));
        assert_eq!(m("cardless_emi").as_deref(), Some("emi"));
        assert_eq!(m("net_banking").as_deref(), Some("netbanking"));
        assert_eq!(m("pay_later").as_deref(), Some("paylater"));
        assert_eq!(m("wallet").as_deref(), Some("wallet"));
        assert_eq!(m("bank_transfer").as_deref(), Some("bank_transfer"));
        assert_eq!(m("Something New").as_deref(), None);
        assert_eq!(instrument(None), None);
    }

    #[test]
    fn phone_and_bounds() {
        assert_eq!(national_phone("+919840012345"), "9840012345");
        assert_eq!(national_phone("9840012345"), "9840012345");
        assert_eq!(bounded("Al", 3, 100), None);
        assert_eq!(bounded(" Arun ", 3, 100).as_deref(), Some("Arun"));
        assert_eq!(
            bounded(&"x".repeat(150), 3, 100).map(|s| s.len()),
            Some(100)
        );
    }

    #[test]
    fn error_messages_are_scrubbed() {
        assert_eq!(
            scrub("customer_phone : 9840012345 is invalid"),
            "customer_phone : [redacted] is invalid"
        );
        assert_eq!(scrub("bad email a@b.co"), "bad email [redacted]");
        assert_eq!(
            scrub("order with same id is already present"),
            "order with same id is already present"
        );
    }

    #[test]
    fn idempotency_key_is_stable_uuid() {
        let a = order_idempotency_key("ZM-261020-1001_8f3a2b1c9d0e");
        assert_eq!(a, order_idempotency_key("ZM-261020-1001_8f3a2b1c9d0e"));
        assert_ne!(a, order_idempotency_key("ZM-261020-1002_8f3a2b1c9d0e"));
        assert_eq!(a.get_version_num(), 4);
    }

    #[test]
    fn summary_is_bounded_and_errors_classified() {
        let e = GatewayError::Rejected {
            status: 400,
            code: "request_failed".into(),
            message: "x".repeat(1000),
        };
        assert!(e.summary().chars().count() <= 300);
        assert!(!e.is_not_found() && !e.is_conflict());
        let nf = GatewayError::Rejected {
            status: 404,
            code: "x".into(),
            message: String::new(),
        };
        assert!(nf.is_not_found());
        assert!(guard_id("ZM-1/../x").is_err());
        assert!(guard_id("ZM-261020-1001_8f3a2b1c9d0e").is_ok());
    }
}
