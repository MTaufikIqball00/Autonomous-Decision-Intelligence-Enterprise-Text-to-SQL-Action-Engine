use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{password::verify_password, AuthUser, RbacPolicy},
    db::app_db::record_audit_event,
    error::AppError,
    state::AppState,
};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct UserProfileDto {
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub tenant_slug: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub policy: RbacPolicy,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserProfileDto,
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest("Email and password are required.".to_string()));
    }

    let row = sqlx::query(
        r#"
        SELECT u.id, u.tenant_id, t.slug AS tenant_slug, u.email, u.display_name, u.password_hash, u.role, u.is_active,
               p.allowed_databases, p.allowed_schemas, p.allowed_tables, p.denied_columns,
               p.allow_export, p.allow_schema_refresh, p.allow_admin_config, p.row_limit, p.region_filter
        FROM users u
        JOIN tenants t ON t.id = u.tenant_id
        JOIN rbac_policies p ON p.tenant_id = u.tenant_id AND p.role = u.role
        WHERE LOWER(u.email) = $1
        "#,
    )
    .bind(&email)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let Some(r) = row else {
        return Err(AppError::Unauthorized);
    };

    let pw_hash: String = r.get("password_hash");
    if !verify_password(&req.password, &pw_hash) {
        let tid: Uuid = r.get("tenant_id");
        let uid: Uuid = r.get("id");
        record_audit_event(
            &state.app_pool,
            tid,
            Some(uid),
            &email,
            "user_login",
            "auth",
            &email,
            "denied",
            json!({"reason": "invalid password"}),
        )
        .await;
        return Err(AppError::Unauthorized);
    }

    let is_active: bool = r.get("is_active");
    if !is_active {
        return Err(AppError::Forbidden("User account is disabled.".to_string()));
    }

    let user_id: Uuid = r.get("id");
    let tenant_id: Uuid = r.get("tenant_id");
    let role_str: String = r.get("role");
    let row_limit: i32 = r.get("row_limit");

    let token = state
        .jwt
        .issue_token(user_id, tenant_id, &email)
        .map_err(AppError::Internal)?;

    record_audit_event(
        &state.app_pool,
        tenant_id,
        Some(user_id),
        &email,
        "user_login",
        "auth",
        &email,
        "allowed",
        json!({"role": role_str}),
    )
    .await;

    let policy = RbacPolicy {
        role: role_str.clone(),
        allowed_databases: serde_json::from_value(r.get("allowed_databases")).unwrap_or_default(),
        allowed_schemas: serde_json::from_value(r.get("allowed_schemas")).unwrap_or_default(),
        allowed_tables: serde_json::from_value(r.get("allowed_tables")).unwrap_or_default(),
        denied_columns: serde_json::from_value(r.get("denied_columns")).unwrap_or_default(),
        allow_export: r.get("allow_export"),
        allow_schema_refresh: r.get("allow_schema_refresh"),
        allow_admin_config: r.get("allow_admin_config"),
        row_limit: row_limit.max(1) as u32,
        region_filter: r.get("region_filter"),
    };

    Ok(Json(LoginResponse {
        token,
        user: UserProfileDto {
            user_id,
            tenant_id,
            tenant_slug: r.get("tenant_slug"),
            email,
            display_name: r.get("display_name"),
            role: role_str,
            policy,
        },
    }))
}

pub async fn me(user: AuthUser) -> Json<UserProfileDto> {
    Json(UserProfileDto {
        user_id: user.user_id,
        tenant_id: user.tenant_id,
        tenant_slug: user.tenant_slug,
        email: user.email,
        display_name: user.display_name,
        role: user.role.as_str().to_string(),
        policy: user.policy,
    })
}
