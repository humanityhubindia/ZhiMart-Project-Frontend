//! Client network metadata. The edge middleware (`middleware::client_ip`)
//! resolves the real client IP once per request (honouring X-Forwarded-For only
//! from TRUSTED_PROXIES) and stores it as [`ResolvedClientIp`]. Handlers read it
//! through the [`ClientMeta`] extractor.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header, request::Parts},
};

/// Request extension inserted by the client-IP middleware.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedClientIp(pub IpAddr);

const MAX_USER_AGENT_LEN: usize = 512;

/// Client IP + user agent, for audit logging and session metadata.
#[derive(Debug, Clone)]
pub struct ClientMeta {
    pub ip: IpAddr,
    pub user_agent: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for ClientMeta {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ResolvedClientIp>()
            .map(|r| r.0)
            .or_else(|| {
                parts
                    .extensions
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|c| c.0.ip())
            })
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|ua| ua.chars().take(MAX_USER_AGENT_LEN).collect::<String>());

        Ok(ClientMeta { ip, user_agent })
    }
}
