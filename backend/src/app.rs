//! HTTP application: routes, the edge middleware stack, and the hardened
//! connection accept loop.
//!
//! Edge stack, outermost first (a request flows top → bottom, the response
//! bottom → top):
//!
//! 1. request-id sanitise → set (`x-request-id`, UUID) → propagate to response
//! 2. mark `Authorization`/`Cookie` sensitive → `TraceLayer` (span carries the
//!    request id + client IP) → mark `Set-Cookie` sensitive
//! 3. security headers — outside everything that can produce an error, so
//!    503/413/500 responses get them too
//! 4. JSON error normaliser — rewrites any non-JSON 4xx/5xx (axum rejections,
//!    default 405, tower-http 413) into the `{"error":{...}}` shape
//! 5. catch-panic → JSON 500 (no panic text leaked)
//! 6. CORS — answers preflights before any limiter/DB work, and decorates
//!    429/503 responses so the SPA can read them
//! 7. client IP resolution (`ResolvedClientIp`)
//! 8. split: `/health/*` probes get only a timeout (never shed, so an
//!    overloaded instance is not restarted); everything else gets:
//!    - global concurrency limit + load shedding → immediate 503
//!      `service_unavailable` instead of queueing
//!    - request timeout → 503 `service_unavailable`
//!    - request body limit → 413 (JSON via 4)
//!    - routes: `/api` has a per-IP global limiter; credential and session
//!      routes each add their own stricter per-IP limiter.

use std::{
    any::Any,
    future::Future,
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    error_handling::HandleErrorLayer,
    extract::{ConnectInfo, Request},
    http::{HeaderName, HeaderValue, Method, Response, StatusCode, header},
    middleware::{from_fn, from_fn_with_state, map_request},
    response::IntoResponse,
};
use hyper::body::Incoming;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder as ConnBuilder,
};
use ipnetwork::IpNetwork;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, watch},
    time::Sleep,
};
use tower::{
    BoxError, ServiceBuilder, ServiceExt, limit::ConcurrencyLimitLayer, load_shed::LoadShedLayer,
};
use tower_http::{
    LatencyUnit,
    catch_panic::CatchPanicLayer,
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    trace::{DefaultOnFailure, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, Span};

use crate::{
    config::Config,
    error::AppError,
    middleware::{
        client_ip::{ClientIpResolver, resolve_client_ip},
        conn_limit::{MAX_CONNECTIONS_PER_IP, PerIpConnections},
        json_error, json_errors,
        rate_limit::{self, IpRateLimiter, RateLimiters},
        security_headers::{SecurityHeaders, security_headers},
    },
    routes,
    state::AppState,
};

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
static X_REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

/// Longest client-supplied `x-request-id` we keep (UUIDs are 36).
const MAX_REQUEST_ID_LEN: usize = 64;
/// Longest request path written to logs.
const MAX_LOGGED_PATH: usize = 256;
const CORS_MAX_AGE: Duration = Duration::from_secs(10 * 60);

// ---- connection-level hardening (accept loop) ----

/// Hard cap on open client connections. The permit is taken *before*
/// `accept()`, so under a connection flood we stop accepting (the kernel
/// backlog fills and the LB/proxy sees back-pressure) instead of exhausting
/// file descriptors or memory.
pub const MAX_CONNECTIONS: u32 = 10_000;
/// A connection with no request in flight for this long is closed gracefully
/// (HTTP/2 GOAWAY). hyper has no HTTP/2 idle timeout and its keep-alive pings
/// keep an idle-but-responsive h2 connection open forever otherwise. HTTP/1
/// idle keep-alive connections are already closed by `HEADER_READ_TIMEOUT`.
pub const CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Time a client has to send its complete request head (HTTP/1) — and, via
/// [`HandshakeDeadline`], to send its first bytes on a fresh connection.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Per-connection HTTP/1 read buffer cap (bounds header size; min is 8 KiB).
const H1_MAX_BUF_SIZE: usize = 64 * 1024;
const H1_MAX_HEADERS: usize = 64;
const H2_MAX_CONCURRENT_STREAMS: u32 = 100;
const H2_MAX_HEADER_LIST_SIZE: u32 = 16 * 1024;
const H2_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);
const H2_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long in-flight requests get to finish after a shutdown signal.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(20);
const ACCEPT_BACKOFF_MIN: Duration = Duration::from_millis(5);
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(1);

/// The router plus handles the background jobs need.
pub struct App {
    pub router: Router,
    pub limiters: RateLimiters,
}

/// Full HTTP application: routes + middleware stack.
pub fn build_router(state: AppState) -> Router {
    build_app(state).router
}

/// Like [`build_router`], but also returns the rate limiters so the caller can
/// garbage-collect them (see `jobs`).
pub fn build_app(state: AppState) -> App {
    let config = Arc::clone(&state.config);
    let limiters = RateLimiters::from_config(&config);
    let (probes, routes) = route_tree(state, &limiters);
    App {
        router: with_edge_stack(probes, routes, &config),
        limiters,
    }
}

fn limited(router: Router<AppState>, limiter: &Arc<IpRateLimiter>) -> Router<AppState> {
    // `route_layer` panics on an empty router; also keeps the limiter off the
    // fallback so it only counts requests that hit a real endpoint.
    if router.has_routes() {
        router.route_layer(from_fn_with_state(Arc::clone(limiter), rate_limit::enforce))
    } else {
        router
    }
}

