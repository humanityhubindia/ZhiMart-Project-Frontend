//! Admin dashboard (`GET /admin/dashboard`) and the cheap change marker the
//! admin UI polls (`GET /admin/sync`).

use axum::{Router, extract::State, response::Response, routing::get};
use chrono::NaiveDate;
use serde::Serialize;

use crate::{
    auth::AuthAdmin,
    error::AppResult,
    money::serialize_rupees,
    repo::{
        self,
        dashboard::SyncMarker,
        orders::{ListFilter, Order},
    },
    routes::admin::orders::no_store,
    state::AppState,
};

const SALES_DAYS: i32 = 14;
const TOP_PRODUCTS_DAYS: i32 = 30;
const TOP_PRODUCTS_LIMIT: i32 = 5;
const LOW_STOCK_LIMIT: i32 = 10;
const RECENT_ORDERS: i64 = 5;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(dashboard))
        .route("/sync", get(sync))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Dashboard {
    revenue: Revenue,
    orders: OrderCounts,
    inventory: Inventory,
    sales_by_day: Vec<DayPoint>,
    top_products: Vec<TopProduct>,
    recent_orders: Vec<Order>,
    /// Live orders placed under `PAYMENT_TEST_MODE` (excluded from `revenue`).
    test: TestSummary,
    /// Orders still awaiting online payment (stock reserved).
    unpaid: UnpaidSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TestSummary {
    orders: i64,
    #[serde(serialize_with = "serialize_rupees")]
    revenue: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnpaidSummary {
    orders: i64,
    #[serde(serialize_with = "serialize_rupees")]
    amount: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Revenue {
    #[serde(serialize_with = "serialize_rupees")]
    today: i64,
    #[serde(rename = "last7Days", serialize_with = "serialize_rupees")]
    last_7_days: i64,
    #[serde(rename = "last30Days", serialize_with = "serialize_rupees")]
    last_30_days: i64,
    #[serde(serialize_with = "serialize_rupees")]
    all_time: i64,
    #[serde(serialize_with = "serialize_rupees")]
    paid: i64,
    #[serde(serialize_with = "serialize_rupees")]
    pending_collection: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderCounts {
    today: i64,
    total: i64,
    by_status: ByStatus,
}

/// Keys are the wire status values (snake_case), per API.md.
#[derive(Serialize)]
struct ByStatus {
    pending_payment: i64,
    pending_pack: i64,
    dispatched: i64,
    in_transit: i64,
    out_for_delivery: i64,
    delivered: i64,
    cancelled: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    active_products: i64,
    total_units: i64,
    low_stock_count: i64,
    out_of_stock_count: i64,
    low_stock: Vec<LowStockItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LowStockItem {
    product_id: String,
    name: String,
    stock: i32,
    low_stock_threshold: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DayPoint {
    date: NaiveDate,
    orders: i64,
    #[serde(serialize_with = "serialize_rupees")]
    revenue: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TopProduct {
    product_id: String,
    name: String,
    units_sold: i64,
    #[serde(serialize_with = "serialize_rupees")]
    revenue: i64,
}

async fn recent_orders(state: &AppState) -> AppResult<Vec<Order>> {
    let mut conn = state.db.acquire().await?;
    // Unpaid checkouts are not "orders" yet from the shop's point of view.
    let filter = ListFilter {
        exclude_unpaid: true,
        ..ListFilter::default()
    };
    let (orders, _) = repo::orders::list(&mut conn, &filter, RECENT_ORDERS).await?;
    Ok(orders)
}

async fn dashboard(State(state): State<AppState>, _admin: AuthAdmin) -> AppResult<Response> {
    let db = &state.db;
    // Independent read-only queries, run concurrently on separate pool connections.
    let (stats, inv, days, top, recent) = tokio::try_join!(
        async { repo::dashboard::order_stats(db).await.map_err(Into::into) },
        async {
            repo::dashboard::inventory_stats(db, LOW_STOCK_LIMIT)
                .await
                .map_err(Into::into)
        },
        async {
            repo::dashboard::sales_by_day(db, SALES_DAYS)
                .await
                .map_err(Into::into)
        },
        async {
            repo::dashboard::top_products(db, TOP_PRODUCTS_DAYS, TOP_PRODUCTS_LIMIT)
                .await
                .map_err(Into::into)
        },
        recent_orders(&state),
    )?;

    let low_stock = inv
        .low_ids
        .into_iter()
        .zip(inv.low_names)
        .zip(inv.low_stock.into_iter().zip(inv.low_thresholds))
        .map(
            |((product_id, name), (stock, low_stock_threshold))| LowStockItem {
                product_id,
                name,
                stock,
                low_stock_threshold,
            },
        )
        .collect();

    Ok(no_store(Dashboard {
        revenue: Revenue {
            today: stats.revenue_today,
            last_7_days: stats.revenue_7d,
            last_30_days: stats.revenue_30d,
            all_time: stats.revenue_all,
            paid: stats.revenue_paid,
            pending_collection: stats.revenue_pending,
        },
        orders: OrderCounts {
            today: stats.orders_today,
            total: stats.orders_total,
            by_status: ByStatus {
                pending_payment: stats.pending_payment,
                pending_pack: stats.pending_pack,
                dispatched: stats.dispatched,
                in_transit: stats.in_transit,
                out_for_delivery: stats.out_for_delivery,
                delivered: stats.delivered,
                cancelled: stats.cancelled,
            },
        },
        inventory: Inventory {
            active_products: inv.active_products,
            total_units: inv.total_units,
            low_stock_count: inv.low_stock_count,
            out_of_stock_count: inv.out_of_stock_count,
            low_stock,
        },
        sales_by_day: days
            .into_iter()
            .map(|d| DayPoint {
                date: d.day,
                orders: d.orders,
                revenue: d.revenue,
            })
            .collect(),
        top_products: top
            .into_iter()
            .map(|t| TopProduct {
                product_id: t.product_id,
                name: t.name,
                units_sold: t.units_sold,
                revenue: t.revenue,
            })
            .collect(),
        recent_orders: recent,
        test: TestSummary {
            orders: stats.test_orders,
            revenue: stats.test_revenue,
        },
        unpaid: UnpaidSummary {
            orders: stats.unpaid_orders,
            amount: stats.unpaid_amount,
        },
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sync {
    orders_version: String,
    products_version: String,
    pending_orders: i64,
    latest_order_number: Option<String>,
}

/// Opaque version strings: `<max updated_at, epoch µs>-<row count>-<recent version sum>`.
fn versions(m: &SyncMarker) -> (String, String) {
    let micros = |t: Option<chrono::DateTime<chrono::Utc>>| t.map_or(0, |t| t.timestamp_micros());
    (
        format!(
            "{}-{}-{}",
            micros(m.orders_max_updated),
            m.orders_count,
            m.orders_recent_sum
        ),
        format!(
            "{}-{}-{}",
            micros(m.products_max_updated),
            m.products_count,
            m.products_recent_sum
        ),
    )
}

async fn sync(State(state): State<AppState>, _admin: AuthAdmin) -> AppResult<Response> {
    let m = repo::dashboard::sync_marker(&state.db).await?;
    let (orders_version, products_version) = versions(&m);
    Ok(no_store(Sync {
        orders_version,
        products_version,
        pending_orders: m.pending_orders,
        latest_order_number: m.latest_order_number,
    }))
}
