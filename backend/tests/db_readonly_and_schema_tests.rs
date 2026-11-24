use uuid::Uuid;

use dta_backend::{
    auth::RbacPolicy,
    config::AppConfig,
    db::{ConnectionParams, DatabaseAdapter, ExecutionContext, PostgresAdapter},
    error::AppError,
    schema::linker::{filter_catalog_for_policy, link_schema_for_question},
    semantic::SemanticMetric,
};

fn load_ro_params() -> ConnectionParams {
    let cfg = AppConfig::from_env().expect("AppConfig from .env");
    ConnectionParams {
        id: Uuid::new_v4(),
        name: "test-ro".to_string(),
        adapter_type: "postgresql".to_string(),
        host: cfg.pg_host,
        port: cfg.pg_port,
        database_name: cfg.analytics_database,
        username: cfg.readonly_db_user,
        password: cfg.readonly_db_password,
        ssl_mode: cfg.pg_ssl_mode,
        allowed_schemas: cfg.allowed_schemas,
    }
}

#[tokio::test]
async fn postgres_readonly_role_and_transaction_enforced_at_engine_level() {
    let params = load_ro_params();
    let adapter = PostgresAdapter::connect(params.clone())
        .await
        .expect("connect as dta_readonly");

    let health = adapter.health_check().await.expect("health_check");
    assert!(health.contains("dta_readonly"));

    let ctx = ExecutionContext {
        timeout_ms: 5000,
        row_limit: 50,
        max_result_bytes: 1_048_576,
        tenant_region: "ALL".to_string(),
    };

    // Valid SELECT succeeds
    let ok_res = adapter
        .execute_readonly_query("SELECT region_code, region_name FROM analytics.regions ORDER BY region_id", &ctx)
        .await
        .expect("select succeeds");
    assert_eq!(ok_res.rows_returned, 4);

    // Transaction read-only is ON inside execution
    let ro_flag = adapter
        .execute_readonly_query("SHOW transaction_read_only", &ctx)
        .await
        .expect("show tx read only");
    assert_eq!(ro_flag.rows[0][0].as_str(), Some("on"));

    // Direct PostgreSQL permission / read-only rejections (bypassing AST validator intentionally to prove defense-in-depth!)
    let err_insert = adapter
        .execute_readonly_query(
            "INSERT INTO analytics.regions (region_code, region_name, country) VALUES ('ZZ', 'Evil', 'None')",
            &ctx,
        )
        .await
        .expect_err("INSERT must fail at PG level");
    assert!(matches!(err_insert, AppError::DatabaseError(_)));

    let err_ddl = adapter
        .execute_readonly_query("CREATE TABLE analytics.evil_tbl (id INT)", &ctx)
        .await
        .expect_err("CREATE TABLE must fail at PG level");
    assert!(matches!(err_ddl, AppError::DatabaseError(_)));

    let err_hr = adapter
        .execute_readonly_query("SELECT * FROM restricted_hr.employee_salaries", &ctx)
        .await
        .expect_err("restricted_hr must fail at PG permission boundary");
    assert!(matches!(err_hr, AppError::DatabaseError(ref m) if m.to_lowercase().contains("permission denied")));
}

#[tokio::test]
async fn postgres_timeout_truncation_byte_limit_and_rls_work() {
    let params = load_ro_params();
    let adapter = PostgresAdapter::connect(params).await.expect("connect");

    // 1. Statement timeout enforcement
    let fast_timeout_ctx = ExecutionContext {
        timeout_ms: 250,
        row_limit: 50,
        max_result_bytes: 1_048_576,
        tenant_region: "ALL".to_string(),
    };
    let timeout_err = adapter
        .execute_readonly_query(
            "WITH RECURSIVE spin(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM spin) SELECT COUNT(*) FROM spin",
            &fast_timeout_ctx,
        )
        .await
        .expect_err("unbounded CTE must hit statement timeout");
    assert!(
        matches!(timeout_err, AppError::Timeout),
        "Expected AppError::Timeout, got: {timeout_err:?}"
    );

    // 2. Row limit truncation
    let small_row_ctx = ExecutionContext {
        timeout_ms: 5000,
        row_limit: 3,
        max_result_bytes: 1_048_576,
        tenant_region: "ALL".to_string(),
    };
    let trunc_res = adapter
        .execute_readonly_query("SELECT order_id FROM analytics.orders ORDER BY order_id LIMIT 10", &small_row_ctx)
        .await
        .expect("query succeeds");
    assert_eq!(trunc_res.rows_returned, 3);
    assert!(trunc_res.is_truncated);

    // 3. Max bytes truncation
    let tiny_byte_ctx = ExecutionContext {
        timeout_ms: 5000,
        row_limit: 500,
        max_result_bytes: 120,
        tenant_region: "ALL".to_string(),
    };
    let byte_trunc = adapter
        .execute_readonly_query(
            "SELECT order_number, status, net_amount FROM analytics.orders ORDER BY order_id LIMIT 50",
            &tiny_byte_ctx,
        )
        .await
        .expect("query succeeds");
    assert!(byte_trunc.is_truncated);
    assert!(byte_trunc.bytes_read <= 120);

    // 4. PostgreSQL Row-Level Security (RLS) restricts analytics.orders to EMEA when app.current_tenant_region = EMEA
    let emea_ctx = ExecutionContext {
        timeout_ms: 5000,
        row_limit: 50,
        max_result_bytes: 1_048_576,
        tenant_region: "EMEA".to_string(),
    };
    let rls_res = adapter
        .execute_readonly_query(
            "SELECT DISTINCT r.region_code FROM analytics.orders o JOIN analytics.regions r ON r.region_id = o.region_id",
            &emea_ctx,
        )
        .await
        .expect("RLS query succeeds");
    assert_eq!(rls_res.rows_returned, 1);
    assert_eq!(rls_res.rows[0][0].as_str(), Some("EMEA"));

    // 5. Connection timeout & credential redaction check on closed port
    let mut bad_params = load_ro_params();
    bad_params.port = 59999;
    bad_params.password = "SUPER_SECRET_PASSWORD_123".to_string();
    let conn_err = match PostgresAdapter::connect(bad_params).await {
        Ok(_) => panic!("closed port should fail"),
        Err(e) => e,
    };
    assert!(!conn_err.to_string().contains("SUPER_SECRET_PASSWORD_123"));
}

