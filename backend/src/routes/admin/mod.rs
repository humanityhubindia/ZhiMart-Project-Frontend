//! Admin API (`/api/v1/admin/*`). Every handler takes the `AuthAdmin`
//! extractor, so all routes here require a valid access token AND a live
//! session row.

pub mod dashboard;
pub mod inventory;
pub mod orders;
pub mod payments;
pub mod products;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(products::routes())
        .merge(inventory::routes())
        .merge(orders::routes())
        .merge(dashboard::routes())
        .merge(payments::routes())
}
