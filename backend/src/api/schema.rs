use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    db::app_db::{load_active_connection_params, record_audit_event},
    error::AppError,
    schema::{
        discovery::{load_latest_schema_or_refresh, refresh_and_persist_schema},
        linker::{filter_catalog_for_policy, generate_schema_examples},
        ManualRelationshipInput, SchemaCatalog,
    },
    semantic::list_active_metrics,
    state::AppState,
};

#[derive(Serialize)]
pub struct SchemaResponse {
    pub catalog: SchemaCatalog,
    pub examples: Vec<String>,
}

pub async fn get_schema(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<SchemaResponse>, AppError> {
    let conn_params =
        load_active_connection_params(&state.app_pool, user.tenant_id, None, &state.cipher).await?;
    let catalog =
        load_latest_schema_or_refresh(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered = filter_catalog_for_policy(&catalog, &user.policy);
    let semantics = list_active_metrics(&state.app_pool, user.tenant_id).await?;
    let examples = generate_schema_examples(&filtered, &semantics);
    Ok(Json(SchemaResponse {
        catalog: filtered,
        examples,
    }))
}

pub async fn refresh_schema(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<SchemaResponse>, AppError> {
    if !user.policy.allow_schema_refresh {
        record_audit_event(
            &state.app_pool,
            user.tenant_id,
            Some(user.user_id),
            &user.email,
            "schema_refresh",
            "schema",
            "active_connection",
            "denied",
            json!({"role": user.role.as_str()}),
        )
        .await;
        return Err(AppError::Forbidden(format!(
            "Role '{}' is not authorized to refresh the schema catalog.",
            user.role.as_str()
        )));
    }

    let conn_params =
        load_active_connection_params(&state.app_pool, user.tenant_id, None, &state.cipher).await?;
    let catalog =
        refresh_and_persist_schema(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered = filter_catalog_for_policy(&catalog, &user.policy);
    let semantics = list_active_metrics(&state.app_pool, user.tenant_id).await?;
    let examples = generate_schema_examples(&filtered, &semantics);

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "schema_refresh",
        "schema",
        &conn_params.id.to_string(),
        "allowed",
        json!({"version": filtered.version, "tables": filtered.tables.len()}),
    )
    .await;

    Ok(Json(SchemaResponse {
        catalog: filtered,
        examples,
    }))
}

#[derive(Serialize)]
pub struct ManualRelationshipDto {
    pub id: Uuid,
    pub from_schema: String,
    pub from_table: String,
    pub from_column: String,
    pub to_schema: String,
    pub to_table: String,
    pub to_column: String,
    pub relationship_type: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list_manual_relationships(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ManualRelationshipDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, from_schema, from_table, from_column, to_schema, to_table, to_column, relationship_type, description, created_at
        FROM manual_relationships
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| ManualRelationshipDto {
                id: r.get("id"),
                from_schema: r.get("from_schema"),
                from_table: r.get("from_table"),
                from_column: r.get("from_column"),
                to_schema: r.get("to_schema"),
                to_table: r.get("to_table"),
                to_column: r.get("to_column"),
                relationship_type: r.get("relationship_type"),
                description: r.get("description"),
                created_at: r.get("created_at"),
            })
            .collect(),
    ))
}

pub async fn add_manual_relationship(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ManualRelationshipInput>,
) -> Result<Json<ManualRelationshipDto>, AppError> {
    if !user.policy.allow_schema_refresh {
        return Err(AppError::Forbidden(
            "Only Administrators or Data Engineers can define semantic schema relationships."
                .to_string(),
        ));
    }

    let conn_params =
        load_active_connection_params(&state.app_pool, user.tenant_id, None, &state.cipher).await?;
    let id = Uuid::new_v4();
    let row = sqlx::query(
        r#"
        INSERT INTO manual_relationships (
            id, tenant_id, connection_id, from_schema, from_table, from_column,
            to_schema, to_table, to_column, relationship_type, description, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING id, from_schema, from_table, from_column, to_schema, to_table, to_column, relationship_type, description, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(conn_params.id)
    .bind(req.from_schema.trim().to_lowercase())
    .bind(req.from_table.trim().to_lowercase())
    .bind(req.from_column.trim().to_lowercase())
    .bind(req.to_schema.trim().to_lowercase())
    .bind(req.to_table.trim().to_lowercase())
    .bind(req.to_column.trim().to_lowercase())
    .bind(req.relationship_type.trim())
    .bind(req.description.trim())
    .bind(user.user_id)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let _ = refresh_and_persist_schema(&state.app_pool, user.tenant_id, &conn_params).await;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "manual_relationship_create",
        "schema",
        &id.to_string(),
        "allowed",
        json!({"from": format!("{}.{}", req.from_schema, req.from_table), "to": format!("{}.{}", req.to_schema, req.to_table)}),
    )
    .await;

    Ok(Json(ManualRelationshipDto {
        id: row.get("id"),
        from_schema: row.get("from_schema"),
        from_table: row.get("from_table"),
        from_column: row.get("from_column"),
        to_schema: row.get("to_schema"),
        to_table: row.get("to_table"),
        to_column: row.get("to_column"),
        relationship_type: row.get("relationship_type"),
        description: row.get("description"),
        created_at: row.get("created_at"),
    }))
}
