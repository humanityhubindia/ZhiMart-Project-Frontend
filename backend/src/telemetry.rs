//! Structured logging. JSON lines in production (for log shippers), human
//! readable output in development. Verbosity comes from `RUST_LOG`.

use std::io::IsTerminal;

use anyhow::Context;
use tracing_subscriber::{EnvFilter, fmt};

/// Filter used when `RUST_LOG` is unset or empty.
pub const DEFAULT_FILTER: &str = "info,zhimart_backend=info,tower_http=info,sqlx=warn,aws=warn";

/// Output format for log lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// One JSON object per line (production).
    Json,
    /// Colourised, multi-field human output (development).
    Pretty,
}

impl LogFormat {
    /// Pick the format the same way `Config` resolves `APP_ENV` (default:
    /// production → JSON), without requiring the rest of the config to be
    /// valid. This lets startup errors be logged in the right format.
    pub fn from_env() -> Self {
        match std::env::var("APP_ENV").map(|v| v.trim().to_ascii_lowercase()) {
            Ok(v) if matches!(v.as_str(), "development" | "dev" | "local") => LogFormat::Pretty,
            _ => LogFormat::Json,
        }
    }
}

/// Install the global tracing subscriber. Call once, as early as possible.
pub fn init(format: LogFormat) -> anyhow::Result<()> {
    let filter = match std::env::var("RUST_LOG") {
        Ok(v) if !v.trim().is_empty() => {
            EnvFilter::try_new(v.trim()).context("invalid RUST_LOG filter")?
        }
        _ => EnvFilter::new(DEFAULT_FILTER),
    };

    let result = match format {
        LogFormat::Json => fmt()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_target(true)
            .with_env_filter(filter)
            .with_writer(std::io::stdout)
            .try_init(),
        LogFormat::Pretty => fmt()
            .with_target(true)
            .with_ansi(std::io::stdout().is_terminal())
            .with_env_filter(filter)
            .with_writer(std::io::stdout)
            .try_init(),
    };

    result.map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))
}
