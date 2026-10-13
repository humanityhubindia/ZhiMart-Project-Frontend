//! Real client IP resolution.
//!
//! The TCP peer address is the only thing a client cannot forge. We consult
//! `X-Forwarded-For` **only** when that peer is one of `TRUSTED_PROXIES`, and
//! then use the "rightmost untrusted" rule: walk the list from the right
//! (entries appended by our own proxies) and stop at the first address that is
//! not a trusted proxy. Anything to the left of it was written by the client
//! and is ignored. With no trusted proxies configured the header is ignored
//! entirely, so spoofing it has no effect.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName},
    middleware::Next,
    response::Response,
};
use ipnetwork::IpNetwork;

use crate::net::ResolvedClientIp;

static X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// Upper bound on hops examined in `X-Forwarded-For`. Real deployments have a
/// handful of proxies; anything longer is either misconfiguration or abuse.
const MAX_FORWARDED_HOPS: usize = 16;

/// Trusted proxy set, shared by every request.
#[derive(Debug, Clone)]
pub struct ClientIpResolver {
    trusted_proxies: Arc<[IpNetwork]>,
}

impl ClientIpResolver {
    pub fn new(trusted_proxies: &[IpNetwork]) -> Self {
        Self {
            trusted_proxies: Arc::from(trusted_proxies),
        }
    }

    fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted_proxies.iter().any(|net| net.contains(ip))
    }

    /// Resolve the client IP for a request that arrived from `peer`.
    pub fn resolve(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let peer = peer.to_canonical();
        if self.trusted_proxies.is_empty() || !self.is_trusted(peer) {
            return peer;
        }

        // Multiple X-Forwarded-For header lines are one list, in order.
        let mut examined = 0usize;
        let mut leftmost_trusted = None;
        for value in headers.get_all(&X_FORWARDED_FOR).iter().rev() {
            let Ok(value) = value.to_str() else {
                return peer;
            };
            for entry in value.rsplit(',') {
                let entry = entry.trim();
                if entry.is_empty() {
                    // Tolerate "a, , b" and trailing commas.
                    continue;
                }
                examined += 1;
                if examined > MAX_FORWARDED_HOPS {
                    return peer;
                }
                let Some(ip) = parse_forwarded_ip(entry) else {
                    // A trusted hop wrote something we cannot parse: nothing to
                    // the left of it can be attributed reliably.
                    return peer;
                };
                if !self.is_trusted(ip) {
                    return ip;
                }
                leftmost_trusted = Some(ip);
            }
        }

        // Either no header, or every hop is one of our own proxies (e.g. an
        // internal health checker). The leftmost hop is the best we know.
        leftmost_trusted.unwrap_or(peer)
    }
}

/// Parse one `X-Forwarded-For` element. Accepts bare IPv4/IPv6, `ip:port`,
/// `[v6]` and `[v6]:port` (some proxies append the source port).
fn parse_forwarded_ip(entry: &str) -> Option<IpAddr> {
    if let Ok(ip) = entry.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Ok(sock) = entry.parse::<SocketAddr>() {
        return Some(sock.ip().to_canonical());
    }
    entry
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|inner| inner.parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
}

