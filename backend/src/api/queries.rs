use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{AuthUser, Role},
    db::{
        app_db::{load_active_connection_params, record_audit_event},
        DatabaseAdapter, ExecutionContext, PostgresAdapter,
    },
    engine::pipeline::{run_nl_query_pipeline, QueryPipelineRequest, QueryPipelineResponse},
    error::AppError,
    schema::{discovery::load_latest_schema_or_refresh, linker::filter_catalog_for_policy},
    sql_validator::validate_sql,
    state::AppState,
};

pub async fn ask_question(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<QueryPipelineRequest>,
) -> Result<Json<QueryPipelineResponse>, AppError> {
    let resp = run_nl_query_pipeline(&state, &user, req).await?;
    Ok(Json(resp))
}

#[derive(Serialize)]
pub struct QueryHistoryItem {
    pub query_id: Uuid,
    pub user_id: Uuid,
    pub question: String,
    pub provider_used: String,
    pub model_used: String,
    pub generated_sql: String,
    pub schema_version: i32,
    pub validation_status: String,
    pub execution_status: String,
    pub execution_duration_ms: i32,
    pub rows_returned: i32,
    pub is_truncated: bool,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_history(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<QueryHistoryItem>>, AppError> {
    let is_privileged = matches!(user.role, Role::Administrator | Role::Auditor);
    let rows = if is_privileged {
        sqlx::query(
            r#"
            SELECT id, user_id, question, provider_used, model_used, generated_sql, schema_version,
                   validation_status, execution_status, execution_duration_ms, rows_returned,
                   is_truncated, error_message, created_at
            FROM query_executions
            WHERE tenant_id = $1
            ORDER BY created_at DESC
            LIMIT 100
            "#,
        )
        .bind(user.tenant_id)
        .fetch_all(&state.app_pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT id, user_id, question, provider_used, model_used, generated_sql, schema_version,
                   validation_status, execution_status, execution_duration_ms, rows_returned,
                   is_truncated, error_message, created_at
            FROM query_executions
            WHERE tenant_id = $1 AND user_id = $2
            ORDER BY created_at DESC
            LIMIT 100
            "#,
        )
        .bind(user.tenant_id)
        .bind(user.user_id)
        .fetch_all(&state.app_pool)
        .await
    }
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let items = rows
        .into_iter()
        .map(|r| QueryHistoryItem {
            query_id: r.get("id"),
            user_id: r.get("user_id"),
            question: r.get("question"),
            provider_used: r.get("provider_used"),
            model_used: r.get("model_used"),
            generated_sql: r.get("generated_sql"),
            schema_version: r.get("schema_version"),
            validation_status: r.get("validation_status"),
            execution_status: r.get("execution_status"),
            execution_duration_ms: r.get("execution_duration_ms"),
            rows_returned: r.get("rows_returned"),
            is_truncated: r.get("is_truncated"),
            error_message: r.get("error_message"),
            created_at: r.get("created_at"),
        })
        .collect();

    Ok(Json(items))
}

#[derive(Deserialize)]
pub struct ExportRequest {
    pub query_id: Uuid,
}

pub async fn export_csv(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ExportRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !user.policy.allow_export {
        record_audit_event(
            &state.app_pool,
            user.tenant_id,
            Some(user.user_id),
            &user.email,
            "data_export",
            "query",
            &req.query_id.to_string(),
            "denied",
            json!({"role": user.role.as_str()}),
        )
        .await;
        return Err(AppError::Forbidden(format!(
            "Role '{}' does not have export permission.",
            user.role.as_str()
        )));
    }

    let row = sqlx::query(
        r#"
        SELECT connection_id, generated_sql, validation_status
        FROM query_executions
        WHERE id = $1 AND tenant_id = $2
        "#,
    )
    .bind(req.query_id)
    .bind(user.tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Executed query not found for tenant".to_string()))?;

    let val_status: String = row.get("validation_status");
    if val_status != "validation_passed" {
        return Err(AppError::BadRequest(
            "Only validated queries may be exported.".to_string(),
        ));
    }

    let conn_id: Uuid = row.get("connection_id");
    let generated_sql: String = row.get("generated_sql");
    let conn_params = load_active_connection_params(
        &state.app_pool,
        user.tenant_id,
        Some(conn_id),
        &state.cipher,
    )
    .await?;

    let catalog =
        load_latest_schema_or_refresh(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered_catalog = filter_catalog_for_policy(&catalog, &user.policy);
    let report = validate_sql(
        &generated_sql,
        &filtered_catalog,
        &user.policy,
        &conn_params.allowed_schemas,
        None,
    );
    if !report.is_valid {
        return Err(AppError::SqlValidationRejected(
            report
                .rejection_reason
                .unwrap_or_else(|| "Query failed export security revalidation".to_string()),
        ));
    }

    let adapter = PostgresAdapter::connect(conn_params).await?;
    let exec_ctx = ExecutionContext {
        timeout_ms: state.config.query_timeout_ms,
        row_limit: user.policy.row_limit,
        max_result_bytes: state.config.max_result_bytes,
        tenant_region: user.policy.region_filter.clone(),
    };
    let raw = adapter
        .execute_readonly_query(report.executable_sql.as_deref().unwrap_or_default(), &exec_ctx)
        .await?;

    let mut csv = String::new();
    let headers: Vec<String> = raw.columns.iter().map(|c| csv_escape(&c.name)).collect();
    csv.push_str(&headers.join(","));
    csv.push('\n');

    for r in &raw.rows {
        let cells: Vec<String> = r
            .iter()
            .map(|v| match v {
                Value::Null => String::new(),
                Value::String(s) => csv_escape(s.as_str()),
                other => csv_escape(&other.to_string()),
            })
            .collect();
        csv.push_str(&cells.join(","));
        csv.push('\n');
    }

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "data_export",
        "query",
        &req.query_id.to_string(),
        "allowed",
        json!({"rows_exported": raw.rows_returned}),
    )
    .await;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"query_export.csv\"",
            ),
        ],
        csv,
    ))
}

