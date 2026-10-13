//! Inventory: stock overview, manual adjustments (each recorded in the
//! append-only `stock_movements` ledger) and per-product movement history.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::products::{ProductRow, admin_columns};

/// Upper bound enforced by `products_stock_nonneg`.
pub const MAX_STOCK: i32 = 10_000_000;

pub const REASON_RESTOCK: &str = "restock";
pub const REASON_MANUAL_ADJUST: &str = "manual_adjust";
pub const REASON_MANUAL_SET: &str = "manual_set";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockStatus {
    InStock,
    LowStock,
    OutOfStock,
}

impl StockStatus {
    pub fn of(stock: i32, low_stock_threshold: i32) -> Self {
        if stock <= 0 {
            StockStatus::OutOfStock
        } else if stock <= low_stock_threshold {
            StockStatus::LowStock
        } else {
            StockStatus::InStock
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StockStatus::InStock => "in_stock",
            StockStatus::LowStock => "low_stock",
            StockStatus::OutOfStock => "out_of_stock",
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InventoryRow {
    pub product_id: String,
    pub name: String,
    pub category: String,
    pub image_url: String,
    pub stock: i32,
    pub low_stock_threshold: i32,
    pub bay_location: Option<String>,
    pub is_active: bool,
    pub version: i32,
    pub updated_at: DateTime<Utc>,
}

/// Every product (active first, then storefront order). Callers derive the
/// summary from these rows, so the whole view is one query.
pub async fn list<'e>(db: impl PgExecutor<'e>) -> sqlx::Result<Vec<InventoryRow>> {
    sqlx::query_as::<_, InventoryRow>(
        r#"
        SELECT id AS product_id, name, category, image_url, stock, low_stock_threshold,
               bay_location, is_active, version, updated_at
        FROM products
        ORDER BY is_active DESC, sort_order, id
        "#,
    )
    .fetch_all(db)
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjustment {
    /// Set stock to an absolute value; `expected` is the value the admin saw.
    Set { quantity: i32, expected: i32 },
    /// Restock.
    Add { quantity: i32 },
    /// Write-off.
    Remove { quantity: i32 },
}

#[derive(Debug)]
pub enum AdjustOutcome {
    /// Stock changed and a movement was recorded (caller must commit).
    Adjusted(ProductRow),
    /// `set` to the current value: nothing written.
    Unchanged(ProductRow),
    NotFound,
    /// The caller's `expected` stock no longer matches.
    StockMoved {
        current: i32,
    },
    /// A removal larger than the stock on hand.
    Insufficient {
        available: i32,
    },
    /// The result would exceed [`MAX_STOCK`].
    TooMuch {
        current: i32,
    },
}

/// Apply a manual adjustment inside the caller's transaction.
///
/// Locks the product row (`FOR UPDATE`) so concurrent adjustments and order
/// placements serialise, then updates stock/version and appends the ledger
/// row in a single statement. `expected_stock`, when given for `add`/`remove`,
/// is checked too.
pub async fn adjust(
    conn: &mut PgConnection,
    product_id: &str,
    adjustment: Adjustment,
    expected_stock: Option<i32>,
    note: Option<&str>,
    admin_id: Uuid,
) -> sqlx::Result<AdjustOutcome> {
    let current = sqlx::query_as::<_, ProductRow>(concat!(
        "SELECT ",
        admin_columns!(),
        " FROM products WHERE id = $1 FOR UPDATE"
    ))
    .bind(product_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(current) = current else {
        return Ok(AdjustOutcome::NotFound);
    };
    let stock = current.catalog.stock;

    let expected = match adjustment {
        Adjustment::Set { expected, .. } => Some(expected),
        _ => expected_stock,
    };
    if expected.is_some_and(|e| e != stock) {
        return Ok(AdjustOutcome::StockMoved { current: stock });
    }

    let (new_stock, reason) = match adjustment {
        Adjustment::Set { quantity, .. } => (i64::from(quantity), REASON_MANUAL_SET),
        Adjustment::Add { quantity } => (i64::from(stock) + i64::from(quantity), REASON_RESTOCK),
        Adjustment::Remove { quantity } => {
            if quantity > stock {
                return Ok(AdjustOutcome::Insufficient { available: stock });
            }
            (i64::from(stock) - i64::from(quantity), REASON_MANUAL_ADJUST)
        }
    };
    let new_stock = match i32::try_from(new_stock) {
        Ok(v) if (0..=MAX_STOCK).contains(&v) => v,
        _ => return Ok(AdjustOutcome::TooMuch { current: stock }),
    };
    if new_stock == stock {
        return Ok(AdjustOutcome::Unchanged(current));
    }

    let row = sqlx::query_as::<_, ProductRow>(concat!(
        "WITH u AS (
            UPDATE products SET stock = $2, version = version + 1, updated_at = now()
            WHERE id = $1
            RETURNING ",
        admin_columns!(),
        "
        ), m AS (
            INSERT INTO stock_movements (product_id, delta, stock_after, reason, admin_id, note)
            SELECT id, $3, stock, $4, $5, $6 FROM u
        )
        SELECT ",
        admin_columns!(),
        " FROM u"
    ))
    .bind(product_id)
    .bind(new_stock)
    .bind(new_stock - stock)
    .bind(reason)
    .bind(admin_id)
    .bind(note)
    .fetch_one(&mut *conn)
    .await?;
    Ok(AdjustOutcome::Adjusted(row))
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MovementRow {
    pub id: i64,
    pub delta: i32,
    pub stock_after: i32,
    pub reason: String,
    pub order_number: Option<String>,
    pub admin_email: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Newest-first ledger for one product, or `None` if the product does not exist.
pub async fn movements(
    conn: &mut PgConnection,
    product_id: &str,
    limit: i64,
) -> sqlx::Result<Option<Vec<MovementRow>>> {
    let rows = sqlx::query_as::<_, MovementRow>(
        r#"
        SELECT m.id, m.delta, m.stock_after, m.reason, o.order_number,
               a.email AS admin_email, m.note, m.created_at
        FROM stock_movements m
        LEFT JOIN orders o ON o.id = m.order_id
        LEFT JOIN admins a ON a.id = m.admin_id
        WHERE m.product_id = $1
        ORDER BY m.created_at DESC, m.id DESC
        LIMIT $2
        "#,
    )
    .bind(product_id)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    if !rows.is_empty() {
        return Ok(Some(rows));
    }
    // Empty ledger: distinguish "no history yet" from "no such product".
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM products WHERE id = $1)")
            .bind(product_id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(exists.then_some(rows))
}