const API_V1: &str = "/api/v1";
static X_ORDER_TOKEN: HeaderName = HeaderName::from_static("x-order-token");

/// Full matched path of the logout route (nested under /api/v1).
pub const LOGOUT_ROUTE: &str = "/api/v1/auth/logout";

/// Like [`limited`] with exemptions. Logout must never be rate limited: a
/// rejected logout leaves the session alive while the user believes they
/// signed out. It stays under the global /api limiter.
fn limited_except(
    router: Router<AppState>,
    limiter: &Arc<IpRateLimiter>,
    exempt: &[(Method, &'static str)],
) -> Router<AppState> {
    if router.has_routes() {
        router.route_layer(from_fn_with_state(
            rate_limit::LimitPolicy::new(limiter, exempt),
            rate_limit::enforce_policy,
        ))
    } else {
        router
    }
}

/// Returns `(probes, everything_else)`.
fn route_tree(state: AppState, limiters: &RateLimiters) -> (Router, Router) {
    let v1 = Router::new()
        .merge(limited(
            routes::auth::credential_routes(),
            &limiters.credentials,
        ))
        .merge(limited_except(
            routes::auth::session_routes(),
            &limiters.session,
            &[(Method::POST, LOGOUT_ROUTE)],
        ))
        .merge(routes::catalog::public_routes())
        .merge(limited(routes::orders::public_routes(), &limiters.orders))
        .merge(limited(
            routes::payments::public_routes(),
            &limiters.payments,
        ))
        .merge(limited(
            routes::payments::verify_routes(),
            &limiters.payment_verify,
        ))
        .nest("/admin", routes::admin::routes());

    // Cashfree webhooks: authenticated by HMAC signature, delivered in bursts
    // from a few gateway IPs. Kept OUT of the global per-IP /api limiter; only
    // a generous dedicated safety-valve limiter applies. Body limit, timeout,
    // load shedding, security headers and tracing still apply (edge stack).
    let webhooks = limited(routes::payments::webhook_routes(), &limiters.webhooks);

    let probes = routes::health::routes().with_state(state.clone());
    let app = mount_api(v1, webhooks, &limiters.global).with_state(state);
    (probes, app)
}

/// Mount `v1` at `/api/v1` behind the global per-IP limiter, and `unthrottled`
/// at `/api/v1` beside it (not behind that limiter). Static webhook paths take
/// precedence over the `/api` fallback catch-all, so the two do not overlap.
fn mount_api<S: Clone + Send + Sync + 'static>(
    v1: Router<S>,
    unthrottled: Router<S>,
    global: &Arc<IpRateLimiter>,
) -> Router<S> {
    // `.layer` (not `route_layer`) so 404 probing under /api is budgeted too.
    let api = Router::new()
        .nest("/v1", v1)
        .fallback(not_found)
        .layer(from_fn_with_state(Arc::clone(global), rate_limit::enforce));

    let mut app = Router::new().nest("/api", api);
    if unthrottled.has_routes() {
        app = app.merge(Router::new().nest(API_V1, unthrottled));
    }
    app.fallback(not_found)
}

async fn not_found() -> AppError {
    AppError::NotFound
}

fn cors_layer(config: &Config) -> CorsLayer {
    // An empty list matches no origin: same-origin only (dev uses a Vite proxy).
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(config.allowed_origins.iter().cloned()))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            X_REQUESTED_WITH.clone(),
            // Guest order capability token for /orders/{n}/pay*, /orders/{n}.
            X_ORDER_TOKEN.clone(),
        ])
        .allow_credentials(true)
        .expose_headers([header::RETRY_AFTER, X_REQUEST_ID.clone()])
        .max_age(CORS_MAX_AGE)
}

fn with_edge_stack(probes: Router, routes: Router, config: &Config) -> Router {
    let trace = TraceLayer::new_for_http()
        .make_span_with(make_span)
        .on_request(())
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(LatencyUnit::Millis),
        )
        // Status is already logged by on_response; AppError logs internals.
        .on_failure(
            DefaultOnFailure::new()
                .level(Level::DEBUG)
                .latency_unit(LatencyUnit::Millis),
        );

    // Application traffic: shed load, bound time and body size.
    let guarded = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(edge_error_response))
        .layer(LoadShedLayer::new())
        .layer(ConcurrencyLimitLayer::new(config.max_concurrent_requests))
        .layer(tower::timeout::TimeoutLayer::new(config.request_timeout))
        .layer(RequestBodyLimitLayer::new(config.max_body_bytes))
        .service(routes);

    // Probes bypass load shedding: under overload /health/live must still say
    // "alive" or the orchestrator restarts healthy instances and makes the
    // overload worse. They are cheap (readiness is cached/single-flighted)
    // and only get a timeout. Router::layer is fine here: Timeout is stateless.
    let probes = probes.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(edge_error_response))
            .layer(tower::timeout::TimeoutLayer::new(config.request_timeout)),
    );
    let inner = probes.fallback_service(guarded);

    let stack = ServiceBuilder::new()
        .layer(map_request(sanitize_request_id))
        .layer(SetRequestIdLayer::new(
            X_REQUEST_ID.clone(),
            MakeRequestUuid,
        ))
        .layer(PropagateRequestIdLayer::new(X_REQUEST_ID.clone()))
        .layer(SetSensitiveRequestHeadersLayer::new([
            header::AUTHORIZATION,
            header::COOKIE,
        ]))
        .layer(trace)
        .layer(SetSensitiveResponseHeadersLayer::new([header::SET_COOKIE]))
        .layer(from_fn_with_state(
            SecurityHeaders::new(config.app_env.is_production()),
            security_headers,
        ))
        .layer(from_fn(json_errors))
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(cors_layer(config))
        .layer(from_fn_with_state(
            ClientIpResolver::new(&config.trusted_proxies),
            resolve_client_ip,
        ))
        .service(inner);

    // Mounting each stack as a single fallback service keeps ONE instance of
    // every stateful layer (Router::layer would wrap each route separately,
    // e.g. giving every route its own concurrency budget).
    Router::new().fallback_service(stack)
}

