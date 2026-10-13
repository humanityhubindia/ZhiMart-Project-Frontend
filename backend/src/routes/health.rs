//! Liveness and readiness probes. Not rate limited (load balancers poll them),
//! so readiness is cached and single-flighted: however hard `/health/ready` is
//! hit, it costs at most one `SELECT 1` per [`READY_CACHE_TTL`].

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use tokio::sync::Mutex;

use crate::state::AppState;

/// Upper bound for the readiness DB ping (including pool acquire).
const READY_DB_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a readiness result is reused.
const READY_CACHE_TTL: Duration = Duration::from_secs(1);

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Make `/health/ready` report 503 so load balancers stop routing new traffic
/// here while in-flight requests drain.
pub fn begin_shutdown() {
    SHUTTING_DOWN.store(true, Ordering::Relaxed);
}

#[derive(Default)]
struct ReadyCache {
    last: Mutex<Option<(Instant, bool)>>,
}

/// GET /health/live and GET /health/ready (DB ping).
pub fn routes() -> Router<AppState> {
    let cache = Arc::new(ReadyCache::default());
    Router::new().route("/health/live", get(live)).route(
        "/health/ready",
        get(move |State(state): State<AppState>| ready(state, cache.clone())),
    )
}

async fn live() -> Response {
    probe_response(StatusCode::OK, "ok")
}

async fn ready(state: AppState, cache: Arc<ReadyCache>) -> Response {
    if SHUTTING_DOWN.load(Ordering::Relaxed) {
        return probe_response(StatusCode::SERVICE_UNAVAILABLE, "shutting_down");
    }

    let healthy = {
        let mut last = cache.last.lock().await;
        match *last {
            Some((at, healthy)) if at.elapsed() < READY_CACHE_TTL => healthy,
            _ => {
                let healthy = ping_db(&state).await;
                *last = Some((Instant::now(), healthy));
                healthy
            }
        }
    };

    if healthy {
        probe_response(StatusCode::OK, "ok")
    } else {
        probe_response(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
    }
}

async fn ping_db(state: &AppState) -> bool {
    match tokio::time::timeout(READY_DB_TIMEOUT, sqlx::query("SELECT 1").execute(&state.db)).await {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "readiness check: database query failed");
            false
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = READY_DB_TIMEOUT.as_millis() as u64,
                "readiness check: database ping timed out"
            );
            false
        }
    }
}

fn probe_response(status: StatusCode, label: &'static str) -> Response {
    let mut res = (status, Json(json!({ "status": label }))).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}
