//! HS256 access tokens. Short-lived (ACCESS_TOKEN_TTL_SECS) and never valid past
//! the owning session's absolute expiry. Only HS256 is accepted on decode.

use std::time::Duration;

use anyhow::{Context, ensure};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{AppError, AppResult},
};

const TOKEN_TYPE_ACCESS: &str = "access";
/// Clock-skew tolerance for exp/nbf.
const LEEWAY_SECS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessClaims {
    /// Admin id.
    pub sub: Uuid,
    /// Session id.
    pub sid: Uuid,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub jti: Uuid,
    pub typ: String,
}

/// A freshly minted access token.
#[derive(Debug)]
pub struct IssuedToken {
    pub token: String,
    /// Seconds until `exp`.
    pub expires_in: i64,
}

pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    issuer: String,
    audience: String,
    access_ttl: Duration,
}

impl JwtKeys {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let secret = config.jwt_secret.expose_secret().as_bytes();
        ensure!(secret.len() >= 32, "JWT secret must be at least 32 bytes");
        ensure!(
            !config.jwt_issuer.is_empty() && !config.jwt_audience.is_empty(),
            "JWT issuer/audience must be set"
        );
        i64::try_from(config.access_token_ttl.as_secs())
            .context("access token TTL out of range")?;

        let mut validation = Validation::new(Algorithm::HS256);
        validation.algorithms = vec![Algorithm::HS256];
        validation.set_issuer(&[config.jwt_issuer.as_str()]);
        validation.set_audience(&[config.jwt_audience.as_str()]);
        validation.set_required_spec_claims(&["exp", "nbf", "iat", "iss", "aud", "sub"]);
        validation.leeway = LEEWAY_SECS;
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.validate_aud = true;

