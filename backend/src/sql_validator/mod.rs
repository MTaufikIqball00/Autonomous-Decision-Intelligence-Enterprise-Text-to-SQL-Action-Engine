use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sqlparser::{
    ast::{
        BinaryOperator, CastKind, Distinct, Expr, FunctionArg, FunctionArgExpr, FunctionArguments,
        GroupByExpr, Ident, JoinConstraint, JoinOperator, ObjectName, Query, Select, SelectItem,
        SetExpr, Statement, TableFactor, TableWithJoins, ValueWithSpan, WindowSpec, WindowType,
        With,
    },
    dialect::PostgreSqlDialect,
    parser::Parser,
};

use crate::{auth::RbacPolicy, schema::SchemaCatalog};

/// Explicit allowlist of safe, read-only PostgreSQL scalar, math, string, date, aggregate, and window functions.
const ALLOWED_FUNCTIONS: &[&str] = &[
    // Aggregates
    "count", "sum", "avg", "min", "max", "coalesce", "nullif", "greatest", "least",
    "stddev", "stddev_pop", "stddev_samp", "variance", "var_pop", "var_samp",
    "percentile_cont", "percentile_disc", "bool_and", "bool_or", "array_agg", "string_agg",
    // Math
    "abs", "ceil", "ceiling", "floor", "round", "trunc", "mod", "power", "sqrt", "sign",
    // Date / Time (pure / read-only)
    "date_trunc", "date_part", "extract", "now", "current_date", "current_timestamp",
    "make_date", "to_char", "to_date", "age", "justify_interval",
    // String (pure)
    "lower", "upper", "trim", "btrim", "ltrim", "rtrim", "length", "char_length",
    "substring", "substr", "left", "right", "concat", "concat_ws", "replace", "split_part",
    "position",
    // Window functions
    "row_number", "rank", "dense_rank", "ntile", "lag", "lead", "first_value", "last_value",
];

/// Explicit dangerous function prefix/names always blocked for defense-in-depth logging clarity.
const DANGEROUS_FUNCTIONS: &[&str] = &[
    "pg_read_file", "pg_read_binary_file", "pg_ls_dir", "pg_stat_file", "pg_sleep",
    "pg_terminate_backend", "pg_cancel_backend", "pg_reload_conf", "pg_rotate_logfile",
    "dblink", "dblink_exec", "dblink_connect", "lo_import", "lo_export", "lo_unlink",
    "set_config", "current_setting", "query_to_xml", "query_to_json", "txid_current",
    "pg_export_snapshot", "inet_server_addr", "inet_client_addr", "version",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub check_name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub is_valid: bool,
    pub original_sql: String,
    pub executable_sql: Option<String>,
    pub referenced_tables: Vec<String>,
    pub referenced_columns: Vec<String>,
    pub referenced_functions: Vec<String>,
    pub applied_row_limit: u32,
    pub rejection_reason: Option<String>,
    pub checks: Vec<ValidationCheck>,
}

#[derive(Clone)]
struct ScopeContext {
    /// alias_lowercase -> (schema_lowercase, table_lowercase) for real tables
    table_aliases: BTreeMap<String, (String, String)>,
    /// CTE or derived table alias_lowercase -> exposed column names (None = unknown/unrestricted inside CTE)
    cte_columns: BTreeMap<String, Option<BTreeSet<String>>>,
    /// Real tables in scope (schema, table)
    real_tables: Vec<(String, String)>,
}

impl ScopeContext {
    fn new() -> Self {
        Self {
            table_aliases: BTreeMap::new(),
            cte_columns: BTreeMap::new(),
            real_tables: Vec::new(),
        }
    }
}

struct ValidatorState<'a> {
    catalog: &'a SchemaCatalog,
    policy: &'a RbacPolicy,
    connection_allowed_schemas: &'a [String],
    referenced_tables: BTreeSet<String>,
    referenced_columns: BTreeSet<String>,
    referenced_functions: BTreeSet<String>,
    checks: Vec<ValidationCheck>,
}

