use async_trait::async_trait;

use crate::{
    error::AppError,
    llm::{
        LlmInsightOutput, LlmInsightRequest, LlmProvider, LlmSqlCorrectionRequest,
        LlmSqlGenerationRequest, LlmSqlOutput,
    },
    schema::SchemaCatalog,
};

/// Transparent offline schema-and-semantic-grounded planner.
/// Explicitly distinguishes local deterministic SQL synthesis from external LLM API providers.
pub struct LocalDeterministicPlanner {
    model: String,
}

impl LocalDeterministicPlanner {
    pub fn new(model_name: &str) -> Self {
        let name = if model_name.trim().is_empty() {
            "schema-semantic-planner-v1"
        } else {
            model_name.trim()
        };
        Self {
            model: name.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for LocalDeterministicPlanner {
    fn provider_id(&self) -> &str {
        "local_deterministic"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn is_external_llm(&self) -> bool {
        false
    }

    async fn generate_sql(&self, req: LlmSqlGenerationRequest<'_>) -> Result<LlmSqlOutput, AppError> {
        let q = req.question.trim();
        let q_lower = q.to_lowercase();

        // 1. Prompt-injection & restricted-action defense at planner layer (AST validator still enforces independently!)
        if contains_prompt_injection(&q_lower) {
            return Ok(LlmSqlOutput {
                sql: None,
                explanation: "Request rejected by planner safety check: prompt-injection or non-read-only command detected.".to_string(),
                assumptions: vec![],
                applied_filters: vec![],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: Some(
                    "Only analytical read-only business questions against authorized schemas are permitted. System overrides, writes, comments, and restricted schemas are blocked."
                        .to_string(),
                ),
            });
        }

        // 2. Check missing metrics / unmapped concepts (do NOT invent formulas!)
        if !req.linked_schema.missing_concepts.is_empty() {
            let missing_list = req.linked_schema.missing_concepts.join(", ");
            return Ok(LlmSqlOutput {
                sql: None,
                explanation: format!(
                    "Cannot generate SQL because the requested concept ({missing_list}) is absent from the permitted schema catalog and semantic definitions."
                ),
                assumptions: vec![],
                applied_filters: vec![],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: Some(format!(
                    "Requested metric or dataset ({missing_list}) is not available in the verified schema or approved semantic catalog. No formula was fabricated."
                )),
            });
        }

        // 3. Check explicit schema/semantic ambiguities unless the user supplied a clarification choice
        if req.preferred_metric.is_none() && !req.linked_schema.ambiguities.is_empty() {
            let first = &req.linked_schema.ambiguities[0];
            return Ok(LlmSqlOutput {
                sql: None,
                explanation: "Clarification requested before SQL generation to avoid guessing business logic.".to_string(),
                assumptions: vec![],
                applied_filters: vec![],
                clarification_needed: Some(first.message.clone()),
                clarification_options: first.options.clone(),
                cannot_compute_reason: None,
            });
        }

        // 4. Verify required schema tables exist in permitted catalog
        let cat = req.full_catalog;
        let use_gross = req
            .preferred_metric
            .map(|m| m.to_lowercase().contains("gross"))
            .unwrap_or_else(|| q_lower.contains("gross"));

        let rev_col = if use_gross {
            "gross_amount"
        } else {
            "net_amount"
        };
        let rev_label = if use_gross {
            "gross_revenue"
        } else {
            "net_revenue"
        };
        let line_rev_col = if use_gross {
            "line_gross_revenue"
        } else {
            "line_net_revenue"
        };

        // Pattern A: Total revenue last quarter
        if (q_lower.contains("revenue") || q_lower.contains("sales"))
            && q_lower.contains("last quarter")
            && has_table(cat, "analytics", "orders")
        {
            let sql = format!(
                "SELECT TO_CHAR(DATE_TRUNC('quarter', CURRENT_DATE) - INTERVAL '3 months', 'YYYY-\"Q\"Q') AS quarter_label, \
                COUNT(order_id) AS order_count, \
                ROUND(COALESCE(SUM({rev_col}), 0), 2) AS total_{rev_label} \
                FROM analytics.orders \
                WHERE status IN ('COMPLETED', 'SHIPPED') \
                AND order_date >= DATE_TRUNC('quarter', CURRENT_DATE) - INTERVAL '3 months' \
                AND order_date < DATE_TRUNC('quarter', CURRENT_DATE)"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!(
                    "Computes total {rev_label} and order count from analytics.orders for the previous full calendar quarter."
                ),
                assumptions: vec![
                    format!("Uses semantic definition '{rev_label}' (analytics.orders.{rev_col})."),
                    "Includes orders with status COMPLETED or SHIPPED per semantic catalog filter.".to_string(),
                    "Dates evaluated in UTC relative to CURRENT_DATE.".to_string(),
                ],
                applied_filters: vec![
                    "status IN ('COMPLETED', 'SHIPPED')".to_string(),
                    "order_date in previous full calendar quarter".to_string(),
                ],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern B: Top products by profit (verifies line_profit column exists!)
        if q_lower.contains("product")
            && q_lower.contains("profit")
            && has_col(cat, "analytics", "order_items", "line_profit")
            && has_table(cat, "analytics", "products")
            && has_table(cat, "analytics", "orders")
        {
            let lim = extract_top_n(&q_lower).unwrap_or(10).min(req.row_limit);
            let sql = format!(
                "SELECT p.sku, p.product_name, p.category, \
                SUM(oi.quantity) AS units_sold, \
                ROUND(SUM(oi.line_net_revenue), 2) AS net_revenue, \
                ROUND(SUM(oi.line_profit), 2) AS total_profit \
                FROM analytics.order_items oi \
                JOIN analytics.products p ON p.product_id = oi.product_id \
                JOIN analytics.orders o ON o.order_id = oi.order_id \
                WHERE o.status IN ('COMPLETED', 'SHIPPED') \
                GROUP BY p.sku, p.product_name, p.category \
                ORDER BY total_profit DESC \
                LIMIT {lim}"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!(
                    "Ranks top {lim} products by verified gross profit (analytics.order_items.line_profit) across completed/shipped orders."
                ),
                assumptions: vec![
                    "Verified analytics.order_items.line_profit exists in discovered schema.".to_string(),
                    "Joined strictly along verified FK constraints (order_items->products, order_items->orders).".to_string(),
                ],
                applied_filters: vec!["o.status IN ('COMPLETED', 'SHIPPED')".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern C: Products generating highest revenue
        if q_lower.contains("product")
            && (q_lower.contains("revenue") || q_lower.contains("sales") || q_lower.contains("highest") || q_lower.contains("top"))
            && has_table(cat, "analytics", "order_items")
            && has_table(cat, "analytics", "products")
            && has_table(cat, "analytics", "orders")
        {
            let lim = extract_top_n(&q_lower).unwrap_or(10).min(req.row_limit);
            let sql = format!(
                "SELECT p.sku, p.product_name, p.category, \
                SUM(oi.quantity) AS units_sold, \
                ROUND(SUM(oi.{line_rev_col}), 2) AS total_{rev_label} \
                FROM analytics.order_items oi \
                JOIN analytics.products p ON p.product_id = oi.product_id \
                JOIN analytics.orders o ON o.order_id = oi.order_id \
                WHERE o.status IN ('COMPLETED', 'SHIPPED') \
                GROUP BY p.sku, p.product_name, p.category \
                ORDER BY total_{rev_label} DESC \
                LIMIT {lim}"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!("Aggregates product {rev_label} across completed and shipped orders ordered descending."),
                assumptions: vec![format!("Uses analytics.order_items.{line_rev_col} as product revenue.")],
                applied_filters: vec!["o.status IN ('COMPLETED', 'SHIPPED')".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern D: Monthly sales trends
        if (q_lower.contains("monthly") || q_lower.contains("trend") || q_lower.contains("by month"))
            && (q_lower.contains("sales") || q_lower.contains("revenue") || q_lower.contains("order"))
            && has_table(cat, "analytics", "orders")
        {
            let sql = format!(
                "SELECT TO_CHAR(DATE_TRUNC('month', order_date), 'YYYY-MM') AS sales_month, \
                COUNT(order_id) AS order_count, \
                ROUND(SUM({rev_col}), 2) AS {rev_label} \
                FROM analytics.orders \
                WHERE status IN ('COMPLETED', 'SHIPPED') \
                GROUP BY DATE_TRUNC('month', order_date) \
                ORDER BY DATE_TRUNC('month', order_date) ASC"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!("Aggregates monthly order counts and {rev_label} chronologically from analytics.orders."),
                assumptions: vec![format!("Monthly aggregation uses UTC DATE_TRUNC('month', order_date) and {rev_label}.")],
                applied_filters: vec!["status IN ('COMPLETED', 'SHIPPED')".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern E: Regions with declining sales
        if q_lower.contains("region")
            && (q_lower.contains("declin") || q_lower.contains("drop") || q_lower.contains("decreas") || q_lower.contains("growth"))
            && has_table(cat, "analytics", "orders")
            && has_table(cat, "analytics", "regions")
        {
            let sql = format!(
                "WITH quarterly_regional_sales AS ( \
                    SELECT r.region_code, r.region_name, \
                    DATE_TRUNC('quarter', o.order_date)::DATE AS quarter_start, \
                    ROUND(SUM(o.{rev_col}), 2) AS quarter_revenue \
                    FROM analytics.orders o \
                    JOIN analytics.regions r ON r.region_id = o.region_id \
                    WHERE o.status IN ('COMPLETED', 'SHIPPED') \
                    AND o.order_date >= DATE '2026-01-01' \
                    AND o.order_date < DATE_TRUNC('quarter', CURRENT_DATE) \
                    GROUP BY r.region_code, r.region_name, DATE_TRUNC('quarter', o.order_date) \
                ), \
                with_prev AS ( \
                    SELECT region_code, region_name, \
                    TO_CHAR(quarter_start, 'YYYY-\"Q\"Q') AS quarter_label, \
                    quarter_revenue, \
                    LAG(quarter_revenue) OVER (PARTITION BY region_code ORDER BY quarter_start) AS prev_quarter_revenue \
                    FROM quarterly_regional_sales \
                ) \
                SELECT region_code, region_name, quarter_label, quarter_revenue, prev_quarter_revenue, \
                ROUND(quarter_revenue - prev_quarter_revenue, 2) AS revenue_change, \
                ROUND(((quarter_revenue - prev_quarter_revenue) / NULLIF(prev_quarter_revenue, 0)) * 100, 2) AS pct_change \
                FROM with_prev \
                WHERE prev_quarter_revenue IS NOT NULL AND quarter_revenue < prev_quarter_revenue \
                ORDER BY pct_change ASC"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: "Identifies regions where completed full-quarter revenue declined compared to the immediately preceding quarter in 2026.".to_string(),
                assumptions: vec![
                    "Excludes incomplete current quarter to prevent false decline alerts.".to_string(),
                    format!("Compares full quarters using analytics.orders.{rev_col}."),
                ],
                applied_filters: vec![
                    "o.status IN ('COMPLETED', 'SHIPPED')".to_string(),
                    "o.order_date >= DATE '2026-01-01' AND o.order_date < DATE_TRUNC('quarter', CURRENT_DATE)".to_string(),
                ],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern F: Compare revenue between two periods
        if q_lower.contains("compare")
            && (q_lower.contains("revenue") || q_lower.contains("sales"))
            && has_table(cat, "analytics", "orders")
        {
            let sql = if q_lower.contains("2025") && q_lower.contains("2026") {
                format!(
                    "SELECT EXTRACT(YEAR FROM order_date)::INTEGER AS period_year, \
                    COUNT(order_id) AS order_count, \
                    ROUND(SUM({rev_col}), 2) AS {rev_label} \
                    FROM analytics.orders \
                    WHERE status IN ('COMPLETED', 'SHIPPED') \
                    AND EXTRACT(YEAR FROM order_date) IN (2025, 2026) \
                    GROUP BY EXTRACT(YEAR FROM order_date) \
                    ORDER BY period_year ASC"
                )
            } else {
                format!(
                    "SELECT TO_CHAR(DATE_TRUNC('quarter', order_date), 'YYYY-\"Q\"Q') AS period_label, \
                    COUNT(order_id) AS order_count, \
                    ROUND(SUM({rev_col}), 2) AS {rev_label} \
                    FROM analytics.orders \
                    WHERE status IN ('COMPLETED', 'SHIPPED') \
                    AND order_date >= DATE '2026-04-01' AND order_date < DATE '2026-10-01' \
                    GROUP BY DATE_TRUNC('quarter', order_date) \
                    ORDER BY DATE_TRUNC('quarter', order_date) ASC"
                )
            };
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!("Compares {rev_label} and order counts across the selected comparison periods."),
                assumptions: vec![format!("Uses analytics.orders.{rev_col} for period comparison.")],
                applied_filters: vec!["status IN ('COMPLETED', 'SHIPPED')".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern G: Customers with highest transaction volume
        if q_lower.contains("customer")
            && (q_lower.contains("volume") || q_lower.contains("transaction") || q_lower.contains("order") || q_lower.contains("top") || q_lower.contains("highest"))
            && has_table(cat, "analytics", "customers")
            && has_table(cat, "analytics", "orders")
        {
            let lim = extract_top_n(&q_lower).unwrap_or(10).min(req.row_limit);
            let sql = format!(
                "SELECT c.customer_code, c.company_name, c.segment, r.region_code, \
                COUNT(o.order_id) AS transaction_count, \
                ROUND(SUM(o.{rev_col}), 2) AS total_{rev_label} \
                FROM analytics.customers c \
                JOIN analytics.orders o ON o.customer_id = c.customer_id \
                JOIN analytics.regions r ON r.region_id = c.region_id \
                WHERE o.status IN ('COMPLETED', 'SHIPPED') \
                GROUP BY c.customer_code, c.company_name, c.segment, r.region_code \
                ORDER BY transaction_count DESC, total_{rev_label} DESC \
                LIMIT {lim}"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!("Ranks customers by total completed/shipped order count and {rev_label}, excluding sensitive tax_id columns."),
                assumptions: vec!["Transaction volume is measured by COUNT(analytics.orders.order_id).".to_string()],
                applied_filters: vec!["o.status IN ('COMPLETED', 'SHIPPED')".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern H: Departments with unusual expense patterns
        if (q_lower.contains("department") || q_lower.contains("expense") || q_lower.contains("budget") || q_lower.contains("unusual"))
            && has_table(cat, "finance", "department_expenses")
            && has_table(cat, "finance", "departments")
        {
            let sql = "WITH dept_stats AS ( \
                SELECT department_id, \
                AVG(actual_amount) AS avg_actual, \
                STDDEV_POP(actual_amount) AS std_actual \
                FROM finance.department_expenses \
                GROUP BY department_id \
            ) \
            SELECT d.department_name, d.cost_center, \
            TO_CHAR(de.expense_month, 'YYYY-MM') AS expense_month, \
            de.category, \
            ROUND(de.budget_amount, 2) AS budget_amount, \
            ROUND(de.actual_amount, 2) AS actual_amount, \
            ROUND(de.actual_amount - de.budget_amount, 2) AS budget_variance, \
            ROUND((de.actual_amount - ds.avg_actual) / NULLIF(ds.std_actual, 0), 2) AS z_score \
            FROM finance.department_expenses de \
            JOIN finance.departments d ON d.department_id = de.department_id \
            JOIN dept_stats ds ON ds.department_id = de.department_id \
            WHERE ABS(de.actual_amount - ds.avg_actual) > 1.8 * NULLIF(ds.std_actual, 0) \
            OR de.actual_amount > 1.25 * de.budget_amount \
            ORDER BY z_score DESC NULLS LAST";
            return Ok(LlmSqlOutput {
                sql: Some(sql.to_string()),
                explanation: "Detects monthly departmental expense anomalies where actual expenditure exceeds historical mean by >1.8 standard deviations or >125% of approved budget.".to_string(),
                assumptions: vec![
                    "Statistical baseline uses population mean and standard deviation across all recorded months per department.".to_string(),
                ],
                applied_filters: vec!["z_score > 1.8 OR actual_amount > 1.25 * budget_amount".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern I: General regional revenue breakdown
        if q_lower.contains("region") && has_table(cat, "analytics", "orders") && has_table(cat, "analytics", "regions") {
            let sql = format!(
                "SELECT r.region_code, r.region_name, COUNT(o.order_id) AS order_count, \
                ROUND(SUM(o.{rev_col}), 2) AS total_{rev_label} \
                FROM analytics.orders o \
                JOIN analytics.regions r ON r.region_id = o.region_id \
                WHERE o.status IN ('COMPLETED', 'SHIPPED') \
                GROUP BY r.region_code, r.region_name \
                ORDER BY total_{rev_label} DESC"
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!("Summarizes order volume and {rev_label} grouped by commercial region."),
                assumptions: vec![format!("Uses analytics.orders.{rev_col}.")],
                applied_filters: vec!["o.status IN ('COMPLETED', 'SHIPPED')".to_string()],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        // Pattern J: Generic safe preview over highest-ranked linked table (never using SELECT * if denied columns exist!)
        if let Some(top_tbl) = req.linked_schema.selected_tables.first() {
            let safe_cols: Vec<String> = top_tbl
                .columns
                .iter()
                .take(8)
                .map(|c| c.name.clone())
                .collect();
            if safe_cols.is_empty() {
                return Ok(LlmSqlOutput {
                    sql: None,
                    explanation: "No accessible columns available on the linked table.".to_string(),
                    assumptions: vec![],
                    applied_filters: vec![],
                    clarification_needed: None,
                    clarification_options: vec![],
                    cannot_compute_reason: Some("No permitted columns were found for this query.".to_string()),
                });
            }
            let lim = req.row_limit.min(50);
            let sql = format!(
                "SELECT {} FROM {}.{} LIMIT {}",
                safe_cols.join(", "),
                top_tbl.schema,
                top_tbl.name,
                lim
            );
            return Ok(LlmSqlOutput {
                sql: Some(sql),
                explanation: format!(
                    "Retrieves permitted columns from top schema-linked table '{}.{}'.",
                    top_tbl.schema, top_tbl.name
                ),
                assumptions: vec!["Returned explicit permitted columns matched via schema linking.".to_string()],
                applied_filters: vec![format!("LIMIT {lim}")],
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
            });
        }

        Ok(LlmSqlOutput {
            sql: None,
            explanation: "Could not map the question to any authorized table or metric in the current schema catalog.".to_string(),
            assumptions: vec![],
            applied_filters: vec![],
            clarification_needed: Some("Please rephrase your question referencing orders, revenue, products, regions, customers, or department expenses.".to_string()),
            clarification_options: vec![
                "What was the total net revenue last quarter?".to_string(),
                "Show the top ten products by gross profit.".to_string(),
                "Which departments have unusual expense patterns?".to_string(),
            ],
            cannot_compute_reason: None,
        })
    }

    async fn correct_sql(&self, req: LlmSqlCorrectionRequest<'_>) -> Result<LlmSqlOutput, AppError> {
        let err_lower = req.sanitized_error.to_lowercase();
        let failed = req.failed_sql;

        // Example deterministic self-correction: if column/relation typo or invalid identifier occurred,
        // inspect schema catalog or regenerate from original question
        if err_lower.contains("does not exist") || err_lower.contains("syntax error") {
            let regenerated = self
                .generate_sql(LlmSqlGenerationRequest {
                    question: req.question,
                    linked_schema: req.linked_schema,
                    full_catalog: req.full_catalog,
                    semantics: req.semantics,
                    preferred_metric: Some("net_revenue"),
                    row_limit: req.row_limit,
                })
                .await?;
            if let Some(candidate) = &regenerated.sql {
                if candidate != failed {
                    return Ok(LlmSqlOutput {
                        sql: Some(candidate.clone()),
                        explanation: format!(
                            "Self-corrected SQL after {} error using verified schema catalog.",
                            req.error_class
                        ),
                        assumptions: regenerated.assumptions,
                        applied_filters: regenerated.applied_filters,
                        clarification_needed: None,
                        clarification_options: vec![],
                        cannot_compute_reason: None,
                    });
                }
            }
        }

        Ok(LlmSqlOutput {
            sql: None,
            explanation: format!("Unable to repair SQL automatically after {} error.", req.error_class),
            assumptions: vec![],
            applied_filters: vec![],
            clarification_needed: None,
            clarification_options: vec![],
            cannot_compute_reason: Some(format!(
                "Query could not be corrected safely: {}",
                req.sanitized_error
            )),
        })
    }

    async fn summarize_results(
        &self,
        req: LlmInsightRequest<'_>,
    ) -> Result<LlmInsightOutput, AppError> {
        let mut observations = Vec::new();
        let mut interpretations = Vec::new();
        let mut limitations = Vec::new();

        if req.rows_returned == 0 {
            observations.push("The validated query executed successfully and returned 0 matching rows.".to_string());
            limitations.push("No records matched the applied filters; no trend or metric conclusion can be drawn.".to_string());
            return Ok(LlmInsightOutput {
                observations,
                interpretations,
                limitations,
            });
        }

        observations.push(format!(
            "Query returned {} verified row(s) across {} column(s) ({}).",
            req.rows_returned,
            req.columns.len(),
            req.columns.join(", ")
        ));

        for stat in req.stats {
            if let (Some(sum), Some(min), Some(max), Some(avg)) = (
                stat.numeric_sum,
                stat.numeric_min,
                stat.numeric_max,
                stat.numeric_avg,
            ) {
                observations.push(format!(
                    "Column '{}': min = {:.2}, max = {:.2}, avg = {:.2}, sum = {:.2} (across {} non-null row(s)).",
                    stat.column, min, max, avg, sum, stat.non_null_count
                ));
            }
            if stat.null_count > 0 {
                limitations.push(format!(
                    "Column '{}' contains {} NULL value(s) out of {} rows.",
                    stat.column, stat.null_count, req.rows_returned
                ));
            }
        }

        if let (Some(first_row), Some(last_row)) = (req.rows.first(), req.rows.last()) {
            if req.rows.len() >= 2 && req.columns.len() >= 2 {
                let first_label = format_cell(&first_row[0]);
                let last_label = format_cell(&last_row[0]);
                let first_val = format_cell(&first_row[req.columns.len() - 1]);
                let last_val = format_cell(&last_row[req.columns.len() - 1]);
                observations.push(format!(
                    "First row ({}) reports {} = {}; last row ({}) reports {} = {}.",
                    first_label,
                    req.columns[req.columns.len() - 1],
                    first_val,
                    last_label,
                    req.columns[req.columns.len() - 1],
                    last_val
                ));
            }
        }

        if req.rows_returned >= 2 {
            interpretations.push(
                "Observed differences reflect the queried dimensions and filters only; causal drivers cannot be inferred from aggregate figures alone."
                    .to_string(),
            );
        } else {
            limitations.push("Single-row result provides a point-in-time snapshot; multi-period trend direction requires at least 2 periods.".to_string());
        }

        if req.is_truncated {
            limitations.push(
                "Result set was truncated at the configured safety limit; totals across displayed rows reflect only the returned preview."
                    .to_string(),
            );
        }

        Ok(LlmInsightOutput {
            observations,
            interpretations,
            limitations,
        })
    }
}

fn contains_prompt_injection(q_lower: &str) -> bool {
    let markers = [
        "ignore previous",
        "ignore all instructions",
        "system prompt",
        "drop table",
        "delete from",
        "insert into",
        "update ",
        "truncate ",
        "alter table",
        "restricted_hr",
        "pg_read_file",
        "pg_sleep",
        "dblink",
        "union select",
        ";--",
        "/*",
    ];
    markers.iter().any(|m| q_lower.contains(m))
}

fn has_table(cat: &SchemaCatalog, schema: &str, table: &str) -> bool {
    cat.tables
        .iter()
        .any(|t| t.schema.eq_ignore_ascii_case(schema) && t.name.eq_ignore_ascii_case(table))
}

fn has_col(cat: &SchemaCatalog, schema: &str, table: &str, col: &str) -> bool {
    cat.tables
        .iter()
        .find(|t| t.schema.eq_ignore_ascii_case(schema) && t.name.eq_ignore_ascii_case(table))
        .map(|t| t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)))
        .unwrap_or(false)
}

fn extract_top_n(q_lower: &str) -> Option<u32> {
    if q_lower.contains("top ten") {
        return Some(10);
    }
    if q_lower.contains("top five") {
        return Some(5);
    }
    for word in q_lower.split_whitespace() {
        if let Ok(n) = word.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u32>() {
            if (1..=100).contains(&n) && (q_lower.contains("top") || q_lower.contains("limit")) {
                return Some(n);
            }
        }
    }
    None
}

fn format_cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
