-- ==============================================================================
-- DATA-TO-ACTION: Application Metadata Database Schema (data_to_action_app)
-- Logically and physically separated from source analytics data.
-- ==============================================================================

CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY,
    slug VARCHAR(64) NOT NULL UNIQUE,
    name VARCHAR(150) NOT NULL,
    allowed_region_code VARCHAR(16) NOT NULL DEFAULT 'ALL',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    email VARCHAR(160) NOT NULL UNIQUE,
    display_name VARCHAR(120) NOT NULL,
    password_hash TEXT NOT NULL,
    role VARCHAR(32) NOT NULL CHECK (role IN ('administrator', 'data_engineer', 'analyst', 'business_user', 'auditor')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS rbac_policies (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    role VARCHAR(32) NOT NULL,
    allowed_databases JSONB NOT NULL,
    allowed_schemas JSONB NOT NULL,
    allowed_tables JSONB NOT NULL,
    denied_columns JSONB NOT NULL,
    allow_export BOOLEAN NOT NULL DEFAULT FALSE,
    allow_schema_refresh BOOLEAN NOT NULL DEFAULT FALSE,
    allow_admin_config BOOLEAN NOT NULL DEFAULT FALSE,
    row_limit INTEGER NOT NULL DEFAULT 500,
    region_filter VARCHAR(16) NOT NULL DEFAULT 'ALL',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, role)
);

CREATE TABLE IF NOT EXISTS db_connections (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(120) NOT NULL,
    adapter_type VARCHAR(32) NOT NULL CHECK (adapter_type IN ('postgresql', 'bigquery')),
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL,
    database_name VARCHAR(128) NOT NULL,
    username VARCHAR(128) NOT NULL,
    encrypted_password TEXT NOT NULL,
    ssl_mode VARCHAR(32) NOT NULL DEFAULT 'prefer',
    allowed_schemas JSONB NOT NULL,
    is_readonly BOOLEAN NOT NULL DEFAULT TRUE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, name)
);

CREATE TABLE IF NOT EXISTS schema_snapshots (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    connection_id UUID NOT NULL REFERENCES db_connections(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    catalog_json JSONB NOT NULL,
    table_count INTEGER NOT NULL,
    column_count INTEGER NOT NULL,
    refreshed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (connection_id, version)
);

CREATE TABLE IF NOT EXISTS manual_relationships (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    connection_id UUID NOT NULL REFERENCES db_connections(id) ON DELETE CASCADE,
    from_schema VARCHAR(64) NOT NULL,
    from_table VARCHAR(128) NOT NULL,
    from_column VARCHAR(128) NOT NULL,
    to_schema VARCHAR(64) NOT NULL,
    to_table VARCHAR(128) NOT NULL,
    to_column VARCHAR(128) NOT NULL,
    relationship_type VARCHAR(32) NOT NULL DEFAULT 'many_to_one',
    description TEXT NOT NULL DEFAULT '',
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS semantic_metrics (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    metric_key VARCHAR(80) NOT NULL,
    display_name VARCHAR(120) NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    definition TEXT NOT NULL,
    sql_formula TEXT NOT NULL,
    source_table VARCHAR(160) NOT NULL,
    approved_dimensions JSONB NOT NULL DEFAULT '[]'::jsonb,
    approved_filters JSONB NOT NULL DEFAULT '[]'::jsonb,
    currency_unit VARCHAR(16) NOT NULL DEFAULT 'USD',
    timezone_convention VARCHAR(64) NOT NULL DEFAULT 'UTC',
    owner_name VARCHAR(120) NOT NULL DEFAULT 'Finance Analytics',
    freshness_sla VARCHAR(64) NOT NULL DEFAULT 'Daily 02:00 UTC',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    updated_by UUID REFERENCES users(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, metric_key, version)
);

CREATE TABLE IF NOT EXISTS llm_providers (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(80) NOT NULL,
    provider_type VARCHAR(32) NOT NULL CHECK (provider_type IN ('local_deterministic', 'openai', 'gemini')),
    model_name VARCHAR(128) NOT NULL DEFAULT '',
    base_url VARCHAR(255) NOT NULL DEFAULT '',
    encrypted_api_key TEXT NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, name)
);

CREATE TABLE IF NOT EXISTS query_executions (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    connection_id UUID NOT NULL REFERENCES db_connections(id),
    question TEXT NOT NULL,
    provider_used VARCHAR(64) NOT NULL,
    model_used VARCHAR(128) NOT NULL,
    generated_sql TEXT NOT NULL DEFAULT '',
    validated_sql TEXT NOT NULL DEFAULT '',
    schema_version INTEGER NOT NULL DEFAULT 0,
    validation_status VARCHAR(48) NOT NULL,
    validation_details JSONB NOT NULL DEFAULT '{}'::jsonb,
    execution_status VARCHAR(48) NOT NULL,
    execution_duration_ms INTEGER NOT NULL DEFAULT 0,
    rows_returned INTEGER NOT NULL DEFAULT 0,
    is_truncated BOOLEAN NOT NULL DEFAULT FALSE,
    correction_attempts JSONB NOT NULL DEFAULT '[]'::jsonb,
    result_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    chart_spec JSONB NOT NULL DEFAULT '{}'::jsonb,
    insight_report JSONB NOT NULL DEFAULT '{}'::jsonb,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS saved_queries (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    query_execution_id UUID REFERENCES query_executions(id) ON DELETE SET NULL,
    title VARCHAR(160) NOT NULL,
    question TEXT NOT NULL,
    sql_text TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    is_shared BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS dashboards (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    title VARCHAR(160) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    widgets_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    is_shared BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id),
    user_email VARCHAR(160) NOT NULL DEFAULT '',
    event_type VARCHAR(64) NOT NULL,
    resource_type VARCHAR(64) NOT NULL DEFAULT '',
    resource_id VARCHAR(128) NOT NULL DEFAULT '',
    decision VARCHAR(32) NOT NULL DEFAULT 'info',
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_query_executions_tenant_created ON query_executions(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_tenant_created ON audit_logs(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_semantic_metrics_tenant_active ON semantic_metrics(tenant_id, metric_key, is_active);
