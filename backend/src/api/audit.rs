use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{AuthUser, Role},
    error::AppError,
    state::AppState,
};

#[derive(Serialize)]
pub struct AuditLogDto {
    pub id: Uuid,
    pub user_email: String,
    pub event_type: String,
    pub resource_type: String,
    pub resource_id: String,
    pub decision: String,
    pub details: Value,
    pub created_at: DateTime<Utc>,
}

pub async fn list_audit_logs(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<AuditLogDto>>, AppError> {
    if !matches!(user.role, Role::Administrator | Role::Auditor) {
        return Err(AppError::Forbidden(
            "Only Administrators or Auditors can inspect security audit logs.".to_string(),
        ));
    }

    let rows = sqlx::query(
        r#"
        SELECT id, user_email, event_type, resource_type, resource_id, decision, details, created_at
        FROM audit_logs
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        LIMIT 200
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AuditLogDto {
                id: r.get("id"),
                user_email: r.get("user_email"),
                event_type: r.get("event_type"),
                resource_type: r.get("resource_type"),
                resource_id: r.get("resource_id"),
                decision: r.get("decision"),
                details: r.get("details"),
                created_at: r.get("created_at"),
            })
            .collect(),
    ))
}
