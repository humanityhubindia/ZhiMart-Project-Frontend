//! Per-client-IP rate limiting (GCRA via `governor`).
//!
//! Keys are the resolved client IP (see `client_ip`), except that IPv6
//! addresses are bucketed by their /64 prefix: a single host is routinely
//! assigned a whole /64 and could otherwise rotate addresses to evade limits.
//!
//! Limits are per process. With several replicas each enforces its own budget;
//! see README for the shared-store / edge rate-limiting scale path.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{ConnectInfo, MatchedPath, Request, State},
    http::Method,
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock},
    middleware::NoOpMiddleware,
    state::keyed::DefaultKeyedStateStore,
};

use crate::{config::Config, error::AppError, net::ResolvedClientIp};

/// Session endpoints (refresh/logout/me): steady 60/min with a burst of 20, so
/// several tabs refreshing at once are fine.
pub const SESSION_PER_MINUTE: u32 = 60;
pub const SESSION_BURST: u32 = 20;
/// Public order placement: a real customer places one order, maybe retries once.
pub const ORDERS_PER_MINUTE: u32 = 6;
pub const ORDERS_BURST: u32 = 3;
/// Guest payment endpoints (storefront config, pay, order status).
/// The order status page polls `GET /orders/{n}` every ~3 s (20/min) for a
/// couple of minutes after checkout, plus pay; 40/min leaves room for a
/// second tab. Every call also needs a valid per-order token.
pub const PAYMENTS_PER_MINUTE: u32 = 40;
pub const PAYMENTS_BURST: u32 = 15;
/// `POST /orders/{n}/pay/verify` has its own budget: while a payment is
/// pending the storefront polls it every ~5 s (12/min) for up to 2 minutes,
/// independently of the status polling above. 30/min with a burst of 10 fits
/// two tabs (or two customers behind one carrier-grade NAT address) polling
/// at once. Each call costs one Cashfree API read, so it stays bounded.
pub const VERIFY_PER_MINUTE: u32 = 30;
pub const VERIFY_BURST: u32 = 10;
/// Cashfree webhooks are authenticated by HMAC, not by IP, and arrive in
/// bursts from a few gateway IPs. This is only a safety valve against a
/// flood of unsigned junk from one address, far above legitimate volume.
pub const WEBHOOKS_PER_MINUTE: u32 = 600;
pub const WEBHOOKS_BURST: u32 = 100;

/// Credential endpoints get a small burst: password + OTP + one resend fits,
/// a brute-force run does not.
const CREDENTIAL_MAX_BURST: u32 = 5;
const CREDENTIAL_MIN_BURST: u32 = 3;

type KeyedLimiter<C> =
    RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, C, NoOpMiddleware<<C as Clock>::Instant>>;

/// One named, keyed GCRA limiter.
pub struct IpRateLimiter<C: Clock = DefaultClock> {
    name: &'static str,
    limiter: KeyedLimiter<C>,
}

impl IpRateLimiter<DefaultClock> {
    pub fn new(name: &'static str, quota: Quota) -> Self {
        Self::with_clock(name, quota, DefaultClock::default())
    }
}