#[derive(Deserialize)]
pub struct ValidateSqlRequest {
    pub sql: String,
    pub requested_row_limit: Option<u32>,
}

pub async fn validate_sql_preview(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ValidateSqlRequest>,
) -> Result<Json<crate::sql_validator::ValidationReport>, AppError> {
    let conn_params =
        load_active_connection_params(&state.app_pool, user.tenant_id, None, &state.cipher).await?;
    let catalog =
        load_latest_schema_or_refresh(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered_catalog = filter_catalog_for_policy(&catalog, &user.policy);
    let report = validate_sql(
        &req.sql,
        &filtered_catalog,
        &user.policy,
        &conn_params.allowed_schemas,
        req.requested_row_limit,
    );

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "sql_validation_preview",
        "sql",
        "manual",
        if report.is_valid { "allowed" } else { "rejected" },
        json!({
            "sql": report.original_sql,
            "rejection_reason": report.rejection_reason
        }),
    )
    .await;

    Ok(Json(report))
}

#[derive(Deserialize)]
pub struct ExecuteValidatedSqlRequest {
    pub sql: String,
    pub question: Option<String>,
    pub requested_row_limit: Option<u32>,
}

pub async fn execute_validated_sql(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ExecuteValidatedSqlRequest>,
) -> Result<Json<QueryPipelineResponse>, AppError> {
    let conn_params =
        load_active_connection_params(&state.app_pool, user.tenant_id, None, &state.cipher).await?;
    if !user.policy.is_database_allowed(&conn_params.database_name) {
        return Err(AppError::Forbidden(format!(
            "Role '{}' is not authorized to query database '{}'.",
            user.policy.role, conn_params.database_name
        )));
    }

    let catalog =
        load_latest_schema_or_refresh(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered_catalog = filter_catalog_for_policy(&catalog, &user.policy);

    let report = validate_sql(
        &req.sql,
        &filtered_catalog,
        &user.policy,
        &conn_params.allowed_schemas,
        req.requested_row_limit,
    );

    let query_id = Uuid::new_v4();
    let created_at = Utc::now();
    let question = req
        .question
        .unwrap_or_else(|| "Manual AST-validated SQL execution".to_string());

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "sql_validation_manual",
        "query",
        &query_id.to_string(),
        if report.is_valid { "allowed" } else { "rejected" },
        json!({"sql": report.original_sql, "rejection_reason": report.rejection_reason}),
    )
    .await;

    if !report.is_valid {
        let rej = report
            .rejection_reason
            .clone()
            .unwrap_or_else(|| "SQL rejected by AST validator.".to_string());
        return Ok(Json(QueryPipelineResponse {
            query_id,
            question,
            provider_used: "manual_sql_validator".to_string(),
            model_used: "sqlparser-postgresql-ast".to_string(),
            is_external_llm: false,
            schema_version: filtered_catalog.version,
            linked_tables: vec![],
            join_paths: vec![],
            generated_sql: req.sql,
            validated_sql: String::new(),
            explanation: "Manual SQL rejected before database execution.".to_string(),
            assumptions: vec![],
            applied_filters: vec![],
            validation_status: "validation_rejected".to_string(),
            validation_report: Some(report),
            execution_status: crate::engine::verifier::VerifiedExecutionStatus::ValidationRejected,
            correction_attempts: vec![],
            columns: vec![],
            rows: vec![],
            result_summary: None,
            chart_spec: None,
            insight_report: None,
            clarification_needed: None,
            clarification_options: vec![],
            cannot_compute_reason: None,
            error_message: Some(rej),
            created_at,
        }));
    }

    let exec_sql = report.executable_sql.clone().unwrap_or_default();
    let adapter = PostgresAdapter::connect(conn_params).await?;
    let exec_ctx = ExecutionContext {
        timeout_ms: state.config.query_timeout_ms,
        row_limit: report.applied_row_limit,
        max_result_bytes: state.config.max_result_bytes,
        tenant_region: user.policy.region_filter.clone(),
    };

    let raw = adapter.execute_readonly_query(&exec_sql, &exec_ctx).await?;
    let summary = crate::engine::verifier::verify_execution_result(&raw, "USD", "UTC");
    let chart = crate::engine::analytics::select_chart_spec(
        query_id,
        &raw,
        &[],
        &summary.currency_unit,
        &req.sql,
    );
    let planner = crate::llm::local_deterministic::LocalDeterministicPlanner::new("manual-verifier");
    let insight = crate::engine::insights::generate_grounded_insights(
        query_id,
        &question,
        &req.sql,
        &raw,
        &summary,
        &[],
        &planner,
    )
    .await;

    Ok(Json(QueryPipelineResponse {
        query_id,
        question,
        provider_used: "manual_sql_validator".to_string(),
        model_used: "sqlparser-postgresql-ast".to_string(),
        is_external_llm: false,
        schema_version: filtered_catalog.version,
        linked_tables: vec![],
        join_paths: vec![],
        generated_sql: req.sql,
        validated_sql: exec_sql,
        explanation: "Executed user-supplied SQL after passing deterministic PostgreSQL AST security validation.".to_string(),
        assumptions: vec![],
        applied_filters: vec![],
        validation_status: "validation_passed".to_string(),
        validation_report: Some(report),
        execution_status: summary.status.clone(),
        correction_attempts: vec![],
        columns: raw.columns,
        rows: raw.rows,
        result_summary: Some(summary),
        chart_spec: Some(chart),
        insight_report: Some(insight),
        clarification_needed: None,
        clarification_options: vec![],
        cannot_compute_reason: None,
        error_message: None,
        created_at,
    }))
}

fn csv_escape(val: &str) -> String {
    if val.contains(',') || val.contains('"') || val.contains('\n') || val.contains('\r') {
        format!("\"{}\"", val.replace('"', "\"\""))
    } else {
        val.to_string()
    }
}
