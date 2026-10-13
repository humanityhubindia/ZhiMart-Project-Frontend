//! `products`: storefront reads, admin CRUD with optimistic concurrency
//! (`version`), and archive/restore. Stock is never written here except for
//! the opening balance on create; all later stock changes go through
//! `repo::inventory` (and the orders module) so the ledger stays complete.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor, Postgres, postgres::PgArguments, query::QueryAs};
use uuid::Uuid;

/// Columns needed to render a storefront `Product`.
macro_rules! catalog_columns {
    () => {
        "id, name, tamil_name, category, brand, pack_info, pieces_per_pack, \
         price_paise, original_price_paise, image_url, gallery, description, sound_level, \
         is_popular, is_best_selling, is_new_arrival, is_party, stock"
    };
}

/// Columns needed to render an `AdminProduct` (decodes as [`ProductRow`]).
macro_rules! admin_columns {
    () => {
        concat!(
            $crate::repo::products::catalog_columns!(),
            ", low_stock_threshold, bay_location, is_active, sort_order, version, created_at, updated_at"
        )
    };
}

pub(crate) use {admin_columns, catalog_columns};

/// SQLSTATE for `unique_violation`.
const UNIQUE_VIOLATION: &str = "23505";
/// SQLSTATE for `check_violation`.
const CHECK_VIOLATION: &str = "23514";
const PRODUCTS_PKEY: &str = "products_pkey";

/// Maximum product id length (see `products_id_format`).
pub const MAX_ID_LEN: usize = 40;

/// `^[a-z0-9][a-z0-9-]{1,39}$` — the `products_id_format` CHECK, without regex.
pub fn is_valid_product_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=MAX_ID_LEN).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// True if `e` is a CHECK-constraint violation (input slipped past validation).
pub fn is_check_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some(CHECK_VIOLATION))
}

fn is_duplicate_id(e: &sqlx::Error) -> bool {
    matches!(
        e,
        sqlx::Error::Database(db)
            if db.code().as_deref() == Some(UNIQUE_VIOLATION) && db.constraint() == Some(PRODUCTS_PKEY)
    )
}

/// Storefront view of a product.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CatalogRow {
    pub id: String,
    pub name: String,
    pub tamil_name: Option<String>,
    pub category: String,
    pub brand: String,
    pub pack_info: String,
    pub pieces_per_pack: Option<i32>,
    pub price_paise: i64,
    pub original_price_paise: i64,
    pub image_url: String,
    pub gallery: Vec<String>,
    pub description: String,
    pub sound_level: Option<String>,
    pub is_popular: bool,
    pub is_best_selling: bool,
    pub is_new_arrival: bool,
    pub is_party: bool,
    pub stock: i32,
}

/// Full admin view of a product.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProductRow {
    #[sqlx(flatten)]
    pub catalog: CatalogRow,
    pub low_stock_threshold: i32,
    pub bay_location: Option<String>,
    pub is_active: bool,
    pub sort_order: i32,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Validated, admin-editable product attributes (everything except id and stock).
#[derive(Debug, Clone)]
pub struct ProductFields {
    pub name: String,
    pub tamil_name: Option<String>,
    pub category: String,
    pub brand: String,
    pub pack_info: String,
    pub pieces_per_pack: Option<i32>,
    pub price_paise: i64,
    pub original_price_paise: i64,
    pub image_url: String,
    pub gallery: Vec<String>,
    pub description: String,
    pub sound_level: Option<String>,
    pub is_popular: bool,
    pub is_best_selling: bool,
    pub is_new_arrival: bool,
    pub is_party: bool,
    pub low_stock_threshold: i32,
    pub bay_location: Option<String>,
    pub sort_order: i32,
}

/// Bind [`ProductFields`] as `$1..$19`, in the column order used by
/// [`insert`] and [`update`].
fn bind_fields<'q, O>(
    q: QueryAs<'q, Postgres, O, PgArguments>,
    f: &'q ProductFields,
) -> QueryAs<'q, Postgres, O, PgArguments> {
    q.bind(f.name.as_str())
        .bind(f.tamil_name.as_deref())
        .bind(f.category.as_str())
        .bind(f.brand.as_str())
        .bind(f.pack_info.as_str())
        .bind(f.pieces_per_pack)
        .bind(f.price_paise)
        .bind(f.original_price_paise)
        .bind(f.image_url.as_str())
        .bind(f.gallery.as_slice())
        .bind(f.description.as_str())
        .bind(f.sound_level.as_deref())
        .bind(f.is_popular)
        .bind(f.is_best_selling)
        .bind(f.is_new_arrival)
        .bind(f.is_party)
        .bind(f.low_stock_threshold)
        .bind(f.bay_location.as_deref())
        .bind(f.sort_order)
}

/// Active products in storefront order. Served by `products_storefront_idx`.
pub async fn list_active<'e>(db: impl PgExecutor<'e>) -> sqlx::Result<Vec<CatalogRow>> {
    sqlx::query_as::<_, CatalogRow>(concat!(
        "SELECT ",
        catalog_columns!(),
        " FROM products WHERE is_active ORDER BY sort_order, id"
    ))
    .fetch_all(db)
    .await
}

