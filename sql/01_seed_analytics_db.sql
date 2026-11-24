-- ==============================================================================
-- DATA-TO-ACTION: Source Analytics Database Seed (data_to_action_analytics)
-- Includes:
--   - analytics schema (regions, customers, products, orders + RLS, order_items, monthly_sales_summary view)
--   - finance schema (departments, department_expenses with realistic statistical anomaly)
--   - restricted_hr schema (NOT allowlisted, ZERO grants to dta_readonly)
-- ==============================================================================

\set ON_ERROR_STOP on

REVOKE ALL ON SCHEMA public FROM PUBLIC;

CREATE SCHEMA IF NOT EXISTS analytics;
CREATE SCHEMA IF NOT EXISTS finance;
CREATE SCHEMA IF NOT EXISTS restricted_hr;

-- Ensure clean idempotent re-seed when initializing fresh analytics database
DROP VIEW IF EXISTS analytics.monthly_sales_summary;
DROP TABLE IF EXISTS analytics.order_items CASCADE;
DROP TABLE IF EXISTS analytics.orders CASCADE;
DROP TABLE IF EXISTS finance.department_expenses CASCADE;
DROP TABLE IF EXISTS finance.departments CASCADE;
DROP TABLE IF EXISTS analytics.products CASCADE;
DROP TABLE IF EXISTS analytics.customers CASCADE;
DROP TABLE IF EXISTS analytics.regions CASCADE;
DROP TABLE IF EXISTS restricted_hr.employee_salaries CASCADE;

-- ------------------------------------------------------------------------------
-- Schema: analytics
-- ------------------------------------------------------------------------------
CREATE TABLE analytics.regions (
    region_id SERIAL PRIMARY KEY,
    region_code VARCHAR(16) NOT NULL UNIQUE,
    region_name VARCHAR(100) NOT NULL,
    country VARCHAR(80) NOT NULL,
    currency_code VARCHAR(3) NOT NULL DEFAULT 'USD'
);
COMMENT ON TABLE analytics.regions IS 'Geographic commercial operating regions and reporting currencies.';
COMMENT ON COLUMN analytics.regions.region_code IS 'Standardized region code (NA, EMEA, APAC, LATAM).';
COMMENT ON COLUMN analytics.regions.region_name IS 'Human-readable region title.';

