//! Thin data-access layer over PostgreSQL. Functions take a `&PgPool` or a
//! `&mut PgConnection` (pass `&mut *tx` / `&mut tx` for a transaction) and use
//! runtime-checked queries against `migrations/`.

pub mod admins;
pub mod audit;
pub mod dashboard;
pub mod inventory;
pub mod maintenance;
pub mod orders;
pub mod otp;
pub mod outbox;
pub mod payments;
pub mod products;
pub mod sessions;
pub mod sheet_sync;

use ipnetwork::IpNetwork;

use crate::net::ClientMeta;

/// Client IP for INET columns; the unspecified address (no IP information)
/// is stored as NULL.
pub(crate) fn client_ip(meta: &ClientMeta) -> Option<IpNetwork> {
    (!meta.ip.is_unspecified()).then(|| IpNetwork::from(meta.ip))
}