pub fn validate_sql(
    raw_sql: &str,
    catalog: &SchemaCatalog,
    policy: &RbacPolicy,
    connection_allowed_schemas: &[String],
    requested_row_limit: Option<u32>,
) -> ValidationReport {
    let trimmed = raw_sql.trim();
    let effective_limit = requested_row_limit
        .map(|l| l.clamp(1, policy.row_limit))
        .unwrap_or(policy.row_limit);

    let mut state = ValidatorState {
        catalog,
        policy,
        connection_allowed_schemas,
        referenced_tables: BTreeSet::new(),
        referenced_columns: BTreeSet::new(),
        referenced_functions: BTreeSet::new(),
        checks: Vec::new(),
    };

    if let Err(reason) = pre_parse_checks(trimmed, &mut state) {
        return reject(trimmed, effective_limit, reason, state);
    }

    let dialect = PostgreSqlDialect {};
    let statements = match Parser::parse_sql(&dialect, trimmed) {
        Ok(stmts) => stmts,
        Err(e) => {
            state.checks.push(ValidationCheck {
                check_name: "ast_parse".to_string(),
                passed: false,
                detail: format!("PostgreSQL SQL syntax parse failed: {e}"),
            });
            return reject(trimmed, effective_limit, format!("Invalid SQL syntax: {e}"), state);
        }
    };

    if statements.len() != 1 {
        state.checks.push(ValidationCheck {
            check_name: "single_statement".to_string(),
            passed: false,
            detail: format!("Expected exactly 1 SQL statement, found {}", statements.len()),
        });
        return reject(
            trimmed,
            effective_limit,
            "Multiple SQL statements are prohibited.".to_string(),
            state,
        );
    }

    state.checks.push(ValidationCheck {
        check_name: "single_statement".to_string(),
        passed: true,
        detail: "Parsed single PostgreSQL AST statement.".to_string(),
    });

    let query = match &statements[0] {
        Statement::Query(q) => q,
        other => {
            let stmt_kind = statement_kind_name(other);
            state.checks.push(ValidationCheck {
                check_name: "read_only_statement".to_string(),
                passed: false,
                detail: format!("Non-SELECT statement rejected: {stmt_kind}"),
            });
            return reject(
                trimmed,
                effective_limit,
                format!("Statement type '{stmt_kind}' is not permitted; only read-only SELECT queries are allowed."),
                state,
            );
        }
    };

    state.checks.push(ValidationCheck {
        check_name: "read_only_statement".to_string(),
        passed: true,
        detail: "Top-level AST node is a read-only Query.".to_string(),
    });

    let root_scope = ScopeContext::new();
    if let Err(reason) = validate_query(query, &root_scope, &mut state, false) {
        return reject(trimmed, effective_limit, reason, state);
    }

    state.checks.push(ValidationCheck {
        check_name: "schema_and_rbac_policy".to_string(),
        passed: true,
        detail: format!(
            "Verified {} table(s), {} column reference(s), and {} function(s) against schema catalog & RBAC.",
            state.referenced_tables.len(),
            state.referenced_columns.len(),
            state.referenced_functions.len()
        ),
    });

    // Normalize semicolon and wrap in bounded row-cap subquery (+1 enables deterministic truncation check)
    let clean_inner = trimmed.trim_end_matches(';').trim();
    let executable_sql = format!(
        "SELECT * FROM ({clean_inner}) AS _dta_safe_result LIMIT {}",
        effective_limit + 1
    );

    state.checks.push(ValidationCheck {
        check_name: "row_limit_wrapper".to_string(),
        passed: true,
        detail: format!("Enforced mandatory outer cap of {effective_limit} rows."),
    });

    ValidationReport {
        is_valid: true,
        original_sql: trimmed.to_string(),
        executable_sql: Some(executable_sql),
        referenced_tables: state.referenced_tables.into_iter().collect(),
        referenced_columns: state.referenced_columns.into_iter().collect(),
        referenced_functions: state.referenced_functions.into_iter().collect(),
        applied_row_limit: effective_limit,
        rejection_reason: None,
        checks: state.checks,
    }
}

fn reject(
    original_sql: &str,
    effective_limit: u32,
    reason: String,
    state: ValidatorState<'_>,
) -> ValidationReport {
    ValidationReport {
        is_valid: false,
        original_sql: original_sql.to_string(),
        executable_sql: None,
        referenced_tables: state.referenced_tables.into_iter().collect(),
        referenced_columns: state.referenced_columns.into_iter().collect(),
        referenced_functions: state.referenced_functions.into_iter().collect(),
        applied_row_limit: effective_limit,
        rejection_reason: Some(reason),
        checks: state.checks,
    }
}

fn pre_parse_checks(sql: &str, state: &mut ValidatorState<'_>) -> Result<(), String> {
    if sql.is_empty() {
        return Err("SQL cannot be empty.".to_string());
    }
    if sql.contains('\0') {
        return Err("Null bytes in SQL are prohibited.".to_string());
    }
    if contains_sql_comments_outside_strings(sql) {
        state.checks.push(ValidationCheck {
            check_name: "no_comments".to_string(),
            passed: false,
            detail: "SQL comments (-- or /* */) are prohibited.".to_string(),
        });
        return Err("SQL comments (-- or /* */) are not permitted in generated queries.".to_string());
    }
    // Check for extra semicolons outside string literals
    let stripped = sql.trim_end_matches(';').trim();
    if contains_semicolon_outside_strings(stripped) {
        state.checks.push(ValidationCheck {
            check_name: "no_statement_separator".to_string(),
            passed: false,
            detail: "Embedded semicolons outside string literals are prohibited.".to_string(),
        });
        return Err("Multiple SQL statements or embedded semicolons are prohibited.".to_string());
    }
    state.checks.push(ValidationCheck {
        check_name: "lexical_hygiene".to_string(),
        passed: true,
        detail: "No null bytes, comments, or embedded semicolons detected.".to_string(),
    });
    Ok(())
}

