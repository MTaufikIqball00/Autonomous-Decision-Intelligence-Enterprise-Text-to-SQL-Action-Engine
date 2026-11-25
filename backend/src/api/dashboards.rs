use axum::{
    extract::{Path, State},
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
        DatabaseAdapter, ExecutionContext,
    },
    engine::{analytics::select_chart_spec, verifier::verify_execution_result},
    error::AppError,
    schema::{discovery::load_latest_schema_or_refresh_with_adapter, linker::filter_catalog_for_policy},
    sql_validator::validate_sql,
    state::AppState,
};

#[derive(Serialize)]
pub struct DashboardDto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub description: String,
    pub widgets: Value,
    pub is_shared: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateDashboardRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub widgets: Value,
    #[serde(default)]
    pub is_shared: bool,
}

pub async fn list_dashboards(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<DashboardDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, title, description, widgets_json, is_shared, created_at, updated_at
        FROM dashboards
        WHERE tenant_id = $1 AND (user_id = $2 OR is_shared = TRUE)
        ORDER BY updated_at DESC
        "#,
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| DashboardDto {
                id: r.get("id"),
                user_id: r.get("user_id"),
                title: r.get("title"),
                description: r.get("description"),
                widgets: r.get("widgets_json"),
                is_shared: r.get("is_shared"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect(),
    ))
}

pub async fn create_dashboard(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateDashboardRequest>,
) -> Result<Json<DashboardDto>, AppError> {
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("Dashboard title is required.".to_string()));
    }

    let id = Uuid::new_v4();
    let widgets = if req.widgets.is_array() {
        req.widgets
    } else {
        json!([])
    };

    let row = sqlx::query(
        r#"
        INSERT INTO dashboards (id, tenant_id, user_id, title, description, widgets_json, is_shared)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, user_id, title, description, widgets_json, is_shared, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(req.title.trim())
    .bind(req.description.trim())
    .bind(widgets)
    .bind(req.is_shared)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "dashboard_create",
        "dashboard",
        &id.to_string(),
        "allowed",
        json!({"title": req.title}),
    )
    .await;

    Ok(Json(DashboardDto {
        id: row.get("id"),
        user_id: row.get("user_id"),
        title: row.get("title"),
        description: row.get("description"),
        widgets: row.get("widgets_json"),
        is_shared: row.get("is_shared"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }))
}

pub async fn delete_dashboard(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let is_admin = matches!(user.role, Role::Administrator);
    let res = if is_admin {
        sqlx::query("DELETE FROM dashboards WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(user.tenant_id)
            .execute(&state.app_pool)
            .await
    } else {
        sqlx::query("DELETE FROM dashboards WHERE id = $1 AND tenant_id = $2 AND user_id = $3")
            .bind(id)
            .bind(user.tenant_id)
            .bind(user.user_id)
            .execute(&state.app_pool)
            .await
    }
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Dashboard not found or access denied.".to_string()));
    }
    Ok(Json(json!({"deleted": true, "id": id})))
}

#[derive(Deserialize)]
pub struct AddWidgetRequest {
    pub widget: Value,
}

