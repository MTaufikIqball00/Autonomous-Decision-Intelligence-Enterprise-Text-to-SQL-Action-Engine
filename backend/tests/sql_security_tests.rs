use chrono::Utc;
use uuid::Uuid;

use dta_backend::{
    auth::RbacPolicy,
    schema::{ColumnCatalog, ForeignKeyMeta, SchemaCatalog, TableMeta},
    sql_validator::validate_sql,
};

fn mock_catalog() -> SchemaCatalog {
    SchemaCatalog {
        connection_id: Uuid::new_v4(),
        database_name: "data_to_action_analytics".to_string(),
        allowed_schemas: vec!["analytics".to_string(), "finance".to_string()],
        version: 1,
        refreshed_at: Utc::now(),
        tables: vec![
            TableMeta {
                schema: "analytics".to_string(),
                name: "orders".to_string(),
                table_type: "BASE TABLE".to_string(),
                description: None,
                columns: vec![
                    col("order_id", "integer", true),
                    col("customer_id", "integer", false),
                    col("region_id", "integer", false),
                    col("order_date", "date", false),
                    col("status", "character varying", false),
                    col("net_amount", "numeric", false),
                    col("gross_amount", "numeric", false),
                ],
                primary_keys: vec!["order_id".to_string()],
                foreign_keys: vec![],
                indexes: vec![],
            },
            TableMeta {
                schema: "analytics".to_string(),
                name: "customers".to_string(),
                table_type: "BASE TABLE".to_string(),
                description: None,
                columns: vec![
                    col("customer_id", "integer", true),
                    col("company_name", "character varying", false),
                    col("region_id", "integer", false),
                    col("tax_id", "character varying", false),
                ],
                primary_keys: vec!["customer_id".to_string()],
                foreign_keys: vec![],
                indexes: vec![],
            },
            TableMeta {
                schema: "analytics".to_string(),
                name: "regions".to_string(),
                table_type: "BASE TABLE".to_string(),
                description: None,
                columns: vec![
                    col("region_id", "integer", true),
                    col("region_code", "character varying", false),
                    col("region_name", "character varying", false),
                ],
                primary_keys: vec!["region_id".to_string()],
                foreign_keys: vec![],
                indexes: vec![],
            },
            TableMeta {
                schema: "finance".to_string(),
                name: "department_expenses".to_string(),
                table_type: "BASE TABLE".to_string(),
                description: None,
                columns: vec![
                    col("expense_id", "integer", true),
                    col("department_id", "integer", false),
                    col("expense_month", "date", false),
                    col("actual_amount", "numeric", false),
                ],
                primary_keys: vec!["expense_id".to_string()],
                foreign_keys: vec![],
                indexes: vec![],
            },
        ],
        relationships: vec![ForeignKeyMeta {
            constraint_name: "fk_orders_customer".to_string(),
            from_schema: "analytics".to_string(),
            from_table: "orders".to_string(),
            from_column: "customer_id".to_string(),
            to_schema: "analytics".to_string(),
            to_table: "customers".to_string(),
            to_column: "customer_id".to_string(),
            source: "foreign_key".to_string(),
        }],
    }
}

fn col(name: &str, data_type: &str, pk: bool) -> ColumnCatalog {
    ColumnCatalog {
        name: name.to_string(),
        data_type: data_type.to_string(),
        is_nullable: false,
        is_primary_key: pk,
        description: None,
    }
}

fn analyst_policy() -> RbacPolicy {
    RbacPolicy {
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
    }
}

fn check(sql: &str, policy: &RbacPolicy) -> dta_backend::sql_validator::ValidationReport {
    let cat = mock_catalog();
    let conn_schemas = vec!["analytics".to_string(), "finance".to_string()];
    validate_sql(sql, &cat, policy, &conn_schemas, None)
}

#[test]
fn valid_select_variants_are_accepted_and_row_capped() {
    let p = analyst_policy();
    let valid_queries = [
        "SELECT order_id, net_amount FROM analytics.orders",
        "SELECT order_id, net_amount FROM analytics.orders WHERE status = 'COMPLETED'",
        "SELECT r.region_name, SUM(o.net_amount) AS rev FROM analytics.orders o JOIN analytics.regions r ON r.region_id = o.region_id GROUP BY r.region_name ORDER BY rev DESC",
        "WITH regional AS (SELECT region_id, SUM(net_amount) AS total_rev FROM analytics.orders GROUP BY region_id) SELECT r.region_code, regional.total_rev FROM regional JOIN analytics.regions r ON r.region_id = regional.region_id",
        "SELECT company_name FROM analytics.customers WHERE customer_id IN (SELECT customer_id FROM analytics.orders WHERE net_amount > 1000)",
        "SELECT COUNT(*) AS cnt, ROUND(AVG(net_amount), 2) AS avg_rev FROM analytics.orders",
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 5) SELECT n FROM seq LIMIT 5",
    ];

    for sql in valid_queries {
        let rep = check(sql, &p);
        assert!(
            rep.is_valid,
            "Expected valid SQL to pass: {sql}\nReason: {:?}",
            rep.rejection_reason
        );
        let exec_sql = rep.executable_sql.expect("executable sql");
        assert!(
            exec_sql.contains("LIMIT 501"),
            "Expected wrapped row limit +1 on: {exec_sql}"
        );
    }
}