fn contains_sql_comments_outside_strings(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if b == b'\'' {
            in_single = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double = true;
            i += 1;
            continue;
        }
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            return true;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            return true;
        }
        i += 1;
    }
    false
}

fn contains_semicolon_outside_strings(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
        } else if in_double {
            if b == b'"' {
                in_double = false;
            }
        } else if b == b'\'' {
            in_single = true;
        } else if b == b'"' {
            in_double = true;
        } else if b == b';' {
            return true;
        }
        i += 1;
    }
    false
}

fn validate_query(
    query: &Query,
    parent_scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    inside_recursive_cte: bool,
) -> Result<Option<BTreeSet<String>>, String> {
    if !query.locks.is_empty() {
        return Err("Row locking clauses (FOR UPDATE / FOR SHARE) are prohibited.".to_string());
    }
    if query.fetch.is_some() {
        return Err("FETCH FIRST/NEXT clauses are not supported; use LIMIT.".to_string());
    }

    let mut scope = parent_scope.clone();
    if let Some(with) = &query.with {
        validate_with(with, &mut scope, state, query.limit.is_some())?;
    }

    let (output_cols, body_scope) = validate_set_expr(&query.body, &scope, state, inside_recursive_cte)?;

    if let Some(order_by) = &query.order_by {
        match &order_by.kind {
            sqlparser::ast::OrderByKind::Expressions(exprs) => {
                for ob in exprs {
                    validate_expr(&ob.expr, &body_scope, state, output_cols.as_ref())?;
                }
            }
            sqlparser::ast::OrderByKind::All(_) => {}
        }
    }

    if let Some(limit) = &query.limit {
        validate_expr(limit, &scope, state, None)?;
    }
    if let Some(offset) = &query.offset {
        validate_expr(&offset.value, &scope, state, None)?;
    }

    Ok(output_cols)
}

fn validate_with(
    with: &With,
    scope: &mut ScopeContext,
    state: &mut ValidatorState<'_>,
    outer_has_limit: bool,
) -> Result<(), String> {
    if with.recursive {
        // Recursive CTE resource guard: require explicit LIMIT on CTE or outer query
        let any_cte_has_limit = with.cte_tables.iter().any(|c| c.query.limit.is_some());
        if !any_cte_has_limit && !outer_has_limit {
            state.checks.push(ValidationCheck {
                check_name: "recursive_cte_guard".to_string(),
                passed: false,
                detail: "Recursive CTE without explicit LIMIT is rejected to prevent unbounded execution.".to_string(),
            });
            return Err("Recursive CTEs must specify an explicit LIMIT clause to protect database resources.".to_string());
        }
        state.checks.push(ValidationCheck {
            check_name: "recursive_cte_guard".to_string(),
            passed: true,
            detail: "Recursive CTE includes explicit LIMIT bound.".to_string(),
        });
    }

    for cte in &with.cte_tables {
        let cte_name = cte.alias.name.value.to_lowercase();
        if with.recursive {
            // Register CTE name ahead of validating body for self-reference
            let explicit_cols: Option<BTreeSet<String>> = if cte.alias.columns.is_empty() {
                None
            } else {
                Some(
                    cte.alias
                        .columns
                        .iter()
                        .map(|c| c.name.value.to_lowercase())
                        .collect(),
                )
            };
            scope.cte_columns.insert(cte_name.clone(), explicit_cols);
        }

        let cols = validate_query(&cte.query, scope, state, with.recursive)?;
        let final_cols = if !cte.alias.columns.is_empty() {
            Some(
                cte.alias
                    .columns
                    .iter()
                    .map(|c| c.name.value.to_lowercase())
                    .collect(),
            )
        } else {
            cols
        };
        scope.cte_columns.insert(cte_name, final_cols);
    }
    Ok(())
}

fn validate_set_expr(
    body: &SetExpr,
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    inside_recursive_cte: bool,
) -> Result<(Option<BTreeSet<String>>, ScopeContext), String> {
    match body {
        SetExpr::Select(select) => validate_select(select, scope, state, inside_recursive_cte),
        SetExpr::Query(q) => {
            let cols = validate_query(q, scope, state, inside_recursive_cte)?;
            Ok((cols, scope.clone()))
        }
        SetExpr::SetOperation { left, right, .. } => {
            let (l_cols, _) = validate_set_expr(left, scope, state, inside_recursive_cte)?;
            let _ = validate_set_expr(right, scope, state, inside_recursive_cte)?;
            Ok((l_cols, scope.clone()))
        }
        SetExpr::Values(vals) => {
            for row in &vals.rows {
                for expr in row {
                    validate_expr(expr, scope, state, None)?;
                }
            }
            Ok((None, scope.clone()))
        }
        other => Err(format!(
            "Unsupported or non-read-only query body expression: {other:?}"
        )),
    }
}

