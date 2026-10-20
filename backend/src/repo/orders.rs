//! `orders` + `order_items` (+ the `stock_movements` they produce): checkout,
//! admin listing/lookup, status/shipping/payment changes and cancellation.
//!
//! Stock changes always happen inside the caller's transaction with the
//! affected `products` rows locked in `id` order (deterministic lock order, so
//! concurrent checkouts/cancellations can never deadlock).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use ipnetwork::IpNetwork;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::money::serialize_rupees;

/// Column list shared by every query that returns an [`OrderRow`]. A macro
/// (not a `const`) so it can be spliced into SQL literals with `concat!`.
macro_rules! order_cols {
    () => {
        "id, order_number, idempotency_key, customer_name, customer_phone, customer_email, \
         address_line, city, state, pincode, notes, item_count, mrp_total_paise, total_paise, \
         status, payment_method, payment_status, payment_reference, paid_at, refunded_at, \
         transport_partner, docket_number, dispatched_at, delivered_at, cancelled_at, \
         cancel_reason, version, created_at, updated_at, is_test, payment_expires_at, \
         gateway_order_id, gateway_payment_id, payment_instrument, amount_refunded_paise, \
         payment_attention, pricing_tier, delivery_fee_paise"
    };
}

// ---------------------------------------------------------------------------
// Status vocabularies
// ---------------------------------------------------------------------------

/// Order status. `PendingPayment` (stock reserved, awaiting online payment) is
/// left only by the payment service (→ `PendingPack`) or by cancellation/expiry.
/// From `PendingPack` the pipeline is forward-only; `Cancelled` is a side exit
/// from `PendingPayment`/`PendingPack` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    PendingPayment,
    PendingPack,
    Dispatched,
    InTransit,
    OutForDelivery,
    Delivered,
    Cancelled,
}

impl OrderStatus {
    pub const ALL: [OrderStatus; 7] = [
        OrderStatus::PendingPayment,
        OrderStatus::PendingPack,
        OrderStatus::Dispatched,
        OrderStatus::InTransit,
        OrderStatus::OutForDelivery,
        OrderStatus::Delivered,
        OrderStatus::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            OrderStatus::PendingPayment => "pending_payment",
            OrderStatus::PendingPack => "pending_pack",
            OrderStatus::Dispatched => "dispatched",
            OrderStatus::InTransit => "in_transit",
            OrderStatus::OutForDelivery => "out_for_delivery",
            OrderStatus::Delivered => "delivered",
            OrderStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Position in the admin-driven fulfilment pipeline; `None` for
    /// `PendingPayment` (only the payment service moves it on) and `Cancelled`.
    pub fn rank(self) -> Option<u8> {
        match self {
            OrderStatus::PendingPayment => None,
            OrderStatus::PendingPack => Some(0),
            OrderStatus::Dispatched => Some(1),
            OrderStatus::InTransit => Some(2),
            OrderStatus::OutForDelivery => Some(3),
            OrderStatus::Delivered => Some(4),
            OrderStatus::Cancelled => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, OrderStatus::Delivered | OrderStatus::Cancelled)
    }

    /// `dispatched` or later: the parcel has left, so a transport partner and
    /// docket number must be on record.
    pub fn requires_shipment(self) -> bool {
        self.rank().is_some_and(|r| r >= 1)
    }

    /// Statuses an admin may cancel from (stock is released).
    pub fn is_cancellable(self) -> bool {
        matches!(self, OrderStatus::PendingPayment | OrderStatus::PendingPack)
    }
}

/// Payment method of every online-payment order placed through the storefront
/// checkout (Cashfree).
pub const METHOD_CASHFREE: &str = "cashfree";
/// Legacy online method: orders paid through Razorpay before the switch to
/// Cashfree (refund those from the Razorpay Dashboard).
pub const METHOD_RAZORPAY_LEGACY: &str = "razorpay";

/// `true` for online-gateway methods, whose payment state is owned by the
/// payment service and never edited manually.
pub fn is_online_method(method: &str) -> bool {
    method == METHOD_CASHFREE || method == METHOD_RAZORPAY_LEGACY
}
/// `cancel_reason` written by the unpaid-order expiry sweep.
pub const EXPIRY_CANCEL_REASON: &str = "Payment not completed";

/// Price tier an order was charged at (`orders.pricing_tier`, see
/// [`crate::config::Pricing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingTier {
    /// Price-list prices (order value at list prices ≥ the bulk minimum).
    Bulk,
    /// MRP less the standard discount on every line (smaller orders).
    Standard,
}

impl PricingTier {
    pub fn as_str(self) -> &'static str {
        match self {
            PricingTier::Bulk => "bulk",
            PricingTier::Standard => "standard",
        }
    }
}

