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
    db::app_db::record_audit_event,
    error::AppError,
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
