pub mod app_db;
pub mod postgres_adapter;

pub use postgres_adapter::PostgresAdapter;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::AppError, schema::SchemaCatalog};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionParams {
    pub id: Uuid,
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

impl ConnectionParams {
    /// Safe non-secret identity label for audit logs and UI status.
    pub fn safe_identity(&self) -> String {
        format!(
            "{}://{}:{}@{}:{}/{}",
            self.adapter_type, self.username, "***", self.host, self.port, self.database_name
        )
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub timeout_ms: u64,
    pub row_limit: u32,
    pub max_result_bytes: usize,
    pub tenant_region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawExecutionResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub rows_returned: usize,
    pub is_truncated: bool,
    pub truncation_reason: Option<String>,
    pub duration_ms: u64,
    pub bytes_read: usize,
    pub db_identity: String,
}

#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    async fn health_check(&self) -> Result<String, AppError>;
    async fn discover_schema(
        &self,
        connection_id: Uuid,
        allowed_schemas: &[String],
    ) -> Result<SchemaCatalog, AppError>;
    async fn execute_readonly_query(
        &self,
        executable_sql: &str,
        ctx: &ExecutionContext,
    ) -> Result<RawExecutionResult, AppError>;
}

/// Explicit future adapter stub (does not claim BigQuery support without real verification).
pub struct BigQueryAdapterStub {
    pub project_id: String,
}

#[async_trait]
impl DatabaseAdapter for BigQueryAdapterStub {
    async fn health_check(&self) -> Result<String, AppError> {
        Err(AppError::AdapterUnavailable(format!(
            "BigQuery adapter for project '{}' is an unimplemented stub; PostgreSQL is the verified primary adapter.",
            self.project_id
        )))
    }

    async fn discover_schema(
        &self,
        _connection_id: Uuid,
        _allowed_schemas: &[String],
    ) -> Result<SchemaCatalog, AppError> {
        Err(AppError::AdapterUnavailable(
            "BigQuery schema discovery is not implemented.".to_string(),
        ))
    }

    async fn execute_readonly_query(
        &self,
        _executable_sql: &str,
        _ctx: &ExecutionContext,
    ) -> Result<RawExecutionResult, AppError> {
        Err(AppError::AdapterUnavailable(
            "BigQuery query execution is not implemented.".to_string(),
        ))
    }
}