        Ok(Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation,
            issuer: config.jwt_issuer.clone(),
            audience: config.jwt_audience.clone(),
            access_ttl: config.access_token_ttl,
        })
    }

    /// Mint an access token for `admin_id`/`session_id`. `exp` is clamped to the
    /// session's absolute expiry. Fails with `SessionExpired` if that is already past.
    pub fn issue(
        &self,
        admin_id: Uuid,
        session_id: Uuid,
        session_expires_at: DateTime<Utc>,
    ) -> AppResult<IssuedToken> {
        self.issue_at(Utc::now(), admin_id, session_id, session_expires_at)
    }

    fn issue_at(
        &self,
        now: DateTime<Utc>,
        admin_id: Uuid,
        session_id: Uuid,
        session_expires_at: DateTime<Utc>,
    ) -> AppResult<IssuedToken> {
        let now_ts = now.timestamp();
        // Validated to fit in i64 at construction.
        let ttl = i64::try_from(self.access_ttl.as_secs()).unwrap_or(i64::MAX);
        let exp = now_ts
            .saturating_add(ttl)
            .min(session_expires_at.timestamp());
        if exp <= now_ts {
            return Err(AppError::SessionExpired);
        }

        let claims = AccessClaims {
            sub: admin_id,
            sid: session_id,
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            iat: now_ts,
            nbf: now_ts,
            exp,
            jti: Uuid::now_v7(),
            typ: TOKEN_TYPE_ACCESS.to_owned(),
        };
        let token = jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .context("failed to sign access token")?;
        Ok(IssuedToken {
            token,
            expires_in: exp - now_ts,
        })
    }

    /// Verify signature, algorithm, issuer, audience, exp/nbf and token type.
    /// Every failure maps to `Unauthorized`; the reason is only logged at debug.
    pub fn verify(&self, token: &str) -> AppResult<AccessClaims> {
        let data = jsonwebtoken::decode::<AccessClaims>(token, &self.decoding, &self.validation)
            .map_err(|e| {
                tracing::debug!(reason = ?e.kind(), "access token rejected");
                AppError::Unauthorized
            })?;
        let claims = data.claims;
        if claims.typ != TOKEN_TYPE_ACCESS || claims.iat > claims.exp {
            tracing::debug!("access token rejected: wrong type or inconsistent timestamps");
            return Err(AppError::Unauthorized);
        }
        Ok(claims)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::Duration as ChronoDuration;

    use super::*;
    use crate::config::{AppEnv, EmailProvider};

    pub(crate) fn test_config() -> Config {
        Config {
            app_env: AppEnv::Development,
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            database_url: "postgres://localhost/test".into(),
            db_max_connections: 5,
            db_min_connections: 0,
            db_acquire_timeout: Duration::from_secs(5),
            jwt_secret: "unit-test-jwt-secret-0123456789abcdef-XYZ".into(),
            jwt_issuer: "zhimart-api".into(),
            jwt_audience: "zhimart-admin".into(),
            access_token_ttl: Duration::from_secs(900),
            session_ttl: Duration::from_secs(30 * 24 * 3600),
            otp_pepper: "unit-test-otp-pepper-0123456789abcdef-XYZ".into(),
            otp_ttl: Duration::from_secs(300),
            otp_max_attempts: 5,
            otp_resend_cooldown: Duration::from_secs(60),
            otp_max_resends: 3,
            login_max_failures: 5,
            login_lockout: Duration::from_secs(900),
            cookie_secure: true,
            allowed_origins: vec![axum::http::HeaderValue::from_static(
                "https://admin.zhimart.com",
            )],
            trusted_proxies: vec![],
            rate_limit_global_per_minute: 300,
            rate_limit_auth_per_minute: 10,
            request_timeout: Duration::from_secs(15),
            max_body_bytes: 16_384,
            max_concurrent_requests: 1024,
            email_provider: EmailProvider::Log,
            ses_from_address: String::new(),
            ses_configuration_set: None,
            payments_enabled: true,
            payment_test_mode: false,
            razorpay_key_id: "rzp_test_0123456789abcd".into(),
            razorpay_key_secret: "test_key_secret_0123456789".into(),
            razorpay_webhook_secret: "test_webhook_secret_0123456789".into(),
            razorpay_api_base: "https://api.razorpay.com".into(),
            payment_expiry: std::time::Duration::from_secs(30 * 60),
            store_name: "Zhi-Mart Fireworks".into(),
            order_notifications_enabled: false,
            order_notification_emails: Vec::new(),
            public_site_url: None,
            customer_order_emails_enabled: false,
            pricing: Default::default(),
        }
    }

    fn keys() -> JwtKeys {
        JwtKeys::new(&test_config()).expect("keys")
    }

    fn sign_raw(claims: &serde_json::Value, alg: Algorithm, secret: &[u8]) -> String {
        jsonwebtoken::encode(&Header::new(alg), claims, &EncodingKey::from_secret(secret))
            .expect("encode")
    }

    fn valid_claims_json() -> serde_json::Value {
        let now = Utc::now().timestamp();
        serde_json::json!({
            "sub": Uuid::now_v7(), "sid": Uuid::now_v7(), "iss": "zhimart-api", "aud": "zhimart-admin",
            "iat": now, "nbf": now, "exp": now + 600, "jti": Uuid::now_v7(), "typ": "access"
        })
    }

    const SECRET: &[u8] = b"unit-test-jwt-secret-0123456789abcdef-XYZ";

    #[test]
    fn roundtrip() {
        let k = keys();
        let (admin, sid) = (Uuid::now_v7(), Uuid::now_v7());
        let issued = k
            .issue(admin, sid, Utc::now() + ChronoDuration::days(30))
            .expect("issue");
        assert_eq!(issued.expires_in, 900);
        let claims = k.verify(&issued.token).expect("verify");
        assert_eq!(claims.sub, admin);
        assert_eq!(claims.sid, sid);
        assert_eq!(claims.typ, "access");
        assert_eq!(claims.exp - claims.iat, 900);
        assert_eq!(claims.jti.get_version_num(), 7);
    }

    #[test]
    fn hand_signed_valid_token_is_accepted() {
        assert!(
            keys()
                .verify(&sign_raw(&valid_claims_json(), Algorithm::HS256, SECRET))
                .is_ok()
        );
    }

    #[test]
    fn exp_is_clamped_to_session_expiry() {
        let k = keys();
        let now = Utc::now();
        let session_end = now + ChronoDuration::seconds(120);
        let issued = k
            .issue_at(now, Uuid::now_v7(), Uuid::now_v7(), session_end)
            .expect("issue");
        assert_eq!(issued.expires_in, 120);
        let claims = k.verify(&issued.token).expect("verify");
        assert_eq!(claims.exp, session_end.timestamp());
    }

    #[test]
    fn issuing_for_ended_session_fails() {
        let k = keys();
        let now = Utc::now();
        let r = k.issue_at(
            now,
            Uuid::now_v7(),
            Uuid::now_v7(),
            now - ChronoDuration::seconds(1),
        );
        assert!(matches!(r, Err(AppError::SessionExpired)));
    }

    #[test]
    fn rejects_wrong_audience_and_issuer() {
        let k = keys();
        let mut c = valid_claims_json();
        c["aud"] = "someone-else".into();
        assert!(matches!(
            k.verify(&sign_raw(&c, Algorithm::HS256, SECRET)),
            Err(AppError::Unauthorized)
        ));

        let mut c = valid_claims_json();
        c["iss"] = "evil".into();
        assert!(matches!(
            k.verify(&sign_raw(&c, Algorithm::HS256, SECRET)),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn rejects_wrong_type_and_missing_claims() {
        let k = keys();
        let mut c = valid_claims_json();
        c["typ"] = "refresh".into();
        assert!(k.verify(&sign_raw(&c, Algorithm::HS256, SECRET)).is_err());

        let mut c = valid_claims_json();
        c.as_object_mut().expect("object").remove("nbf");
        assert!(k.verify(&sign_raw(&c, Algorithm::HS256, SECRET)).is_err());
    }

    #[test]
    fn rejects_expired_and_not_yet_valid() {
        let k = keys();
        let now = Utc::now().timestamp();
        let mut c = valid_claims_json();
        c["iat"] = (now - 1000).into();
        c["nbf"] = (now - 1000).into();
        c["exp"] = (now - 100).into();
        assert!(k.verify(&sign_raw(&c, Algorithm::HS256, SECRET)).is_err());

        let mut c = valid_claims_json();
        c["nbf"] = (now + 120).into();
        assert!(k.verify(&sign_raw(&c, Algorithm::HS256, SECRET)).is_err());
    }

    #[test]
    fn rejects_other_algorithms_and_keys() {
        let k = keys();
        // Different HMAC algorithm with the same secret.
        assert!(
            k.verify(&sign_raw(&valid_claims_json(), Algorithm::HS512, SECRET))
                .is_err()
        );
        // Wrong secret.
        assert!(
            k.verify(&sign_raw(
                &valid_claims_json(),
                Algorithm::HS256,
                b"another-secret-of-sufficient-length!!"
            ))
            .is_err()
        );
        // alg: none (unsigned).
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(valid_claims_json().to_string());
        assert!(k.verify(&format!("{header}.{payload}.")).is_err());
        assert!(k.verify(&format!("{header}.{payload}")).is_err());
    }

    #[test]
    fn rejects_tampered_payload() {
        let k = keys();
        let issued = k
            .issue(
                Uuid::now_v7(),
                Uuid::now_v7(),
                Utc::now() + ChronoDuration::days(1),
            )
            .expect("issue");
        let mut parts: Vec<String> = issued.token.split('.').map(str::to_owned).collect();
        let mut claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[1]).expect("b64")).expect("json");
        claims["sub"] = Uuid::now_v7().to_string().into();
        parts[1] = URL_SAFE_NO_PAD.encode(claims.to_string());
        assert!(k.verify(&parts.join(".")).is_err());
        assert!(k.verify("garbage").is_err());
        assert!(k.verify("").is_err());
    }
}