fn validate_select(
    select: &Select,
    parent_scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    _inside_recursive_cte: bool,
) -> Result<(Option<BTreeSet<String>>, ScopeContext), String> {
    if select.into.is_some() {
        return Err("SELECT ... INTO is prohibited (creates/writes tables).".to_string());
    }

    if let Some(Distinct::On(exprs)) = &select.distinct {
        for e in exprs {
            validate_expr(e, parent_scope, state, None)?;
        }
    }

    let mut local_scope = parent_scope.clone();
    for twj in &select.from {
        register_table_with_joins(twj, &mut local_scope, state)?;
    }

    // Validate SELECT projections and track output column names/aliases
    let mut output_aliases = BTreeSet::new();
    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(expr) => {
                validate_expr(expr, &local_scope, state, None)?;
                if let Some(col_name) = infer_expr_output_name(expr) {
                    output_aliases.insert(col_name);
                }
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                validate_expr(expr, &local_scope, state, None)?;
                output_aliases.insert(alias.value.to_lowercase());
            }
            SelectItem::Wildcard(_) => {
                // Reject SELECT * if ANY real table in scope has RBAC denied columns
                for (schema, table) in &local_scope.real_tables {
                    if state.policy.table_has_denied_columns(schema, table) {
                        return Err(format!(
                            "Wildcard SELECT * is denied on '{schema}.{table}' because it contains restricted columns for your role."
                        ));
                    }
                    if let Some(tm) = find_single_table(state.catalog, Some(schema), table).ok() {
                        for col in &tm.columns {
                            state
                                .referenced_columns
                                .insert(format!("{}.{}.{}", tm.schema, tm.name, col.name));
                            output_aliases.insert(col.name.to_lowercase());
                        }
                    }
                }
            }
            SelectItem::QualifiedWildcard(kind, _) => {
                let obj_name = match kind {
                    sqlparser::ast::SelectItemQualifiedWildcardKind::ObjectName(obj) => obj,
                    sqlparser::ast::SelectItemQualifiedWildcardKind::Expr(_) => {
                        return Err("Expression qualified wildcards are not permitted.".to_string());
                    }
                };
                let qual = object_name_to_parts(obj_name)
                    .last()
                    .cloned()
                    .unwrap_or_default()
                    .to_lowercase();
                if let Some((schema, table)) = local_scope.table_aliases.get(&qual).cloned() {
                    if state.policy.table_has_denied_columns(&schema, &table) {
                        return Err(format!(
                            "Wildcard '{qual}.*' is denied on '{schema}.{table}' because it contains restricted columns for your role."
                        ));
                    }
                    if let Ok(tm) = find_single_table(state.catalog, Some(&schema), &table) {
                        for col in &tm.columns {
                            state
                                .referenced_columns
                                .insert(format!("{}.{}.{}", tm.schema, tm.name, col.name));
                            output_aliases.insert(col.name.to_lowercase());
                        }
                    }
                } else if !local_scope.cte_columns.contains_key(&qual) {
                    return Err(format!("Unknown table qualifier '{qual}.*' in SELECT."));
                }
            }
        }
    }

    if let Some(sel) = &select.selection {
        validate_expr(sel, &local_scope, state, None)?;
    }

    match &select.group_by {
        GroupByExpr::All(_) => {}
        GroupByExpr::Expressions(exprs, _) => {
            for expr in exprs {
                validate_expr(expr, &local_scope, state, Some(&output_aliases))?;
            }
        }
    }

    if let Some(having) = &select.having {
        validate_expr(having, &local_scope, state, Some(&output_aliases))?;
    }

    if let Some(qualify) = &select.qualify {
        validate_expr(qualify, &local_scope, state, Some(&output_aliases))?;
    }

    Ok((Some(output_aliases), local_scope))
}

