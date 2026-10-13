//! Admin inventory (`/api/v1/admin/inventory…`): stock overview, manual
//! adjustments recorded in the stock ledger, and per-product movement history.

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    auth::AuthAdmin,
    error::{AppError, AppResult, StockShortage},
    repo::inventory::{self, AdjustOutcome, Adjustment, MAX_STOCK, StockStatus},
    routes::{
        catalog::{AdminProduct, product_id_param, rfc3339},
        json::ApiJson,
    },
    state::AppState,
};

const MAX_ADJUST_QUANTITY: i64 = 1_000_000;
const MAX_NOTE_CHARS: usize = 300;
const DEFAULT_MOVEMENTS_LIMIT: i64 = 50;
const MAX_MOVEMENTS_LIMIT: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/inventory", get(list))
        .route("/inventory/{product_id}/adjust", post(adjust))
        .route("/inventory/{product_id}/movements", get(movements))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InventoryItem {
    product_id: String,
    name: String,
    category: String,
    image: String,
    stock: i32,
    low_stock_threshold: i32,
    bay_location: Option<String>,
    is_active: bool,
    status: &'static str,
    version: i32,
    #[serde(serialize_with = "rfc3339")]
    updated_at: DateTime<Utc>,
}

/// Totals over **active** products only (archived items are listed but not counted).
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct InventorySummary {
    total_units: i64,
    in_stock: i64,
    low_stock: i64,
    out_of_stock: i64,
}

