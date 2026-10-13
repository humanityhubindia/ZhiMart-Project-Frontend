//! Admin product management (`/api/v1/admin/products…`). Every handler takes
//! `AuthAdmin`. Edits use optimistic concurrency (`expectedVersion`); stock is
//! only changed through `/admin/inventory` so the ledger stays complete.

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    routing::{get, post},
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    auth::AuthAdmin,
    error::{AppError, AppResult},
    money,
    repo::{
        inventory::MAX_STOCK,
        products::{self, CreateOutcome, ProductFields, WriteOutcome},
    },
    routes::{
        catalog::{AdminProduct, product_id_param},
        json::ApiJson,
    },
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/products", get(list).post(create))
        .route("/products/{id}", get(get_one).put(update).delete(archive))
        .route("/products/{id}/restore", post(restore))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    #[serde(default)]
    include_archived: bool,
}

#[derive(Serialize)]
struct AdminProductList {
    products: Vec<AdminProduct>,
}

/// `ProductInput` (API.md). Numbers are taken as `i64`/`f64` so out-of-range
/// values get a field-specific message instead of a generic parse error.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ProductInput {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(default)]
    tamil_name: Option<String>,
    category: String,
    brand: String,
    pack_info: String,
    #[serde(default)]
    pieces_per_pack: Option<i64>,
    price: f64,
    original_price: f64,
    image: String,
    #[serde(default)]
    gallery: Vec<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    sound_level: Option<String>,
    #[serde(default)]
    is_popular: bool,
    #[serde(default)]
    is_best_selling: bool,
    #[serde(default)]
    is_new_arrival: bool,
    #[serde(default)]
    is_party: bool,
    #[serde(default)]
    low_stock_threshold: Option<i64>,
    #[serde(default)]
    bay_location: Option<String>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    initial_stock: Option<i64>,
    #[serde(default)]
    expected_version: Option<i64>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

const MAX_NAME: usize = 120;
const MAX_TAMIL_NAME: usize = 120;
const MAX_CATEGORY: usize = 80;
const MAX_BRAND: usize = 80;
const MAX_PACK_INFO: usize = 60;
const MAX_DESCRIPTION: usize = 4000;
const MAX_BAY: usize = 80;
const MAX_URL: usize = 2048;
const MAX_GALLERY: usize = 10;
const MAX_PIECES_PER_PACK: i64 = 100_000;
const MAX_LOW_STOCK_THRESHOLD: i64 = 100_000;
const SORT_ORDER_LIMIT: i64 = 1_000_000;
const DEFAULT_LOW_STOCK_THRESHOLD: i32 = 20;
const SOUND_LEVELS: [&str; 4] = [
    "Low Sound",
    "Single Loud Blast",
    "Repetitive Blast",
    "Silent & Visual",
];

/// Generated ids: slug (≤ 28 chars) + `-` + 4 random base32 chars.
const SLUG_MAX: usize = 28;
const ID_SUFFIX_LEN: usize = 4;
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const ID_GENERATION_ATTEMPTS: usize = 3;

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::InvalidField(msg.into())
}

/// Trim and check a text field. Single-line fields reject all control
/// characters; multi-line fields allow `\n`, `\r` and `\t`.
fn text(field: &str, raw: &str, min: usize, max: usize, multiline: bool) -> AppResult<String> {
    let value = raw.trim();
    let len = value.chars().count();
    if len < min || len > max {
        return Err(if min > 0 {
            invalid(format!("{field} must be {min}-{max} characters"))
        } else {
            invalid(format!("{field} must be at most {max} characters"))
        });
    }
    let bad = |c: char| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t'));
    if value.chars().any(bad) {
        return Err(invalid(format!("{field} contains invalid characters")));
    }
    Ok(value.to_owned())
}

/// Optional text: missing, null, or blank after trimming ⇒ `None`.
fn optional_text(field: &str, raw: Option<&str>, max: usize) -> AppResult<Option<String>> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => text(field, v, 1, max, false).map(Some),
    }
}

fn int_in(field: &str, value: i64, min: i64, max: i64) -> AppResult<i32> {
    if (min..=max).contains(&value) {
        i32::try_from(value)
            .map_err(|_| invalid(format!("{field} must be between {min} and {max}")))
    } else {
        Err(invalid(format!("{field} must be between {min} and {max}")))
    }
}