fn register_table_with_joins(
    twj: &TableWithJoins,
    scope: &mut ScopeContext,
    state: &mut ValidatorState<'_>,
) -> Result<(), String> {
    register_table_factor(&twj.relation, scope, state)?;
    for join in &twj.joins {
        register_table_factor(&join.relation, scope, state)?;
        match &join.join_operator {
            JoinOperator::Join(c)
            | JoinOperator::Inner(c)
            | JoinOperator::Left(c)
            | JoinOperator::LeftOuter(c)
            | JoinOperator::Right(c)
            | JoinOperator::RightOuter(c)
            | JoinOperator::FullOuter(c)
            | JoinOperator::LeftSemi(c)
            | JoinOperator::RightSemi(c)
            | JoinOperator::LeftAnti(c)
            | JoinOperator::RightAnti(c) => {
                validate_join_constraint(c, scope, state)?;
            }
            JoinOperator::CrossJoin | JoinOperator::CrossApply | JoinOperator::OuterApply => {
                return Err(
                    "Unconditional CROSS JOIN / APPLY is prohibited to prevent Cartesian explosions."
                        .to_string(),
                );
            }
            _ => {
                return Err("Unsupported JOIN operator.".to_string());
            }
        }
    }
    Ok(())
}

fn validate_join_constraint(
    constraint: &JoinConstraint,
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
) -> Result<(), String> {
    match constraint {
        JoinConstraint::On(expr) => validate_expr(expr, scope, state, None),
        JoinConstraint::Using(cols) => {
            for c in cols {
                let parts = object_name_to_parts(c);
                let col_name = parts
                    .last()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                validate_unqualified_column(&col_name, scope, state, None)?;
            }
            Ok(())
        }
        JoinConstraint::Natural => {
            Err("NATURAL JOIN is prohibited; specify explicit ON join conditions.".to_string())
        }
        JoinConstraint::None => {
            Err("JOIN without an ON or USING condition is prohibited.".to_string())
        }
    }
}

fn register_table_factor(
    tf: &TableFactor,
    scope: &mut ScopeContext,
    state: &mut ValidatorState<'_>,
) -> Result<(), String> {
    match tf {
        TableFactor::Table {
            name, alias, args, ..
        } => {
            if args.is_some() {
                return Err("Table-valued functions are not permitted in FROM clause.".to_string());
            }
            let parts = object_name_to_parts(name);
            match parts.len() {
                1 => {
                    let tbl = parts[0].to_lowercase();
                    // Could be a CTE in scope
                    if scope.cte_columns.contains_key(&tbl) {
                        let alias_key = alias
                            .as_ref()
                            .map(|a| a.name.value.to_lowercase())
                            .unwrap_or_else(|| tbl.clone());
                        if alias_key != tbl {
                            let cols = scope.cte_columns.get(&tbl).cloned().flatten();
                            scope.cte_columns.insert(alias_key, cols);
                        }
                        return Ok(());
                    }
                    // Otherwise must resolve unambiguously in permitted SchemaCatalog
                    let tm = find_single_table(state.catalog, None, &tbl)?;
                    enforce_table_access(&tm.schema, &tm.name, None, state)?;
                    register_resolved_table(
                        &tm.schema,
                        &tm.name,
                        alias.as_ref().map(|a| a.name.value.as_str()),
                        scope,
                        state,
                    );
                    Ok(())
                }
                2 => {
                    let schema = parts[0].to_lowercase();
                    let tbl = parts[1].to_lowercase();
                    enforce_table_access(&schema, &tbl, None, state)?;
                    let tm = find_single_table(state.catalog, Some(&schema), &tbl)?;
                    register_resolved_table(
                        &tm.schema,
                        &tm.name,
                        alias.as_ref().map(|a| a.name.value.as_str()),
                        scope,
                        state,
                    );
                    Ok(())
                }
                3 => {
                    let db = parts[0].to_lowercase();
                    let schema = parts[1].to_lowercase();
                    let tbl = parts[2].to_lowercase();
                    enforce_table_access(&schema, &tbl, Some(&db), state)?;
                    let tm = find_single_table(state.catalog, Some(&schema), &tbl)?;
                    register_resolved_table(
                        &tm.schema,
                        &tm.name,
                        alias.as_ref().map(|a| a.name.value.as_str()),
                        scope,
                        state,
                    );
                    Ok(())
                }
                _ => Err(format!("Unsupported table reference format: {name}")),
            }
        }
        TableFactor::Derived {
            subquery, alias, ..
        } => {
            let cols = validate_query(subquery, scope, state, false)?;
            if let Some(a) = alias {
                let alias_name = a.name.value.to_lowercase();
                let explicit = if !a.columns.is_empty() {
                    Some(
                        a.columns
                            .iter()
                            .map(|c| c.name.value.to_lowercase())
                            .collect(),
                    )
                } else {
                    cols
                };
                scope.cte_columns.insert(alias_name, explicit);
            }
            Ok(())
        }
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => register_table_with_joins(table_with_joins, scope, state),
        other => Err(format!("Unsupported FROM table factor: {other:?}")),
    }
}

