use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    db::{ConnectionParams, DatabaseAdapter, PostgresAdapter},
    error::AppError,
    schema::{ForeignKeyMeta, SchemaCatalog},
};

pub async fn refresh_and_persist_schema(
    app_pool: &PgPool,
    tenant_id: Uuid,
    conn_params: &ConnectionParams,
) -> Result<SchemaCatalog, AppError> {
    if conn_params.adapter_type.eq_ignore_ascii_case("bigquery") {
        return Err(AppError::AdapterUnavailable(
            "BigQuery schema discovery is not implemented.".to_string(),
        ));
    }

    let adapter = PostgresAdapter::connect(conn_params.clone()).await?;
    refresh_and_persist_schema_with_adapter(app_pool, tenant_id, conn_params, &adapter).await
}

pub async fn refresh_and_persist_schema_with_adapter(
    app_pool: &PgPool,
    tenant_id: Uuid,
    conn_params: &ConnectionParams,
    adapter: &PostgresAdapter,
) -> Result<SchemaCatalog, AppError> {
    if conn_params.adapter_type.eq_ignore_ascii_case("bigquery") {
        return Err(AppError::AdapterUnavailable(
            "BigQuery schema discovery is not implemented.".to_string(),
        ));
    }

    let mut catalog = adapter
        .discover_schema(conn_params.id, &conn_params.allowed_schemas)
        .await?;

    // Merge administrator-documented manual relationships
    let manual_rows = sqlx::query(
        r#"
        SELECT id, from_schema, from_table, from_column, to_schema, to_table, to_column
        FROM manual_relationships
        WHERE tenant_id = $1 AND connection_id = $2
        ORDER BY created_at ASC
        "#,
    )
    .bind(tenant_id)
    .bind(conn_params.id)
    .fetch_all(app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    for r in manual_rows {
        let rel_id: Uuid = r.get("id");
        let fk = ForeignKeyMeta {
            constraint_name: format!("manual_{rel_id}"),
            from_schema: r.get("from_schema"),
            from_table: r.get("from_table"),
            from_column: r.get("from_column"),
            to_schema: r.get("to_schema"),
            to_table: r.get("to_table"),
            to_column: r.get("to_column"),
            source: "manual_relationship".to_string(),
        };
        if let Some(tbl) = catalog
            .tables
            .iter_mut()
            .find(|t| t.schema == fk.from_schema && t.name == fk.from_table)
        {
            tbl.foreign_keys.push(fk.clone());
        }
        catalog.relationships.push(fk);
    }

    // Increment snapshot version
    let latest_version: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version) FROM schema_snapshots WHERE tenant_id = $1 AND connection_id = $2",
    )
    .bind(tenant_id)
    .bind(conn_params.id)
    .fetch_one(app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let next_version = latest_version.unwrap_or(0) + 1;
    catalog.version = next_version;
    catalog.refreshed_at = Utc::now();

    let table_count = catalog.tables.len() as i32;
    let column_count: i32 = catalog.tables.iter().map(|t| t.columns.len() as i32).sum();
    let catalog_json =
        serde_json::to_value(&catalog).map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO schema_snapshots (id, tenant_id, connection_id, version, catalog_json, table_count, column_count, refreshed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(conn_params.id)
    .bind(next_version)
    .bind(catalog_json)
    .bind(table_count)
    .bind(column_count)
    .bind(catalog.refreshed_at)
    .execute(app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(catalog)
}

async fn load_latest_schema_snapshot(
    app_pool: &PgPool,
    tenant_id: Uuid,
    connection_id: Uuid,
) -> Result<Option<SchemaCatalog>, AppError> {
    let row = sqlx::query(
        r#"
        SELECT version, refreshed_at, catalog_json
        FROM schema_snapshots
        WHERE tenant_id = $1 AND connection_id = $2
        ORDER BY version DESC
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .bind(connection_id)
    .fetch_optional(app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let Some(r) = row else {
        return Ok(None);
    };
    let version: i32 = r.get("version");
    let refreshed_at: DateTime<Utc> = r.get("refreshed_at");
    let json_val: Value = r.get("catalog_json");
    let mut catalog: SchemaCatalog =
        serde_json::from_value(json_val).map_err(|e| AppError::Internal(e.to_string()))?;
    catalog.version = version;
    catalog.refreshed_at = refreshed_at;
    Ok(Some(catalog))
}

pub async fn load_latest_schema_or_refresh(
    app_pool: &PgPool,
    tenant_id: Uuid,
    conn_params: &ConnectionParams,
) -> Result<SchemaCatalog, AppError> {
    if let Some(catalog) = load_latest_schema_snapshot(app_pool, tenant_id, conn_params.id).await? {
        return Ok(catalog);
    }
    refresh_and_persist_schema(app_pool, tenant_id, conn_params).await
}

pub async fn load_latest_schema_or_refresh_with_adapter(
    app_pool: &PgPool,
    tenant_id: Uuid,
    conn_params: &ConnectionParams,
    adapter: &PostgresAdapter,
) -> Result<SchemaCatalog, AppError> {
    if let Some(catalog) = load_latest_schema_snapshot(app_pool, tenant_id, conn_params.id).await? {
        return Ok(catalog);
    }
    refresh_and_persist_schema_with_adapter(app_pool, tenant_id, conn_params, adapter).await
}