/// One active product (archived products are invisible to the storefront).
pub async fn get_active<'e>(db: impl PgExecutor<'e>, id: &str) -> sqlx::Result<Option<CatalogRow>> {
    sqlx::query_as::<_, CatalogRow>(concat!(
        "SELECT ",
        catalog_columns!(),
        " FROM products WHERE id = $1 AND is_active"
    ))
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Admin product list, optionally including archived products.
pub async fn list_admin<'e>(
    db: impl PgExecutor<'e>,
    include_archived: bool,
) -> sqlx::Result<Vec<ProductRow>> {
    sqlx::query_as::<_, ProductRow>(concat!(
        "SELECT ",
        admin_columns!(),
        " FROM products WHERE ($1 OR is_active) ORDER BY sort_order, id"
    ))
    .bind(include_archived)
    .fetch_all(db)
    .await
}

/// Any product, active or archived.
pub async fn get<'e>(db: impl PgExecutor<'e>, id: &str) -> sqlx::Result<Option<ProductRow>> {
    sqlx::query_as::<_, ProductRow>(concat!(
        "SELECT ",
        admin_columns!(),
        " FROM products WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await
}

#[derive(Debug)]
pub enum CreateOutcome {
    Created(Box<ProductRow>),
    DuplicateId,
}

/// Insert a product and, if `initial_stock > 0`, its `initial` stock movement.
/// One statement, so both rows commit or neither does.
pub async fn insert<'e>(
    db: impl PgExecutor<'e>,
    id: &str,
    fields: &ProductFields,
    initial_stock: i32,
    admin_id: Uuid,
) -> sqlx::Result<CreateOutcome> {
    let q = sqlx::query_as::<_, ProductRow>(concat!(
        "WITH p AS (
            INSERT INTO products (
                name, tamil_name, category, brand, pack_info, pieces_per_pack,
                price_paise, original_price_paise, image_url, gallery, description, sound_level,
                is_popular, is_best_selling, is_new_arrival, is_party,
                low_stock_threshold, bay_location, sort_order,
                id, stock
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)
            RETURNING ",
        admin_columns!(),
        "
        ), m AS (
            INSERT INTO stock_movements (product_id, delta, stock_after, reason, admin_id)
            SELECT id, stock, stock, 'initial', $22 FROM p WHERE stock > 0
        )
        SELECT ",
        admin_columns!(),
        " FROM p"
    ));
    let result = bind_fields(q, fields)
        .bind(id)
        .bind(initial_stock)
        .bind(admin_id)
        .fetch_one(db)
        .await;
    match result {
        Ok(row) => Ok(CreateOutcome::Created(Box::new(row))),
        Err(e) if is_duplicate_id(&e) => Ok(CreateOutcome::DuplicateId),
        Err(e) => Err(e),
    }
}

#[derive(Debug)]
pub enum WriteOutcome {
    Written(Box<ProductRow>),
    NotFound,
    VersionConflict,
}

/// Replace a product's editable attributes if its version still equals
/// `expected_version`. Stock is deliberately not touched.
pub async fn update(
    conn: &mut PgConnection,
    id: &str,
    expected_version: i32,
    fields: &ProductFields,
) -> sqlx::Result<WriteOutcome> {
    let q = sqlx::query_as::<_, ProductRow>(concat!(
        "UPDATE products SET
            name = $1, tamil_name = $2, category = $3, brand = $4, pack_info = $5,
            pieces_per_pack = $6, price_paise = $7, original_price_paise = $8,
            image_url = $9, gallery = $10, description = $11, sound_level = $12,
            is_popular = $13, is_best_selling = $14, is_new_arrival = $15, is_party = $16,
            low_stock_threshold = $17, bay_location = $18, sort_order = $19,
            version = version + 1, updated_at = now()
         WHERE id = $20 AND version = $21
         RETURNING ",
        admin_columns!()
    ));
    let updated = bind_fields(q, fields)
        .bind(id)
        .bind(expected_version)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(row) = updated {
        return Ok(WriteOutcome::Written(Box::new(row)));
    }
    // Failure path only: tell "gone" apart from "changed under you".
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM products WHERE id = $1)")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(if exists {
        WriteOutcome::VersionConflict
    } else {
        WriteOutcome::NotFound
    })
}

/// Archive (`active = false`) or restore (`active = true`) a product. Idempotent:
/// a product already in the requested state is returned unchanged (no version bump).
pub async fn set_active(
    conn: &mut PgConnection,
    id: &str,
    active: bool,
) -> sqlx::Result<Option<ProductRow>> {
    let changed = sqlx::query_as::<_, ProductRow>(concat!(
        "UPDATE products SET is_active = $2, version = version + 1, updated_at = now()
         WHERE id = $1 AND is_active <> $2
         RETURNING ",
        admin_columns!()
    ))
    .bind(id)
    .bind(active)
    .fetch_optional(&mut *conn)
    .await?;
    match changed {
        Some(row) => Ok(Some(row)),
        None => get(&mut *conn, id).await,
    }
}