fn price(field: &str, rupees: f64) -> AppResult<i64> {
    match money::from_rupees(rupees) {
        Some(p) if p > 0 => Ok(p),
        _ => Err(invalid(format!(
            "{field} must be greater than 0, at most 1000000, with at most 2 decimal places"
        ))),
    }
}

/// Characters allowed in a site-relative asset path.
fn is_path_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'/' | b'.' | b'-' | b'_')
}

/// Printable ASCII that is safe inside an absolute URL (no quotes, angle
/// brackets, backslashes, backticks, braces, pipes, carets or spaces).
fn is_url_char(c: u8) -> bool {
    c.is_ascii_graphic()
        && !matches!(
            c,
            b'"' | b'<' | b'>' | b'\\' | b'`' | b'{' | b'}' | b'|' | b'^'
        )
}

/// Host part of an https URL: DNS name or IPv4, optional `:port`, no userinfo.
fn is_valid_authority(authority: &str) -> bool {
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    let port_ok =
        port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()));
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    port_ok && host_ok
}

/// `/assets/...` path or `https://host/...` URL, ≤ 2048 chars.
fn validate_image(field: &str, raw: &str) -> AppResult<String> {
    let value = raw.trim();
    let err = || {
        invalid(format!(
            "{field} must be a path starting with /assets/ or an https:// URL of at most {MAX_URL} characters"
        ))
    };
    if value.is_empty() || value.len() > MAX_URL {
        return Err(err());
    }
    let ok = if let Some(rest) = value.strip_prefix("/assets/") {
        !rest.is_empty()
            && rest.bytes().all(is_path_char)
            && !value.contains("..")
            && !value.contains("//")
            && !rest.ends_with('/')
    } else if let Some(rest) = value.strip_prefix("https://") {
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        value.bytes().all(is_url_char) && is_valid_authority(&rest[..end])
    } else {
        false
    };
    if ok { Ok(value.to_owned()) } else { Err(err()) }
}

/// Validate and normalise everything except `id`, `initialStock` and
/// `expectedVersion`.
fn validate_fields(input: &ProductInput) -> AppResult<ProductFields> {
    let name = text("name", &input.name, 1, MAX_NAME, false)?;
    let tamil_name = optional_text("tamilName", input.tamil_name.as_deref(), MAX_TAMIL_NAME)?;
    let category = text("category", &input.category, 1, MAX_CATEGORY, false)?;
    let brand = text("brand", &input.brand, 1, MAX_BRAND, false)?;
    let pack_info = text("packInfo", &input.pack_info, 1, MAX_PACK_INFO, false)?;
    let pieces_per_pack = input
        .pieces_per_pack
        .map(|n| int_in("piecesPerPack", n, 1, MAX_PIECES_PER_PACK))
        .transpose()?;

    let price_paise = price("price", input.price)?;
    let original_price_paise = price("originalPrice", input.original_price)?;
    if original_price_paise < price_paise {
        return Err(invalid(
            "originalPrice must be greater than or equal to price",
        ));
    }

    let image_url = validate_image("image", &input.image)?;
    if input.gallery.len() > MAX_GALLERY {
        return Err(invalid(format!(
            "gallery may contain at most {MAX_GALLERY} images"
        )));
    }
    let gallery = input
        .gallery
        .iter()
        .map(|g| validate_image("gallery", g))
        .collect::<AppResult<Vec<_>>>()?;

    let description = match input.description.as_deref() {
        Some(d) => text("description", d, 0, MAX_DESCRIPTION, true)?,
        None => String::new(),
    };
    let sound_level = match input.sound_level.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(s) => match SOUND_LEVELS.iter().find(|l| **l == s) {
            Some(l) => Some((*l).to_owned()),
            None => {
                return Err(invalid(
                    "soundLevel must be one of: Low Sound, Single Loud Blast, Repetitive Blast, Silent & Visual",
                ));
            }
        },
    };
    let low_stock_threshold = match input.low_stock_threshold {
        Some(n) => int_in("lowStockThreshold", n, 0, MAX_LOW_STOCK_THRESHOLD)?,
        None => DEFAULT_LOW_STOCK_THRESHOLD,
    };
    let bay_location = optional_text("bayLocation", input.bay_location.as_deref(), MAX_BAY)?;
    let sort_order = match input.sort_order {
        Some(n) => int_in("sortOrder", n, -SORT_ORDER_LIMIT, SORT_ORDER_LIMIT)?,
        None => 0,
    };

    Ok(ProductFields {
        name,
        tamil_name,
        category,
        brand,
        pack_info,
        pieces_per_pack,
        price_paise,
        original_price_paise,
        image_url,
        gallery,
        description,
        sound_level,
        is_popular: input.is_popular,
        is_best_selling: input.is_best_selling,
        is_new_arrival: input.is_new_arrival,
        is_party: input.is_party,
        low_stock_threshold,
        bay_location,
        sort_order,
    })
}