pub const PAYMENT_STATUSES: [&str; 4] = ["pending", "paid", "refunded", "failed"];
/// Methods an admin may record manually (`mark_paid`) on offline (non-gateway) orders.
pub const PAYMENT_METHODS: [&str; 6] = [
    "pay_later",
    "upi",
    "card",
    "netbanking",
    "cod",
    "bank_transfer",
];

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrderRow {
    pub id: Uuid,
    pub order_number: String,
    pub idempotency_key: Uuid,
    pub customer_name: String,
    pub customer_phone: String,
    pub customer_email: Option<String>,
    pub address_line: String,
    pub city: String,
    pub state: String,
    pub pincode: String,
    pub notes: Option<String>,
    pub item_count: i32,
    pub mrp_total_paise: i64,
    /// Payable amount: goods (sum of line totals) + `delivery_fee_paise`.
    pub total_paise: i64,
    pub status: String,
    pub payment_method: String,
    pub payment_status: String,
    pub payment_reference: Option<String>,
    pub paid_at: Option<DateTime<Utc>>,
    pub refunded_at: Option<DateTime<Utc>>,
    pub transport_partner: Option<String>,
    pub docket_number: Option<String>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_test: bool,
    pub payment_expires_at: Option<DateTime<Utc>>,
    /// Our Cashfree order id (or a legacy Razorpay `order_…` id).
    pub gateway_order_id: Option<String>,
    /// Cashfree `cf_payment_id` of the settled payment (or a legacy `pay_…` id).
    pub gateway_payment_id: Option<String>,
    pub payment_instrument: Option<String>,
    pub amount_refunded_paise: i64,
    pub payment_attention: Option<String>,
    /// `bulk` | `standard`.
    pub pricing_tier: String,
    /// Delivery charge included in `total_paise` (0 = free delivery).
    pub delivery_fee_paise: i64,
}