impl<C: Clock> IpRateLimiter<C> {
    pub fn with_clock(name: &'static str, quota: Quota, clock: C) -> Self {
        Self {
            name,
            limiter: RateLimiter::new(quota, DefaultKeyedStateStore::default(), clock),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Consume one cell for `ip`. On rejection returns the whole seconds
    /// (rounded up, at least 1) until a request would be allowed again.
    pub fn check(&self, ip: IpAddr) -> Result<(), u64> {
        match self.limiter.check_key(&bucket_key(ip)) {
            Ok(()) => Ok(()),
            Err(not_until) => Err(ceil_secs(
                not_until.wait_time_from(self.limiter.clock().now()),
            )),
        }
    }

    /// Drop keys whose state is indistinguishable from fresh, then release
    /// memory. Returns the number of keys still tracked.
    pub fn gc(&self) -> usize {
        self.limiter.retain_recent();
        self.limiter.shrink_to_fit();
        self.limiter.len()
    }

    pub fn len(&self) -> usize {
        self.limiter.len()
    }

    pub fn is_empty(&self) -> bool {
        self.limiter.is_empty()
    }
}

/// The limiters used by the router. Cheap to clone; the jobs module holds a
/// clone to garbage-collect them.
#[derive(Clone)]
pub struct RateLimiters {
    /// Whole `/api` subtree.
    pub global: Arc<IpRateLimiter>,
    /// login / otp verify / otp resend.
    pub credentials: Arc<IpRateLimiter>,
    /// refresh / logout / me.
    pub session: Arc<IpRateLimiter>,
    /// Public checkout (`POST /orders`): placing an order reserves stock, so
    /// this is deliberately tight.
    pub orders: Arc<IpRateLimiter>,
    /// Guest payment endpoints (`/storefront/config`, `/orders/{n}[/pay]`).
    pub payments: Arc<IpRateLimiter>,
    /// `POST /orders/{n}/pay/verify` (polled while a payment is pending).
    pub payment_verify: Arc<IpRateLimiter>,
    /// `POST /webhooks/cashfree` only (it is outside the global /api limiter).
    pub webhooks: Arc<IpRateLimiter>,
}

impl RateLimiters {
    pub fn from_config(config: &Config) -> Self {
        let global = config.rate_limit_global_per_minute;
        let auth = config.rate_limit_auth_per_minute;
        Self {
            global: Arc::new(IpRateLimiter::new(
                "global",
                per_minute(global, (global / 4).max(1)),
            )),
            credentials: Arc::new(IpRateLimiter::new(
                "credentials",
                per_minute(
                    auth,
                    (auth / 2)
                        .clamp(CREDENTIAL_MIN_BURST, CREDENTIAL_MAX_BURST)
                        .min(auth),
                ),
            )),
            session: Arc::new(IpRateLimiter::new(
                "session",
                per_minute(SESSION_PER_MINUTE, SESSION_BURST),
            )),
            orders: Arc::new(IpRateLimiter::new(
                "orders",
                per_minute(ORDERS_PER_MINUTE, ORDERS_BURST),
            )),
            payments: Arc::new(IpRateLimiter::new(
                "payments",
                per_minute(PAYMENTS_PER_MINUTE, PAYMENTS_BURST),
            )),
            payment_verify: Arc::new(IpRateLimiter::new(
                "payment_verify",
                per_minute(VERIFY_PER_MINUTE, VERIFY_BURST),
            )),
            webhooks: Arc::new(IpRateLimiter::new(
                "webhooks",
                per_minute(WEBHOOKS_PER_MINUTE, WEBHOOKS_BURST),
            )),
        }
    }

    pub fn all(&self) -> [&Arc<IpRateLimiter>; 7] {
        [
            &self.global,
            &self.credentials,
            &self.session,
            &self.orders,
            &self.payments,
            &self.payment_verify,
            &self.webhooks,
        ]
    }
}

/// `rate` cells replenished evenly over a minute, up to `burst` at once.
pub fn per_minute(rate: u32, burst: u32) -> Quota {
    let rate = NonZeroU32::new(rate).unwrap_or(NonZeroU32::MIN);
    let burst = NonZeroU32::new(burst).unwrap_or(NonZeroU32::MIN);
    Quota::per_minute(rate).allow_burst(burst)
}

/// IPv4 (and IPv4-mapped IPv6) addresses are keyed as-is; native IPv6 is
/// truncated to its /64 network.
pub fn bucket_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & !((1u128 << 64) - 1))),
    }
}

fn ceil_secs(d: Duration) -> u64 {
    (d.as_secs() + u64::from(d.subsec_nanos() > 0)).max(1)
}

/// Axum middleware enforcing one limiter. Attach with
/// `axum::middleware::from_fn_with_state(limiter, rate_limit::enforce)`.
pub async fn enforce(
    State(limiter): State<Arc<IpRateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    check_then_run(&limiter, req, next).await
}

