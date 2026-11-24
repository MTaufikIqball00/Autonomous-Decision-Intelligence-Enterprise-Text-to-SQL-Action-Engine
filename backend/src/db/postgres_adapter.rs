use std::{collections::BTreeMap, str::FromStr, time::{Duration, Instant}};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use serde_json::{json, Number, Value};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions, PgRow, PgSslMode},
    Column, PgPool, Row, TypeInfo,
};
use uuid::Uuid;

use crate::{
    db::{ColumnMeta, ConnectionParams, DatabaseAdapter, ExecutionContext, RawExecutionResult},
    error::{sanitize_error_message, AppError},
    schema::{ColumnCatalog, ForeignKeyMeta, IndexMeta, SchemaCatalog, TableMeta},
};

pub struct PostgresAdapter {
    pool: PgPool,
    params: ConnectionParams,
}

impl PostgresAdapter {
    pub async fn connect(params: ConnectionParams) -> Result<Self, AppError> {
        let ssl_mode = match params.ssl_mode.to_lowercase().as_str() {
            "disable" => PgSslMode::Disable,
            "allow" => PgSslMode::Allow,
            "require" => PgSslMode::Require,
            "verify-ca" => PgSslMode::VerifyCa,
            "verify-full" => PgSslMode::VerifyFull,
            _ => PgSslMode::Prefer,
        };

        let opts = PgConnectOptions::new()
            .host(&params.host)
            .port(params.port)
            .database(&params.database_name)
            .username(&params.username)
            .password(&params.password)
            .ssl_mode(ssl_mode);

        let pool = PgPoolOptions::new()
            .max_connections(8)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(opts)
            .await
            .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        Ok(Self { pool, params })
    }

