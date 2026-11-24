use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    db::{app_db::load_active_connection_params, DatabaseAdapter, PostgresAdapter},
    engine::pipeline::load_active_provider_config,
    error::AppError,
    schema::{discovery::load_latest_schema_or_refresh, linker::filter_catalog_for_policy},
    state::AppState,
};

#[derive(Serialize)]
pub struct RecentExecutionSummary {
    pub query_id: Uuid,
    pub question: String,
    pub execution_status: String,
    pub validation_status: String,
    pub rows_returned: i32,
    pub duration_ms: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct OverviewResponse {
    pub source_db_status: String,
    pub source_db_identity: String,
    pub schema_version: i32,
    pub permitted_table_count: usize,
    pub permitted_column_count: usize,
    pub active_metric_count: i64,
    pub total_query_executions: i64,
    pub rejected_query_count: i64,
    pub saved_query_count: i64,
    pub dashboard_count: i64,
    pub active_provider_type: String,
    pub active_provider_model: String,
    pub recent_queries: Vec<RecentExecutionSummary>,
}

pub async fn get_overview(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<OverviewResponse>, AppError> {
    let conn_params =
        load_active_connection_params(&state.app_pool, user.tenant_id, None, &state.cipher).await?;
    let source_db_identity = conn_params.safe_identity();

    let source_db_status = match PostgresAdapter::connect(conn_params.clone()).await {
        Ok(adapter) => adapter.health_check().await.unwrap_or_else(|e| format!("degraded: {e}")),
        Err(e) => format!("unavailable: {e}"),
    };

    let catalog =
        load_latest_schema_or_refresh(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered = filter_catalog_for_policy(&catalog, &user.policy);
    let permitted_table_count = filtered.tables.len();
    let permitted_column_count: usize = filtered.tables.iter().map(|t| t.columns.len()).sum();

    let active_metric_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM semantic_metrics WHERE tenant_id = $1 AND is_active = TRUE",
    )
    .bind(user.tenant_id)
    .fetch_one(&state.app_pool)
    .await
    .unwrap_or(0);

    let total_query_executions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM query_executions WHERE tenant_id = $1")
            .bind(user.tenant_id)
            .fetch_one(&state.app_pool)
            .await
            .unwrap_or(0);

    let rejected_query_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM query_executions WHERE tenant_id = $1 AND validation_status = 'validation_rejected'",
    )
    .bind(user.tenant_id)
    .fetch_one(&state.app_pool)
    .await
    .unwrap_or(0);

    let saved_query_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM saved_queries WHERE tenant_id = $1")
            .bind(user.tenant_id)
            .fetch_one(&state.app_pool)
            .await
            .unwrap_or(0);

    let dashboard_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM dashboards WHERE tenant_id = $1")
            .bind(user.tenant_id)
            .fetch_one(&state.app_pool)
            .await
            .unwrap_or(0);

    let provider_cfg = load_active_provider_config(&state, user.tenant_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT id, question, execution_status, validation_status, rows_returned, execution_duration_ms, created_at
        FROM query_executions
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        LIMIT 8
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let recent_queries = rows
        .into_iter()
        .map(|r| RecentExecutionSummary {
            query_id: r.get("id"),
            question: r.get("question"),
            execution_status: r.get("execution_status"),
            validation_status: r.get("validation_status"),
            rows_returned: r.get("rows_returned"),
            duration_ms: r.get("execution_duration_ms"),
            created_at: r.get("created_at"),
        })
        .collect();

    Ok(Json(OverviewResponse {
        source_db_status,
        source_db_identity,
        schema_version: filtered.version,
        permitted_table_count,
        permitted_column_count,
        active_metric_count,
        total_query_executions,
        rejected_query_count,
        saved_query_count,
        dashboard_count,
        active_provider_type: provider_cfg.provider_type,
        active_provider_model: provider_cfg.model_name,
        recent_queries,
    }))
}