fn enforce_table_access(
    schema: &str,
    table: &str,
    db_opt: Option<&str>,
    state: &ValidatorState<'_>,
) -> Result<(), String> {
    if let Some(db) = db_opt {
        if !db.eq_ignore_ascii_case(&state.catalog.database_name)
            || !state.policy.is_database_allowed(db)
        {
            return Err(format!(
                "Access to unauthorized database '{db}' is denied."
            ));
        }
    }
    if !state
        .connection_allowed_schemas
        .iter()
        .any(|s| s.eq_ignore_ascii_case(schema))
    {
        return Err(format!(
            "Schema '{schema}' is outside the connection schema allowlist."
        ));
    }
    if !state.policy.is_schema_allowed(schema) {
        return Err(format!(
            "Role '{}' is not authorized to access schema '{schema}'.",
            state.policy.role
        ));
    }
    if !state.policy.is_table_allowed(schema, table) {
        return Err(format!(
            "Role '{}' is not authorized to access table '{schema}.{table}'.",
            state.policy.role
        ));
    }
    Ok(())
}

fn find_single_table<'a>(
    catalog: &'a SchemaCatalog,
    schema: Option<&str>,
    table: &str,
) -> Result<&'a crate::schema::TableMeta, String> {
    let matches = catalog.find_table(schema, table);
    match matches.len() {
        0 => {
            if let Some(s) = schema {
                Err(format!(
                    "Table or view '{s}.{table}' does not exist in the permitted schema catalog."
                ))
            } else {
                Err(format!(
                    "Table or view '{table}' does not exist in the permitted schema catalog."
                ))
            }
        }
        1 => Ok(matches[0]),
        _ => Err(format!(
            "Unqualified table '{table}' is ambiguous across multiple schemas; please schema-qualify it."
        )),
    }
}

fn register_resolved_table(
    schema: &str,
    table: &str,
    alias: Option<&str>,
    scope: &mut ScopeContext,
    state: &mut ValidatorState<'_>,
) {
    let s = schema.to_lowercase();
    let t = table.to_lowercase();
    state.referenced_tables.insert(format!("{s}.{t}"));
    scope.real_tables.push((s.clone(), t.clone()));
    scope.table_aliases.insert(t.clone(), (s.clone(), t.clone()));
    scope
        .table_aliases
        .insert(format!("{s}.{t}"), (s.clone(), t.clone()));
    if let Some(a) = alias {
        scope.table_aliases.insert(a.to_lowercase(), (s, t));
    }
}

