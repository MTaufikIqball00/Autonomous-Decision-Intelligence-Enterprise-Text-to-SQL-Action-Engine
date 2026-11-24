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
    auth::AuthUser,
    db::app_db::record_audit_event,
    error::AppError,
    state::AppState,
};

#[derive(Serialize)]
pub struct ProviderDto {
    pub id: Uuid,
    pub name: String,
    pub provider_type: String,
    pub model_name: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub is_active: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub provider_type: String,
    pub model_name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

pub async fn list_providers(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ProviderDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, provider_type, model_name, base_url, encrypted_api_key, is_active, updated_at
        FROM llm_providers
        WHERE tenant_id = $1
        ORDER BY is_active DESC, name ASC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let enc: String = r.get("encrypted_api_key");
                ProviderDto {
                    id: r.get("id"),
                    name: r.get("name"),
                    provider_type: r.get("provider_type"),
                    model_name: r.get("model_name"),
                    base_url: r.get("base_url"),
                    has_api_key: !enc.trim().is_empty(),
                    is_active: r.get("is_active"),
                    updated_at: r.get("updated_at"),
                }
            })
            .collect(),
    ))
}

pub async fn create_provider(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateProviderRequest>,
) -> Result<Json<ProviderDto>, AppError> {
    if !user.policy.allow_admin_config {
        return Err(AppError::Forbidden(
            "Only Administrators can configure LLM providers.".to_string(),
        ));
    }

    let ptype = req.provider_type.trim().to_lowercase();
    if !matches!(ptype.as_str(), "local_deterministic" | "openai" | "gemini") {
        return Err(AppError::BadRequest(
            "provider_type must be local_deterministic, openai, or gemini".to_string(),
        ));
    }
    if req.model_name.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Explicit model_name is required (no hardcoded default model).".to_string(),
        ));
    }

    let enc_key = if req.api_key.trim().is_empty() {
        String::new()
    } else {
        state
            .cipher
            .encrypt(req.api_key.trim())
            .map_err(AppError::Internal)?
    };

    let id = Uuid::new_v4();
    let row = sqlx::query(
        r#"
        INSERT INTO llm_providers (id, tenant_id, name, provider_type, model_name, base_url, encrypted_api_key, is_active)
        VALUES ($1, $2, $3, $4, $5, $6, $7, FALSE)
        RETURNING id, name, provider_type, model_name, base_url, encrypted_api_key, is_active, updated_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.name.trim())
    .bind(&ptype)
    .bind(req.model_name.trim())
    .bind(req.base_url.trim())
    .bind(enc_key)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "provider_create",
        "llm_provider",
        &id.to_string(),
        "allowed",
        json!({"provider_type": ptype, "model_name": req.model_name}),
    )
    .await;

    let enc: String = row.get("encrypted_api_key");
    Ok(Json(ProviderDto {
        id: row.get("id"),
        name: row.get("name"),
        provider_type: row.get("provider_type"),
        model_name: row.get("model_name"),
        base_url: row.get("base_url"),
        has_api_key: !enc.is_empty(),
        is_active: row.get("is_active"),
        updated_at: row.get("updated_at"),
    }))
}

pub async fn activate_provider(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !user.policy.allow_admin_config {
        return Err(AppError::Forbidden(
            "Only Administrators can switch active LLM providers.".to_string(),
        ));
    }

    let mut tx = state
        .app_pool
        .begin()
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    sqlx::query("UPDATE llm_providers SET is_active = FALSE WHERE tenant_id = $1")
        .bind(user.tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let res = sqlx::query(
        "UPDATE llm_providers SET is_active = TRUE, updated_at = NOW() WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(user.tenant_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Provider not found".to_string()));
    }

    tx.commit()
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "provider_activate",
        "llm_provider",
        &id.to_string(),
        "allowed",
        json!({}),
    )
    .await;

    Ok(Json(json!({"activated": true, "id": id})))
}