/// A limiter plus routes it must never block. Used for endpoints whose
/// rejection would be harmful, e.g. a rate-limited logout silently leaves the
/// session alive.
#[derive(Clone)]
pub struct LimitPolicy {
    limiter: Arc<IpRateLimiter>,
    /// `(method, matched route path)` pairs, matched against axum's
    /// `MatchedPath` (the full template, e.g. `/api/v1/auth/logout`).
    exempt: Arc<[(Method, &'static str)]>,
}

impl LimitPolicy {
    pub fn new(limiter: &Arc<IpRateLimiter>, exempt: &[(Method, &'static str)]) -> Self {
        Self {
            limiter: Arc::clone(limiter),
            exempt: exempt.into(),
        }
    }

    fn is_exempt(&self, req: &Request) -> bool {
        let Some(path) = req.extensions().get::<MatchedPath>() else {
            return false;
        };
        self.exempt
            .iter()
            .any(|(m, p)| m == req.method() && *p == path.as_str())
    }
}

/// Like [`enforce`], but skips the policy's exempt routes. Must be attached
/// with `route_layer` (so `MatchedPath` is set).
pub async fn enforce_policy(
    State(policy): State<LimitPolicy>,
    req: Request,
    next: Next,
) -> Response {
    if policy.is_exempt(&req) {
        return next.run(req).await;
    }
    check_then_run(&policy.limiter, req, next).await
}

async fn check_then_run(limiter: &IpRateLimiter, req: Request, next: Next) -> Response {
    let ip = req
        .extensions()
        .get::<ResolvedClientIp>()
        .map(|r| r.0)
        .or_else(|| {
            req.extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip())
        })
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    match limiter.check(ip) {
        Ok(()) => next.run(req).await,
        Err(retry_after_secs) => {
            tracing::debug!(limiter = limiter.name(), client_ip = %ip, retry_after_secs, "rate limited");
            AppError::TooManyRequests { retry_after_secs }.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use governor::clock::FakeRelativeClock;

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("valid ip")
    }

    #[test]
    fn allows_burst_then_denies_with_retry_after() {
        let clock = FakeRelativeClock::default();
        // 10/min => one cell every 6s, burst 3.
        let l = IpRateLimiter::with_clock("t", per_minute(10, 3), clock.clone());
        let a = ip("198.51.100.1");
        assert!(l.check(a).is_ok());
        assert!(l.check(a).is_ok());
        assert!(l.check(a).is_ok());
        assert_eq!(l.check(a), Err(6));

        clock.advance(Duration::from_millis(4_500));
        assert_eq!(l.check(a), Err(2), "1.5s remaining rounds up to 2");

        clock.advance(Duration::from_millis(1_500));
        assert!(l.check(a).is_ok());
        assert!(l.check(a).is_err());
    }

    #[test]
    fn keys_are_independent() {
        let l = IpRateLimiter::with_clock("t", per_minute(1, 1), FakeRelativeClock::default());
        assert!(l.check(ip("198.51.100.1")).is_ok());
        assert!(l.check(ip("198.51.100.1")).is_err());
        assert!(l.check(ip("198.51.100.2")).is_ok());
    }

    #[test]
    fn retry_after_is_at_least_one_second() {
        assert_eq!(ceil_secs(Duration::ZERO), 1);
        assert_eq!(ceil_secs(Duration::from_millis(1)), 1);
        assert_eq!(ceil_secs(Duration::from_secs(2)), 2);
        assert_eq!(ceil_secs(Duration::from_millis(2_001)), 3);
    }

    #[test]
    fn ipv6_is_bucketed_by_slash_64() {
        assert_eq!(
            bucket_key(ip("2001:db8:1:2:aaaa:bbbb:cccc:dddd")),
            ip("2001:db8:1:2::")
        );
        assert_eq!(
            bucket_key(ip("2001:db8:1:2::1")),
            bucket_key(ip("2001:db8:1:2:ffff::9"))
        );
        assert_ne!(
            bucket_key(ip("2001:db8:1:2::1")),
            bucket_key(ip("2001:db8:1:3::1"))
        );
        assert_eq!(bucket_key(ip("203.0.113.7")), ip("203.0.113.7"));
        assert_eq!(bucket_key(ip("::ffff:203.0.113.7")), ip("203.0.113.7"));

        let l = IpRateLimiter::with_clock("t", per_minute(1, 1), FakeRelativeClock::default());
        assert!(l.check(ip("2001:db8:1:2::1")).is_ok());
        assert!(
            l.check(ip("2001:db8:1:2:dead:beef::1")).is_err(),
            "same /64 shares a bucket"
        );
        assert!(l.check(ip("2001:db8:1:3::1")).is_ok());
    }

    #[test]
    fn gc_drops_idle_keys() {
        let clock = FakeRelativeClock::default();
        let l = IpRateLimiter::with_clock("t", per_minute(60, 5), clock.clone());
        for i in 0..50u8 {
            assert!(l.check(IpAddr::V4(Ipv4Addr::new(198, 51, 100, i))).is_ok());
        }
        assert_eq!(l.len(), 50);
        clock.advance(Duration::from_secs(120));
        assert_eq!(l.gc(), 0);
        assert!(l.is_empty());
    }

    #[test]
    fn verify_polling_fits_the_budget() {
        // Two tabs polling every 5 s for 2 minutes, plus the initial call each.
        let clock = FakeRelativeClock::default();
        let l = IpRateLimiter::with_clock(
            "v",
            per_minute(VERIFY_PER_MINUTE, VERIFY_BURST),
            clock.clone(),
        );
        let a = ip("198.51.100.9");
        for _ in 0..24 {
            assert!(l.check(a).is_ok(), "tab 1");
            assert!(l.check(a).is_ok(), "tab 2");
            clock.advance(Duration::from_secs(5));
        }
        // A tight loop is still cut off.
        let b = ip("198.51.100.10");
        let allowed = (0..100).filter(|_| l.check(b).is_ok()).count();
        assert_eq!(allowed, VERIFY_BURST as usize);
    }

    #[test]
    fn credential_burst_derivation() {
        let q = |auth: u32| {
            (auth / 2)
                .clamp(CREDENTIAL_MIN_BURST, CREDENTIAL_MAX_BURST)
                .min(auth)
        };
        assert_eq!(q(1), 1);
        assert_eq!(q(2), 2);
        assert_eq!(q(10), 5);
        assert_eq!(q(7), 3);
        assert_eq!(q(100), 5);
    }
}
