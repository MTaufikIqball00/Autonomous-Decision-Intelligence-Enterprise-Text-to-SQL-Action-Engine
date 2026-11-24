use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{AuthUser, Role},
    db::app_db::record_audit_event,
    error::AppError,
    state::AppState,
};

#[derive(Serialize)]
pub struct SavedQueryDto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub query_execution_id: Option<Uuid>,
    pub title: String,
    pub question: String,
    pub sql_text: String,
    pub notes: String,
    pub is_shared: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateSavedQueryRequest {
    pub query_execution_id: Option<Uuid>,
    pub title: String,
    pub question: String,
    pub sql_text: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub is_shared: bool,
}

pub async fn list_saved(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<SavedQueryDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, query_execution_id, title, question, sql_text, notes, is_shared, created_at
        FROM saved_queries
        WHERE tenant_id = $1 AND (user_id = $2 OR is_shared = TRUE)
        ORDER BY created_at DESC
        "#,
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| SavedQueryDto {
                id: r.get("id"),
                user_id: r.get("user_id"),
                query_execution_id: r.get("query_execution_id"),
                title: r.get("title"),
                question: r.get("question"),
                sql_text: r.get("sql_text"),
                notes: r.get("notes"),
                is_shared: r.get("is_shared"),
                created_at: r.get("created_at"),
            })
            .collect(),
    ))
}

pub async fn create_saved(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateSavedQueryRequest>,
) -> Result<Json<SavedQueryDto>, AppError> {
    if req.title.trim().is_empty() || req.sql_text.trim().is_empty() {
        return Err(AppError::BadRequest("Title and SQL text are required.".to_string()));
    }

    let id = Uuid::new_v4();
    let row = sqlx::query(
        r#"
        INSERT INTO saved_queries (id, tenant_id, user_id, query_execution_id, title, question, sql_text, notes, is_shared)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, user_id, query_execution_id, title, question, sql_text, notes, is_shared, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(req.query_execution_id)
    .bind(req.title.trim())
    .bind(req.question.trim())
    .bind(req.sql_text.trim())
    .bind(req.notes.trim())
    .bind(req.is_shared)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "saved_query_create",
        "saved_query",
        &id.to_string(),
        "allowed",
        json!({"title": req.title, "is_shared": req.is_shared}),
    )
    .await;

    Ok(Json(SavedQueryDto {
        id: row.get("id"),
        user_id: row.get("user_id"),
        query_execution_id: row.get("query_execution_id"),
        title: row.get("title"),
        question: row.get("question"),
        sql_text: row.get("sql_text"),
        notes: row.get("notes"),
        is_shared: row.get("is_shared"),
        created_at: row.get("created_at"),
    }))
}

pub async fn delete_saved(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let is_admin = matches!(user.role, Role::Administrator);
    let res = if is_admin {
        sqlx::query("DELETE FROM saved_queries WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(user.tenant_id)
            .execute(&state.app_pool)
            .await
    } else {
        sqlx::query("DELETE FROM saved_queries WHERE id = $1 AND tenant_id = $2 AND user_id = $3")
            .bind(id)
            .bind(user.tenant_id)
            .bind(user.user_id)
            .execute(&state.app_pool)
            .await
    }
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Saved query not found or access denied.".to_string()));
    }
    Ok(Json(json!({"deleted": true, "id": id})))
}