pub async fn add_dashboard_widget(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AddWidgetRequest>,
) -> Result<Json<DashboardDto>, AppError> {
    if !req.widget.is_object() {
        return Err(AppError::BadRequest("Widget payload must be a JSON object.".to_string()));
    }

    let is_admin = matches!(user.role, Role::Administrator);
    let existing = sqlx::query(
        r#"
        SELECT id, user_id, widgets_json, is_shared
        FROM dashboards
        WHERE id = $1 AND tenant_id = $2
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Dashboard not found.".to_string()))?;

    let owner_id: Uuid = existing.get("user_id");
    let is_shared: bool = existing.get("is_shared");
    if !is_admin && owner_id != user.user_id && !is_shared {
        return Err(AppError::Forbidden("You cannot add widgets to another user's private dashboard.".to_string()));
    }

    let mut widgets: Vec<Value> =
        serde_json::from_value(existing.get("widgets_json")).unwrap_or_default();
    widgets.push(req.widget);

    let updated = sqlx::query(
        r#"
        UPDATE dashboards
        SET widgets_json = $3, updated_at = NOW()
        WHERE id = $1 AND tenant_id = $2
        RETURNING id, user_id, title, description, widgets_json, is_shared, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(json!(widgets))
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "dashboard_widget_add",
        "dashboard",
        &id.to_string(),
        "allowed",
        json!({"widget_count": widgets.len()}),
    )
    .await;

    Ok(Json(DashboardDto {
        id: updated.get("id"),
        user_id: updated.get("user_id"),
        title: updated.get("title"),
        description: updated.get("description"),
        widgets: updated.get("widgets_json"),
        is_shared: updated.get("is_shared"),
        created_at: updated.get("created_at"),
        updated_at: updated.get("updated_at"),
    }))
}

#[derive(Deserialize)]
pub struct RefreshDashboardRequest {
    pub connection_id: Option<Uuid>,
}

/// Re-executes every widget's SQL through the deterministic AST validator and caller's RBAC policy.
pub async fn refresh_dashboard(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    req_opt: Option<Json<RefreshDashboardRequest>>,
) -> Result<Json<DashboardDto>, AppError> {
    let existing = sqlx::query(
        r#"
        SELECT id, user_id, title, description, widgets_json, is_shared
        FROM dashboards
        WHERE id = $1 AND tenant_id = $2 AND (user_id = $3 OR is_shared = TRUE)
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Dashboard not found or access denied.".to_string()))?;

    let widgets: Vec<Value> =
        serde_json::from_value(existing.get("widgets_json")).unwrap_or_default();
    let default_conn_override = req_opt.and_then(|Json(r)| r.connection_id);

    let mut refreshed_widgets = Vec::with_capacity(widgets.len());
    let mut refreshed_count = 0usize;
    let mut rejected_count = 0usize;

    for mut w in widgets {
        let mut sql_text = w
            .get("sql_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if sql_text.is_empty() {
            if let Some(sq_id_str) = w.get("saved_query_id").and_then(|v| v.as_str()) {
                if let Ok(sq_id) = Uuid::parse_str(sq_id_str) {
                    if let Ok(Some(sq_sql)) = sqlx::query_scalar::<_, String>(
                        "SELECT sql_text FROM saved_queries WHERE id = $1 AND tenant_id = $2 AND (user_id = $3 OR is_shared = TRUE)",
                    )
                    .bind(sq_id)
                    .bind(user.tenant_id)
                    .bind(user.user_id)
                    .fetch_optional(&state.app_pool)
                    .await
                    {
                        sql_text = sq_sql;
                    }
                }
            }
        }

        if sql_text.is_empty() {
            if let Some(obj) = w.as_object_mut() {
                obj.insert("refresh_error".to_string(), json!("Widget has no sql_text or saved_query_id reference to refresh."));
            }
            refreshed_widgets.push(w);
            rejected_count += 1;
            continue;
        }

        let widget_conn_id = w
            .get("connection_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .or(default_conn_override);

        let conn_params = load_active_connection_params(
            &state.app_pool,
            user.tenant_id,
            widget_conn_id,
            &state.cipher,
        )
        .await?;

        if !user.policy.is_database_allowed(&conn_params.database_name) {
            if let Some(obj) = w.as_object_mut() {
                obj.insert(
                    "refresh_error".to_string(),
                    json!(format!(
                        "Role '{}' is not authorized to query database '{}'.",
                        user.policy.role, conn_params.database_name
                    )),
                );
            }
            refreshed_widgets.push(w);
            rejected_count += 1;
            continue;
        }

        let adapter = state.analytics_adapter(&conn_params).await?;
        let catalog = load_latest_schema_or_refresh_with_adapter(
            &state.app_pool,
            user.tenant_id,
            &conn_params,
            &adapter,
        )
        .await?;
        let filtered_catalog = filter_catalog_for_policy(&catalog, &user.policy);
        let report = validate_sql(
            &sql_text,
            &filtered_catalog,
            &user.policy,
            &conn_params.allowed_schemas,
            None,
        );

        if !report.is_valid {
            let reason = report
                .rejection_reason
                .unwrap_or_else(|| "SQL rejected by AST validator on live refresh.".to_string());
            if let Some(obj) = w.as_object_mut() {
                obj.insert("refresh_error".to_string(), json!(reason));
            }
            refreshed_widgets.push(w);
            rejected_count += 1;
            continue;
        }

        let exec_sql = report.executable_sql.unwrap_or_default();
        let exec_ctx = ExecutionContext {
            timeout_ms: state.config.query_timeout_ms,
            row_limit: report.applied_row_limit,
            max_result_bytes: state.config.max_result_bytes,
            tenant_region: user.policy.region_filter.clone(),
        };

        match adapter.execute_readonly_query(&exec_sql, &exec_ctx).await {
            Ok(raw) => {
                let summary = verify_execution_result(&raw, "USD", "UTC");
                let chart = select_chart_spec(
                    Uuid::new_v4(),
                    &raw,
                    &[],
                    &summary.currency_unit,
                    &sql_text,
                );
                if let Some(obj) = w.as_object_mut() {
                    obj.insert("sql_text".to_string(), json!(sql_text));
                    obj.insert("connection_id".to_string(), json!(conn_params.id));
                    obj.insert("columns".to_string(), json!(raw.columns));
                    obj.insert("rows".to_string(), json!(raw.rows));
                    obj.insert("chart_spec".to_string(), json!(chart));
                    obj.insert("result_summary".to_string(), json!(summary));
                    obj.insert("refreshed_at".to_string(), json!(Utc::now()));
                    obj.remove("refresh_error");
                }
                refreshed_count += 1;
            }
            Err(err) => {
                if let Some(obj) = w.as_object_mut() {
                    obj.insert("refresh_error".to_string(), json!(err.to_string()));
                }
                rejected_count += 1;
            }
        }
        refreshed_widgets.push(w);
    }

    let updated = sqlx::query(
        r#"
        UPDATE dashboards
        SET widgets_json = $3, updated_at = NOW()
        WHERE id = $1 AND tenant_id = $2
        RETURNING id, user_id, title, description, widgets_json, is_shared, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(json!(refreshed_widgets))
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "dashboard_refresh",
        "dashboard",
        &id.to_string(),
        if rejected_count == 0 { "allowed" } else { "partial" },
        json!({"refreshed": refreshed_count, "rejected": rejected_count}),
    )
    .await;

    Ok(Json(DashboardDto {
        id: updated.get("id"),
        user_id: updated.get("user_id"),
        title: updated.get("title"),
        description: updated.get("description"),
        widgets: updated.get("widgets_json"),
        is_shared: updated.get("is_shared"),
        created_at: updated.get("created_at"),
        updated_at: updated.get("updated_at"),
    }))
}