impl OrderRow {
    /// Goods total after tier pricing (= sum of the line totals): the payable
    /// total without the delivery charge.
    pub fn subtotal_paise(&self) -> i64 {
        self.total_paise - self.delivery_fee_paise
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrderItemRow {
    pub order_id: Uuid,
    pub product_id: String,
    pub product_name: String,
    pub pack_info: String,
    pub unit_price_paise: i64,
    pub unit_mrp_paise: i64,
    pub quantity: i32,
    pub line_total_paise: i64,
}

// ---------------------------------------------------------------------------
// JSON shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderItem {
    pub product_id: String,
    pub product_name: String,
    pub pack_info: String,
    pub quantity: i32,
    #[serde(serialize_with = "serialize_rupees")]
    pub unit_price: i64,
    #[serde(serialize_with = "serialize_rupees")]
    pub unit_mrp: i64,
    #[serde(serialize_with = "serialize_rupees")]
    pub line_total: i64,
}

impl From<OrderItemRow> for OrderItem {
    fn from(r: OrderItemRow) -> Self {
        OrderItem {
            product_id: r.product_id,
            product_name: r.product_name,
            pack_info: r.pack_info,
            quantity: r.quantity,
            unit_price: r.unit_price_paise,
            unit_mrp: r.unit_mrp_paise,
            line_total: r.line_total_paise,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Customer {
    pub name: String,
    pub phone: String,
    pub email: Option<String>,
    pub address: String,
    pub city: String,
    pub state: String,
    pub pincode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub method: String,
    pub status: String,
    pub reference: Option<String>,
    pub paid_at: Option<DateTime<Utc>>,
    pub refunded_at: Option<DateTime<Utc>>,
    /// Instrument the gateway reported (upi, card, netbanking, wallet, …).
    pub instrument: Option<String>,
    pub gateway_order_id: Option<String>,
    pub gateway_payment_id: Option<String>,
    #[serde(serialize_with = "serialize_rupees")]
    pub amount_refunded: i64,
    /// Non-null when the payment needs human attention.
    pub attention: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shipping {
    pub transport_partner: Option<String>,
    pub docket_number: Option<String>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
}

/// The admin `Order` shape (API.md "Shared shapes").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: Uuid,
    pub order_number: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    pub customer: Customer,
    pub notes: Option<String>,
    pub items: Vec<OrderItem>,
    pub item_count: i32,
    #[serde(serialize_with = "serialize_rupees")]
    pub mrp_total: i64,
    /// Goods after tier pricing.
    #[serde(serialize_with = "serialize_rupees")]
    pub subtotal: i64,
    /// 0 = free delivery.
    #[serde(serialize_with = "serialize_rupees")]
    pub delivery_fee: i64,
    /// Payable: `subtotal + delivery_fee`.
    #[serde(serialize_with = "serialize_rupees")]
    pub total_amount: i64,
    pub status: String,
    pub payment: Payment,
    pub shipping: Shipping,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
    pub is_test: bool,
    pub payment_expires_at: Option<DateTime<Utc>>,
    /// `bulk` | `standard`.
    pub pricing_tier: String,
}

impl Order {
    pub fn from_parts(r: OrderRow, items: Vec<OrderItem>) -> Self {
        let subtotal = r.subtotal_paise();
        Order {
            id: r.id,
            order_number: r.order_number,
            created_at: r.created_at,
            updated_at: r.updated_at,
            version: r.version,
            customer: Customer {
                name: r.customer_name,
                phone: r.customer_phone,
                email: r.customer_email,
                address: r.address_line,
                city: r.city,
                state: r.state,
                pincode: r.pincode,
            },
            notes: r.notes,
            items,
            item_count: r.item_count,
            mrp_total: r.mrp_total_paise,
            subtotal,
            delivery_fee: r.delivery_fee_paise,
            total_amount: r.total_paise,
            status: r.status,
            payment: Payment {
                method: r.payment_method,
                status: r.payment_status,
                reference: r.payment_reference,
                paid_at: r.paid_at,
                refunded_at: r.refunded_at,
                instrument: r.payment_instrument,
                gateway_order_id: r.gateway_order_id,
                gateway_payment_id: r.gateway_payment_id,
                amount_refunded: r.amount_refunded_paise,
                attention: r.payment_attention,
            },
            shipping: Shipping {
                transport_partner: r.transport_partner,
                docket_number: r.docket_number,
                dispatched_at: r.dispatched_at,
                delivered_at: r.delivered_at,
            },
            cancelled_at: r.cancelled_at,
            cancel_reason: r.cancel_reason,
            is_test: r.is_test,
            payment_expires_at: r.payment_expires_at,
            pricing_tier: r.pricing_tier,
        }
    }
}

/// The customer-facing receipt returned by `POST /orders` (no PII echoed back).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicOrder {
    pub order_number: String,
    pub status: String,
    pub payment_expires_at: Option<DateTime<Utc>>,
    pub is_test: bool,
    /// `bulk` | `standard`.
    pub pricing_tier: String,
    /// Goods after tier pricing.
    #[serde(serialize_with = "serialize_rupees")]
    pub subtotal: i64,
    /// 0 = free delivery.
    #[serde(serialize_with = "serialize_rupees")]
    pub delivery_fee: i64,
    /// Payable: `subtotal + delivery_fee`.
    #[serde(serialize_with = "serialize_rupees")]
    pub total_amount: i64,
    #[serde(serialize_with = "serialize_rupees")]
    pub mrp_total: i64,
    pub item_count: i32,
    pub created_at: DateTime<Utc>,
    pub items: Vec<OrderItem>,
}

impl PublicOrder {
    pub fn from_parts(r: OrderRow, items: Vec<OrderItemRow>) -> Self {
        let subtotal = r.subtotal_paise();
        PublicOrder {
            order_number: r.order_number,
            status: r.status,
            payment_expires_at: r.payment_expires_at,
            is_test: r.is_test,
            pricing_tier: r.pricing_tier,
            subtotal,
            delivery_fee: r.delivery_fee_paise,
            total_amount: r.total_paise,
            mrp_total: r.mrp_total_paise,
            item_count: r.item_count,
            created_at: r.created_at,
            items: items.into_iter().map(OrderItem::from).collect(),
        }
    }
}

/// Join order rows with their items, preserving the order of `rows` and the
/// insertion order of items within each order.
pub fn assemble(rows: Vec<OrderRow>, items: Vec<OrderItemRow>) -> Vec<Order> {
    let mut by_order: HashMap<Uuid, Vec<OrderItem>> = HashMap::with_capacity(rows.len());
    for item in items {
        by_order.entry(item.order_id).or_default().push(item.into());
    }
    rows.into_iter()
        .map(|r| {
            let items = by_order.remove(&r.id).unwrap_or_default();
            Order::from_parts(r, items)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// How an admin addresses an order in a URL.
#[derive(Debug, Clone)]
pub enum OrderRef {
    Id(Uuid),
    Number(String),
}

pub async fn find<'e>(conn: impl PgExecutor<'e>, r: &OrderRef) -> sqlx::Result<Option<OrderRow>> {
    match r {
        OrderRef::Id(id) => {
            sqlx::query_as::<_, OrderRow>(concat!(
                "SELECT ",
                order_cols!(),
                " FROM orders WHERE id = $1"
            ))
            .bind(id)
            .fetch_optional(conn)
            .await
        }
        OrderRef::Number(n) => {
            sqlx::query_as::<_, OrderRow>(concat!(
                "SELECT ",
                order_cols!(),
                " FROM orders WHERE order_number = $1"
            ))
            .bind(n)
            .fetch_optional(conn)
            .await
        }
    }
}

pub async fn find_by_idempotency_key<'e>(
    conn: impl PgExecutor<'e>,
    key: Uuid,
) -> sqlx::Result<Option<OrderRow>> {
    sqlx::query_as::<_, OrderRow>(concat!(
        "SELECT ",
        order_cols!(),
        " FROM orders WHERE idempotency_key = $1"
    ))
    .bind(key)
    .fetch_optional(conn)
    .await
}

/// Items for a set of orders in one query, grouped by order then line order.
pub async fn items_for<'e>(
    conn: impl PgExecutor<'e>,
    order_ids: &[Uuid],
) -> sqlx::Result<Vec<OrderItemRow>> {
    if order_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as::<_, OrderItemRow>(
        r#"
        SELECT order_id, product_id, product_name, pack_info, unit_price_paise, unit_mrp_paise,
               quantity, line_total_paise
        FROM order_items
        WHERE order_id = ANY($1)
        ORDER BY order_id, id
        "#,
    )
    .bind(order_ids)
    .fetch_all(conn)
    .await
}

/// Load one order with its items (2 queries).
pub async fn load_full(conn: &mut PgConnection, r: &OrderRef) -> sqlx::Result<Option<Order>> {
    let Some(row) = find(&mut *conn, r).await? else {
        return Ok(None);
    };
    with_items(&mut *conn, row).await.map(Some)
}

/// Load several orders with their items (2 queries). Missing ids are skipped;
/// the result is in no particular order.
pub async fn load_many(conn: &mut PgConnection, ids: &[Uuid]) -> sqlx::Result<Vec<Order>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, OrderRow>(concat!(
        "SELECT ",
        order_cols!(),
        " FROM orders WHERE id = ANY($1)"
    ))
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let found: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = items_for(&mut *conn, &found).await?;
    Ok(assemble(rows, items))
}

/// Attach items to one already-loaded row.
pub async fn with_items<'e>(conn: impl PgExecutor<'e>, row: OrderRow) -> sqlx::Result<Order> {
    let items = items_for(conn, &[row.id]).await?;
    Ok(Order::from_parts(
        row,
        items.into_iter().map(OrderItem::from).collect(),
    ))
}

/// Keyset position: `(created_at, id)` of the last row on the previous page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub status: Option<OrderStatus>,
    pub payment_status: Option<&'static str>,
    /// Already-escaped `ILIKE` pattern (with surrounding `%`).
    pub text_pattern: Option<String>,
    /// Already-escaped `ILIKE` pattern for the phone column (digits only).
    pub phone_pattern: Option<String>,
    pub cursor: Option<Cursor>,
    /// Hide `pending_payment` orders (unpaid checkouts holding a reservation).
    pub exclude_unpaid: bool,
}

