//! Public storefront catalog (see `API.md`, "Catalog, Inventory & Orders"):
//! `GET /products`, `GET /products/{id}`. Also defines the `Product` /
//! `AdminProduct` JSON shapes shared by the admin product and inventory routes.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Serialize, Serializer};

use crate::{
    config::{Pricing, TEST_MODE_UNIT_PRICE_PAISE},
    error::{AppError, AppResult},
    money,
    repo::products::{self, CatalogRow, ProductRow},
    state::AppState,
};

/// Storefront reads may be cached briefly by browsers/CDNs; stock shown on the
/// storefront is advisory (checkout re-checks it under a row lock).
const CATALOG_CACHE_CONTROL: &str = "public, max-age=10";

/// `availableQuantity` never reveals more than this.
pub const MAX_ADVERTISED_QUANTITY: i32 = 1000;

/// Public storefront product.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub name: String,
    pub tamil_name: Option<String>,
    pub category: String,
    pub brand: String,
    pub pack_info: String,
    pub pieces_per_pack: Option<i32>,
    #[serde(serialize_with = "money::serialize_rupees")]
    pub price: i64,
    #[serde(serialize_with = "money::serialize_rupees")]
    pub original_price: i64,
    pub discount_percent: i32,
    /// Unit price charged when the order is below the bulk minimum (see
    /// [`Pricing`]); equals `price` when the product is not discounted that much.
    #[serde(serialize_with = "money::serialize_rupees")]
    pub standard_price: i64,
    pub standard_discount_percent: i32,
    pub image: String,
    pub gallery: Vec<String>,
    pub description: String,
    pub sound_level: Option<String>,
    pub is_popular: bool,
    pub is_best_selling: bool,
    pub is_new_arrival: bool,
    pub is_party: bool,
    pub in_stock: bool,
    pub available_quantity: i32,
}

impl Product {
    /// Real prices (admin views; the storefront goes through [`Product::for_storefront`]).
    pub fn new(r: CatalogRow, pricing: &Pricing) -> Self {
        let standard_price = pricing.standard_price_paise(r.price_paise, r.original_price_paise);
        Product {
            discount_percent: money::discount_percent(r.price_paise, r.original_price_paise),
            standard_price,
            standard_discount_percent: money::discount_percent(
                standard_price,
                r.original_price_paise,
            ),
            in_stock: r.stock > 0,
            available_quantity: r.stock.clamp(0, MAX_ADVERTISED_QUANTITY),
            id: r.id,
            name: r.name,
            tamil_name: r.tamil_name,
            category: r.category,
            brand: r.brand,
            pack_info: r.pack_info,
            pieces_per_pack: r.pieces_per_pack,
            price: r.price_paise,
            original_price: r.original_price_paise,
            image: r.image_url,
            gallery: r.gallery,
            description: r.description,
            sound_level: r.sound_level,
            is_popular: r.is_popular,
            is_best_selling: r.is_best_selling,
            is_new_arrival: r.is_new_arrival,
            is_party: r.is_party,
        }
    }
}

/// Admin product: `Product` plus exact stock and back-office fields.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminProduct {
    #[serde(flatten)]
    pub product: Product,
    pub stock: i32,
    pub low_stock_threshold: i32,
    pub bay_location: Option<String>,
    pub is_active: bool,
    pub sort_order: i32,
    pub version: i32,
    #[serde(serialize_with = "rfc3339")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339")]
    pub updated_at: DateTime<Utc>,
}

impl AdminProduct {
    pub fn new(r: ProductRow, pricing: &Pricing) -> Self {
        AdminProduct {
            stock: r.catalog.stock,
            product: Product::new(r.catalog, pricing),
            low_stock_threshold: r.low_stock_threshold,
            bay_location: r.bay_location,
            is_active: r.is_active,
            sort_order: r.sort_order,
            version: r.version,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// RFC 3339 UTC with millisecond precision, e.g. `2026-10-13T10:00:00.123Z`.
pub fn rfc3339<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// Validate a product id path segment before it reaches the database. A
/// malformed id cannot exist, so it is reported as `404 not_found`.
pub fn product_id_param(id: &str) -> AppResult<&str> {
    if products::is_valid_product_id(id) {
        Ok(id)
    } else {
        Err(AppError::NotFound)
    }
}

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/products", get(list_products))
        .route("/products/{id}", get(get_product))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductList {
    products: Vec<Product>,
    /// Always present so the storefront can show a "test payments" banner.
    test_mode: bool,
    pricing: PricingInfo,
}

/// The two-tier pricing rule, for the storefront (`GET /products`,
/// `GET /storefront/config`): `{ "bulkMinOrder": 10000, "bulkDiscountPercent": 85,
/// "standardDiscountPercent": 70 }` (rupees / whole percents).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingInfo {
    #[serde(serialize_with = "money::serialize_rupees")]
    pub bulk_min_order: i64,
    pub bulk_discount_percent: u32,
    pub standard_discount_percent: u32,
}

impl From<&Pricing> for PricingInfo {
    fn from(p: &Pricing) -> Self {
        PricingInfo {
            bulk_min_order: p.bulk_min_order_paise,
            bulk_discount_percent: p.bulk_discount_percent,
            standard_discount_percent: p.standard_discount_percent,
        }
    }
}

impl Product {
    /// Storefront view: in payment test mode every product is offered at
    /// [`TEST_MODE_UNIT_PRICE_PAISE`] in both tiers (the real MRP is kept, so
    /// the discounts are recomputed). Admin views always use the real prices
    /// via [`Product::new`].
    pub fn for_storefront(row: CatalogRow, pricing: &Pricing, test_mode: bool) -> Self {
        let mut product = Product::new(row, pricing);
        if test_mode {
            product.price = TEST_MODE_UNIT_PRICE_PAISE;
            product.standard_price = TEST_MODE_UNIT_PRICE_PAISE;
            product.discount_percent =
                money::discount_percent(product.price, product.original_price);
            product.standard_discount_percent = product.discount_percent;
        }
        product
    }
}

fn cacheable(body: impl Serialize) -> Response {
    (
        [(
            header::CACHE_CONTROL,
            HeaderValue::from_static(CATALOG_CACHE_CONTROL),
        )],
        Json(body),
    )
        .into_response()
}

async fn list_products(State(state): State<AppState>) -> AppResult<Response> {
    let test_mode = state.config.payment_test_mode;
    let pricing = &state.config.pricing;
    let rows = products::list_active(&state.db).await?;
    let products = rows
        .into_iter()
        .map(|row| Product::for_storefront(row, pricing, test_mode))
        .collect();
    Ok(cacheable(ProductList {
        products,
        test_mode,
        pricing: pricing.into(),
    }))
}

async fn get_product(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let id = product_id_param(&id)?;
    let row = products::get_active(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(cacheable(Product::for_storefront(
        row,
        &state.config.pricing,
        state.config.payment_test_mode,
    )))
}
