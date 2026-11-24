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
    auth::{AuthUser, RbacPolicy, Role},
    db::app_db::record_audit_event,
    error::AppError,
    state::AppState,
};

#[derive(Serialize)]
pub struct AdminUserDto {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

pub async fn list_users(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<AdminUserDto>>, AppError> {
    if !matches!(user.role, Role::Administrator | Role::Auditor) {
        return Err(AppError::Forbidden(
            "Only Administrators or Auditors can list tenant users.".to_string(),
        ));
    }

    let rows = sqlx::query(
        r#"
        SELECT id, email, display_name, role, is_active, created_at
        FROM users
        WHERE tenant_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AdminUserDto {
                id: r.get("id"),
                email: r.get("email"),
                display_name: r.get("display_name"),
                role: r.get("role"),
                is_active: r.get("is_active"),
                created_at: r.get("created_at"),
            })
            .collect(),
    ))
}

pub async fn list_rbac_policies(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<RbacPolicy>>, AppError> {
    if !matches!(user.role, Role::Administrator | Role::Auditor) {
        return Err(AppError::Forbidden(
            "Only Administrators or Auditors can inspect RBAC policies.".to_string(),
        ));
    }

    let rows = sqlx::query(
        r#"
        SELECT role, allowed_databases, allowed_schemas, allowed_tables, denied_columns,
               allow_export, allow_schema_refresh, allow_admin_config, row_limit, region_filter
        FROM rbac_policies
        WHERE tenant_id = $1
        ORDER BY role ASC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let row_limit: i32 = r.get("row_limit");
                RbacPolicy {
                    role: r.get("role"),
                    allowed_databases: serde_json::from_value(r.get("allowed_databases")).unwrap_or_default(),
                    allowed_schemas: serde_json::from_value(r.get("allowed_schemas")).unwrap_or_default(),
                    allowed_tables: serde_json::from_value(r.get("allowed_tables")).unwrap_or_default(),
                    denied_columns: serde_json::from_value(r.get("denied_columns")).unwrap_or_default(),
                    allow_export: r.get("allow_export"),
                    allow_schema_refresh: r.get("allow_schema_refresh"),
                    allow_admin_config: r.get("allow_admin_config"),
                    row_limit: row_limit.max(1) as u32,
                    region_filter: r.get("region_filter"),
                }
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct UpdateRbacPolicyRequest {
    pub allowed_schemas: Vec<String>,
    pub allowed_tables: Vec<String>,
    pub denied_columns: Vec<String>,
    pub allow_export: bool,
    pub allow_schema_refresh: bool,
    pub row_limit: u32,
    pub region_filter: String,
}

pub async fn update_rbac_policy(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role): Path<String>,
    Json(req): Json<UpdateRbacPolicyRequest>,
) -> Result<Json<RbacPolicy>, AppError> {
    if !user.policy.allow_admin_config {
        return Err(AppError::Forbidden(
            "Only Administrators can update RBAC policies.".to_string(),
        ));
    }

    let role_clean = Role::parse(&role)?.as_str().to_string();
    let row_limit = req.row_limit.clamp(1, 5000) as i32;
    let region = if req.region_filter.trim().is_empty() {
        "ALL".to_string()
    } else {
        req.region_filter.trim().to_uppercase()
    };

    let row = sqlx::query(
        r#"
        UPDATE rbac_policies
        SET allowed_schemas = $3, allowed_tables = $4, denied_columns = $5,
            allow_export = $6, allow_schema_refresh = $7, row_limit = $8, region_filter = $9, updated_at = NOW()
        WHERE tenant_id = $1 AND role = $2
        RETURNING role, allowed_databases, allowed_schemas, allowed_tables, denied_columns,
                  allow_export, allow_schema_refresh, allow_admin_config, row_limit, region_filter
        "#,
    )
    .bind(user.tenant_id)
    .bind(&role_clean)
    .bind(json!(req.allowed_schemas))
    .bind(json!(req.allowed_tables))
    .bind(json!(req.denied_columns))
    .bind(req.allow_export)
    .bind(req.allow_schema_refresh)
    .bind(row_limit)
    .bind(&region)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound(format!("RBAC policy for role '{role_clean}' not found")))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "rbac_policy_update",
        "rbac_policy",
        &role_clean,
        "allowed",
        json!({"row_limit": row_limit, "region_filter": region}),
    )
    .await;

    let rl: i32 = row.get("row_limit");
    Ok(Json(RbacPolicy {
        role: row.get("role"),
        allowed_databases: serde_json::from_value(row.get("allowed_databases")).unwrap_or_default(),
        allowed_schemas: serde_json::from_value(row.get("allowed_schemas")).unwrap_or_default(),
        allowed_tables: serde_json::from_value(row.get("allowed_tables")).unwrap_or_default(),
        denied_columns: serde_json::from_value(row.get("denied_columns")).unwrap_or_default(),
        allow_export: row.get("allow_export"),
        allow_schema_refresh: row.get("allow_schema_refresh"),
        allow_admin_config: row.get("allow_admin_config"),
        row_limit: rl as u32,
        region_filter: row.get("region_filter"),
    }))
}