    pub fn from_pool(pool: PgPool, params: ConnectionParams) -> Self {
        Self { pool, params }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl DatabaseAdapter for PostgresAdapter {
    async fn health_check(&self) -> Result<String, AppError> {
        let row = sqlx::query("SELECT current_database()::text AS db, current_user::text AS usr")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;
        let db: String = row.get("db");
        let usr: String = row.get("usr");
        Ok(format!("connected (database={db}, role={usr})"))
    }

    async fn discover_schema(
        &self,
        connection_id: Uuid,
        allowed_schemas: &[String],
    ) -> Result<SchemaCatalog, AppError> {
        let schema_filter: Vec<String> = allowed_schemas
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        if schema_filter.is_empty() {
            return Err(AppError::BadRequest(
                "Allowed schema list cannot be empty during discovery".to_string(),
            ));
        }

        // 1. Discover tables & views with PostgreSQL descriptions
        let table_rows = sqlx::query(
            r#"
            SELECT
                t.table_schema,
                t.table_name,
                t.table_type,
                obj_description(c.oid, 'pg_class') AS table_description
            FROM information_schema.tables t
            JOIN pg_namespace n ON n.nspname = t.table_schema
            JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = t.table_name
            WHERE t.table_schema = ANY($1)
              AND t.table_type IN ('BASE TABLE', 'VIEW')
            ORDER BY t.table_schema, t.table_name
            "#,
        )
        .bind(&schema_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        // 2. Discover columns, types, nullability, descriptions
        let col_rows = sqlx::query(
            r#"
            SELECT
                cols.table_schema,
                cols.table_name,
                cols.column_name,
                cols.data_type,
                cols.is_nullable,
                col_description(c.oid, cols.ordinal_position) AS column_description
            FROM information_schema.columns cols
            JOIN pg_namespace n ON n.nspname = cols.table_schema
            JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = cols.table_name
            WHERE cols.table_schema = ANY($1)
            ORDER BY cols.table_schema, cols.table_name, cols.ordinal_position
            "#,
        )
        .bind(&schema_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        // 3. Discover Primary Keys (via pg_catalog so SELECT-only roles see PK metadata cleanly)
        let pk_rows = sqlx::query(
            r#"
            SELECT
                n.nspname::text AS table_schema,
                t.relname::text AS table_name,
                a.attname::text AS column_name
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) ON TRUE
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum
            WHERE ix.indisprimary = TRUE
              AND n.nspname = ANY($1)
            ORDER BY n.nspname, t.relname, k.ord
            "#,
        )
        .bind(&schema_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        // 4. Discover Foreign Keys (via pg_constraint — strictly verified constraints, works for non-owner SELECT roles)
        let fk_rows = sqlx::query(
            r#"
            SELECT
                con.conname::text AS constraint_name,
                ns_from.nspname::text AS from_schema,
                cl_from.relname::text AS from_table,
                att_from.attname::text AS from_column,
                ns_to.nspname::text AS to_schema,
                cl_to.relname::text AS to_table,
                att_to.attname::text AS to_column
            FROM pg_constraint con
            JOIN pg_class cl_from ON cl_from.oid = con.conrelid
            JOIN pg_namespace ns_from ON ns_from.oid = cl_from.relnamespace
            JOIN pg_class cl_to ON cl_to.oid = con.confrelid
            JOIN pg_namespace ns_to ON ns_to.oid = cl_to.relnamespace
            JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS cols(from_attnum, to_attnum, ord) ON TRUE
            JOIN pg_attribute att_from ON att_from.attrelid = con.conrelid AND att_from.attnum = cols.from_attnum
            JOIN pg_attribute att_to ON att_to.attrelid = con.confrelid AND att_to.attnum = cols.to_attnum
            WHERE con.contype = 'f'
              AND ns_from.nspname = ANY($1)
              AND ns_to.nspname = ANY($1)
            ORDER BY ns_from.nspname, cl_from.relname, att_from.attname
            "#,
        )
        .bind(&schema_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        // 5. Discover Indexes where permitted
        let idx_rows = sqlx::query(
            r#"
            SELECT
                n.nspname AS table_schema,
                t.relname AS table_name,
                i.relname AS index_name,
                ix.indisunique AS is_unique,
                ARRAY_AGG(a.attname ORDER BY k.ord)::text[] AS column_names
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_class i ON i.oid = ix.indexrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) ON TRUE
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum
            WHERE n.nspname = ANY($1)
            GROUP BY n.nspname, t.relname, i.relname, ix.indisunique
            ORDER BY n.nspname, t.relname, i.relname
            "#,
        )
        .bind(&schema_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        let mut table_map: BTreeMap<(String, String), TableMeta> = BTreeMap::new();
        for r in table_rows {
            let schema: String = r.get("table_schema");
            let name: String = r.get("table_name");
            let table_type: String = r.get("table_type");
            let description: Option<String> = r.get("table_description");
            table_map.insert(
                (schema.clone(), name.clone()),
                TableMeta {
                    schema,
                    name,
                    table_type,
                    description,
                    columns: Vec::new(),
                    primary_keys: Vec::new(),
                    foreign_keys: Vec::new(),
                    indexes: Vec::new(),
                },
            );
        }

        let mut pk_set: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for r in pk_rows {
            let s: String = r.get("table_schema");
            let t: String = r.get("table_name");
            let c: String = r.get("column_name");
            pk_set.entry((s, t)).or_default().push(c);
        }

        for r in col_rows {
            let schema: String = r.get("table_schema");
            let table: String = r.get("table_name");
            let col_name: String = r.get("column_name");
            let data_type: String = r.get("data_type");
            let is_nullable_str: String = r.get("is_nullable");
            let description: Option<String> = r.get("column_description");

            if let Some(tm) = table_map.get_mut(&(schema.clone(), table.clone())) {
                let is_pk = pk_set
                    .get(&(schema, table))
                    .map(|pks| pks.iter().any(|p| p == &col_name))
                    .unwrap_or(false);
                tm.columns.push(ColumnCatalog {
                    name: col_name,
                    data_type,
                    is_nullable: is_nullable_str.eq_ignore_ascii_case("YES"),
                    is_primary_key: is_pk,
                    description,
                });
            }
        }

        for ((s, t), pks) in pk_set {
            if let Some(tm) = table_map.get_mut(&(s, t)) {
                tm.primary_keys = pks;
            }
        }

        let mut all_fks = Vec::new();
        for r in fk_rows {
            let fk = ForeignKeyMeta {
                constraint_name: r.get("constraint_name"),
                from_schema: r.get("from_schema"),
                from_table: r.get("from_table"),
                from_column: r.get("from_column"),
                to_schema: r.get("to_schema"),
                to_table: r.get("to_table"),
                to_column: r.get("to_column"),
                source: "foreign_key".to_string(),
            };
            if let Some(tm) = table_map.get_mut(&(fk.from_schema.clone(), fk.from_table.clone())) {
                tm.foreign_keys.push(fk.clone());
            }
            all_fks.push(fk);
        }

        for r in idx_rows {
            let schema: String = r.get("table_schema");
            let table: String = r.get("table_name");
            if let Some(tm) = table_map.get_mut(&(schema, table)) {
                tm.indexes.push(IndexMeta {
                    index_name: r.get("index_name"),
                    columns: r.get("column_names"),
                    is_unique: r.get("is_unique"),
                });
            }
        }

        Ok(SchemaCatalog {
            connection_id,
            database_name: self.params.database_name.clone(),
            allowed_schemas: schema_filter,
            version: 1,
            refreshed_at: Utc::now(),
            tables: table_map.into_values().collect(),
            relationships: all_fks,
        })
    }

    async fn execute_readonly_query(
        &self,
        executable_sql: &str,
        ctx: &ExecutionContext,
    ) -> Result<RawExecutionResult, AppError> {
        let start = Instant::now();
        let timeout_ms = ctx.timeout_ms.clamp(250, 30_000);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        // Enforce read-only transaction, statement timeout, and PostgreSQL RLS tenant region
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        let timeout_stmt = format!("SET LOCAL statement_timeout = '{timeout_ms}'");
        sqlx::query(&timeout_stmt)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        sqlx::query("SELECT set_config('app.current_tenant_region', $1, true)")
            .bind(&ctx.tenant_region)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError(sanitize_error_message(&e.to_string())))?;

        let query_future = sqlx::query(executable_sql).fetch_all(&mut *tx);
        let rows = match tokio::time::timeout(Duration::from_millis(timeout_ms + 1500), query_future)
            .await
        {
            Ok(Ok(rows)) => rows,
            Ok(Err(e)) => {
                let msg = sanitize_error_message(&e.to_string());
                let _ = tx.rollback().await;
                if msg.to_lowercase().contains("statement timeout")
                    || msg.to_lowercase().contains("canceling statement due to statement timeout")
                {
                    return Err(AppError::Timeout);
                }
                return Err(AppError::DatabaseError(msg));
            }
            Err(_) => {
                let _ = tx.rollback().await;
                return Err(AppError::Timeout);
            }
        };

        let columns: Vec<ColumnMeta> = rows
            .first()
            .map(|r| {
                r.columns()
                    .iter()
                    .map(|c| ColumnMeta {
                        name: c.name().to_string(),
                        type_name: c.type_info().name().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let row_cap = ctx.row_limit.max(1) as usize;
        let mut is_truncated = false;
        let mut truncation_reason: Option<String> = None;

        let mut decoded_rows: Vec<Vec<Value>> = Vec::with_capacity(rows.len().min(row_cap));
        let mut total_bytes: usize = 0;

        for (idx, row) in rows.iter().enumerate() {
            if idx >= row_cap {
                is_truncated = true;
                truncation_reason = Some(format!("Row count exceeded limit of {row_cap} rows"));
                break;
            }
            let vals = decode_pg_row(row);
            let row_bytes = serde_json::to_vec(&vals).map(|b| b.len()).unwrap_or(64);
            if total_bytes + row_bytes > ctx.max_result_bytes {
                is_truncated = true;
                truncation_reason = Some(format!(
                    "Serialized result exceeded maximum byte limit of {} bytes",
                    ctx.max_result_bytes
                ));
                break;
            }
            total_bytes += row_bytes;
            decoded_rows.push(vals);
        }

        // Always rollback read-only transaction cleanly
        let _ = tx.rollback().await;

        let duration_ms = start.elapsed().as_millis() as u64;
        let rows_returned = decoded_rows.len();

        Ok(RawExecutionResult {
            columns,
            rows: decoded_rows,
            rows_returned,
            is_truncated,
            truncation_reason,
            duration_ms,
            bytes_read: total_bytes,
            db_identity: self.params.safe_identity(),
        })
    }
}

fn decode_pg_row(row: &PgRow) -> Vec<Value> {
    let mut out = Vec::with_capacity(row.columns().len());
    for (idx, col) in row.columns().iter().enumerate() {
        let type_name = col.type_info().name().to_uppercase();
        let val = match type_name.as_str() {
            "BOOL" => match row.try_get::<Option<bool>, _>(idx) {
                Ok(Some(b)) => Value::Bool(b),
                Ok(None) => Value::Null,
                Err(_) => Value::Null,
            },
            "INT2" => match row.try_get::<Option<i16>, _>(idx) {
                Ok(Some(n)) => json!(n),
                _ => Value::Null,
            },
            "INT4" => match row.try_get::<Option<i32>, _>(idx) {
                Ok(Some(n)) => json!(n),
                _ => Value::Null,
            },
            "INT8" => match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(n)) => json!(n),
                _ => Value::Null,
            },
            "FLOAT4" => match row.try_get::<Option<f32>, _>(idx) {
                Ok(Some(f)) => Number::from_f64(f64::from(f))
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            },
            "FLOAT8" => match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(f)) => Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            },
            "NUMERIC" => match row.try_get::<Option<Decimal>, _>(idx) {
                Ok(Some(d)) => {
                    let s = d.to_string();
                    if let Ok(f) = f64::from_str(&s) {
                        Number::from_f64(f)
                            .map(Value::Number)
                            .unwrap_or_else(|| Value::String(s))
                    } else {
                        Value::String(s)
                    }
                }
                _ => Value::Null,
            },
            "DATE" => match row.try_get::<Option<NaiveDate>, _>(idx) {
                Ok(Some(d)) => Value::String(d.format("%Y-%m-%d").to_string()),
                _ => Value::Null,
            },
            "TIMESTAMP" => match row.try_get::<Option<NaiveDateTime>, _>(idx) {
                Ok(Some(dt)) => Value::String(dt.format("%Y-%m-%dT%H:%M:%S").to_string()),
                _ => Value::Null,
            },
            "TIMESTAMPTZ" => match row.try_get::<Option<DateTime<Utc>>, _>(idx) {
                Ok(Some(dt)) => Value::String(dt.to_rfc3339()),
                _ => Value::Null,
            },
            "UUID" => match row.try_get::<Option<Uuid>, _>(idx) {
                Ok(Some(u)) => Value::String(u.to_string()),
                _ => Value::Null,
            },
            "JSON" | "JSONB" => match row.try_get::<Option<Value>, _>(idx) {
                Ok(Some(v)) => v,
                _ => Value::Null,
            },
            _ => match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(s)) => Value::String(s),
                Ok(None) => Value::Null,
                Err(_) => Value::Null,
            },
        };
        out.push(val);
    }
    out
}