fn make_span(req: &Request) -> Span {
    let request_id = req
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    let path = req.uri().path();
    let path = path.get(..MAX_LOGGED_PATH).unwrap_or(path);
    tracing::info_span!(
        "http_request",
        method = %req.method(),
        path = %path,
        request_id = %request_id,
        client_ip = tracing::field::Empty,
    )
}

fn is_valid_request_id(v: &HeaderValue) -> bool {
    let b = v.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_REQUEST_ID_LEN
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':'))
}

/// Keep an upstream `x-request-id` (useful behind a proxy that sets one) only
/// if it is a single, short, boring token; otherwise drop it so a fresh UUID
/// is generated. Prevents log pollution through the request id.
async fn sanitize_request_id(mut req: Request) -> Request {
    let headers = req.headers();
    let mut values = headers.get_all(&X_REQUEST_ID).iter();
    let keep = match (values.next(), values.next()) {
        (None, _) => true,
        (Some(v), None) => is_valid_request_id(v),
        (Some(_), Some(_)) => false,
    };
    if !keep {
        req.headers_mut().remove(&X_REQUEST_ID);
    }
    req
}

fn panic_response(err: Box<dyn Any + Send + 'static>) -> Response<Body> {
    let detail = err
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| err.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    // Logged server-side only; the client gets the generic 500 body.
    AppError::Internal(anyhow::anyhow!("request handler panicked: {detail}")).into_response()
}

async fn edge_error_response(err: BoxError) -> axum::response::Response {
    if err.is::<tower::load_shed::error::Overloaded>() {
        tracing::warn!("overloaded: concurrency limit reached, shedding request");
        let mut res = json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "The service is temporarily unavailable. Please try again.",
        );
        res.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        res
    } else if err.is::<tower::timeout::error::Elapsed>() {
        tracing::warn!("request exceeded REQUEST_TIMEOUT_SECS");
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "The service is temporarily unavailable. Please try again.",
        )
    } else {
        AppError::Internal(anyhow::anyhow!("unhandled middleware error: {err}")).into_response()
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

fn connection_builder() -> ConnBuilder<TokioExecutor> {
    let mut builder = ConnBuilder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT)
        .keep_alive(true)
        .max_buf_size(H1_MAX_BUF_SIZE)
        .max_headers(H1_MAX_HEADERS);
    builder
        .http2()
        .timer(TokioTimer::new())
        .max_concurrent_streams(H2_MAX_CONCURRENT_STREAMS)
        .max_header_list_size(H2_MAX_HEADER_LIST_SIZE)
        .keep_alive_interval(H2_KEEPALIVE_INTERVAL)
        .keep_alive_timeout(H2_KEEPALIVE_TIMEOUT);
    builder
}

async fn shutdown_requested(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|stop| *stop).await;
}

/// Errors that concern only the connection being accepted; retry at once.
fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
    )
}

/// Connection-level limits for [`serve`].
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// Peers in these networks are exempt from the per-IP connection cap.
    pub trusted_proxies: Vec<IpNetwork>,
    pub max_connections: u32,
    pub max_connections_per_ip: u32,
    pub idle_timeout: Duration,
}

impl ServeOptions {
    pub fn new(trusted_proxies: &[IpNetwork]) -> Self {
        Self {
            trusted_proxies: trusted_proxies.to_vec(),
            max_connections: MAX_CONNECTIONS,
            max_connections_per_ip: MAX_CONNECTIONS_PER_IP,
            idle_timeout: CONNECTION_IDLE_TIMEOUT,
        }
    }
}

/// Per-connection request activity, for the idle timeout.
struct ConnActivity {
    started: Instant,
    in_flight: AtomicUsize,
    /// Milliseconds since `started` at the last request start/finish.
    last_change_ms: AtomicU64,
}

impl ConnActivity {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            in_flight: AtomicUsize::new(0),
            last_change_ms: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        let ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.last_change_ms.store(ms, Ordering::Relaxed);
    }

    fn begin(self: &Arc<Self>) -> InFlight {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        self.touch();
        InFlight(Arc::clone(self))
    }

    fn idle_for(&self) -> Option<Duration> {
        if self.in_flight.load(Ordering::Acquire) > 0 {
            return None;
        }
        let since = Duration::from_millis(self.last_change_ms.load(Ordering::Relaxed));
        Some(self.started.elapsed().saturating_sub(since))
    }

    /// Resolves once no request has been in flight for `timeout`.
    async fn idle(&self, timeout: Duration) {
        let check_every = (timeout / 4).max(Duration::from_millis(10));
        loop {
            tokio::time::sleep(check_every).await;
            if self.idle_for().is_some_and(|idle| idle >= timeout) {
                return;
            }
        }
    }
}