/// Axum middleware: resolve the client IP once and store it as
/// [`ResolvedClientIp`] for the rate limiter, handlers and audit logging.
pub async fn resolve_client_ip(
    State(resolver): State<ClientIpResolver>,
    mut req: Request,
    next: Next,
) -> Response {
    let peer = match req.extensions().get::<ConnectInfo<SocketAddr>>() {
        Some(ConnectInfo(addr)) => addr.ip(),
        None => {
            // Only happens when the router is driven without the server's
            // accept loop (e.g. in-process tests). Everyone shares one key.
            tracing::debug!("no ConnectInfo on request; client IP unknown");
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        }
    };
    let ip = resolver.resolve(peer, req.headers());
    tracing::Span::current().record("client_ip", tracing::field::display(ip));
    req.extensions_mut().insert(ResolvedClientIp(ip));
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum::http::HeaderValue;

    use super::*;

    fn resolver(nets: &[&str]) -> ClientIpResolver {
        let nets: Vec<IpNetwork> = nets
            .iter()
            .map(|n| IpNetwork::from_str(n).expect("valid network"))
            .collect();
        ClientIpResolver::new(&nets)
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("valid ip")
    }

    fn xff(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append(
                &X_FORWARDED_FOR,
                HeaderValue::from_str(v).expect("valid header"),
            );
        }
        h
    }

    #[test]
    fn no_trusted_proxies_ignores_xff() {
        let r = resolver(&[]);
        assert_eq!(
            r.resolve(ip("203.0.113.9"), &xff(&["1.2.3.4"])),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn spoofed_xff_from_untrusted_peer_is_ignored() {
        let r = resolver(&["10.0.0.0/8"]);
        assert_eq!(
            r.resolve(ip("198.51.100.7"), &xff(&["1.1.1.1, 10.0.0.5"])),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn single_trusted_hop_uses_rightmost_entry() {
        let r = resolver(&["10.0.0.0/8"]);
        // Client sent a fake "6.6.6.6"; the proxy appended the real address.
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["6.6.6.6, 198.51.100.7"])),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn multi_hop_chain_skips_trusted_proxies() {
        let r = resolver(&["10.0.0.0/8", "172.16.0.0/12"]);
        let headers = xff(&["6.6.6.6, 203.0.113.50, 172.16.4.4, 10.1.1.1"]);
        assert_eq!(r.resolve(ip("10.0.0.2"), &headers), ip("203.0.113.50"));
    }

    #[test]
    fn multiple_header_lines_are_one_list() {
        let r = resolver(&["10.0.0.0/8"]);
        let headers = xff(&["6.6.6.6", "203.0.113.50, 10.9.9.9"]);
        assert_eq!(r.resolve(ip("10.0.0.2"), &headers), ip("203.0.113.50"));
        let headers = xff(&["203.0.113.50", "10.9.9.9"]);
        assert_eq!(r.resolve(ip("10.0.0.2"), &headers), ip("203.0.113.50"));
    }

    #[test]
    fn garbage_from_trusted_hop_falls_back_to_peer() {
        let r = resolver(&["10.0.0.0/8"]);
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["1.2.3.4, not-an-ip"])),
            ip("10.0.0.2")
        );
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["unknown"])),
            ip("10.0.0.2")
        );
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["1.2.3.4, 999.1.1.1"])),
            ip("10.0.0.2")
        );
    }

    #[test]
    fn garbage_left_of_real_client_is_ignored() {
        let r = resolver(&["10.0.0.0/8"]);
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["<script>, 203.0.113.50"])),
            ip("203.0.113.50")
        );
    }

    #[test]
    fn non_utf8_header_falls_back_to_peer() {
        let r = resolver(&["10.0.0.0/8"]);
        let mut h = HeaderMap::new();
        h.insert(
            &X_FORWARDED_FOR,
            HeaderValue::from_bytes(b"1.2.3.4\xff").expect("opaque header"),
        );
        assert_eq!(r.resolve(ip("10.0.0.2"), &h), ip("10.0.0.2"));
    }

    #[test]
    fn missing_or_empty_xff_uses_peer() {
        let r = resolver(&["10.0.0.0/8"]);
        assert_eq!(r.resolve(ip("10.0.0.2"), &HeaderMap::new()), ip("10.0.0.2"));
        assert_eq!(r.resolve(ip("10.0.0.2"), &xff(&[" , "])), ip("10.0.0.2"));
    }

    #[test]
    fn all_trusted_hops_yields_leftmost() {
        let r = resolver(&["10.0.0.0/8"]);
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["10.5.5.5, 10.6.6.6"])),
            ip("10.5.5.5")
        );
    }

    #[test]
    fn too_many_hops_falls_back_to_peer() {
        let r = resolver(&["10.0.0.0/8"]);
        let chain = vec!["10.1.1.1"; MAX_FORWARDED_HOPS + 1].join(", ");
        assert_eq!(r.resolve(ip("10.0.0.2"), &xff(&[&chain])), ip("10.0.0.2"));
    }

    #[test]
    fn ipv6_entries_and_ports() {
        let r = resolver(&["10.0.0.0/8", "fd00::/8"]);
        assert_eq!(
            r.resolve(ip("fd00::1"), &xff(&["2001:db8::1"])),
            ip("2001:db8::1")
        );
        assert_eq!(
            r.resolve(ip("fd00::1"), &xff(&["[2001:db8::2]:443"])),
            ip("2001:db8::2")
        );
        assert_eq!(
            r.resolve(ip("fd00::1"), &xff(&["[2001:db8::3]"])),
            ip("2001:db8::3")
        );
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["198.51.100.7:51234"])),
            ip("198.51.100.7")
        );
        assert_eq!(
            r.resolve(ip("10.0.0.2"), &xff(&["2001:db8::4, fd00::9"])),
            ip("2001:db8::4")
        );
    }

    #[test]
    fn ipv4_mapped_addresses_are_normalised() {
        // Dual-stack listener: an IPv4 proxy shows up as ::ffff:10.0.0.2.
        let r = resolver(&["10.0.0.0/8"]);
        assert_eq!(
            r.resolve(ip("::ffff:10.0.0.2"), &xff(&["::ffff:203.0.113.5"])),
            ip("203.0.113.5")
        );
        // And an untrusted mapped peer is reported as plain IPv4.
        let r = resolver(&[]);
        assert_eq!(
            r.resolve(ip("::ffff:198.51.100.1"), &HeaderMap::new()),
            ip("198.51.100.1")
        );
    }
}