#[tokio::test]
async fn live_schema_discovery_and_linking_respects_fks_and_ambiguity() {
    let params = load_ro_params();
    let adapter = PostgresAdapter::connect(params.clone())
        .await
        .expect("connect");

    let catalog = adapter
        .discover_schema(params.id, &params.allowed_schemas)
        .await
        .expect("discover schema");

    // Verify analytics & finance discovered, restricted_hr excluded
    assert!(catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "orders"));
    assert!(catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "monthly_sales_summary" && t.table_type == "VIEW"));
    assert!(catalog.tables.iter().any(|t| t.schema == "finance" && t.name == "department_expenses"));
    assert!(!catalog.tables.iter().any(|t| t.schema == "restricted_hr"));
    assert!(!catalog.relationships.is_empty());

    // RBAC filtering hides tax_id from analyst
    let policy = RbacPolicy {
        role: "analyst".to_string(),
        allowed_databases: vec!["data_to_action_analytics".to_string()],
        allowed_schemas: vec!["analytics".to_string(), "finance".to_string()],
        allowed_tables: vec![],
        denied_columns: vec!["analytics.customers.tax_id".to_string()],
        allow_export: true,
        allow_schema_refresh: false,
        allow_admin_config: false,
        row_limit: 500,
        region_filter: "ALL".to_string(),
    };
    let filtered = filter_catalog_for_policy(&catalog, &policy);
    let cust_tbl = filtered
        .tables
        .iter()
        .find(|t| t.name == "customers")
        .expect("customers table");
    assert!(!cust_tbl.columns.iter().any(|c| c.name == "tax_id"));

    // Ambiguous question triggers ambiguity detection when multiple revenue metrics exist
    let semantics = vec![
        SemanticMetric {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            metric_key: "net_revenue".to_string(),
            display_name: "Recognized Net Revenue".to_string(),
            version: 1,
            definition: "Net after discount".to_string(),
            sql_formula: "SUM(analytics.orders.net_amount)".to_string(),
            source_table: "analytics.orders".to_string(),
            approved_dimensions: vec![],
            approved_filters: vec![],
            currency_unit: "USD".to_string(),
            timezone_convention: "UTC".to_string(),
            owner_name: "FP&A".to_string(),
            freshness_sla: "Daily".to_string(),
            is_active: true,
            updated_at: chrono::Utc::now(),
        },
        SemanticMetric {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            metric_key: "gross_revenue".to_string(),
            display_name: "Gross Revenue".to_string(),
            version: 1,
            definition: "Pre-discount gross".to_string(),
            sql_formula: "SUM(analytics.orders.gross_amount)".to_string(),
            source_table: "analytics.orders".to_string(),
            approved_dimensions: vec![],
            approved_filters: vec![],
            currency_unit: "USD".to_string(),
            timezone_convention: "UTC".to_string(),
            owner_name: "FP&A".to_string(),
            freshness_sla: "Daily".to_string(),
            is_active: true,
            updated_at: chrono::Utc::now(),
        },
    ];

    let ambiguous = link_schema_for_question("What was the revenue last quarter?", &filtered, &semantics, true);
    assert!(!ambiguous.ambiguities.is_empty());

    let missing = link_schema_for_question("What is our customer churn rate and NPS?", &filtered, &semantics, true);
    assert!(missing.missing_concepts.len() >= 2);
}
