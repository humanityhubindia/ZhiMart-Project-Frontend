//! Admin dashboard aggregates and the cheap `/admin/sync` change marker.
//!
//! Business days are Asia/Kolkata calendar days. "Revenue" excludes cancelled
//! orders and refunded payments everywhere.

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgExecutor;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrderStats {
    pub revenue_today: i64,
    pub revenue_7d: i64,
    pub revenue_30d: i64,
    pub revenue_all: i64,
    pub revenue_paid: i64,
    pub revenue_pending: i64,
    pub orders_today: i64,
    pub orders_total: i64,
    pub pending_payment: i64,
    pub pending_pack: i64,
    pub dispatched: i64,
    pub in_transit: i64,
    pub out_for_delivery: i64,
    pub delivered: i64,
    pub cancelled: i64,
    pub test_orders: i64,
    pub test_revenue: i64,
    pub unpaid_orders: i64,
    pub unpaid_amount: i64,
}

/// Revenue windows and order counts in one pass over `orders`.
///
/// - "Real" revenue counts only *live* orders (not `cancelled`, not
///   `pending_payment`) that are not test orders, net of refunds
///   (`total - amount_refunded`, and 0 once the payment is `refunded`).
/// - "Last 7/30 days" are the last 7/30 IST calendar days including today.
/// - Order counts (`today`, `total`, `byStatus`) are operational and include
///   every order (test and unpaid too).
/// - `test_*`: live test orders; `unpaid_*`: orders still in `pending_payment`.
pub async fn order_stats<'e>(conn: impl PgExecutor<'e>) -> sqlx::Result<OrderStats> {
    sqlx::query_as::<_, OrderStats>(
        r#"
        WITH b AS (
            SELECT date_trunc('day', now() AT TIME ZONE 'Asia/Kolkata') AT TIME ZONE 'Asia/Kolkata' AS today
        ),
        o AS (
            SELECT created_at, status, payment_status, total_paise, is_test,
                   CASE WHEN payment_status = 'refunded' THEN 0
                        ELSE total_paise - amount_refunded_paise END AS net,
                   (status NOT IN ('cancelled', 'pending_payment')) AS live
            FROM orders
        )
        SELECT
            COALESCE(sum(net) FILTER (WHERE live AND NOT is_test AND created_at >= b.today), 0)::bigint AS revenue_today,
            COALESCE(sum(net) FILTER (WHERE live AND NOT is_test AND created_at >= b.today - interval '6 days'), 0)::bigint AS revenue_7d,
            COALESCE(sum(net) FILTER (WHERE live AND NOT is_test AND created_at >= b.today - interval '29 days'), 0)::bigint AS revenue_30d,
            COALESCE(sum(net) FILTER (WHERE live AND NOT is_test), 0)::bigint AS revenue_all,
            COALESCE(sum(net) FILTER (WHERE live AND NOT is_test AND payment_status = 'paid'), 0)::bigint AS revenue_paid,
            COALESCE(sum(net) FILTER (WHERE live AND NOT is_test AND payment_status IN ('pending', 'failed')), 0)::bigint AS revenue_pending,
            count(*) FILTER (WHERE created_at >= b.today) AS orders_today,
            count(o.created_at) AS orders_total,
            count(*) FILTER (WHERE status = 'pending_payment') AS pending_payment,
            count(*) FILTER (WHERE status = 'pending_pack') AS pending_pack,
            count(*) FILTER (WHERE status = 'dispatched') AS dispatched,
            count(*) FILTER (WHERE status = 'in_transit') AS in_transit,
            count(*) FILTER (WHERE status = 'out_for_delivery') AS out_for_delivery,
            count(*) FILTER (WHERE status = 'delivered') AS delivered,
            count(*) FILTER (WHERE status = 'cancelled') AS cancelled,
            count(*) FILTER (WHERE live AND is_test) AS test_orders,
            COALESCE(sum(net) FILTER (WHERE live AND is_test), 0)::bigint AS test_revenue,
            count(*) FILTER (WHERE status = 'pending_payment') AS unpaid_orders,
            COALESCE(sum(total_paise) FILTER (WHERE status = 'pending_payment'), 0)::bigint AS unpaid_amount
        FROM b LEFT JOIN o ON TRUE
        GROUP BY b.today
        "#,
    )
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InventoryStats {
    pub active_products: i64,
    pub total_units: i64,
    pub low_stock_count: i64,
    pub out_of_stock_count: i64,
    pub low_ids: Vec<String>,
    pub low_names: Vec<String>,
    pub low_stock: Vec<i32>,
    pub low_thresholds: Vec<i32>,
}

/// Active-catalogue summary plus up to `low_limit` products at or below their
/// threshold (out-of-stock included), lowest stock first — one query.
/// `lowStockCount` counts `0 < stock <= threshold`; `outOfStockCount` counts `stock = 0`.
pub async fn inventory_stats<'e>(
    conn: impl PgExecutor<'e>,
    low_limit: i32,
) -> sqlx::Result<InventoryStats> {
    sqlx::query_as::<_, InventoryStats>(
        r#"
        WITH active AS (
            SELECT id, name, stock, low_stock_threshold FROM products WHERE is_active
        ),
        low AS (
            SELECT id, name, stock, low_stock_threshold
            FROM active
            WHERE stock <= low_stock_threshold
            ORDER BY stock, id
            LIMIT $1
        )
        SELECT
            (SELECT count(*) FROM active) AS active_products,
            (SELECT COALESCE(sum(stock), 0)::bigint FROM active) AS total_units,
            (SELECT count(*) FROM active WHERE stock > 0 AND stock <= low_stock_threshold) AS low_stock_count,
            (SELECT count(*) FROM active WHERE stock = 0) AS out_of_stock_count,
            COALESCE((SELECT array_agg(id ORDER BY stock, id) FROM low), '{}') AS low_ids,
            COALESCE((SELECT array_agg(name ORDER BY stock, id) FROM low), '{}') AS low_names,
            COALESCE((SELECT array_agg(stock ORDER BY stock, id) FROM low), '{}') AS low_stock,
            COALESCE((SELECT array_agg(low_stock_threshold ORDER BY stock, id) FROM low), '{}') AS low_thresholds
        "#,
    )
    .bind(i64::from(low_limit))
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DaySales {
    pub day: NaiveDate,
    pub orders: i64,
    pub revenue: i64,
}

/// The last `days` IST calendar days (oldest first, including today), zero-filled.
/// Real orders only: not test, not cancelled, not awaiting payment. `revenue`
/// is net of refunds.
pub async fn sales_by_day<'e>(conn: impl PgExecutor<'e>, days: i32) -> sqlx::Result<Vec<DaySales>> {
    sqlx::query_as::<_, DaySales>(
        r#"
        WITH b AS (
            SELECT (now() AT TIME ZONE 'Asia/Kolkata')::date AS today
        ),
        agg AS (
            SELECT (o.created_at AT TIME ZONE 'Asia/Kolkata')::date AS day,
                   count(*) AS orders,
                   COALESCE(sum(CASE WHEN o.payment_status = 'refunded' THEN 0
                                     ELSE o.total_paise - o.amount_refunded_paise END), 0)::bigint AS revenue
            FROM orders o, b
            WHERE o.created_at >= ((b.today - ($1 - 1))::timestamp AT TIME ZONE 'Asia/Kolkata')
              AND o.status NOT IN ('cancelled', 'pending_payment')
              AND NOT o.is_test
            GROUP BY 1
        )
        SELECT d.day, COALESCE(agg.orders, 0) AS orders, COALESCE(agg.revenue, 0)::bigint AS revenue
        FROM b,
             LATERAL (SELECT (b.today - g)::date AS day FROM generate_series(0, $1 - 1) AS g) AS d
             LEFT JOIN agg ON agg.day = d.day
        ORDER BY d.day
        "#,
    )
    .bind(days)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TopProduct {
    pub product_id: String,
    pub name: String,
    pub units_sold: i64,
    pub revenue: i64,
}

/// Best sellers by units over the last `days` IST days: real orders only (not
/// test, cancelled, unpaid or fully refunded). Revenue is the line totals
/// (partial refunds are order-level and not attributed to lines). The current
/// catalogue name is shown.
pub async fn top_products<'e>(
    conn: impl PgExecutor<'e>,
    days: i32,
    limit: i32,
) -> sqlx::Result<Vec<TopProduct>> {
    sqlx::query_as::<_, TopProduct>(
        r#"
        WITH b AS (
            SELECT ((now() AT TIME ZONE 'Asia/Kolkata')::date - ($1 - 1))::timestamp AT TIME ZONE 'Asia/Kolkata' AS since
        ),
        agg AS (
            SELECT oi.product_id,
                   sum(oi.quantity)::bigint AS units_sold,
                   sum(oi.line_total_paise)::bigint AS revenue
            FROM order_items oi
            JOIN orders o ON o.id = oi.order_id
            CROSS JOIN b
            WHERE o.created_at >= b.since
              AND o.status NOT IN ('cancelled', 'pending_payment')
              AND o.payment_status <> 'refunded'
              AND NOT o.is_test
            GROUP BY oi.product_id
        )
        SELECT agg.product_id, p.name, agg.units_sold, agg.revenue
        FROM agg
        JOIN products p ON p.id = agg.product_id
        ORDER BY agg.units_sold DESC, agg.revenue DESC, agg.product_id
        LIMIT $2
        "#,
    )
    .bind(days)
    .bind(i64::from(limit))
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SyncMarker {
    pub orders_max_updated: Option<DateTime<Utc>>,
    pub orders_count: i64,
    pub orders_recent_sum: i64,
    pub products_max_updated: Option<DateTime<Utc>>,
    pub products_count: i64,
    pub products_recent_sum: i64,
    pub pending_orders: i64,
    pub latest_order_number: Option<String>,
}

/// One round trip, index-backed: `max(updated_at)` via `*_updated_idx`, plus a
/// row count and the sum of `version` over rows touched in the last few
/// minutes. The recent sum catches a write whose `updated_at` (transaction
/// start time) is older than the current max because it committed late.
pub async fn sync_marker<'e>(conn: impl PgExecutor<'e>) -> sqlx::Result<SyncMarker> {
    sqlx::query_as::<_, SyncMarker>(
        r#"
        SELECT
            (SELECT max(updated_at) FROM orders) AS orders_max_updated,
            (SELECT count(*) FROM orders) AS orders_count,
            (SELECT COALESCE(sum(version), 0)::bigint + count(*) FROM orders
              WHERE updated_at > now() - interval '5 minutes') AS orders_recent_sum,
            (SELECT max(updated_at) FROM products) AS products_max_updated,
            (SELECT count(*) FROM products) AS products_count,
            (SELECT COALESCE(sum(version), 0)::bigint + count(*) FROM products
              WHERE updated_at > now() - interval '5 minutes') AS products_recent_sum,
            (SELECT count(*) FROM orders WHERE status = 'pending_pack') AS pending_orders,
            (SELECT order_number FROM orders ORDER BY created_at DESC, id DESC LIMIT 1) AS latest_order_number
        "#,
    )
    .fetch_one(conn)
    .await
}