/// Fold common Latin-1 / Latin Extended-A letters to ASCII; anything else
/// non-alphanumeric becomes a separator.
fn ascii_fold(c: char) -> Option<char> {
    if c.is_ascii_alphanumeric() {
        return Some(c.to_ascii_lowercase());
    }
    let folded = match c.to_lowercase().next().unwrap_or(c) {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ś' | 'š' | 'ş' => 's',
        'ť' | 'ţ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        'ł' => 'l',
        _ => return None,
    };
    Some(folded)
}

/// `"4\" Deluxe Lakshmi"` → `"4-deluxe-lakshmi"` (≤ [`SLUG_MAX`] chars, may be empty).
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(SLUG_MAX);
    let mut pending_dash = false;
    for c in name.chars() {
        match ascii_fold(c) {
            Some(a) => {
                if pending_dash && !slug.is_empty() {
                    if slug.len() + 1 >= SLUG_MAX {
                        break;
                    }
                    slug.push('-');
                }
                pending_dash = false;
                if slug.len() >= SLUG_MAX {
                    break;
                }
                slug.push(a);
            }
            None => pending_dash = true,
        }
    }
    slug
}

fn generate_id(name: &str) -> String {
    let mut slug = slugify(name);
    if slug.is_empty() {
        slug.push_str("product");
    }
    let mut rng = rand::thread_rng();
    slug.push('-');
    for _ in 0..ID_SUFFIX_LEN {
        slug.push(char::from(BASE32[rng.gen_range(0..BASE32.len())]));
    }
    slug
}

fn map_write_error(e: sqlx::Error) -> AppError {
    if products::is_check_violation(&e) {
        tracing::warn!(error = ?e, "product write violated a CHECK constraint");
        AppError::Validation("product data violates a catalog constraint")
    } else {
        e.into()
    }
}

fn version_conflict() -> AppError {
    AppError::Conflict {
        code: "version_conflict",
        message: "This product was changed by someone else. Reload it and try again.",
    }
}