/// Newest-first page of orders + their items (exactly 2 queries). Returns up to
/// `limit` orders and whether more exist.
pub async fn list(
    conn: &mut PgConnection,
    f: &ListFilter,
    limit: i64,
) -> sqlx::Result<(Vec<Order>, bool)> {
    let mut qb =
        QueryBuilder::<Postgres>::new(concat!("SELECT ", order_cols!(), " FROM orders WHERE TRUE"));
    if let Some(s) = f.status {
        qb.push(" AND status = ").push_bind(s.as_str());
    }
    if let Some(p) = f.payment_status {
        qb.push(" AND payment_status = ").push_bind(p);
    }
    if f.exclude_unpaid {
        qb.push(" AND status <> 'pending_payment'");
    }
    if let Some(pat) = &f.text_pattern {
        qb.push(" AND (order_number ILIKE ")
            .push_bind(pat.clone())
            .push(" ESCAPE '\\' OR customer_name ILIKE ")
            .push_bind(pat.clone())
            .push(" ESCAPE '\\' OR city ILIKE ")
            .push_bind(pat.clone())
            .push(" ESCAPE '\\' OR customer_phone ILIKE ")
            .push_bind(f.phone_pattern.clone().unwrap_or_else(|| pat.clone()))
            .push(" ESCAPE '\\')");
    }
    if let Some(c) = f.cursor {
        qb.push(" AND (created_at, id) < (")
            .push_bind(c.created_at)
            .push(", ")
            .push_bind(c.id)
            .push(")");
    }
    qb.push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit + 1);

    let mut rows: Vec<OrderRow> = qb.build_query_as().fetch_all(&mut *conn).await?;
    let has_more = rows.len() as i64 > limit;
    rows.truncate(usize::try_from(limit).unwrap_or(0));
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = items_for(&mut *conn, &ids).await?;
    Ok((assemble(rows, items), has_more))
}

pub async fn exists<'e>(conn: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM orders WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

// ---------------------------------------------------------------------------
// Checkout
// ---------------------------------------------------------------------------

/// Serialise checkouts from the same client IP / phone for the rest of the
/// transaction so the abuse caps below are race-free. Keys are acquired in
/// sorted order (and always before any product row lock) so this cannot
/// deadlock.
pub async fn lock_checkout_keys(
    conn: &mut PgConnection,
    ip: Option<IpNetwork>,
    phone: &str,
) -> sqlx::Result<()> {
    let mut keys = vec![advisory_key("zm:checkout:phone:", phone)];
    if let Some(ip) = ip {
        keys.push(advisory_key("zm:checkout:ip:", &ip.ip().to_string()));
    }
    keys.sort_unstable();
    keys.dedup();
    sqlx::query("SELECT pg_advisory_xact_lock(k) FROM unnest($1::bigint[]) WITH ORDINALITY AS t(k, n) ORDER BY n")
        .bind(&keys)
        .execute(conn)
        .await?;
    Ok(())
}

fn advisory_key(namespace: &str, value: &str) -> i64 {
    let digest = Sha256::new()
        .chain_update(namespace)
        .chain_update(value)
        .finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(b)
}

/// Seconds until the client may order again, per cap; `None` when under the cap.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct CheckoutThrottle {
    pub ip_retry_secs: Option<i64>,
    pub phone_retry_secs: Option<i64>,
}