#[test]
fn dangerous_non_readonly_and_injected_statements_are_rejected() {
    let p = analyst_policy();
    let forbidden_queries = [
        ("INSERT", "INSERT INTO analytics.regions (region_id, region_code, region_name) VALUES (9, 'XX', 'X')"),
        ("UPDATE", "UPDATE analytics.orders SET net_amount = 0 WHERE order_id = 1"),
        ("DELETE", "DELETE FROM analytics.orders WHERE order_id = 1"),
        ("DROP", "DROP TABLE analytics.orders"),
        ("ALTER", "ALTER TABLE analytics.orders ADD COLUMN evil TEXT"),
        ("CREATE", "CREATE TABLE analytics.evil (id INT)"),
        ("TRUNCATE", "TRUNCATE TABLE analytics.orders"),
        ("GRANT", "GRANT ALL ON analytics.orders TO PUBLIC"),
        ("REVOKE", "REVOKE SELECT ON analytics.orders FROM dta_readonly"),
        ("COPY", "COPY analytics.orders TO '/tmp/leak.csv'"),
        ("CALL", "CALL some_proc()"),
        ("DO", "DO $$ BEGIN RAISE NOTICE 'x'; END $$"),
        ("SET", "SET default_transaction_read_only = off"),
        ("RESET", "RESET statement_timeout"),
        ("BEGIN", "BEGIN; SELECT 1"),
        ("MULTI_STMT", "SELECT order_id FROM analytics.orders; DROP TABLE analytics.orders"),
        ("LINE_COMMENT", "SELECT order_id FROM analytics.orders -- comment"),
        ("BLOCK_COMMENT", "SELECT /* evil */ order_id FROM analytics.orders"),
        ("SELECT_INTO", "SELECT order_id INTO analytics.evil_copy FROM analytics.orders"),
        ("LOCKING_CLAUSE", "SELECT order_id FROM analytics.orders FOR UPDATE"),
        ("CROSS_JOIN", "SELECT o.order_id FROM analytics.orders o CROSS JOIN analytics.regions r"),
        ("UNBOUNDED_RECURSIVE", "WITH RECURSIVE loop(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM loop) SELECT n FROM loop"),
        ("INVALID_SQL", "SELECT FROM WHERE ???"),
    ];

    for (label, sql) in forbidden_queries {
        let rep = check(sql, &p);
        assert!(
            !rep.is_valid,
            "Expected {label} SQL to be REJECTED, but it passed: {sql}"
        );
    }
}

#[test]
fn unauthorized_schemas_tables_columns_and_functions_are_rejected() {
    let p = analyst_policy();

    // Unauthorized schema (restricted_hr and pg_catalog)
    assert!(!check("SELECT * FROM restricted_hr.employee_salaries", &p).is_valid);
    assert!(!check("SELECT * FROM pg_catalog.pg_roles", &p).is_valid);

    // Unauthorized table / missing table
    assert!(!check("SELECT id FROM analytics.nonexistent_table", &p).is_valid);

    // Hallucinated / non-existent column
    assert!(!check("SELECT nonexistent_col FROM analytics.orders", &p).is_valid);

    // RBAC-denied sensitive column (explicit & SELECT * wildcard leak!)
    let denied_col = check("SELECT company_name, tax_id FROM analytics.customers", &p);
    assert!(!denied_col.is_valid);
    assert!(denied_col
        .rejection_reason
        .unwrap_or_default()
        .contains("tax_id"));

    let wildcard_leak = check("SELECT * FROM analytics.customers", &p);
    assert!(!wildcard_leak.is_valid);

    let qualified_wildcard_leak = check("SELECT c.* FROM analytics.customers c", &p);
    assert!(!qualified_wildcard_leak.is_valid);

    // Dangerous and unlisted functions
    assert!(!check("SELECT pg_read_file('/etc/passwd') FROM analytics.orders", &p).is_valid);
    assert!(!check("SELECT pg_sleep(5) FROM analytics.orders", &p).is_valid);
    assert!(!check("SELECT dblink('host=evil', 'SELECT 1') FROM analytics.orders", &p).is_valid);
    assert!(!check("SELECT set_config('app.x', '1', false) FROM analytics.orders", &p).is_valid);
    assert!(!check("SELECT pg_catalog.version() FROM analytics.orders", &p).is_valid);
    assert!(!check("SELECT random() FROM analytics.orders", &p).is_valid);

    // Business user cannot query finance schema
    let mut biz = analyst_policy();
    biz.role = "business_user".to_string();
    biz.allowed_schemas = vec!["analytics".to_string()];
    assert!(!check("SELECT actual_amount FROM finance.department_expenses", &biz).is_valid);
}