#[derive(Serialize)]
struct InventoryResponse {
    items: Vec<InventoryItem>,
    summary: InventorySummary,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AdjustRequest {
    mode: String,
    quantity: i64,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    expected_stock: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MovementsQuery {
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Movement {
    id: i64,
    delta: i32,
    stock_after: i32,
    reason: String,
    order_number: Option<String>,
    admin_email: Option<String>,
    note: Option<String>,
    #[serde(serialize_with = "rfc3339")]
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct MovementsResponse {
    movements: Vec<Movement>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list(
    _admin: AuthAdmin,
    State(state): State<AppState>,
) -> AppResult<Json<InventoryResponse>> {
    let rows = inventory::list(&state.db).await?;
    let mut summary = InventorySummary::default();
    let items = rows
        .into_iter()
        .map(|r| {
            let status = StockStatus::of(r.stock, r.low_stock_threshold);
            if r.is_active {
                summary.total_units += i64::from(r.stock);
                match status {
                    StockStatus::InStock => summary.in_stock += 1,
                    StockStatus::LowStock => summary.low_stock += 1,
                    StockStatus::OutOfStock => summary.out_of_stock += 1,
                }
            }
            InventoryItem {
                product_id: r.product_id,
                name: r.name,
                category: r.category,
                image: r.image_url,
                stock: r.stock,
                low_stock_threshold: r.low_stock_threshold,
                bay_location: r.bay_location,
                is_active: r.is_active,
                status: status.as_str(),
                version: r.version,
                updated_at: r.updated_at,
            }
        })
        .collect();
    Ok(Json(InventoryResponse { items, summary }))
}

fn quantity_in(value: i64, min: i64, max: i64) -> AppResult<i32> {
    if (min..=max).contains(&value) {
        if let Ok(v) = i32::try_from(value) {
            return Ok(v);
        }
    }
    Err(AppError::InvalidField(format!(
        "quantity must be between {min} and {max}"
    )))
}

fn parse_adjustment(req: &AdjustRequest) -> AppResult<(Adjustment, Option<i32>)> {
    let max_stock = i64::from(MAX_STOCK);
    let expected = match req.expected_stock {
        Some(e) if (0..=max_stock).contains(&e) => i32::try_from(e).ok(),
        Some(_) => {
            return Err(AppError::InvalidField(format!(
                "expectedStock must be between 0 and {max_stock}"
            )));
        }
        None => None,
    };
    let adjustment = match req.mode.as_str() {
        "set" => {
            let quantity = quantity_in(req.quantity, 0, max_stock)?;
            let expected = expected.ok_or_else(|| {
                AppError::InvalidField("expectedStock is required when mode is set".to_owned())
            })?;
            Adjustment::Set { quantity, expected }
        }
        "add" => Adjustment::Add {
            quantity: quantity_in(req.quantity, 1, MAX_ADJUST_QUANTITY)?,
        },
        "remove" => Adjustment::Remove {
            quantity: quantity_in(req.quantity, 1, MAX_ADJUST_QUANTITY)?,
        },
        _ => {
            return Err(AppError::Validation(
                "mode must be one of: set, add, remove",
            ));
        }
    };
    Ok((adjustment, expected))
}

fn parse_note(raw: Option<&str>) -> AppResult<Option<String>> {
    let Some(note) = raw.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(AppError::InvalidField(format!(
            "note must be at most {MAX_NOTE_CHARS} characters"
        )));
    }
    if note
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(AppError::Validation("note contains invalid characters"));
    }
    Ok(Some(note.to_owned()))
}

async fn adjust(
    admin: AuthAdmin,
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    ApiJson(req): ApiJson<AdjustRequest>,
) -> AppResult<Json<AdminProduct>> {
    let product_id = product_id_param(&product_id)?;
    let (adjustment, expected_stock) = parse_adjustment(&req)?;
    let note = parse_note(req.note.as_deref())?;

    let mut tx = state.db.begin().await?;
    let outcome = inventory::adjust(
        &mut tx,
        product_id,
        adjustment,
        expected_stock,
        note.as_deref(),
        admin.admin_id,
    )
    .await?;
    // Every non-`Adjusted` outcome wrote nothing; dropping `tx` rolls back the lock.
    match outcome {
        AdjustOutcome::Adjusted(row) => {
            tx.commit().await?;
            tracing::info!(
                admin_id = %admin.admin_id,
                product_id = %product_id,
                stock = row.catalog.stock,
                "inventory adjusted"
            );
            Ok(Json(AdminProduct::new(row, &state.config.pricing)))
        }
        AdjustOutcome::Unchanged(row) => Ok(Json(AdminProduct::new(row, &state.config.pricing))),
        AdjustOutcome::NotFound => Err(AppError::NotFound),
        AdjustOutcome::StockMoved { .. } => Err(AppError::Conflict {
            code: "version_conflict",
            message: "Stock changed since you loaded it. Reload and try again.",
        }),
        AdjustOutcome::Insufficient { available } => Err(AppError::OutOfStock {
            items: vec![StockShortage {
                product_id: product_id.to_owned(),
                requested: match adjustment {
                    Adjustment::Remove { quantity } => quantity,
                    _ => 0,
                },
                available,
            }],
        }),
        AdjustOutcome::TooMuch { .. } => Err(AppError::InvalidField(format!(
            "resulting stock must not exceed {MAX_STOCK}"
        ))),
    }
}

async fn movements(
    _admin: AuthAdmin,
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    query: Result<Query<MovementsQuery>, QueryRejection>,
) -> AppResult<Json<MovementsResponse>> {
    let product_id = product_id_param(&product_id)?;
    let limit_err =
        || AppError::InvalidField(format!("limit must be between 1 and {MAX_MOVEMENTS_LIMIT}"));
    let Query(q) = query.map_err(|_| limit_err())?;
    let limit = q.limit.unwrap_or(DEFAULT_MOVEMENTS_LIMIT);
    if !(1..=MAX_MOVEMENTS_LIMIT).contains(&limit) {
        return Err(limit_err());
    }

    let mut conn = state.db.acquire().await?;
    let rows = inventory::movements(&mut conn, product_id, limit)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(MovementsResponse {
        movements: rows
            .into_iter()
            .map(|m| Movement {
                id: m.id,
                delta: m.delta,
                stock_after: m.stock_after,
                reason: m.reason,
                order_number: m.order_number,
                admin_email: m.admin_email,
                note: m.note,
                created_at: m.created_at,
            })
            .collect(),
    }))
}