/// Checks the per-IP hourly cap and the per-phone daily cap on open orders
/// (`pending_payment` reservations and `pending_pack`, so unpaid checkouts
/// cannot be used to hoard stock). The `cap`-th most recent order inside the window is
/// the one that has to age out before another is allowed, so its timestamp
/// gives an exact `Retry-After`.
pub async fn checkout_throttle(
    conn: &mut PgConnection,
    ip: Option<IpNetwork>,
    ip_cap_per_hour: i64,
    phone: &str,
    phone_cap_per_day: i64,
) -> sqlx::Result<CheckoutThrottle> {
    sqlx::query_as::<_, CheckoutThrottle>(
        r#"
        SELECT
            (SELECT ceil(extract(epoch FROM created_at + interval '1 hour' - now()))::bigint
             FROM orders
             WHERE client_ip = $1 AND created_at > now() - interval '1 hour'
             ORDER BY created_at DESC
             OFFSET $2 LIMIT 1) AS ip_retry_secs,
            (SELECT ceil(extract(epoch FROM created_at + interval '1 day' - now()))::bigint
             FROM orders
             WHERE customer_phone = $3 AND status IN ('pending_payment', 'pending_pack')
               AND created_at > now() - interval '1 day'
             ORDER BY created_at DESC
             OFFSET $4 LIMIT 1) AS phone_retry_secs
        "#,
    )
    .bind(ip)
    .bind((ip_cap_per_hour - 1).max(0))
    .bind(phone)
    .bind((phone_cap_per_day - 1).max(0))
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LockedProduct {
    pub id: String,
    pub name: String,
    pub pack_info: String,
    pub price_paise: i64,
    pub original_price_paise: i64,
    pub stock: i32,
    pub is_active: bool,
}

/// `SELECT … FOR UPDATE` the given products in `id` order.
pub async fn lock_products(
    conn: &mut PgConnection,
    ids: &[String],
) -> sqlx::Result<Vec<LockedProduct>> {
    sqlx::query_as::<_, LockedProduct>(
        r#"
        SELECT id, name, pack_info, price_paise, original_price_paise, stock, is_active
        FROM products
        WHERE id = ANY($1)
        ORDER BY id
        FOR UPDATE
        "#,
    )
    .bind(ids)
    .fetch_all(conn)
    .await
}

/// Everything needed to write a new order. Prices/names come from
/// [`LockedProduct`] rows, never from the client.
#[derive(Debug, Clone)]
pub struct NewOrder<'a> {
    pub id: Uuid,
    pub idempotency_key: Uuid,
    pub customer_name: &'a str,
    pub customer_phone: &'a str,
    pub customer_email: Option<&'a str>,
    pub address_line: &'a str,
    pub city: &'a str,
    pub state: &'a str,
    pub pincode: &'a str,
    pub notes: Option<&'a str>,
    pub client_ip: Option<IpNetwork>,
    pub user_agent: Option<&'a str>,
    pub item_count: i32,
    pub mrp_total_paise: i64,
    /// Payable: the line totals + `delivery_fee_paise` (cross-checked against SQL).
    pub total_paise: i64,
    /// Delivery charge decided by the caller (0 = free delivery).
    pub delivery_fee_paise: i64,
    /// Placed under `PAYMENT_TEST_MODE` (lines priced at ₹1/unit by the caller).
    pub is_test: bool,
    /// Tier the lines were priced at by the caller.
    pub pricing_tier: PricingTier,
    /// `true` (Cashfree enabled): `pending_payment` / `cashfree`, reserved until
    /// `payment_expires_at = now() + payment_expiry` (DB clock). `false`:
    /// `pending_pack` / `pay_later`, no expiry (payment collected offline).
    pub online_payment: bool,
    /// Reservation lifetime for online payment; ignored when `online_payment` is false.
    pub payment_expiry: std::time::Duration,
    pub lines: Vec<NewOrderLine>,
}

#[derive(Debug, Clone)]
pub struct NewOrderLine {
    pub product_id: String,
    pub product_name: String,
    pub pack_info: String,
    pub unit_price_paise: i64,
    pub unit_mrp_paise: i64,
    pub quantity: i32,
}

impl NewOrderLine {
    pub fn to_item_row(&self, order_id: Uuid) -> OrderItemRow {
        OrderItemRow {
            order_id,
            product_id: self.product_id.clone(),
            product_name: self.product_name.clone(),
            pack_info: self.pack_info.clone(),
            unit_price_paise: self.unit_price_paise,
            unit_mrp_paise: self.unit_mrp_paise,
            quantity: self.quantity,
            line_total_paise: self.unit_price_paise * i64::from(self.quantity),
        }
    }
}

#[derive(sqlx::FromRow)]
struct InsertedOrder {
    #[sqlx(flatten)]
    row: OrderRow,
    decremented: i64,
}

/// Name of the unique index that makes checkout idempotent.
pub const IDEMPOTENCY_CONSTRAINT: &str = "orders_idempotency_key";

