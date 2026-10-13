//! `AuthAdmin`: the authenticated admin for Bearer-protected endpoints.
//!
//! Verifies the JWT and then checks the session row on every request, so a
//! logout, reuse-triggered revocation, password reset or deactivation takes
//! effect immediately rather than when the access token expires.

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header, request::Parts},
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    auth::jwt::AccessClaims,
    error::{AppError, AppResult},
    repo,
    state::AppState,
};

/// Upper bound on the size of a presented access token.
pub const MAX_TOKEN_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct AuthAdmin {
    pub admin_id: Uuid,
    pub session_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub session_expires_at: DateTime<Utc>,
}

/// Extract the token from exactly one `Authorization: Bearer <token>` header.
/// Returns `Ok(None)` if there is no Authorization header at all.
pub fn bearer_token(headers: &HeaderMap) -> AppResult<Option<&str>> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = match (values.next(), values.next()) {
        (None, _) => return Ok(None),
        (Some(v), None) => v,
        (Some(_), Some(_)) => return Err(AppError::Unauthorized),
    };
    let value = value.to_str().map_err(|_| AppError::Unauthorized)?;
    let (scheme, token) = value.split_once(' ').ok_or(AppError::Unauthorized)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AppError::Unauthorized);
    }
    let token = token.trim();
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
        return Err(AppError::Unauthorized);
    }
    Ok(Some(token))
}

/// Verify the Bearer token in `headers` (signature and claims only, no DB).
///
/// Deliberately private: a valid signature alone does NOT mean the session is
/// live (it may be logged out, revoked or past its absolute expiry). Endpoints
/// must authenticate with the [`AuthAdmin`] extractor, which also checks the
/// session row.
fn verified_claims(headers: &HeaderMap, state: &AppState) -> AppResult<AccessClaims> {
    let token = bearer_token(headers)?.ok_or(AppError::Unauthorized)?;
    state.jwt.verify(token)
}

impl FromRequestParts<AppState> for AuthAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let claims = verified_claims(&parts.headers, state)?;
        let row = repo::sessions::load_for_auth(&state.db, claims.sid, claims.sub)
            .await?
            .ok_or_else(|| {
                tracing::debug!(session_id = %claims.sid, "access token for a dead session");
                AppError::Unauthorized
            })?;
        Ok(AuthAdmin {
            admin_id: claims.sub,
            session_id: claims.sid,
            email: row.email,
            display_name: row.display_name,
            session_expires_at: row.absolute_expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn h(values: &[&'static str]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for v in values {
            m.append(header::AUTHORIZATION, HeaderValue::from_static(v));
        }
        m
    }

    #[test]
    fn parses_bearer() {
        assert_eq!(
            bearer_token(&h(&["Bearer abc.def.ghi"])).ok().flatten(),
            Some("abc.def.ghi")
        );
        assert_eq!(
            bearer_token(&h(&["bearer abc"])).ok().flatten(),
            Some("abc")
        );
        assert_eq!(bearer_token(&HeaderMap::new()).ok().flatten(), None);
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(bearer_token(&h(&["Basic abc"])).is_err());
        assert!(bearer_token(&h(&["Bearer"])).is_err());
        assert!(bearer_token(&h(&["Bearer "])).is_err());
        assert!(bearer_token(&h(&["Bearer a", "Bearer b"])).is_err());
        let long = format!("Bearer {}", "a".repeat(MAX_TOKEN_BYTES + 1));
        let mut m = HeaderMap::new();
        m.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&long).expect("header"),
        );
        assert!(bearer_token(&m).is_err());
    }
}
