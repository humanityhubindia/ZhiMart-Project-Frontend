use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::{
    auth::jwt::JwtKeys, config::Config, email::EmailSender, payments::CashfreeClient,
    sheets::SheetsClient,
};

/// Shared application state. Cheap to clone (everything is behind `Arc`/pool handles).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub jwt: Arc<JwtKeys>,
    pub email: Arc<dyn EmailSender>,
    pub cashfree: Arc<CashfreeClient>,
    /// Google Sheets order sync client; `None` unless `GOOGLE_SHEETS_ENABLED=true`.
    /// Only the background worker uses it (never a request handler).
    pub sheets: Option<Arc<SheetsClient>>,
    /// Bounds concurrent Argon2 work so a login flood cannot pin every core.
    pub hash_permits: Arc<Semaphore>,
}

impl AppState {
    pub fn new(
        config: Arc<Config>,
        db: PgPool,
        email: Arc<dyn EmailSender>,
    ) -> anyhow::Result<Self> {
        let jwt = Arc::new(JwtKeys::new(&config)?);
        let cashfree = Arc::new(CashfreeClient::new(&config)?);
        let sheets = config
            .google_sheets
            .as_ref()
            .map(SheetsClient::new)
            .transpose()?
            .map(Arc::new);
        let permits = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .max(2);
        Ok(Self {
            db,
            config,
            jwt,
            email,
            cashfree,
            sheets,
            hash_permits: Arc::new(Semaphore::new(permits)),
        })
    }
}
