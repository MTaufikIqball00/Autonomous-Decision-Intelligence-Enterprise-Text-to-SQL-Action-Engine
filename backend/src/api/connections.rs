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
    db::{
        app_db::{load_active_connection_params, record_audit_event},
        BigQueryAdapterStub, DatabaseAdapter,
    },
    error::AppError,
    state::AppState,
};

/// Safe DTO: NEVER exposes password, encrypted_password, or full connection URL.
#[derive(Serialize)]
pub struct ConnectionDto {
    pub id: Uuid,
    pub name: String,
    pub adapter_type: String,
    pub host: String,
    pub port: i32,
    pub database_name: String,
    pub username: String,
    pub ssl_mode: String,
    pub allowed_schemas: Vec<String>,
    pub is_readonly: bool,
    pub is_active: bool,
    pub has_encrypted_secret: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateConnectionRequest {
    pub name: String,
    pub adapter_type: String,
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub username: String,
    pub password: String,
    pub ssl_mode: String,
    pub allowed_schemas: Vec<String>,
}

pub async fn list_connections(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ConnectionDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, adapter_type, host, port, database_name, username, ssl_mode,
               allowed_schemas, is_readonly, is_active, encrypted_password, created_at
        FROM db_connections
        WHERE tenant_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let list = rows
        .into_iter()
        .map(|r| {
            let enc: String = r.get("encrypted_password");
            let allowed: Vec<String> =
                serde_json::from_value(r.get("allowed_schemas")).unwrap_or_default();
            ConnectionDto {
                id: r.get("id"),
                name: r.get("name"),
                adapter_type: r.get("adapter_type"),
                host: r.get("host"),
                port: r.get("port"),
                database_name: r.get("database_name"),
                username: r.get("username"),
                ssl_mode: r.get("ssl_mode"),
                allowed_schemas: allowed,
                is_readonly: r.get("is_readonly"),
                is_active: r.get("is_active"),
                has_encrypted_secret: !enc.is_empty(),
                created_at: r.get("created_at"),
            }
        })
        .collect();

    Ok(Json(list))
}

pub async fn create_connection(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateConnectionRequest>,
) -> Result<Json<ConnectionDto>, AppError> {
    if !user.policy.allow_admin_config {
        return Err(AppError::Forbidden(
            "Only Administrators can register database connections.".to_string(),
        ));
    }
    if req.name.trim().is_empty() || req.host.trim().is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest(
            "Connection name, host, and password are required.".to_string(),
        ));
    }
    let adapter = req.adapter_type.trim().to_lowercase();
    if adapter != "postgresql" && adapter != "bigquery" {
        return Err(AppError::BadRequest(
            "adapter_type must be 'postgresql' or 'bigquery'.".to_string(),
        ));
    }

    let encrypted_pw = state
        .cipher
        .encrypt(&req.password)
        .map_err(AppError::Internal)?;
    let id = Uuid::new_v4();
    let schemas_clean: Vec<String> = req
        .allowed_schemas
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let row = sqlx::query(
        r#"
        INSERT INTO db_connections (
            id, tenant_id, name, adapter_type, host, port, database_name,
            username, encrypted_password, ssl_mode, allowed_schemas, is_readonly, is_active
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE, TRUE)
        RETURNING id, name, adapter_type, host, port, database_name, username, ssl_mode,
                  allowed_schemas, is_readonly, is_active, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.name.trim())
    .bind(&adapter)
    .bind(req.host.trim())
    .bind(req.port as i32)
    .bind(req.database_name.trim())
    .bind(req.username.trim())
    .bind(encrypted_pw)
    .bind(req.ssl_mode.trim())
    .bind(json!(schemas_clean))
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    state.invalidate_analytics_pool(id).await;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "connection_create",
        "db_connection",
        &id.to_string(),
        "allowed",
        json!({"name": req.name, "adapter_type": adapter, "database": req.database_name}),
    )
    .await;

    Ok(Json(ConnectionDto {
        id: row.get("id"),
        name: row.get("name"),
        adapter_type: row.get("adapter_type"),
        host: row.get("host"),
        port: row.get("port"),
        database_name: row.get("database_name"),
        username: row.get("username"),
        ssl_mode: row.get("ssl_mode"),
        allowed_schemas: serde_json::from_value(row.get("allowed_schemas")).unwrap_or_default(),
        is_readonly: row.get("is_readonly"),
        is_active: row.get("is_active"),
        has_encrypted_secret: true,
        created_at: row.get("created_at"),
    }))
}

pub async fn test_connection(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let params =
        load_active_connection_params(&state.app_pool, user.tenant_id, Some(id), &state.cipher)
            .await?;

    if params.adapter_type.eq_ignore_ascii_case("bigquery") {
        let stub = BigQueryAdapterStub {
            project_id: params.database_name.clone(),
        };
        stub.health_check().await?;
    }

    state.invalidate_analytics_pool(id).await;
    let adapter = state.analytics_adapter(&params).await?;
    let status = adapter.health_check().await?;
    Ok(Json(json!({
        "connection_id": id,
        "identity": params.safe_identity(),
        "status": status
    })))
}