fn list_query(q: Result<Query<ListQuery>, QueryRejection>) -> AppResult<ListQuery> {
    q.map(|Query(q)| q)
        .map_err(|_| AppError::Validation("includeArchived must be true or false"))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list(
    _admin: AuthAdmin,
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> AppResult<Json<AdminProductList>> {
    let q = list_query(query)?;
    let rows = products::list_admin(&state.db, q.include_archived).await?;
    Ok(Json(AdminProductList {
        products: rows
            .into_iter()
            .map(|r| AdminProduct::new(r, &state.config.pricing))
            .collect(),
    }))
}

async fn get_one(
    _admin: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<AdminProduct>> {
    let id = product_id_param(&id)?;
    let row = products::get(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(AdminProduct::new(row, &state.config.pricing)))
}

async fn create(
    admin: AuthAdmin,
    State(state): State<AppState>,
    ApiJson(input): ApiJson<ProductInput>,
) -> AppResult<(StatusCode, Json<AdminProduct>)> {
    let explicit_id = match input.id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(id) if products::is_valid_product_id(id) => Some(id.to_owned()),
        Some(_) => {
            return Err(invalid(
                "id must be 2-40 characters of lowercase letters, digits and hyphens, starting with a letter or digit",
            ));
        }
    };
    let fields = validate_fields(&input)?;
    let initial_stock = match input.initial_stock {
        Some(n) => int_in("initialStock", n, 0, i64::from(MAX_STOCK))?,
        None => 0,
    };

    // A generated id can collide (tiny odds); retry with a fresh suffix.
    let attempts = if explicit_id.is_some() {
        1
    } else {
        ID_GENERATION_ATTEMPTS
    };
    for _ in 0..attempts {
        let id = explicit_id
            .clone()
            .unwrap_or_else(|| generate_id(&fields.name));
        match products::insert(&state.db, &id, &fields, initial_stock, admin.admin_id)
            .await
            .map_err(map_write_error)?
        {
            CreateOutcome::Created(row) => {
                tracing::info!(admin_id = %admin.admin_id, product_id = %row.catalog.id, initial_stock, "product created");
                return Ok((
                    StatusCode::CREATED,
                    Json(AdminProduct::new(*row, &state.config.pricing)),
                ));
            }
            CreateOutcome::DuplicateId => continue,
        }
    }
    Err(AppError::Conflict {
        code: "duplicate_id",
        message: "A product with this id already exists.",
    })
}

async fn update(
    admin: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(input): ApiJson<ProductInput>,
) -> AppResult<Json<AdminProduct>> {
    let id = product_id_param(&id)?;
    if input
        .id
        .as_deref()
        .map(str::trim)
        .is_some_and(|body_id| !body_id.is_empty() && body_id != id)
    {
        return Err(invalid("id cannot be changed"));
    }
    if input.initial_stock.is_some() {
        return Err(invalid(
            "initialStock is only accepted on create; adjust stock via /admin/inventory",
        ));
    }
    let expected_version = match input.expected_version {
        Some(v) => int_in("expectedVersion", v, 1, i64::from(i32::MAX))?,
        None => return Err(invalid("expectedVersion is required")),
    };
    let fields = validate_fields(&input)?;

    let mut conn = state.db.acquire().await?;
    match products::update(&mut conn, id, expected_version, &fields)
        .await
        .map_err(map_write_error)?
    {
        WriteOutcome::Written(row) => {
            tracing::info!(admin_id = %admin.admin_id, product_id = %id, version = row.version, "product updated");
            Ok(Json(AdminProduct::new(*row, &state.config.pricing)))
        }
        WriteOutcome::NotFound => Err(AppError::NotFound),
        WriteOutcome::VersionConflict => Err(version_conflict()),
    }
}

async fn archive(
    admin: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let id = product_id_param(&id)?;
    let mut conn = state.db.acquire().await?;
    products::set_active(&mut conn, id, false)
        .await?
        .ok_or(AppError::NotFound)?;
    tracing::info!(admin_id = %admin.admin_id, product_id = %id, "product archived");
    Ok(StatusCode::NO_CONTENT)
}

async fn restore(
    admin: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<AdminProduct>> {
    let id = product_id_param(&id)?;
    let mut conn = state.db.acquire().await?;
    let row = products::set_active(&mut conn, id, true)
        .await?
        .ok_or(AppError::NotFound)?;
    tracing::info!(admin_id = %admin.admin_id, product_id = %id, "product restored");
    Ok(Json(AdminProduct::new(row, &state.config.pricing)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("4\" Deluxe Lakshmi"), "4-deluxe-lakshmi");
        assert_eq!(
            slugify("  Ganga Jamuna(Shower+Blast) "),
            "ganga-jamuna-shower-blast"
        );
        assert_eq!(slugify("Crème Brûlée Pots"), "creme-brulee-pots");
        assert_eq!(slugify("லட்சுமி"), "");
        let long = slugify("Zhi-mart Mega Family Keepsake Chest Deluxe");
        assert!(long.len() <= SLUG_MAX && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn generated_ids_are_valid() {
        for name in [
            "4\" Lakshmi",
            "லட்சுமி",
            "Zhi-mart Mega Family Keepsake Chest Deluxe Edition",
            "--",
        ] {
            let id = generate_id(name);
            assert!(products::is_valid_product_id(&id), "{id}");
        }
    }

    #[test]
    fn image_urls() {
        let ok = [
            "/assets/products/x.jpg",
            "https://cdn.example.com/a/b.jpg?w=1",
            "https://example.com:8443/x",
        ];
        for u in ok {
            assert!(validate_image("image", u).is_ok(), "{u}");
        }
        let bad = [
            "",
            "/assets/",
            "/assets/../secret",
            "/assets//x.jpg",
            "/assets/x y.jpg",
            "/static/x.jpg",
            "http://example.com/x.jpg",
            "https://",
            "https:///x.jpg",
            "https://user@example.com/x.jpg",
            "https://example.com/x\".jpg",
            "javascript:alert(1)",
            "https://exa_mple.com/x.jpg",
        ];
        for u in bad {
            assert!(validate_image("image", u).is_err(), "{u}");
        }
        assert!(
            validate_image("image", &format!("https://e.com/{}", "a".repeat(MAX_URL))).is_err()
        );
    }
}
