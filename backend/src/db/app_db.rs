use std::time::Duration;

use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use uuid::Uuid;

use crate::{
    auth::password::hash_password,
    config::AppConfig,
    crypto::SecretCipher,
};

pub async fn connect_app_pool(cfg: &AppConfig) -> Result<PgPool, String> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.app_database_url())
        .await
        .map_err(|e| format!("Failed to connect to app database: {e}"))
}

pub async fn bootstrap_app_database(
    pool: &PgPool,
    cfg: &AppConfig,
    cipher: &SecretCipher,
) -> Result<(Uuid, Uuid), String> {
    // Apply idempotent application DDL
    let ddl = include_str!("../../../sql/02_init_app_db.sql");
    for stmt in ddl.split(';') {
        let trimmed = stmt.trim();
        if trimmed.is_empty() {
            continue;
        }
        sqlx::query(trimmed)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed applying app DB statement: {e}"))?;
    }

    // Check if default tenant exists
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM tenants WHERE slug = $1")
        .bind("acme-enterprise")
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Tenant query failed: {e}"))?;

    let tenant_id = match existing {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO tenants (id, slug, name, allowed_region_code) VALUES ($1, $2, $3, $4)",
            )
            .bind(id)
            .bind("acme-enterprise")
            .bind("Acme Enterprise Analytics")
            .bind("ALL")
            .execute(pool)
            .await
            .map_err(|e| format!("Insert default tenant failed: {e}"))?;
            id
        }
    };

    // Ensure demo RBAC policies exist for all 5 roles
    let roles = [
        (
            "administrator",
            true,
            true,
            true,
            2000_i32,
            json!([]),
            json!(["analytics", "finance"]),
            "ALL",
        ),
        (
            "data_engineer",
            true,
            true,
            false,
            1000_i32,
            json!(["analytics.customers.tax_id"]),
            json!(["analytics", "finance"]),
            "ALL",
        ),
        (
            "analyst",
            true,
            false,
            false,
            500_i32,
            json!(["analytics.customers.tax_id"]),
            json!(["analytics", "finance"]),
            "ALL",
        ),
        (
            "business_user",
            false,
            false,
            false,
            250_i32,
            json!(["analytics.customers.tax_id"]),
            json!(["analytics"]),
            "ALL",
        ),
        (
            "auditor",
            false,
            false,
            false,
            500_i32,
            json!(["analytics.customers.tax_id"]),
            json!(["analytics", "finance"]),
            "ALL",
        ),
    ];

    for (role, allow_export, allow_refresh, allow_admin, row_limit, denied_cols, allowed_schemas, region) in roles {
        sqlx::query(
            r#"
            INSERT INTO rbac_policies (
                id, tenant_id, role, allowed_databases, allowed_schemas, allowed_tables,
                denied_columns, allow_export, allow_schema_refresh, allow_admin_config, row_limit, region_filter
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (tenant_id, role) DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(role)
        .bind(json!([cfg.analytics_database]))
        .bind(allowed_schemas)
        .bind(json!([]))
        .bind(denied_cols)
        .bind(allow_export)
        .bind(allow_refresh)
        .bind(allow_admin)
        .bind(row_limit)
        .bind(region)
        .execute(pool)
        .await
        .map_err(|e| format!("Insert RBAC policy for {role} failed: {e}"))?;
    }

    // Seed default demo users if absent (dev/demo credentials documented in README)
    let demo_users = [
        ("admin@enterprise.local", "Enterprise Admin", "administrator", "AdminPass!2026"),
        ("analyst@enterprise.local", "Lead Analyst", "analyst", "AnalystPass!2026"),
        ("business@enterprise.local", "VP Commercial", "business_user", "BusinessPass!2026"),
        ("auditor@enterprise.local", "Security Auditor", "auditor", "AuditorPass!2026"),
    ];

    let mut admin_user_id: Option<Uuid> = None;
    for (email, name, role, default_pw) in demo_users {
        let existing_uid: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Query user {email} failed: {e}"))?;
        let uid = match existing_uid {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4();
                let pw_hash = hash_password(default_pw)?;
                sqlx::query(
                    r#"
                    INSERT INTO users (id, tenant_id, email, display_name, password_hash, role, is_active)
                    VALUES ($1, $2, $3, $4, $5, $6, TRUE)
                    "#,
                )
                .bind(id)
                .bind(tenant_id)
                .bind(email)
                .bind(name)
                .bind(pw_hash)
                .bind(role)
                .execute(pool)
                .await
                .map_err(|e| format!("Insert demo user {email} failed: {e}"))?;
                id
            }
        };
        if role == "administrator" {
            admin_user_id = Some(uid);
        }
    }

    // Seed default encrypted read-only connection
    let existing_conn: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM db_connections WHERE tenant_id = $1 AND is_active = TRUE LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Query default connection failed: {e}"))?;

    let connection_id = match existing_conn {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            let encrypted_pw = cipher.encrypt(&cfg.readonly_db_password)?;
            sqlx::query(
                r#"
                INSERT INTO db_connections (
                    id, tenant_id, name, adapter_type, host, port, database_name,
                    username, encrypted_password, ssl_mode, allowed_schemas, is_readonly, is_active
                ) VALUES ($1, $2, $3, 'postgresql', $4, $5, $6, $7, $8, $9, $10, TRUE, TRUE)
                "#,
            )
            .bind(id)
            .bind(tenant_id)
            .bind("Primary Enterprise Analytics (Read-Only)")
            .bind(&cfg.pg_host)
            .bind(cfg.pg_port as i32)
            .bind(&cfg.analytics_database)
            .bind(&cfg.readonly_db_user)
            .bind(encrypted_pw)
            .bind(&cfg.pg_ssl_mode)
            .bind(json!(cfg.allowed_schemas))
            .execute(pool)
            .await
            .map_err(|e| format!("Insert default db_connection failed: {e}"))?;
            id
        }
    };

    // Seed LLM provider entries (local_deterministic active by default unless external key configured)
    let provider_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM llm_providers WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Provider count failed: {e}"))?;

    if provider_count == 0 {
        let has_openai = cfg.openai_api_key.is_some() && cfg.openai_model.is_some();
        let has_gemini = cfg.gemini_api_key.is_some() && cfg.gemini_model.is_some();

        sqlx::query(
            r#"
            INSERT INTO llm_providers (id, tenant_id, name, provider_type, model_name, base_url, encrypted_api_key, is_active)
            VALUES ($1, $2, $3, 'local_deterministic', 'schema-semantic-planner-v1', '', '', $4)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind("Local Deterministic Planner (Offline Grounded)")
        .bind(!has_openai && !has_gemini)
        .execute(pool)
        .await
        .map_err(|e| format!("Insert local provider failed: {e}"))?;

        if let Some(api_key) = &cfg.openai_api_key {
            let enc = cipher.encrypt(api_key)?;
            sqlx::query(
                r#"
                INSERT INTO llm_providers (id, tenant_id, name, provider_type, model_name, base_url, encrypted_api_key, is_active)
                VALUES ($1, $2, $3, 'openai', $4, $5, $6, $7)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(tenant_id)
            .bind("OpenAI Provider")
            .bind(cfg.openai_model.clone().unwrap_or_default())
            .bind(&cfg.openai_base_url)
            .bind(enc)
            .bind(has_openai)
            .execute(pool)
            .await
            .map_err(|e| format!("Insert openai provider failed: {e}"))?;
        }

        if let Some(api_key) = &cfg.gemini_api_key {
            let enc = cipher.encrypt(api_key)?;
            sqlx::query(
                r#"
                INSERT INTO llm_providers (id, tenant_id, name, provider_type, model_name, base_url, encrypted_api_key, is_active)
                VALUES ($1, $2, $3, 'gemini', $4, 'https://generativelanguage.googleapis.com/v1beta', $5, $6)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(tenant_id)
            .bind("Google Gemini Provider")
            .bind(cfg.gemini_model.clone().unwrap_or_default())
            .bind(enc)
            .bind(!has_openai && has_gemini)
            .execute(pool)
            .await
            .map_err(|e| format!("Insert gemini provider failed: {e}"))?;
        }
    }

    // Seed initial versioned semantic metrics if empty
    let sem_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM semantic_metrics WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Semantic metric count failed: {e}"))?;

    if sem_count == 0 {
        let initial_metrics = [
            (
                "net_revenue",
                "Recognized Net Revenue",
                "Sum of net_amount across completed or shipped orders after discounts in USD.",
                "SUM(analytics.orders.net_amount)",
                "analytics.orders",
                json!(["order_date", "region_name", "segment", "company_name"]),
                json!(["analytics.orders.status IN ('COMPLETED', 'SHIPPED')"]),
            ),
            (
                "gross_revenue",
                "Gross Revenue (Pre-Discount)",
                "Sum of gross_amount across completed or shipped orders before discounts in USD.",
                "SUM(analytics.orders.gross_amount)",
                "analytics.orders",
                json!(["order_date", "region_name", "segment", "category"]),
                json!(["analytics.orders.status IN ('COMPLETED', 'SHIPPED')"]),
            ),
            (
                "gross_profit",
                "Product Gross Profit",
                "Sum of line_profit across order_items for completed or shipped orders in USD.",
                "SUM(analytics.order_items.line_profit)",
                "analytics.order_items",
                json!(["product_name", "category", "region_name", "order_date"]),
                json!(["analytics.orders.status IN ('COMPLETED', 'SHIPPED')"]),
            ),
            (
                "department_expense",
                "Department Actual Operating Expense",
                "Sum of actual_amount across finance.department_expenses compared against budget_amount.",
                "SUM(finance.department_expenses.actual_amount)",
                "finance.department_expenses",
                json!(["department_name", "expense_month", "category", "cost_center"]),
                json!([]),
            ),
        ];

        for (key, name, def, formula, tbl, dims, filters) in initial_metrics {
            sqlx::query(
                r#"
                INSERT INTO semantic_metrics (
                    id, tenant_id, metric_key, display_name, version, definition, sql_formula,
                    source_table, approved_dimensions, approved_filters, currency_unit,
                    timezone_convention, owner_name, freshness_sla, is_active, updated_by
                ) VALUES ($1, $2, $3, $4, 1, $5, $6, $7, $8, $9, 'USD', 'UTC', 'Enterprise FP&A', 'Daily 02:00 UTC', TRUE, $10)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(tenant_id)
            .bind(key)
            .bind(name)
            .bind(def)
            .bind(formula)
            .bind(tbl)
            .bind(dims)
            .bind(filters)
            .bind(admin_user_id)
            .execute(pool)
            .await
            .map_err(|e| format!("Insert semantic metric {key} failed: {e}"))?;
        }
    }

    Ok((tenant_id, connection_id))
}

#[allow(clippy::too_many_arguments)]
pub async fn record_audit_event(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Option<Uuid>,
    user_email: &str,
    event_type: &str,
    resource_type: &str,
    resource_id: &str,
    decision: &str,
    details: serde_json::Value,
) {
    if let Err(err) = sqlx::query(
        r#"
        INSERT INTO audit_logs (id, tenant_id, user_id, user_email, event_type, resource_type, resource_id, decision, details)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(user_id)
    .bind(user_email)
    .bind(event_type)
    .bind(resource_type)
    .bind(resource_id)
    .bind(decision)
    .bind(details)
    .execute(pool)
    .await
    {
        tracing::warn!(
            tenant_id = %tenant_id,
            event_type = event_type,
            resource_id = resource_id,
            error = %err,
            "Failed to persist audit_logs event"
        );
    }
}

pub async fn load_active_connection_params(
    pool: &PgPool,
    tenant_id: Uuid,
    connection_id: Option<Uuid>,
    cipher: &SecretCipher,
) -> Result<crate::db::ConnectionParams, crate::error::AppError> {
    let row = if let Some(cid) = connection_id {
        sqlx::query(
            r#"
            SELECT id, name, adapter_type, host, port, database_name, username, encrypted_password, ssl_mode, allowed_schemas
            FROM db_connections
            WHERE id = $1 AND tenant_id = $2 AND is_active = TRUE
            "#,
        )
        .bind(cid)
        .bind(tenant_id)
        .fetch_optional(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT id, name, adapter_type, host, port, database_name, username, encrypted_password, ssl_mode, allowed_schemas
            FROM db_connections
            WHERE tenant_id = $1 AND is_active = TRUE
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .bind(tenant_id)
        .fetch_optional(pool)
        .await
    }
    .map_err(|e| crate::error::AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| crate::error::AppError::NotFound("No active database connection configured".to_string()))?;

    let enc_pw: String = row.get("encrypted_password");
    let password = cipher
        .decrypt(&enc_pw)
        .map_err(crate::error::AppError::Internal)?;
    let port: i32 = row.get("port");
    let allowed_schemas: Vec<String> =
        serde_json::from_value(row.get("allowed_schemas")).unwrap_or_default();

    Ok(crate::db::ConnectionParams {
        id: row.get("id"),
        name: row.get("name"),
        adapter_type: row.get("adapter_type"),
        host: row.get("host"),
        port: port as u16,
        database_name: row.get("database_name"),
        username: row.get("username"),
        password,
        ssl_mode: row.get("ssl_mode"),
        allowed_schemas,
    })
}