/// Decrement stock for every line, insert the order (`pending_payment`,
/// Cashfree, expiring after `payment_expiry`), its items and one
/// `order_placed` movement per line — all in ONE statement. The products must
/// already be locked (see [`lock_products`]) and stock checked by the caller;
/// the `stock >= 0` CHECK is the last line of defence.
///
/// Returns `Ok(None)` if the stock update did not touch every line (a product
/// vanished under us), in which case the caller must roll back.
pub async fn insert_order(
    conn: &mut PgConnection,
    o: &NewOrder<'_>,
) -> sqlx::Result<Option<OrderRow>> {
    let n = o.lines.len();
    let mut ids = Vec::with_capacity(n);
    let mut qtys = Vec::with_capacity(n);
    let mut names = Vec::with_capacity(n);
    let mut packs = Vec::with_capacity(n);
    let mut prices = Vec::with_capacity(n);
    let mut mrps = Vec::with_capacity(n);
    for l in &o.lines {
        ids.push(l.product_id.as_str());
        qtys.push(l.quantity);
        names.push(l.product_name.as_str());
        packs.push(l.pack_info.as_str());
        prices.push(l.unit_price_paise);
        mrps.push(l.unit_mrp_paise);
    }

    let inserted = sqlx::query_as::<_, InsertedOrder>(concat!(
        r#"
        WITH lines AS (
            SELECT *
            FROM unnest($15::text[], $16::int[], $17::text[], $18::text[], $19::bigint[], $20::bigint[])
                 WITH ORDINALITY AS l(product_id, quantity, product_name, pack_info, unit_price, unit_mrp, ord)
        ),
        dec AS (
            UPDATE products p
            SET stock = p.stock - l.quantity, version = p.version + 1, updated_at = now()
            FROM lines l
            WHERE p.id = l.product_id
            RETURNING p.id, p.stock AS stock_after, l.quantity, l.ord
        ),
        new_order AS (
            INSERT INTO orders (
                id, order_number, idempotency_key, customer_name, customer_phone, customer_email,
                address_line, city, state, pincode, notes, client_ip, user_agent,
                item_count, mrp_total_paise, total_paise,
                status, payment_method, payment_status, is_test, payment_expires_at,
                pricing_tier, delivery_fee_paise
            )
            VALUES (
                $1,
                'ZM-' || to_char(now() AT TIME ZONE 'Asia/Kolkata', 'YYMMDD') || '-' || nextval('order_number_seq'),
                $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                (SELECT sum(unit_price * quantity) FROM lines)::bigint + $25,
                CASE WHEN $23 THEN 'pending_payment' ELSE 'pending_pack' END,
                CASE WHEN $23 THEN 'cashfree' ELSE 'pay_later' END,
                'pending', $21,
                CASE WHEN $23 THEN now() + make_interval(secs => $22) END,
                $24, $25
            )
            RETURNING "#,
        order_cols!(),
        r#"
        ),
        items AS (
            INSERT INTO order_items (
                order_id, product_id, product_name, pack_info, unit_price_paise, unit_mrp_paise,
                quantity, line_total_paise
            )
            SELECT $1, product_id, product_name, pack_info, unit_price, unit_mrp, quantity, unit_price * quantity
            FROM lines
            ORDER BY ord
        ),
        moves AS (
            INSERT INTO stock_movements (product_id, delta, stock_after, reason, order_id)
            SELECT id, -quantity, stock_after, 'order_placed', $1
            FROM dec
            ORDER BY ord
        )
        SELECT new_order.*, (SELECT count(*) FROM dec) AS decremented
        FROM new_order
        "#
    ))
    .bind(o.id)
    .bind(o.idempotency_key)
    .bind(o.customer_name)
    .bind(o.customer_phone)
    .bind(o.customer_email)
    .bind(o.address_line)
    .bind(o.city)
    .bind(o.state)
    .bind(o.pincode)
    .bind(o.notes)
    .bind(o.client_ip)
    .bind(o.user_agent)
    .bind(o.item_count)
    .bind(o.mrp_total_paise)
    .bind(&ids)
    .bind(&qtys)
    .bind(&names)
    .bind(&packs)
    .bind(&prices)
    .bind(&mrps)
    .bind(o.is_test)
    .bind(o.payment_expiry.as_secs_f64())
    .bind(o.online_payment)
    .bind(o.pricing_tier.as_str())
    .bind(o.delivery_fee_paise)
    .fetch_one(conn)
    .await?;

    if inserted.decremented != n as i64 {
        return Ok(None);
    }
    // Cross-check the server-side total against the one computed from the same rows.
    if inserted.row.total_paise != o.total_paise {
        tracing::error!(order_id = %o.id, "order total mismatch between Rust and SQL");
        return Ok(None);
    }
    Ok(Some(inserted.row))
}

// ---------------------------------------------------------------------------
// Admin mutations. Each is a single guarded UPDATE: `WHERE id = $1 AND
// version = $2`, so `None` means "not found or someone else changed it".
// ---------------------------------------------------------------------------

pub async fn update_status<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    expected_version: i32,
    status: OrderStatus,
    transport_partner: Option<&str>,
    docket_number: Option<&str>,
) -> sqlx::Result<Option<OrderRow>> {
    sqlx::query_as::<_, OrderRow>(concat!(
        r#"
        UPDATE orders
        SET status = $3,
            transport_partner = $4,
            docket_number = $5,
            dispatched_at = CASE WHEN $6 THEN COALESCE(dispatched_at, now()) ELSE dispatched_at END,
            delivered_at = CASE WHEN $3 = 'delivered' THEN COALESCE(delivered_at, now()) ELSE delivered_at END,
            version = version + 1,
            updated_at = now()
        WHERE id = $1 AND version = $2
        RETURNING "#,
        order_cols!()
    ))
    .bind(id)
    .bind(expected_version)
    .bind(status.as_str())
    .bind(transport_partner)
    .bind(docket_number)
    .bind(status.requires_shipment())
    .fetch_optional(conn)
    .await
}

pub async fn update_shipping<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    expected_version: i32,
    transport_partner: Option<&str>,
    docket_number: Option<&str>,
) -> sqlx::Result<Option<OrderRow>> {
    sqlx::query_as::<_, OrderRow>(concat!(
        r#"
        UPDATE orders
        SET transport_partner = $3, docket_number = $4, version = version + 1, updated_at = now()
        WHERE id = $1 AND version = $2
        RETURNING "#,
        order_cols!()
    ))
    .bind(id)
    .bind(expected_version)
    .bind(transport_partner)
    .bind(docket_number)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, Copy)]
pub struct PaymentUpdate<'a> {
    pub status: &'static str,
    pub method: &'a str,
    pub reference: Option<&'a str>,
    pub set_paid_at: bool,
    pub set_refunded_at: bool,
}

