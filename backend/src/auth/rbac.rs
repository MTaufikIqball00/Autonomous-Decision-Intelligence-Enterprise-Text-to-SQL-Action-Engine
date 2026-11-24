use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{error::AppError, state::AppState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Administrator,
    DataEngineer,
    Analyst,
    BusinessUser,
    Auditor,
}

impl Role {
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s.trim().to_lowercase().as_str() {
            "administrator" => Ok(Self::Administrator),
            "data_engineer" => Ok(Self::DataEngineer),
            "analyst" => Ok(Self::Analyst),
            "business_user" => Ok(Self::BusinessUser),
            "auditor" => Ok(Self::Auditor),
            other => Err(AppError::Forbidden(format!("Unsupported role '{other}'"))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Administrator => "administrator",
            Self::DataEngineer => "data_engineer",
            Self::Analyst => "analyst",
            Self::BusinessUser => "business_user",
            Self::Auditor => "auditor",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbacPolicy {
    pub role: String,
    pub allowed_databases: Vec<String>,
    pub allowed_schemas: Vec<String>,
    /// Empty vector means all tables inside allowed_schemas are permitted; otherwise explicit `schema.table` entries.
    pub allowed_tables: Vec<String>,
    /// Fully qualified `schema.table.column` strings that this role cannot query or see via `SELECT *`.
    pub denied_columns: Vec<String>,
    pub allow_export: bool,
    pub allow_schema_refresh: bool,
    pub allow_admin_config: bool,
    pub row_limit: u32,
    pub region_filter: String,
}

impl RbacPolicy {
    pub fn is_database_allowed(&self, db_name: &str) -> bool {
        self.allowed_databases
            .iter()
            .any(|d| d == "*" || d.eq_ignore_ascii_case(db_name))
    }

    pub fn is_schema_allowed(&self, schema: &str) -> bool {
        self.allowed_schemas
            .iter()
            .any(|s| s == "*" || s.eq_ignore_ascii_case(schema))
    }

    pub fn is_table_allowed(&self, schema: &str, table: &str) -> bool {
        if !self.is_schema_allowed(schema) {
            return false;
        }
        if self.allowed_tables.is_empty() {
            return true;
        }
        let fq = format!("{}.{}", schema.to_lowercase(), table.to_lowercase());
        self.allowed_tables
            .iter()
            .any(|t| t == "*" || t.eq_ignore_ascii_case(&fq))
    }

    pub fn is_column_denied(&self, schema: &str, table: &str, column: &str) -> bool {
        let fq = format!(
            "{}.{}.{}",
            schema.to_lowercase(),
            table.to_lowercase(),
            column.to_lowercase()
        );
        self.denied_columns
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&fq))
    }

    pub fn table_has_denied_columns(&self, schema: &str, table: &str) -> bool {
        let prefix = format!("{}.{}.", schema.to_lowercase(), table.to_lowercase());
        self.denied_columns
            .iter()
            .any(|c| c.to_lowercase().starts_with(&prefix))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub tenant_slug: String,
    pub email: String,
    pub display_name: String,
    pub role: Role,
    pub policy: RbacPolicy,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(AppError::Unauthorized)?;

        let claims = state
            .jwt
            .verify_token(token)
            .map_err(|_| AppError::Unauthorized)?;

        // Always fetch authoritative role & RBAC policy from application DB; never trust client headers
        let row = sqlx::query(
            r#"
            SELECT u.id, u.tenant_id, t.slug AS tenant_slug, u.email, u.display_name, u.role, u.is_active,
                   p.allowed_databases, p.allowed_schemas, p.allowed_tables, p.denied_columns,
                   p.allow_export, p.allow_schema_refresh, p.allow_admin_config, p.row_limit, p.region_filter
            FROM users u
            JOIN tenants t ON t.id = u.tenant_id
            JOIN rbac_policies p ON p.tenant_id = u.tenant_id AND p.role = u.role
            WHERE u.id = $1 AND u.tenant_id = $2
            "#,
        )
        .bind(claims.sub)
        .bind(claims.tenant_id)
        .fetch_optional(&state.app_pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .ok_or(AppError::Unauthorized)?;

        let is_active: bool = row.get("is_active");
        if !is_active {
            return Err(AppError::Forbidden("User account is disabled".to_string()));
        }

        let role_str: String = row.get("role");
        let role = Role::parse(&role_str)?;

        let allowed_databases: Vec<String> =
            serde_json::from_value(row.get("allowed_databases")).unwrap_or_default();
        let allowed_schemas: Vec<String> =
            serde_json::from_value(row.get("allowed_schemas")).unwrap_or_default();
        let allowed_tables: Vec<String> =
            serde_json::from_value(row.get("allowed_tables")).unwrap_or_default();
        let denied_columns: Vec<String> =
            serde_json::from_value(row.get("denied_columns")).unwrap_or_default();
        let row_limit: i32 = row.get("row_limit");

        Ok(AuthUser {
            user_id: row.get("id"),
            tenant_id: row.get("tenant_id"),
            tenant_slug: row.get("tenant_slug"),
            email: row.get("email"),
            display_name: row.get("display_name"),
            role,
            policy: RbacPolicy {
                role: role_str,
                allowed_databases,
                allowed_schemas,
                allowed_tables,
                denied_columns,
                allow_export: row.get("allow_export"),
                allow_schema_refresh: row.get("allow_schema_refresh"),
                allow_admin_config: row.get("allow_admin_config"),
                row_limit: row_limit.max(1) as u32,
                region_filter: row.get("region_filter"),
            },
        })
    }
}