CREATE TABLE analytics.customers (
    customer_id SERIAL PRIMARY KEY,
    customer_code VARCHAR(32) NOT NULL UNIQUE,
    company_name VARCHAR(150) NOT NULL,
    segment VARCHAR(64) NOT NULL,
    region_id INTEGER NOT NULL REFERENCES analytics.regions(region_id),
    tax_id VARCHAR(32) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
COMMENT ON TABLE analytics.customers IS 'B2B enterprise customer master accounts.';
COMMENT ON COLUMN analytics.customers.company_name IS 'Customer legal organization name.';
COMMENT ON COLUMN analytics.customers.segment IS 'Commercial tier: Enterprise, Mid-Market, or Public Sector.';
COMMENT ON COLUMN analytics.customers.tax_id IS 'Sensitive customer tax identifier (protected by RBAC column policy).';

CREATE TABLE analytics.products (
    product_id SERIAL PRIMARY KEY,
    sku VARCHAR(32) NOT NULL UNIQUE,
    product_name VARCHAR(150) NOT NULL,
    category VARCHAR(80) NOT NULL,
    unit_cost NUMERIC(12, 2) NOT NULL CHECK (unit_cost >= 0),
    list_price NUMERIC(12, 2) NOT NULL CHECK (list_price >= 0),
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);
COMMENT ON TABLE analytics.products IS 'Enterprise software, hardware, and advisory product catalog.';
COMMENT ON COLUMN analytics.products.unit_cost IS 'Standard unit cost in USD used for gross profit calculation.';
COMMENT ON COLUMN analytics.products.list_price IS 'Catalog list price in USD prior to customer discounts.';

CREATE TABLE analytics.orders (
    order_id SERIAL PRIMARY KEY,
    order_number VARCHAR(32) NOT NULL UNIQUE,
    customer_id INTEGER NOT NULL REFERENCES analytics.customers(customer_id),
    region_id INTEGER NOT NULL REFERENCES analytics.regions(region_id),
    order_date DATE NOT NULL,
    status VARCHAR(32) NOT NULL CHECK (status IN ('COMPLETED', 'SHIPPED', 'CANCELLED', 'REFUNDED')),
    currency_code VARCHAR(3) NOT NULL DEFAULT 'USD',
    gross_amount NUMERIC(14, 2) NOT NULL,
    discount_amount NUMERIC(14, 2) NOT NULL DEFAULT 0,
    net_amount NUMERIC(14, 2) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
COMMENT ON TABLE analytics.orders IS 'Customer order headers with gross, discount, and net recognized revenue amounts in USD.';
COMMENT ON COLUMN analytics.orders.gross_amount IS 'Order total before discounts in USD.';
COMMENT ON COLUMN analytics.orders.net_amount IS 'Recognized net sales revenue after discounts in USD.';
COMMENT ON COLUMN analytics.orders.order_date IS 'Order booking calendar date (UTC).';

CREATE TABLE analytics.order_items (
    order_item_id SERIAL PRIMARY KEY,
    order_id INTEGER NOT NULL REFERENCES analytics.orders(order_id),
    product_id INTEGER NOT NULL REFERENCES analytics.products(product_id),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_price NUMERIC(12, 2) NOT NULL,
    unit_cost NUMERIC(12, 2) NOT NULL,
    line_gross_revenue NUMERIC(14, 2) NOT NULL,
    line_net_revenue NUMERIC(14, 2) NOT NULL,
    line_profit NUMERIC(14, 2) NOT NULL
);
COMMENT ON TABLE analytics.order_items IS 'Order line items linking orders and products with line-level revenue and gross profit.';
COMMENT ON COLUMN analytics.order_items.line_net_revenue IS 'Line net revenue after order-level discount allocation in USD.';
COMMENT ON COLUMN analytics.order_items.line_profit IS 'Line gross profit calculated as line_net_revenue minus (quantity * unit_cost) in USD.';

CREATE INDEX idx_customers_region_id ON analytics.customers(region_id);
CREATE INDEX idx_orders_customer_id ON analytics.orders(customer_id);
CREATE INDEX idx_orders_region_id ON analytics.orders(region_id);
CREATE INDEX idx_orders_order_date ON analytics.orders(order_date);
CREATE INDEX idx_orders_status ON analytics.orders(status);
CREATE INDEX idx_order_items_order_id ON analytics.order_items(order_id);
CREATE INDEX idx_order_items_product_id ON analytics.order_items(product_id);

CREATE VIEW analytics.monthly_sales_summary AS
SELECT
    DATE_TRUNC('month', o.order_date)::DATE AS sales_month,
    r.region_code,
    r.region_name,
    COUNT(o.order_id) AS order_count,
    SUM(o.gross_amount) AS gross_revenue,
    SUM(o.net_amount) AS net_revenue
FROM analytics.orders o
JOIN analytics.regions r ON r.region_id = o.region_id
WHERE o.status IN ('COMPLETED', 'SHIPPED')
GROUP BY DATE_TRUNC('month', o.order_date)::DATE, r.region_code, r.region_name;
COMMENT ON VIEW analytics.monthly_sales_summary IS 'Pre-aggregated monthly completed/shipped sales summary by commercial region.';

-- Enable PostgreSQL Row-Level Security on analytics.orders for regional tenant isolation
ALTER TABLE analytics.orders ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS orders_region_isolation ON analytics.orders;
CREATE POLICY orders_region_isolation ON analytics.orders
    FOR SELECT
    USING (
        COALESCE(NULLIF(current_setting('app.current_tenant_region', true), ''), 'ALL') = 'ALL'
        OR region_id IN (
            SELECT r.region_id
            FROM analytics.regions r
            WHERE r.region_code = current_setting('app.current_tenant_region', true)
        )
    );

-- ------------------------------------------------------------------------------
-- Schema: finance
-- ------------------------------------------------------------------------------
CREATE TABLE finance.departments (
    department_id SERIAL PRIMARY KEY,
    department_code VARCHAR(24) NOT NULL UNIQUE,
    department_name VARCHAR(100) NOT NULL,
    cost_center VARCHAR(24) NOT NULL,
    region_id INTEGER NOT NULL REFERENCES analytics.regions(region_id)
);
COMMENT ON TABLE finance.departments IS 'Enterprise cost centers and corporate operating departments.';

CREATE TABLE finance.department_expenses (
    expense_id SERIAL PRIMARY KEY,
    department_id INTEGER NOT NULL REFERENCES finance.departments(department_id),
    expense_month DATE NOT NULL,
    category VARCHAR(64) NOT NULL,
    budget_amount NUMERIC(14, 2) NOT NULL,
    actual_amount NUMERIC(14, 2) NOT NULL,
    currency_code VARCHAR(3) NOT NULL DEFAULT 'USD'
);
COMMENT ON TABLE finance.department_expenses IS 'Monthly departmental operating budget vs actual expenditure in USD.';
COMMENT ON COLUMN finance.department_expenses.actual_amount IS 'Actual recorded monthly expense in USD.';
COMMENT ON COLUMN finance.department_expenses.budget_amount IS 'Approved monthly budget in USD.';

CREATE INDEX idx_departments_region_id ON finance.departments(region_id);
CREATE INDEX idx_dept_expenses_dept_month ON finance.department_expenses(department_id, expense_month);

-- ------------------------------------------------------------------------------
-- Schema: restricted_hr (NON-ALLOWLISTED — must NEVER be accessible to dta_readonly)
-- ------------------------------------------------------------------------------
CREATE TABLE restricted_hr.employee_salaries (
    employee_id SERIAL PRIMARY KEY,
    full_name VARCHAR(120) NOT NULL,
    department_name VARCHAR(100) NOT NULL,
    base_salary NUMERIC(12, 2) NOT NULL,
    ssn_last4 VARCHAR(4) NOT NULL
);

INSERT INTO restricted_hr.employee_salaries (full_name, department_name, base_salary, ssn_last4) VALUES
    ('Alice Morgan', 'Engineering', 185000.00, '4821'),
    ('Marcus Chen', 'Finance', 162000.00, '9104'),
    ('Priya Nair', 'Security', 194000.00, '3317'),
    ('Daniel Ortiz', 'Sales', 148000.00, '7752'),
    ('Sofia Lindqvist', 'Executive', 245000.00, '6089');

-- ------------------------------------------------------------------------------
-- Seed Reference Data
-- ------------------------------------------------------------------------------
INSERT INTO analytics.regions (region_code, region_name, country, currency_code) VALUES
    ('NA', 'North America', 'United States', 'USD'),
    ('EMEA', 'Europe & Middle East', 'Germany', 'USD'),
    ('APAC', 'Asia-Pacific', 'Singapore', 'USD'),
    ('LATAM', 'Latin America', 'Brazil', 'USD');

INSERT INTO analytics.customers (customer_code, company_name, segment, region_id, tax_id) VALUES
    ('CUST-1001', 'Apex Financial Holdings', 'Enterprise', 1, 'US-TAX-9418201'),
    ('CUST-1002', 'Northstar Logistics Corp', 'Enterprise', 1, 'US-TAX-7731904'),
    ('CUST-1003', 'Summit Healthcare Systems', 'Public Sector', 1, 'US-TAX-6612049'),
    ('CUST-1004', 'Vanguard Manufacturing Inc', 'Mid-Market', 1, 'US-TAX-5501928'),
    ('CUST-2001', 'Rheinwerk Industrial GmbH', 'Enterprise', 2, 'DE-VAT-8812031'),
    ('CUST-2002', 'Nordic Grid Energy AB', 'Public Sector', 2, 'SE-VAT-3319402'),
    ('CUST-2003', 'Thames Retail Group PLC', 'Mid-Market', 2, 'UK-VAT-4491023'),
    ('CUST-3001', 'Pacific Meridian Bank', 'Enterprise', 3, 'SG-UEN-7102944'),
    ('CUST-3002', 'Kuroda Robotics KK', 'Enterprise', 3, 'JP-CORP-9012388'),
    ('CUST-3003', 'Southern Cross Telecom', 'Mid-Market', 3, 'AU-ABN-6120934'),
    ('CUST-4001', 'Andes Mining & Metals SA', 'Enterprise', 4, 'BR-CNPJ-2201948'),
    ('CUST-4002', 'Atlantica Agro Exports', 'Mid-Market', 4, 'BR-CNPJ-3391820');

INSERT INTO analytics.products (sku, product_name, category, unit_cost, list_price, is_active) VALUES
    ('SKU-CLD-01', 'Aegis Data Lake Core', 'Cloud Platform', 1800.00, 5200.00, TRUE),
    ('SKU-CLD-02', 'Vector Query Accelerator', 'Cloud Platform', 1200.00, 3800.00, TRUE),
    ('SKU-SEC-01', 'ZeroTrust Policy Gateway', 'Security Modules', 950.00, 3400.00, TRUE),
    ('SKU-SEC-02', 'AuditVault Compliance Pack', 'Security Modules', 600.00, 2400.00, TRUE),
    ('SKU-ANL-01', 'Enterprise BI Workbench', 'Analytics Software', 700.00, 2900.00, TRUE),
    ('SKU-ANL-02', 'Predictive Forecast Engine', 'Analytics Software', 1100.00, 4100.00, TRUE),
    ('SKU-HW-01', 'Edge Telemetry Appliance X4', 'Analytics Hardware', 2400.00, 4600.00, TRUE),
    ('SKU-HW-02', 'High-Density Storage Node', 'Analytics Hardware', 3100.00, 5400.00, TRUE),
    ('SKU-SRV-01', 'Architecture Advisory Retainer', 'Advisory Services', 1500.00, 4500.00, TRUE),
    ('SKU-SRV-02', 'Dedicated Migration Sprint', 'Advisory Services', 2200.00, 6000.00, TRUE),
    ('SKU-ANL-03', 'Real-Time Stream Connector', 'Analytics Software', 450.00, 1950.00, TRUE),
    ('SKU-SEC-03', 'KeyGuard HSM Adapter', 'Security Modules', 800.00, 2800.00, TRUE);

-- Generate deterministic orders & order_items from 2025-01-01 to 2026-11-15
-- Deliberate analytical patterns:
--   * NA & APAC grow steadily through 2026-Q3
--   * EMEA experiences a clear decline from 2026-Q1 -> 2026-Q2 -> 2026-Q3
--   * A small percentage of orders are CANCELLED/REFUNDED to verify status filtering
DO $$
DECLARE
    m_idx INTEGER;
    month_start DATE;
    reg_id INTEGER;
    orders_in_month INTEGER;
    j INTEGER;
    cust_ids INTEGER[];
    cust_id INTEGER;
    ord_date DATE;
    ord_status VARCHAR(32);
    discount_pct NUMERIC(5,4);
    qty_1 INTEGER;
    qty_2 INTEGER;
    prod_1 INTEGER;
    prod_2 INTEGER;
    p1_cost NUMERIC(12,2);
    p1_price NUMERIC(12,2);
    p2_cost NUMERIC(12,2);
    p2_price NUMERIC(12,2);
    gross_val NUMERIC(14,2);
    disc_val NUMERIC(14,2);
    net_val NUMERIC(14,2);
    new_order_id INTEGER;
    ord_seq INTEGER := 10000;
    scale_factor NUMERIC(6,3);
BEGIN
    -- 23 months: 2025-01-01 through 2026-11-01
    FOR m_idx IN 0..22 LOOP
        month_start := (DATE '2025-01-01' + (m_idx || ' months')::INTERVAL)::DATE;

        FOR reg_id IN 1..4 LOOP
            SELECT ARRAY_AGG(customer_id ORDER BY customer_id)
            INTO cust_ids
            FROM analytics.customers
            WHERE region_id = reg_id;

            -- Deterministic order counts per region/month
            IF reg_id = 1 THEN
                -- NA: strong growth (10 -> 16 orders/mo)
                orders_in_month := 10 + (m_idx / 4);
                scale_factor := 1.0 + (m_idx * 0.018);
            ELSIF reg_id = 2 THEN
                -- EMEA: grows in 2025, declines significantly across 2026 (m_idx >= 12)
                IF m_idx < 12 THEN
                    orders_in_month := 9;
                    scale_factor := 1.05;
                ELSIF m_idx < 15 THEN
                    -- 2026 Q1
                    orders_in_month := 9;
                    scale_factor := 1.10;
                ELSIF m_idx < 18 THEN
                    -- 2026 Q2 (declining)
                    orders_in_month := 6;
                    scale_factor := 0.85;
                ELSIF m_idx < 21 THEN
                    -- 2026 Q3 (sharp decline)
                    orders_in_month := 4;
                    scale_factor := 0.72;
                ELSE
                    -- 2026 Q4 partial
                    orders_in_month := 3;
                    scale_factor := 0.68;
                END IF;
            ELSIF reg_id = 3 THEN
                -- APAC: steady expansion
                orders_in_month := 7 + (m_idx / 5);
                scale_factor := 0.95 + (m_idx * 0.022);
            ELSE
                -- LATAM: moderate stable growth
                orders_in_month := 4 + (m_idx / 8);
                scale_factor := 0.88 + (m_idx * 0.010);
            END IF;

            FOR j IN 1..orders_in_month LOOP
                ord_seq := ord_seq + 1;
                cust_id := cust_ids[((m_idx + j - 1) % array_length(cust_ids, 1)) + 1];
                ord_date := month_start + (((j * 3 + m_idx) % 26))::INTEGER;
                IF month_start = DATE '2026-11-01' AND ord_date > DATE '2026-11-18' THEN
                    ord_date := DATE '2026-11-01' + ((j + reg_id) % 17);
                END IF;

                IF (ord_seq % 29) = 0 THEN
                    ord_status := 'CANCELLED';
                ELSIF (ord_seq % 43) = 0 THEN
                    ord_status := 'REFUNDED';
                ELSIF (ord_seq % 3) = 0 THEN
                    ord_status := 'SHIPPED';
                ELSE
                    ord_status := 'COMPLETED';
                END IF;

                discount_pct := CASE WHEN (j % 3) = 0 THEN 0.08 ELSE 0.03 END;

                prod_1 := ((m_idx + j + reg_id) % 12) + 1;
                prod_2 := ((m_idx + j * 2 + reg_id + 3) % 12) + 1;
                IF prod_2 = prod_1 THEN
                    prod_2 := (prod_1 % 12) + 1;
                END IF;

                qty_1 := GREATEST(1, ROUND((1 + ((j + m_idx) % 4)) * scale_factor)::INTEGER);
                qty_2 := GREATEST(1, ROUND((1 + ((j * 2) % 3)) * scale_factor)::INTEGER);

                SELECT unit_cost, list_price INTO p1_cost, p1_price FROM analytics.products WHERE product_id = prod_1;
                SELECT unit_cost, list_price INTO p2_cost, p2_price FROM analytics.products WHERE product_id = prod_2;

                gross_val := ROUND((qty_1 * p1_price) + (qty_2 * p2_price), 2);
                disc_val := ROUND(gross_val * discount_pct, 2);
                net_val := gross_val - disc_val;

                INSERT INTO analytics.orders (
                    order_number, customer_id, region_id, order_date, status,
                    currency_code, gross_amount, discount_amount, net_amount, created_at
                ) VALUES (
                    'ORD-' || ord_seq,
                    cust_id,
                    reg_id,
                    ord_date,
                    ord_status,
                    'USD',
                    gross_val,
                    disc_val,
                    net_val,
                    (ord_date::TIMESTAMP AT TIME ZONE 'UTC') + INTERVAL '10 hours'
                ) RETURNING order_id INTO new_order_id;

                INSERT INTO analytics.order_items (
                    order_id, product_id, quantity, unit_price, unit_cost,
                    line_gross_revenue, line_net_revenue, line_profit
                ) VALUES (
                    new_order_id,
                    prod_1,
                    qty_1,
                    p1_price,
                    p1_cost,
                    ROUND(qty_1 * p1_price, 2),
                    ROUND(qty_1 * p1_price * (1 - discount_pct), 2),
                    ROUND(qty_1 * p1_price * (1 - discount_pct) - (qty_1 * p1_cost), 2)
                ), (
                    new_order_id,
                    prod_2,
                    qty_2,
                    p2_price,
                    p2_cost,
                    ROUND(qty_2 * p2_price, 2),
                    ROUND(qty_2 * p2_price * (1 - discount_pct), 2),
                    ROUND(qty_2 * p2_price * (1 - discount_pct) - (qty_2 * p2_cost), 2)
                );
            END LOOP;
        END LOOP;
    END LOOP;
END $$;

-- Seed Finance Departments & Monthly Expenses (with genuine anomaly in IT Infrastructure 2026-08)
INSERT INTO finance.departments (department_code, department_name, cost_center, region_id) VALUES
    ('DEPT-ENG', 'Engineering', 'CC-100', 1),
    ('DEPT-IT', 'IT Infrastructure', 'CC-200', 1),
    ('DEPT-MKT', 'Sales & Marketing', 'CC-300', 2),
    ('DEPT-CS', 'Customer Success', 'CC-400', 3),
    ('DEPT-GA', 'General & Admin', 'CC-500', 1);

DO $$
DECLARE
    m_idx INTEGER;
    exp_month DATE;
    d_id INTEGER;
    base_budget NUMERIC(14,2);
    actual_val NUMERIC(14,2);
    cat_name VARCHAR(64);
BEGIN
    -- 16 months: 2025-07-01 through 2026-10-01
    FOR m_idx IN 0..15 LOOP
        exp_month := (DATE '2025-07-01' + (m_idx || ' months')::INTERVAL)::DATE;
        FOR d_id IN 1..5 LOOP
            SELECT CASE d_id
                WHEN 1 THEN 95000.00
                WHEN 2 THEN 58000.00
                WHEN 3 THEN 72000.00
                WHEN 4 THEN 44000.00
                ELSE 38000.00
            END INTO base_budget;

            SELECT CASE d_id
                WHEN 1 THEN 'Cloud Compute & Tooling'
                WHEN 2 THEN 'Infrastructure & Licenses'
                WHEN 3 THEN 'Demand Generation'
                WHEN 4 THEN 'Customer Operations'
                ELSE 'Compliance & Facilities'
            END INTO cat_name;

            actual_val := ROUND(base_budget * (0.94 + ((m_idx * 7 + d_id * 3) % 13) * 0.01), 2);

            -- Deliberate unusual expense anomalies for verification:
            -- 1) IT Infrastructure spike in 2026-08 (3.42x budget)
            IF d_id = 2 AND exp_month = DATE '2026-08-01' THEN
                actual_val := 198400.00;
            END IF;
            -- 2) Sales & Marketing conference overage in 2026-09 (1.95x budget)
            IF d_id = 3 AND exp_month = DATE '2026-09-01' THEN
                actual_val := 140400.00;
            END IF;

            INSERT INTO finance.department_expenses (
                department_id, expense_month, category, budget_amount, actual_amount, currency_code
            ) VALUES (
                d_id, exp_month, cat_name, base_budget, actual_val, 'USD'
            );
        END LOOP;
    END LOOP;
END $$;

-- ------------------------------------------------------------------------------
-- Strict Least-Privilege Grants for dta_readonly
-- ------------------------------------------------------------------------------
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA restricted_hr FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA restricted_hr FROM PUBLIC;

REVOKE ALL ON SCHEMA analytics FROM PUBLIC;
REVOKE ALL ON SCHEMA finance FROM PUBLIC;

GRANT USAGE ON SCHEMA analytics TO dta_readonly;
GRANT USAGE ON SCHEMA finance TO dta_readonly;

GRANT SELECT ON ALL TABLES IN SCHEMA analytics TO dta_readonly;
GRANT SELECT ON ALL TABLES IN SCHEMA finance TO dta_readonly;

ALTER DEFAULT PRIVILEGES IN SCHEMA analytics GRANT SELECT ON TABLES TO dta_readonly;
ALTER DEFAULT PRIVILEGES IN SCHEMA finance GRANT SELECT ON TABLES TO dta_readonly;