pub async fn update_payment<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    expected_version: i32,
    p: PaymentUpdate<'_>,
) -> sqlx::Result<Option<OrderRow>> {
    sqlx::query_as::<_, OrderRow>(concat!(
        r#"
        UPDATE orders
        SET payment_status = $3,
            payment_method = $4,
            payment_reference = $5,
            paid_at = CASE WHEN $6 THEN now() ELSE paid_at END,
            refunded_at = CASE WHEN $7 THEN now() ELSE refunded_at END,
            version = version + 1,
            updated_at = now()
        WHERE id = $1 AND version = $2
        RETURNING "#,
        order_cols!()
    ))
    .bind(id)
    .bind(expected_version)
    .bind(p.status)
    .bind(p.method)
    .bind(p.reference)
    .bind(p.set_paid_at)
    .bind(p.set_refunded_at)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrderLock {
    pub status: String,
    pub version: i32,
}

/// `SELECT … FOR UPDATE` the order's status/version (cancellation step 1).
pub async fn lock_order(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<OrderLock>> {
    sqlx::query_as::<_, OrderLock>("SELECT status, version FROM orders WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Lock (in `id` order) every product referenced by the order (cancellation step 2).
pub async fn lock_order_products(conn: &mut PgConnection, order_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        SELECT p.id
        FROM products p
        WHERE p.id IN (SELECT product_id FROM order_items WHERE order_id = $1)
        ORDER BY p.id
        FOR UPDATE OF p
        "#,
    )
    .bind(order_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Restock every line (ONE `UPDATE products`), record `order_cancelled`
/// movements and mark the order cancelled — one statement (cancellation step 3).
/// The order and its products must already be locked by this transaction.
pub async fn cancel_locked(
    conn: &mut PgConnection,
    id: Uuid,
    admin_id: Uuid,
    reason: Option<&str>,
) -> sqlx::Result<OrderRow> {
    sqlx::query_as::<_, OrderRow>(concat!(
        r#"
        WITH restock AS (
            UPDATE products p
            SET stock = p.stock + l.quantity, version = p.version + 1, updated_at = now()
            FROM (SELECT product_id, quantity FROM order_items WHERE order_id = $1) AS l
            WHERE p.id = l.product_id
            RETURNING p.id, p.stock AS stock_after, l.quantity
        ),
        moves AS (
            INSERT INTO stock_movements (product_id, delta, stock_after, reason, order_id, admin_id)
            SELECT id, quantity, stock_after, 'order_cancelled', $1, $2
            FROM restock
            ORDER BY id
        )
        UPDATE orders
        SET status = 'cancelled', cancelled_at = now(), cancel_reason = $3,
            version = version + 1, updated_at = now()
        WHERE id = $1
        RETURNING "#,
        order_cols!()
    ))
    .bind(id)
    .bind(admin_id)
    .bind(reason)
    .fetch_one(conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip_and_order() {
        for s in OrderStatus::ALL {
            assert_eq!(OrderStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(OrderStatus::parse("PENDING_PACK"), None);
        assert!(OrderStatus::Delivered.is_terminal());
        assert!(OrderStatus::Cancelled.is_terminal());
        assert!(!OrderStatus::PendingPack.requires_shipment());
        assert!(OrderStatus::Dispatched.requires_shipment());
        assert!(OrderStatus::Delivered.requires_shipment());
        assert!(!OrderStatus::Cancelled.requires_shipment());
    }

    #[test]
    fn advisory_keys_are_namespaced() {
        assert_ne!(advisory_key("a:", "1"), advisory_key("b:", "1"));
        assert_eq!(advisory_key("a:", "1"), advisory_key("a:", "1"));
    }
}

// ---------------------------------------------------------------------------
// Payment lifecycle hooks (called by `jobs.rs` and the payments service).
//
// Lock order everywhere: an `orders` row first, then `products` rows in `id`
// order. Checkout never locks existing order rows, so no cycle is possible.
// ---------------------------------------------------------------------------

/// `SELECT … FOR UPDATE` a full order row. For callers (the payments service)
/// that must lock the order before calling [`revive_expired_order`].
pub async fn find_for_update(
    conn: &mut PgConnection,
    r: &OrderRef,
) -> sqlx::Result<Option<OrderRow>> {
    match r {
        OrderRef::Id(id) => {
            sqlx::query_as::<_, OrderRow>(concat!(
                "SELECT ",
                order_cols!(),
                " FROM orders WHERE id = $1 FOR UPDATE"
            ))
            .bind(id)
            .fetch_optional(conn)
            .await
        }
        OrderRef::Number(n) => {
            sqlx::query_as::<_, OrderRow>(concat!(
                "SELECT ",
                order_cols!(),
                " FROM orders WHERE order_number = $1 FOR UPDATE"
            ))
            .bind(n)
            .fetch_optional(conn)
            .await
        }
    }
}

/// Outcome of trying to re-reserve stock for an order that expired unpaid but
/// whose payment then arrived late.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviveOutcome {
    /// Stock re-reserved; order is back in `pending_payment` (the caller then
    /// marks it paid in the same transaction).
    Revived,
    /// Not enough stock any more: the caller must refund the payment.
    InsufficientStock,
    /// Order is not in an expired-unpaid state (nothing done).
    NotExpired,
}

/// Cancel up to `limit` `pending_payment` orders whose `payment_expires_at` has
/// passed, restocking every line (`payment_expired` movements). Returns how
/// many orders were expired. Safe to run concurrently on several replicas:
/// candidate orders are claimed with `FOR UPDATE SKIP LOCKED`, so each order is
/// expired by exactly one sweep and a sweep never waits on a row another
/// sweep (or a payment in flight) holds.
///
/// The order keeps `payment_status = 'pending'` (no money moved);
/// `cancel_reason` is [`EXPIRY_CANCEL_REASON`].
pub async fn expire_unpaid_orders(pool: &sqlx::PgPool, limit: i64) -> sqlx::Result<u64> {
    if limit <= 0 {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM orders
        WHERE status = 'pending_payment' AND payment_expires_at <= now()
        ORDER BY payment_expires_at
        LIMIT $1
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    if ids.is_empty() {
        tx.commit().await?;
        return Ok(0);
    }

    sqlx::query(
        r#"
        SELECT p.id
        FROM products p
        WHERE p.id IN (SELECT product_id FROM order_items WHERE order_id = ANY($1))
        ORDER BY p.id
        FOR UPDATE OF p
        "#,
    )
    .bind(&ids)
    .execute(&mut *tx)
    .await?;

    // One restock UPDATE for the whole batch (orders may share products). Each
    // line gets its own movement; `stock_after` is reconstructed as a running
    // total per product so the ledger stays consistent line by line.
    let expired = sqlx::query_scalar::<_, i64>(
        r#"
        WITH lines AS (
            SELECT order_id, product_id, quantity FROM order_items WHERE order_id = ANY($1)
        ),
        per_product AS (
            SELECT product_id, sum(quantity)::int AS qty FROM lines GROUP BY product_id
        ),
        restock AS (
            UPDATE products p
            SET stock = p.stock + pp.qty, version = p.version + 1, updated_at = now()
            FROM per_product pp
            WHERE p.id = pp.product_id
            RETURNING p.id, p.stock AS final_stock, pp.qty AS total_qty
        ),
        moves AS (
            INSERT INTO stock_movements (product_id, delta, stock_after, reason, order_id)
            SELECT l.product_id, l.quantity,
                   r.final_stock - r.total_qty
                     + sum(l.quantity) OVER (PARTITION BY l.product_id ORDER BY l.order_id),
                   'payment_expired', l.order_id
            FROM lines l
            JOIN restock r ON r.id = l.product_id
            ORDER BY l.product_id, l.order_id
        ),
        cancelled AS (
            UPDATE orders
            SET status = 'cancelled', cancelled_at = now(), cancel_reason = $2,
                version = version + 1, updated_at = now()
            WHERE id = ANY($1) AND status = 'pending_payment'
            RETURNING id
        )
        SELECT count(*) FROM cancelled
        "#,
    )
    .bind(&ids)
    .bind(EXPIRY_CANCEL_REASON)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(u64::try_from(expired).unwrap_or(0))
}

#[derive(sqlx::FromRow)]
struct ReviveState {
    status: String,
    cancel_reason: Option<String>,
    last_movement: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ReviveLine {
    quantity: i32,
    stock: i32,
}

/// Inside the caller's transaction (order row already locked by the caller):
/// re-reserve stock for an order cancelled by payment expiry.
///
/// "Cancelled by expiry" means: status `cancelled`, `cancel_reason` is
/// [`EXPIRY_CANCEL_REASON`] AND the order's latest stock movement is
/// `payment_expired` (an admin cancellation writes `order_cancelled`, so it can
/// never be revived even if the admin typed the same reason).
///
/// Revived ⇒ every line re-decremented (`payment_revived` movements), status
/// back to `pending_payment`, `cancelled_at`/`cancel_reason` cleared, version
/// bumped. `payment_expires_at` is left as is (already past); the caller is
/// expected to mark the order paid in the same transaction.
pub async fn revive_expired_order(
    conn: &mut PgConnection,
    order_id: Uuid,
) -> sqlx::Result<ReviveOutcome> {
    let state = sqlx::query_as::<_, ReviveState>(
        r#"
        SELECT o.status, o.cancel_reason,
               (SELECT m.reason FROM stock_movements m WHERE m.order_id = o.id
                ORDER BY m.id DESC LIMIT 1) AS last_movement
        FROM orders o
        WHERE o.id = $1
        "#,
    )
    .bind(order_id)
    .fetch_optional(&mut *conn)
    .await?;
    let expired = state.is_some_and(|s| {
        s.status == OrderStatus::Cancelled.as_str()
            && s.cancel_reason.as_deref() == Some(EXPIRY_CANCEL_REASON)
            && s.last_movement.as_deref() == Some("payment_expired")
    });
    if !expired {
        return Ok(ReviveOutcome::NotExpired);
    }

    let lines = sqlx::query_as::<_, ReviveLine>(
        r#"
        SELECT oi.quantity, p.stock
        FROM products p
        JOIN order_items oi ON oi.product_id = p.id AND oi.order_id = $1
        ORDER BY p.id
        FOR UPDATE OF p
        "#,
    )
    .bind(order_id)
    .fetch_all(&mut *conn)
    .await?;
    if lines.is_empty() || lines.iter().any(|l| l.quantity > l.stock) {
        return Ok(ReviveOutcome::InsufficientStock);
    }

    sqlx::query(
        r#"
        WITH dec AS (
            UPDATE products p
            SET stock = p.stock - l.quantity, version = p.version + 1, updated_at = now()
            FROM (SELECT product_id, quantity FROM order_items WHERE order_id = $1) AS l
            WHERE p.id = l.product_id
            RETURNING p.id, p.stock AS stock_after, l.quantity
        ),
        moves AS (
            INSERT INTO stock_movements (product_id, delta, stock_after, reason, order_id)
            SELECT id, -quantity, stock_after, 'payment_revived', $1
            FROM dec
            ORDER BY id
        )
        UPDATE orders
        SET status = 'pending_payment', cancelled_at = NULL, cancel_reason = NULL,
            version = version + 1, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(order_id)
    .execute(&mut *conn)
    .await?;
    Ok(ReviveOutcome::Revived)
}