fn validate_expr(
    expr: &Expr,
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    select_aliases: Option<&BTreeSet<String>>,
) -> Result<(), String> {
    match expr {
        Expr::Identifier(ident) => {
            validate_unqualified_column(&ident.value.to_lowercase(), scope, state, select_aliases)
        }
        Expr::CompoundIdentifier(idents) => validate_compound_column(idents, scope, state),
        Expr::Value(ValueWithSpan { .. }) => Ok(()),
        Expr::BinaryOp { left, op, right } => {
            if matches!(op, BinaryOperator::Custom(_)) {
                return Err("Custom binary operators are prohibited.".to_string());
            }
            validate_expr(left, scope, state, select_aliases)?;
            validate_expr(right, scope, state, select_aliases)
        }
        Expr::UnaryOp { expr, .. } => validate_expr(expr, scope, state, select_aliases),
        Expr::Nested(inner) => validate_expr(inner, scope, state, select_aliases),
        Expr::Cast {
            expr,
            kind,
            ..
        } => {
            if !matches!(kind, CastKind::Cast | CastKind::DoubleColon) {
                return Err("Unsupported cast syntax.".to_string());
            }
            validate_expr(expr, scope, state, select_aliases)
        }
        Expr::IsNull(e) | Expr::IsNotNull(e) | Expr::IsTrue(e) | Expr::IsFalse(e) => {
            validate_expr(e, scope, state, select_aliases)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            validate_expr(expr, scope, state, select_aliases)?;
            validate_expr(low, scope, state, select_aliases)?;
            validate_expr(high, scope, state, select_aliases)
        }
        Expr::InList { expr, list, .. } => {
            validate_expr(expr, scope, state, select_aliases)?;
            for item in list {
                validate_expr(item, scope, state, select_aliases)?;
            }
            Ok(())
        }
        Expr::InSubquery { expr, subquery, .. } => {
            validate_expr(expr, scope, state, select_aliases)?;
            validate_query(subquery, scope, state, false)?;
            Ok(())
        }
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => {
            validate_expr(expr, scope, state, select_aliases)?;
            validate_expr(pattern, scope, state, select_aliases)
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
        } => {
            if let Some(op) = operand {
                validate_expr(op, scope, state, select_aliases)?;
            }
            for cw in conditions {
                validate_expr(&cw.condition, scope, state, select_aliases)?;
                validate_expr(&cw.result, scope, state, select_aliases)?;
            }
            if let Some(el) = else_result {
                validate_expr(el, scope, state, select_aliases)?;
            }
            Ok(())
        }
        Expr::Subquery(subquery) => {
            validate_query(subquery, scope, state, false)?;
            Ok(())
        }
        Expr::Exists { subquery, .. } => {
            validate_query(subquery, scope, state, false)?;
            Ok(())
        }
        Expr::Function(func) => validate_function(func, scope, state, select_aliases),
        Expr::Extract { expr, .. } => validate_expr(expr, scope, state, select_aliases),
        Expr::Trim {
            expr,
            trim_what,
            trim_characters,
            ..
        } => {
            validate_expr(expr, scope, state, select_aliases)?;
            if let Some(tw) = trim_what {
                validate_expr(tw, scope, state, select_aliases)?;
            }
            if let Some(chars) = trim_characters {
                for c in chars {
                    validate_expr(c, scope, state, select_aliases)?;
                }
            }
            Ok(())
        }
        Expr::Position { expr, r#in } => {
            validate_expr(expr, scope, state, select_aliases)?;
            validate_expr(r#in, scope, state, select_aliases)
        }
        Expr::Substring {
            expr,
            substring_from,
            substring_for,
            ..
        } => {
            validate_expr(expr, scope, state, select_aliases)?;
            if let Some(f) = substring_from {
                validate_expr(f, scope, state, select_aliases)?;
            }
            if let Some(l) = substring_for {
                validate_expr(l, scope, state, select_aliases)?;
            }
            Ok(())
        }
        Expr::Interval(iv) => validate_expr(&iv.value, scope, state, select_aliases),
        Expr::TypedString { .. } => Ok(()),
        Expr::Tuple(exprs) => {
            for e in exprs {
                validate_expr(e, scope, state, select_aliases)?;
            }
            Ok(())
        }
        other => Err(format!("Unsupported or disallowed SQL expression: {other}")),
    }
}

fn validate_function(
    func: &sqlparser::ast::Function,
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    select_aliases: Option<&BTreeSet<String>>,
) -> Result<(), String> {
    let parts = object_name_to_parts(&func.name);
    if parts.len() != 1 {
        return Err(format!(
            "Schema-qualified or custom function calls ('{}') are prohibited.",
            func.name
        ));
    }
    let fname = parts[0].to_lowercase();
    if DANGEROUS_FUNCTIONS.contains(&fname.as_str()) {
        return Err(format!(
            "Dangerous database function '{fname}()' is explicitly prohibited."
        ));
    }
    if !ALLOWED_FUNCTIONS.contains(&fname.as_str()) {
        return Err(format!(
            "Function '{fname}()' is not on the approved read-only analytics allowlist."
        ));
    }
    state.referenced_functions.insert(fname);

    match &func.args {
        FunctionArguments::None => {}
        FunctionArguments::Subquery(q) => {
            validate_query(q, scope, state, false)?;
        }
        FunctionArguments::List(arg_list) => {
            for arg in &arg_list.args {
                match arg {
                    FunctionArg::Unnamed(arg_expr) => {
                        validate_func_arg_expr(arg_expr, scope, state, select_aliases)?;
                    }
                    FunctionArg::Named { arg: arg_expr, .. }
                    | FunctionArg::ExprNamed { arg: arg_expr, .. } => {
                        validate_func_arg_expr(arg_expr, scope, state, select_aliases)?;
                    }
                }
            }
            for clause in &arg_list.clauses {
                if let sqlparser::ast::FunctionArgumentClause::OrderBy(obs) = clause {
                    for ob in obs {
                        validate_expr(&ob.expr, scope, state, select_aliases)?;
                    }
                }
            }
        }
    }

    if let Some(filter) = &func.filter {
        validate_expr(filter, scope, state, select_aliases)?;
    }

    if let Some(over) = &func.over {
        if let WindowType::WindowSpec(WindowSpec {
            partition_by,
            order_by,
            ..
        }) = over
        {
            for p in partition_by {
                validate_expr(p, scope, state, select_aliases)?;
            }
            for ob in order_by {
                validate_expr(&ob.expr, scope, state, select_aliases)?;
            }
        }
    }

    for ob in &func.within_group {
        validate_expr(&ob.expr, scope, state, select_aliases)?;
    }

    Ok(())
}

fn validate_func_arg_expr(
    arg_expr: &FunctionArgExpr,
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    select_aliases: Option<&BTreeSet<String>>,
) -> Result<(), String> {
    match arg_expr {
        FunctionArgExpr::Expr(e) => validate_expr(e, scope, state, select_aliases),
        FunctionArgExpr::Wildcard => {
            // COUNT(*) is safe even when tables have denied columns; COUNT(*) only counts rows
            Ok(())
        }
        FunctionArgExpr::QualifiedWildcard(obj) => {
            let qual = object_name_to_parts(obj)
                .last()
                .cloned()
                .unwrap_or_default()
                .to_lowercase();
            if scope.table_aliases.contains_key(&qual) || scope.cte_columns.contains_key(&qual) {
                Ok(())
            } else {
                Err(format!("Unknown qualifier '{qual}.*' in function argument."))
            }
        }
    }
}

fn validate_unqualified_column(
    col: &str,
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
    select_aliases: Option<&BTreeSet<String>>,
) -> Result<(), String> {
    if let Some(aliases) = select_aliases {
        if aliases.contains(col) {
            return Ok(());
        }
    }

    let mut matched_real: Vec<(String, String)> = Vec::new();
    for (schema, table) in &scope.real_tables {
        if let Ok(tm) = find_single_table(state.catalog, Some(schema), table) {
            if tm.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)) {
                if state.policy.is_column_denied(schema, table, col) {
                    return Err(format!(
                        "Column '{schema}.{table}.{col}' is restricted by RBAC policy for role '{}'.",
                        state.policy.role
                    ));
                }
                matched_real.push((tm.schema.clone(), tm.name.clone()));
            }
        }
    }

    let mut matched_cte = false;
    for cols_opt in scope.cte_columns.values() {
        match cols_opt {
            Some(cols) if cols.contains(col) => matched_cte = true,
            None => matched_cte = true,
            _ => {}
        }
    }

    if matched_real.len() == 1 {
        let (s, t) = &matched_real[0];
        state.referenced_columns.insert(format!("{s}.{t}.{col}"));
        return Ok(());
    }
    if matched_real.len() > 1 {
        // Verify none are denied, allow if unambiguous or flag ambiguity
        for (s, t) in &matched_real {
            state.referenced_columns.insert(format!("{s}.{t}.{col}"));
        }
        return Ok(());
    }
    if matched_cte {
        return Ok(());
    }

    Err(format!(
        "Column '{col}' does not exist in any referenced table or CTE in scope."
    ))
}

