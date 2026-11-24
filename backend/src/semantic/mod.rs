use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMetric {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub metric_key: String,
    pub display_name: String,
    pub version: i32,
    pub definition: String,
    pub sql_formula: String,
    pub source_table: String,
    pub approved_dimensions: Vec<String>,
    pub approved_filters: Vec<String>,
    pub currency_unit: String,
    pub timezone_convention: String,
    pub owner_name: String,
    pub freshness_sla: String,
    pub is_active: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertSemanticMetricRequest {
    pub metric_key: String,
    pub display_name: String,
    pub definition: String,
    pub sql_formula: String,
    pub source_table: String,
    pub approved_dimensions: Vec<String>,
    pub approved_filters: Vec<String>,
    pub currency_unit: String,
    pub timezone_convention: String,
    pub owner_name: String,
    pub freshness_sla: String,
}

pub async fn list_active_metrics(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<SemanticMetric>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, tenant_id, metric_key, display_name, version, definition, sql_formula,
               source_table, approved_dimensions, approved_filters, currency_unit,
               timezone_convention, owner_name, freshness_sla, is_active, updated_at
        FROM semantic_metrics
        WHERE tenant_id = $1 AND is_active = TRUE
        ORDER BY metric_key ASC, version DESC
        "#,
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(rows.into_iter().map(row_to_metric).collect())
}

pub async fn list_all_metric_versions(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<SemanticMetric>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, tenant_id, metric_key, display_name, version, definition, sql_formula,
               source_table, approved_dimensions, approved_filters, currency_unit,
               timezone_convention, owner_name, freshness_sla, is_active, updated_at
        FROM semantic_metrics
        WHERE tenant_id = $1
        ORDER BY metric_key ASC, version DESC
        "#,
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(rows.into_iter().map(row_to_metric).collect())
}

pub async fn create_new_metric_version(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    req: UpsertSemanticMetricRequest,
) -> Result<SemanticMetric, AppError> {
    let key = req.metric_key.trim().to_lowercase();
    if key.is_empty() || req.display_name.trim().is_empty() || req.sql_formula.trim().is_empty() {
        return Err(AppError::BadRequest(
            "metric_key, display_name, and sql_formula are required".to_string(),
        ));
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let max_ver: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version) FROM semantic_metrics WHERE tenant_id = $1 AND metric_key = $2",
    )
    .bind(tenant_id)
    .bind(&key)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let next_ver = max_ver.unwrap_or(0) + 1;

    sqlx::query(
        "UPDATE semantic_metrics SET is_active = FALSE WHERE tenant_id = $1 AND metric_key = $2 AND is_active = TRUE",
    )
    .bind(tenant_id)
    .bind(&key)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let new_id = Uuid::new_v4();
    let dims_json = serde_json::to_value(&req.approved_dimensions).unwrap_or_default();
    let filters_json = serde_json::to_value(&req.approved_filters).unwrap_or_default();

    let row = sqlx::query(
        r#"
        INSERT INTO semantic_metrics (
            id, tenant_id, metric_key, display_name, version, definition, sql_formula,
            source_table, approved_dimensions, approved_filters, currency_unit,
            timezone_convention, owner_name, freshness_sla, is_active, updated_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, TRUE, $15)
        RETURNING id, tenant_id, metric_key, display_name, version, definition, sql_formula,
                  source_table, approved_dimensions, approved_filters, currency_unit,
                  timezone_convention, owner_name, freshness_sla, is_active, updated_at
        "#,
    )
    .bind(new_id)
    .bind(tenant_id)
    .bind(&key)
    .bind(req.display_name.trim())
    .bind(next_ver)
    .bind(req.definition.trim())
    .bind(req.sql_formula.trim())
    .bind(req.source_table.trim())
    .bind(dims_json)
    .bind(filters_json)
    .bind(req.currency_unit.trim())
    .bind(req.timezone_convention.trim())
    .bind(req.owner_name.trim())
    .bind(req.freshness_sla.trim())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(row_to_metric(row))
}

fn row_to_metric(r: sqlx::postgres::PgRow) -> SemanticMetric {
    SemanticMetric {
        id: r.get("id"),
        tenant_id: r.get("tenant_id"),
        metric_key: r.get("metric_key"),
        display_name: r.get("display_name"),
        version: r.get("version"),
        definition: r.get("definition"),
        sql_formula: r.get("sql_formula"),
        source_table: r.get("source_table"),
        approved_dimensions: serde_json::from_value(r.get("approved_dimensions")).unwrap_or_default(),
        approved_filters: serde_json::from_value(r.get("approved_filters")).unwrap_or_default(),
        currency_unit: r.get("currency_unit"),
        timezone_convention: r.get("timezone_convention"),
        owner_name: r.get("owner_name"),
        freshness_sla: r.get("freshness_sla"),
        is_active: r.get("is_active"),
        updated_at: r.get("updated_at"),
    }
}