/// Marks one request in flight on its connection until dropped.
struct InFlight(Arc<ConnActivity>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.0.touch();
    }
}

/// Serve `router` on `listener` until `shutdown` becomes `true`, then stop
/// accepting and give in-flight requests up to [`DRAIN_TIMEOUT`] to finish.
///
/// Uses hyper-util directly (rather than `axum::serve`) for the knobs that
/// matter against slowloris / connection-exhaustion attacks: HTTP/1 header
/// read timeout, HTTP/2 stream and header caps, a first-byte deadline, a
/// global and a per-IP connection cap, and an idle-connection timeout.
pub async fn serve(
    listener: TcpListener,
    router: Router,
    mut shutdown: watch::Receiver<bool>,
    opts: ServeOptions,
) -> anyhow::Result<()> {
    let builder = Arc::new(connection_builder());
    let conn_permits = Arc::new(Semaphore::new(opts.max_connections as usize));
    let per_ip = PerIpConnections::new(opts.max_connections_per_ip, &opts.trusted_proxies);
    let idle_timeout = opts.idle_timeout;
    let mut backoff = Duration::ZERO;

    if let Ok(addr) = listener.local_addr() {
        tracing::info!(
            %addr,
            max_connections = opts.max_connections,
            max_connections_per_ip = opts.max_connections_per_ip,
            "listening"
        );
    }

    loop {
        let permit = tokio::select! {
            biased;
            _ = shutdown_requested(&mut shutdown) => break,
            permit = Arc::clone(&conn_permits).acquire_owned() => match permit {
                Ok(p) => p,
                Err(_) => break, // semaphore closed: never happens, but stop cleanly
            },
        };

        let (stream, peer) = tokio::select! {
            biased;
            _ = shutdown_requested(&mut shutdown) => break,
            accepted = listener.accept() => match accepted {
                Ok(conn) => {
                    backoff = Duration::ZERO;
                    conn
                }
                Err(e) if is_connection_error(&e) => continue,
                Err(e) => {
                    // EMFILE/ENFILE/ENOBUFS/ENOMEM: resource exhaustion. Back off
                    // so we do not spin, and keep serving existing connections.
                    backoff = (backoff * 2).clamp(ACCEPT_BACKOFF_MIN, ACCEPT_BACKOFF_MAX);
                    tracing::error!(error = %e, backoff_ms = backoff.as_millis() as u64, "accept failed");
                    drop(permit);
                    tokio::select! {
                        _ = shutdown_requested(&mut shutdown) => break,
                        _ = tokio::time::sleep(backoff) => continue,
                    }
                }
            },
        };

        let Some(ip_slot) = per_ip.try_acquire(peer.ip()) else {
            // Close immediately: no TLS/HTTP work is spent on the excess socket.
            tracing::debug!(%peer, "per-IP connection cap reached; closing connection");
            drop(stream);
            continue;
        };

        if let Err(e) = stream.set_nodelay(true) {
            tracing::debug!(error = %e, "failed to set TCP_NODELAY");
        }

        let io = TokioIo::new(HandshakeDeadline::new(stream, HEADER_READ_TIMEOUT));
        let activity = Arc::new(ConnActivity::new());
        let service = {
            let router = router.clone();
            let activity = Arc::clone(&activity);
            hyper::service::service_fn(move |mut req: hyper::Request<Incoming>| {
                req.extensions_mut().insert(ConnectInfo(peer));
                let in_flight = activity.begin();
                let response = router.clone().oneshot(req);
                async move {
                    let res = response.await;
                    drop(in_flight);
                    res
                }
            })
        };
        let builder = Arc::clone(&builder);
        let mut conn_shutdown = shutdown.clone();

        tokio::spawn(async move {
            let conn = builder.serve_connection(io, service);
            tokio::pin!(conn);
            let mut closing = false;
            let result = loop {
                tokio::select! {
                    res = conn.as_mut() => break res,
                    _ = shutdown_requested(&mut conn_shutdown), if !closing => {
                        conn.as_mut().graceful_shutdown();
                        closing = true;
                    }
                    _ = activity.idle(idle_timeout), if !closing => {
                        tracing::debug!(%peer, "closing idle connection");
                        conn.as_mut().graceful_shutdown();
                        closing = true;
                    }
                }
            };
            if let Err(e) = result {
                tracing::debug!(%peer, error = %e, "connection closed with error");
            }
            drop(ip_slot);
            drop(permit);
        });
    }

    drop(listener);
    let open = opts.max_connections as usize - conn_permits.available_permits();
    tracing::info!(
        open_connections = open,
        "shutdown: stopped accepting, draining connections"
    );
    // Every connection task holds one permit until it finishes, so getting all
    // of them back means every connection has drained.
    match tokio::time::timeout(
        DRAIN_TIMEOUT,
        conn_permits.acquire_many(opts.max_connections),
    )
    .await
    {
        Ok(_) => tracing::info!("shutdown: all connections drained"),
        Err(_) => tracing::warn!(
            timeout_secs = DRAIN_TIMEOUT.as_secs(),
            remaining = opts.max_connections as usize - conn_permits.available_permits(),
            "shutdown: drain deadline reached, closing remaining connections"
        ),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// First-bytes deadline
// ---------------------------------------------------------------------------

const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// hyper-util's auto (HTTP/1 + HTTP/2) builder first sniffs up to 24 bytes to
/// pick the protocol, and that phase has no timeout of its own: a client could
/// open a socket, send nothing (or a partial HTTP/2 preface) and hold a
/// connection slot forever. This wrapper fails reads once `deadline` passes
/// until the protocol is decided — i.e. the bytes diverge from the HTTP/2
/// preface (HTTP/1, whose header timeout then applies) or the full preface
/// arrived (HTTP/2, kept honest by keep-alive pings).
struct HandshakeDeadline<T> {
    inner: T,
    deadline: Option<Pin<Box<Sleep>>>,
    matched: usize,
}

impl HandshakeDeadline<TcpStream> {
    fn new(inner: TcpStream, timeout: Duration) -> Self {
        Self {
            inner,
            deadline: Some(Box::pin(tokio::time::sleep(timeout))),
            matched: 0,
        }
    }
}

impl<T> HandshakeDeadline<T> {
    fn observe(&mut self, new: &[u8]) {
        if new.is_empty() {
            self.deadline = None; // EOF
            return;
        }
        let want = &H2_PREFACE[self.matched..];
        let n = new.len().min(want.len());
        if new[..n] != want[..n] || self.matched + n == H2_PREFACE.len() {
            self.deadline = None;
        } else {
            self.matched += n;
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for HandshakeDeadline<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if this.deadline.is_some() {
                    this.observe(&buf.filled()[before..]);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => {
                if let Some(deadline) = this.deadline.as_mut()
                    && deadline.as_mut().poll(cx).is_ready()
                {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "client did not start a request in time",
                    )));
                }
                Poll::Pending
            }
        }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for HandshakeDeadline<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        str::FromStr,
    };

    use axum::{extract::ConnectInfo, routing::get};
    use http_body_util::BodyExt;
    use ipnetwork::IpNetwork;
    use secrecy::SecretString;
    use sqlx::postgres::PgPoolOptions;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::{
        config::{AppEnv, EmailProvider},
        email::log::LogEmailSender,
        net::ResolvedClientIp,
    };

    fn test_config() -> Config {
        Config {
            app_env: AppEnv::Production,
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            database_url: SecretString::from("postgres://u:p@127.0.0.1:1/db"),
            db_max_connections: 2,
            db_min_connections: 0,
            db_acquire_timeout: Duration::from_millis(500),
            jwt_secret: SecretString::from("j".repeat(48)),
            jwt_issuer: "zhimart-api".into(),
            jwt_audience: "zhimart-admin".into(),
            access_token_ttl: Duration::from_secs(900),
            session_ttl: Duration::from_secs(86_400),
            otp_pepper: SecretString::from("p".repeat(48)),
            otp_ttl: Duration::from_secs(300),
            otp_max_attempts: 5,
            otp_resend_cooldown: Duration::from_secs(60),
            otp_max_resends: 3,
            login_max_failures: 5,
            login_lockout: Duration::from_secs(900),
            cookie_secure: true,
            allowed_origins: vec![HeaderValue::from_static("https://admin.example.com")],
            trusted_proxies: vec![IpNetwork::from_str("10.0.0.0/8").expect("net")],
            rate_limit_global_per_minute: 4,
            rate_limit_auth_per_minute: 10,
            request_timeout: Duration::from_millis(300),
            max_body_bytes: 1024,
            max_concurrent_requests: 1,
            email_provider: EmailProvider::Ses,
            ses_from_address: "no-reply@example.com".into(),
            ses_configuration_set: None,
            payments_enabled: true,
            payment_test_mode: false,
            cashfree_env: crate::config::CashfreeEnv::Sandbox,
            cashfree_app_id: "TEST1234567890abcdef".into(),
            cashfree_secret_key: SecretString::from("cfsk_ma_test_0123456789abcdef"),
            cashfree_api_base: "https://sandbox.cashfree.com/pg".into(),
            cashfree_notify_url: None,
            payment_expiry: std::time::Duration::from_secs(30 * 60),
            store_name: "Zhi-Mart Fireworks".into(),
            order_notifications_enabled: false,
            order_notification_emails: Vec::new(),
            public_site_url: None,
            customer_order_emails_enabled: false,
            pricing: Default::default(),
            google_sheets: None,
        }
    }

    fn full_app(config: Config) -> Router {
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(500))
            .connect_lazy("postgres://u:p@127.0.0.1:1/db")
            .expect("lazy pool");
        let state = AppState::new(Arc::new(config), pool, Arc::new(LogEmailSender)).expect("state");
        build_router(state)
    }

    fn req(method: Method, uri: &str) -> Request {
        let mut r = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        r.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 1], 4000))));
        r
    }

    async fn json(res: axum::response::Response) -> serde_json::Value {
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json")
    }

    #[tokio::test]
    async fn liveness_has_security_headers_and_request_id() {
        let res = full_app(test_config())
            .oneshot(req(Method::GET, "/health/live"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        let h = res.headers();
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(
            h["strict-transport-security"],
            "max-age=63072000; includeSubDomains"
        );
        let id = h["x-request-id"].to_str().expect("ascii");
        assert!(
            uuid::Uuid::parse_str(id).is_ok(),
            "generated request id is a UUID: {id}"
        );
        assert_eq!(json(res).await["status"], "ok");
    }

    #[tokio::test]
    async fn readiness_reports_unreachable_database() {
        let mut cfg = test_config();
        cfg.request_timeout = Duration::from_secs(10);
        let res = full_app(cfg)
            .oneshot(req(Method::GET, "/health/ready"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(res.headers()["cache-control"], "no-store");
        assert_eq!(json(res).await["status"], "unavailable");
    }

    #[tokio::test]
    async fn request_id_is_sanitised() {
        let app = full_app(test_config());
        let mut r = req(Method::GET, "/health/live");
        r.headers_mut()
            .insert(&X_REQUEST_ID, HeaderValue::from_static("lb-trace.123:abc"));
        let res = app.clone().oneshot(r).await.expect("infallible");
        assert_eq!(res.headers()["x-request-id"], "lb-trace.123:abc");

        let mut r = req(Method::GET, "/health/live");
        r.headers_mut().insert(
            &X_REQUEST_ID,
            HeaderValue::from_static("evil value with spaces"),
        );
        let res = app.oneshot(r).await.expect("infallible");
        let id = res.headers()["x-request-id"].to_str().expect("ascii");
        assert!(uuid::Uuid::parse_str(id).is_ok());
    }

    #[tokio::test]
    async fn api_404_is_json_no_store_and_globally_rate_limited() {
        // Global: 4/min with burst 1.
        let app = full_app(test_config());
        let res = app
            .clone()
            .oneshot(req(Method::GET, "/api/v1/does-not-exist"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(res.headers()["cache-control"], "no-store");
        assert_eq!(json(res).await["error"]["code"], "not_found");

        let res = app
            .clone()
            .oneshot(req(Method::GET, "/api/anything"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res.headers()["retry-after"], "15");
        assert_eq!(json(res).await["error"]["retryAfter"], 15);

        // Health is outside /api and never limited.
        let res = app
            .oneshot(req(Method::GET, "/health/live"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn logout_is_exempt_from_the_session_limiter() {
        let mut cfg = test_config();
        cfg.rate_limit_global_per_minute = 10_000;
        cfg.request_timeout = Duration::from_secs(10);
        let app = full_app(cfg);
        let post = |path: &str| {
            let mut r = req(Method::POST, path);
            r.headers_mut().insert(
                "x-requested-with",
                HeaderValue::from_static("XMLHttpRequest"),
            );
            r
        };
        // Exhaust the session limiter (burst 20) via /auth/refresh.
        let mut limited = false;
        for _ in 0..=rate_limit::SESSION_BURST {
            let res = app
                .clone()
                .oneshot(post("/api/v1/auth/refresh"))
                .await
                .expect("infallible");
            limited |= res.status() == StatusCode::TOO_MANY_REQUESTS;
        }
        assert!(limited, "refresh is limited once the burst is spent");
        let res = app
            .clone()
            .oneshot(post("/api/v1/auth/refresh"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

        for _ in 0..5 {
            let res = app
                .clone()
                .oneshot(post(LOGOUT_ROUTE))
                .await
                .expect("infallible");
            assert_ne!(
                res.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "logout must not be session-limited"
            );
        }
    }

    #[tokio::test]
    async fn handler_cache_control_is_preserved() {
        let cfg = test_config();
        let app = edge_with(
            Router::new()
                .route(
                    "/api/v1/products",
                    get(|| async { ([(header::CACHE_CONTROL, "public, max-age=10")], "[]") }),
                )
                .route("/api/v1/other", get(|| async { "x" })),
            &cfg,
        );
        let res = app
            .clone()
            .oneshot(req(Method::GET, "/api/v1/products"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        let values: Vec<_> = res
            .headers()
            .get_all(header::CACHE_CONTROL)
            .iter()
            .collect();
        assert_eq!(
            values,
            ["public, max-age=10"],
            "no-store must not be added or appended"
        );
        assert_eq!(res.headers()["x-content-type-options"], "nosniff");

        let res = app
            .oneshot(req(Method::GET, "/api/v1/other"))
            .await
            .expect("infallible");
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn idle_connections_are_closed_and_per_ip_cap_applies() {
        let router = Router::new().route("/x", get(|| async { "ok" }));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = watch::channel(false);
        let opts = ServeOptions {
            trusted_proxies: vec![],
            max_connections: 100,
            max_connections_per_ip: 2,
            idle_timeout: Duration::from_millis(300),
        };
        let server = tokio::spawn(serve(listener, router, rx, opts));

        // Two connections fill this IP's quota; a third is closed at once.
        let mut c1 = TcpStream::connect(addr).await.expect("c1");
        let _c2 = TcpStream::connect(addr).await.expect("c2");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut c3 = TcpStream::connect(addr).await.expect("c3");
        let mut buf = [0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(2), c3.read(&mut buf))
            .await
            .expect("closed promptly")
            .unwrap_or(0);
        assert_eq!(n, 0, "over-cap connection is closed without a response");

        // c1 serves a keep-alive request, then is closed once idle.
        c1.write_all(b"GET /x HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .expect("write");
        let n = c1.read(&mut buf).await.expect("read");
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));
        let started = std::time::Instant::now();
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), c1.read_to_end(&mut rest))
            .await
            .expect("idle connection closed")
            .expect("read");
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "not closed before the idle timeout"
        );

        tx.send(true).expect("send");
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("server stops")
            .expect("join")
            .expect("serve ok");
    }

    #[tokio::test]
    async fn webhooks_bypass_the_global_limiter_but_keep_edge_limits() {
        let cfg = test_config(); // body limit 1024
        let global = Arc::new(IpRateLimiter::new("global", rate_limit::per_minute(1, 1)));
        let webhook_limiter = Arc::new(IpRateLimiter::new(
            "webhooks",
            rate_limit::per_minute(600, 100),
        ));
        let v1 = Router::new().route("/things", get(|| async { "things" }));
        let webhooks = Router::new()
            .route(
                "/webhooks/cashfree",
                axum::routing::post(|| async { "ack" }),
            )
            .route_layer(from_fn_with_state(webhook_limiter, rate_limit::enforce));
        let app = with_edge_stack(Router::new(), mount_api(v1, webhooks, &global), &cfg);

        let res = app
            .clone()
            .oneshot(req(Method::GET, "/api/v1/things"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        let res = app
            .clone()
            .oneshot(req(Method::GET, "/api/v1/things"))
            .await
            .expect("infallible");
        assert_eq!(
            res.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "global limiter spent"
        );

        for _ in 0..20 {
            let res = app
                .clone()
                .oneshot(req(Method::POST, "/api/v1/webhooks/cashfree"))
                .await
                .expect("infallible");
            assert_eq!(
                res.status(),
                StatusCode::OK,
                "webhook is not globally limited"
            );
            assert_eq!(res.headers()["x-content-type-options"], "nosniff");
            assert!(res.headers().contains_key("x-request-id"));
        }

        // Unknown paths (even beside the webhook) stay under the global limiter.
        for path in ["/api/v1/webhooks/other", "/api/v1/nope", "/api/other"] {
            let res = app
                .clone()
                .oneshot(req(Method::POST, path))
                .await
                .expect("infallible");
            assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS, "{path}");
        }

        // Body limit still applies to the webhook.
        let mut r = req(Method::POST, "/api/v1/webhooks/cashfree");
        r.headers_mut()
            .insert(header::CONTENT_LENGTH, HeaderValue::from(4096u32));
        *r.body_mut() = Body::from(vec![b'{'; 4096]);
        let res = app.clone().oneshot(r).await.expect("infallible");
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Wrong method on the webhook path is a JSON 405, not a panic or 404.
        let res = app
            .oneshot(req(Method::GET, "/api/v1/webhooks/cashfree"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn cashfree_webhook_is_mounted_outside_the_global_limiter() {
        // Global limit 4/min (test_config): the old Razorpay path is gone (404),
        // and the Cashfree webhook keeps answering (400: unsigned) past that limit.
        let app = full_app(test_config());
        let res = app
            .clone()
            .oneshot(req(Method::POST, "/api/v1/webhooks/razorpay"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        for _ in 0..8 {
            let res = app
                .clone()
                .oneshot(req(Method::POST, "/api/v1/webhooks/cashfree"))
                .await
                .expect("infallible");
            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
            assert_eq!(json(res).await["error"]["code"], "validation_error");
        }
    }

    #[tokio::test]
    async fn cors_allows_order_token_header() {
        let mut r = req(Method::OPTIONS, "/api/v1/orders/ZM-1/pay");
        r.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("https://admin.example.com"),
        );
        r.headers_mut().insert(
            header::ACCESS_CONTROL_REQUEST_METHOD,
            HeaderValue::from_static("POST"),
        );
        r.headers_mut().insert(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            HeaderValue::from_static("content-type,x-order-token"),
        );
        let res = full_app(test_config())
            .oneshot(r)
            .await
            .expect("infallible");
        let allowed = res.headers()["access-control-allow-headers"]
            .to_str()
            .expect("ascii");
        assert!(allowed.contains("x-order-token"), "{allowed}");
    }

    #[tokio::test]
    async fn non_api_404_is_json() {
        let res = full_app(test_config())
            .oneshot(req(Method::GET, "/nope"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(json(res).await["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn oversized_body_is_413_json() {
        let mut r = req(Method::POST, "/api/v1/auth/login");
        r.headers_mut()
            .insert(header::CONTENT_LENGTH, HeaderValue::from(4096u32));
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        *r.body_mut() = Body::from(vec![b'a'; 4096]);
        let res = full_app(test_config())
            .oneshot(r)
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(res.headers()["content-type"], "application/json");
        assert_eq!(json(res).await["error"]["code"], "payload_too_large");
    }

    #[tokio::test]
    async fn cors_preflight_only_for_allowed_origins() {
        let app = full_app(test_config());
        let preflight = |origin: &'static str| {
            let mut r = req(Method::OPTIONS, "/api/v1/auth/login");
            r.headers_mut()
                .insert(header::ORIGIN, HeaderValue::from_static(origin));
            r.headers_mut().insert(
                header::ACCESS_CONTROL_REQUEST_METHOD,
                HeaderValue::from_static("POST"),
            );
            r.headers_mut().insert(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                HeaderValue::from_static("content-type"),
            );
            r
        };
        let res = app
            .clone()
            .oneshot(preflight("https://admin.example.com"))
            .await
            .expect("infallible");
        assert_eq!(
            res.headers()["access-control-allow-origin"],
            "https://admin.example.com"
        );
        assert_eq!(res.headers()["access-control-allow-credentials"], "true");
        assert_eq!(res.headers()["access-control-max-age"], "600");

        let res = app
            .oneshot(preflight("https://evil.example"))
            .await
            .expect("infallible");
        assert!(!res.headers().contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn empty_allowed_origins_allows_no_cross_origin() {
        let mut cfg = test_config();
        cfg.allowed_origins.clear();
        let mut r = req(Method::GET, "/health/live");
        r.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:5173"),
        );
        let res = full_app(cfg).oneshot(r).await.expect("infallible");
        assert!(!res.headers().contains_key("access-control-allow-origin"));
    }

    fn edge_with(routes: Router, cfg: &Config) -> Router {
        with_edge_stack(Router::new(), routes, cfg)
    }

    #[tokio::test]
    async fn overload_sheds_with_503_and_timeout_is_503() {
        let cfg = test_config(); // concurrency 1, timeout 300ms
        let app = edge_with(
            Router::new()
                .route(
                    "/slow",
                    get(|| async { tokio::time::sleep(Duration::from_secs(5)).await }),
                )
                .route("/fast", get(|| async { "ok" })),
            &cfg,
        );

        let slow = tokio::spawn(app.clone().oneshot(req(Method::GET, "/slow")));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let res = app
            .clone()
            .oneshot(req(Method::GET, "/fast"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(res.headers()["retry-after"], "1");
        assert_eq!(
            res.headers()["x-frame-options"],
            "DENY",
            "edge errors carry security headers"
        );
        assert_eq!(json(res).await["error"]["code"], "service_unavailable");

        let res = slow.await.expect("join").expect("infallible");
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE, "timed out");
        assert_eq!(json(res).await["error"]["code"], "service_unavailable");

        // Permit released after the timeout.
        let res = app
            .oneshot(req(Method::GET, "/fast"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn probes_are_never_shed() {
        let cfg = test_config(); // concurrency 1
        let app = with_edge_stack(
            Router::new().route("/health/live", get(|| async { "alive" })),
            Router::new().route(
                "/slow",
                get(|| async { tokio::time::sleep(Duration::from_secs(5)).await }),
            ),
            &cfg,
        );
        let slow = tokio::spawn(app.clone().oneshot(req(Method::GET, "/slow")));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let res = app
            .clone()
            .oneshot(req(Method::GET, "/health/live"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-content-type-options"], "nosniff");
        let res = app
            .oneshot(req(Method::GET, "/other"))
            .await
            .expect("infallible");
        assert_eq!(
            res.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "app traffic is shed"
        );
        slow.abort();
    }

    #[tokio::test]
    async fn panics_become_generic_json_500() {
        let cfg = test_config();
        let app = edge_with(
            Router::new().route(
                "/boom",
                get(|| async {
                    if true {
                        panic!("secret internal detail");
                    }
                }),
            ),
            &cfg,
        );
        let res = app
            .oneshot(req(Method::GET, "/boom"))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(res.headers().contains_key("x-request-id"));
        let body = json(res).await;
        assert_eq!(body["error"]["code"], "internal_error");
        assert!(!body.to_string().contains("secret"));
    }

    #[tokio::test]
    async fn client_ip_resolution_runs_in_the_stack() {
        let cfg = test_config(); // trusts 10.0.0.0/8
        let app = edge_with(
            Router::new().route(
                "/ip",
                get(
                    |axum::Extension(ip): axum::Extension<ResolvedClientIp>| async move {
                        ip.0.to_string()
                    },
                ),
            ),
            &cfg,
        );
        let mut r = req(Method::GET, "/ip");
        r.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 7], 1234))));
        r.headers_mut().insert(
            "x-forwarded-for",
            HeaderValue::from_static("6.6.6.6, 203.0.113.9"),
        );
        let res = app.oneshot(r).await.expect("infallible");
        let body = res.into_body().collect().await.expect("body").to_bytes();
        assert_eq!(body, "203.0.113.9");
    }

    #[test]
    fn handshake_deadline_disarms_once_protocol_is_known() {
        let new = |matched| HandshakeDeadline::<()> {
            inner: (),
            deadline: Some(Box::pin(tokio::time::sleep(Duration::from_secs(60)))),
            matched,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("rt");
        rt.block_on(async {
            // HTTP/1: diverges at the first byte.
            let mut d = new(0);
            d.observe(b"G");
            assert!(d.deadline.is_none());

            // Partial HTTP/2 preface keeps the deadline armed...
            let mut d = new(0);
            d.observe(b"PRI * HT");
            assert!(d.deadline.is_some());
            d.observe(b"TP/2.0\r\n");
            assert!(d.deadline.is_some());
            // ...until the full preface arrives.
            d.observe(b"\r\nSM\r\n\r\nEXTRA");
            assert!(d.deadline.is_none());

            // "POST" diverges at byte 2.
            let mut d = new(0);
            d.observe(b"PO");
            assert!(d.deadline.is_none());

            // EOF disarms (hyper handles it).
            let mut d = new(3);
            d.observe(b"");
            assert!(d.deadline.is_none());
        });
    }

    #[tokio::test]
    async fn serve_inserts_connect_info_and_shuts_down_gracefully() {
        let router = Router::new().route(
            "/peer",
            get(|ConnectInfo(addr): ConnectInfo<SocketAddr>| async move { addr.ip().to_string() }),
        );
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = watch::channel(false);
        let server = tokio::spawn(serve(listener, router, rx, ServeOptions::new(&[])));

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(b"GET /peer HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("write");
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.expect("read");
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(
            text.ends_with(&IpAddr::V4(Ipv4Addr::LOCALHOST).to_string()),
            "{text}"
        );

        tx.send(true).expect("send");
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("server stops promptly")
            .expect("join")
            .expect("serve ok");
    }
}