fn validate_compound_column(
    idents: &[Ident],
    scope: &ScopeContext,
    state: &mut ValidatorState<'_>,
) -> Result<(), String> {
    match idents.len() {
        2 => {
            let qual = idents[0].value.to_lowercase();
            let col = idents[1].value.to_lowercase();
            if let Some((schema, table)) = scope.table_aliases.get(&qual) {
                return verify_real_table_column(schema, table, &col, state);
            }
            if let Some(cte_cols) = scope.cte_columns.get(&qual) {
                if let Some(cols) = cte_cols {
                    if !cols.contains(&col) {
                        return Err(format!("Column '{qual}.{col}' does not exist in CTE/subquery '{qual}'."));
                    }
                }
                return Ok(());
            }
            Err(format!("Unknown table or CTE alias '{qual}' in column reference '{qual}.{col}'."))
        }
        3 => {
            let schema = idents[0].value.to_lowercase();
            let table = idents[1].value.to_lowercase();
            let col = idents[2].value.to_lowercase();
            let key = format!("{schema}.{table}");
            if !scope.table_aliases.contains_key(&key) {
                return Err(format!(
                    "Table '{key}' referenced by '{key}.{col}' is not in the FROM clause."
                ));
            }
            verify_real_table_column(&schema, &table, &col, state)
        }
        _ => Err(format!("Unsupported compound column reference length: {}", idents.len())),
    }
}

fn verify_real_table_column(
    schema: &str,
    table: &str,
    col: &str,
    state: &mut ValidatorState<'_>,
) -> Result<(), String> {
    let tm = find_single_table(state.catalog, Some(schema), table)?;
    if !tm.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)) {
        return Err(format!(
            "Column '{col}' does not exist on table '{schema}.{table}'."
        ));
    }
    if state.policy.is_column_denied(schema, table, col) {
        return Err(format!(
            "Column '{schema}.{table}.{col}' is restricted by RBAC policy for role '{}'.",
            state.policy.role
        ));
    }
    state
        .referenced_columns
        .insert(format!("{}.{}.{}", tm.schema, tm.name, col));
    Ok(())
}

fn object_name_to_parts(name: &ObjectName) -> Vec<String> {
    name.0
        .iter()
        .map(|part| {
            part.as_ident()
                .map(|id| id.value.clone())
                .unwrap_or_else(|| part.to_string())
        })
        .collect()
}

fn infer_expr_output_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Identifier(id) => Some(id.value.to_lowercase()),
        Expr::CompoundIdentifier(ids) => ids.last().map(|id| id.value.to_lowercase()),
        _ => None,
    }
}

fn statement_kind_name(stmt: &Statement) -> String {
    let raw = format!("{stmt:?}");
    raw.split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("Unknown")
        .to_string()
}
