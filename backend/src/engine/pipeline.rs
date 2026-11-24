use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    db::{
        app_db::{load_active_connection_params, record_audit_event},
        ColumnMeta, DatabaseAdapter, ExecutionContext, PostgresAdapter,
    },
    engine::{
        analytics::{select_chart_spec, ChartRecommendation},
        insights::{generate_grounded_insights, InsightReport},
        verifier::{verify_execution_result, VerifiedExecutionStatus, VerifiedResultSummary},
    },
    error::{sanitize_error_message, AppError},
    llm::{
        create_provider, LlmProvider, LlmSqlCorrectionRequest, LlmSqlGenerationRequest,
        ProviderConfigRecord,
    },
    schema::{
        discovery::load_latest_schema_or_refresh,
        linker::{filter_catalog_for_policy, link_schema_for_question},
        CandidateTableMatch, ForeignKeyMeta, SchemaCatalog,
    },
    semantic::list_active_metrics,
    sql_validator::{validate_sql, ValidationReport},
    state::AppState,
};

#[derive(Debug, Clone, Deserialize)]
pub struct QueryPipelineRequest {
    pub question: String,
    pub connection_id: Option<Uuid>,
    pub preferred_metric: Option<String>,
    #[serde(default = "default_true")]
    pub clarify_ambiguity: bool,
    pub requested_row_limit: Option<u32>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionAttemptRecord {
    pub attempt: u32,
    pub previous_sql: String,
    pub error_class: String,
    pub sanitized_error: String,
    pub candidate_sql: Option<String>,
    pub revalidation_passed: bool,
    pub rejection_reason: Option<String>,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPipelineResponse {
    pub query_id: Uuid,
    pub question: String,
    pub provider_used: String,
    pub model_used: String,
    pub is_external_llm: bool,
    pub schema_version: i32,
    pub linked_tables: Vec<CandidateTableMatch>,
    pub join_paths: Vec<ForeignKeyMeta>,
    pub generated_sql: String,
    pub validated_sql: String,
    pub explanation: String,
    pub assumptions: Vec<String>,
    pub applied_filters: Vec<String>,
    pub validation_status: String,
    pub validation_report: Option<ValidationReport>,
    pub execution_status: VerifiedExecutionStatus,
    pub correction_attempts: Vec<CorrectionAttemptRecord>,
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub result_summary: Option<VerifiedResultSummary>,
    pub chart_spec: Option<ChartRecommendation>,
    pub insight_report: Option<InsightReport>,
    pub clarification_needed: Option<String>,
    pub clarification_options: Vec<String>,
    pub cannot_compute_reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn run_nl_query_pipeline(
    state: &AppState,
    user: &AuthUser,
    req: QueryPipelineRequest,
) -> Result<QueryPipelineResponse, AppError> {
    let question = req.question.trim().to_string();
    if question.is_empty() {
        return Err(AppError::BadRequest("Question cannot be empty.".to_string()));
    }

    let query_id = Uuid::new_v4();
    let created_at = Utc::now();

    let conn_params = load_active_connection_params(
        &state.app_pool,
        user.tenant_id,
        req.connection_id,
        &state.cipher,
    )
    .await?;

    if !user.policy.is_database_allowed(&conn_params.database_name) {
        record_audit_event(
            &state.app_pool,
            user.tenant_id,
            Some(user.user_id),
            &user.email,
            "query_authorization",
            "database",
            &conn_params.database_name,
            "denied",
            json!({"reason": "database not in role allowed_databases"}),
        )
        .await;
        return Err(AppError::Forbidden(format!(
            "Role '{}' is not authorized to query database '{}'.",
            user.policy.role, conn_params.database_name
        )));
    }

    let provider_cfg = load_active_provider_config(state, user.tenant_id).await?;
    let provider = create_provider(&provider_cfg)?;

    let semantics = list_active_metrics(&state.app_pool, user.tenant_id).await?;
    let raw_catalog =
        load_latest_schema_or_refresh(&state.app_pool, user.tenant_id, &conn_params).await?;
    let filtered_catalog = filter_catalog_for_policy(&raw_catalog, &user.policy);

    let linked = link_schema_for_question(
        &question,
        &filtered_catalog,
        &semantics,
        req.clarify_ambiguity,
    );

    let effective_limit = req
        .requested_row_limit
        .map(|l| l.clamp(1, user.policy.row_limit))
        .unwrap_or(user.policy.row_limit);

    let gen_out = provider
        .generate_sql(LlmSqlGenerationRequest {
            question: &question,
            linked_schema: &linked,
            full_catalog: &filtered_catalog,
            semantics: &semantics,
            preferred_metric: req.preferred_metric.as_deref(),
            row_limit: effective_limit,
        })
        .await?;

    // Handle clarification request safely without fabricating SQL
    if let Some(clarify_msg) = &gen_out.clarification_needed {
        let resp = QueryPipelineResponse {
            query_id,
            question,
            provider_used: provider.provider_id().to_string(),
            model_used: provider.model_name().to_string(),
            is_external_llm: provider.is_external_llm(),
            schema_version: filtered_catalog.version,
            linked_tables: linked.candidate_rankings,
            join_paths: linked.join_paths,
            generated_sql: String::new(),
            validated_sql: String::new(),
            explanation: gen_out.explanation,
            assumptions: gen_out.assumptions,
            applied_filters: gen_out.applied_filters,
            validation_status: "not_applicable".to_string(),
            validation_report: None,
            execution_status: VerifiedExecutionStatus::ClarificationRequired,
            correction_attempts: vec![],
            columns: vec![],
            rows: vec![],
            result_summary: None,
            chart_spec: None,
            insight_report: None,
            clarification_needed: Some(clarify_msg.clone()),
            clarification_options: gen_out.clarification_options,
            cannot_compute_reason: None,
            error_message: None,
            created_at,
        };
        persist_execution(state, user, conn_params.id, &resp).await;
        return Ok(resp);
    }

    if let Some(cannot_reason) = &gen_out.cannot_compute_reason {
        let resp = QueryPipelineResponse {
            query_id,
            question,
            provider_used: provider.provider_id().to_string(),
            model_used: provider.model_name().to_string(),
            is_external_llm: provider.is_external_llm(),
            schema_version: filtered_catalog.version,
            linked_tables: linked.candidate_rankings,
            join_paths: linked.join_paths,
            generated_sql: String::new(),
            validated_sql: String::new(),
            explanation: gen_out.explanation,
            assumptions: gen_out.assumptions,
            applied_filters: gen_out.applied_filters,
            validation_status: "not_applicable".to_string(),
            validation_report: None,
            execution_status: VerifiedExecutionStatus::CannotCompute,
            correction_attempts: vec![],
            columns: vec![],
            rows: vec![],
            result_summary: None,
            chart_spec: None,
            insight_report: None,
            clarification_needed: None,
            clarification_options: vec![],
            cannot_compute_reason: Some(cannot_reason.clone()),
            error_message: None,
            created_at,
        };
        persist_execution(state, user, conn_params.id, &resp).await;
        return Ok(resp);
    }

    let generated_sql = gen_out.sql.unwrap_or_default();

    // MANDATORY DETERMINISTIC AST SECURITY VALIDATION GATE
    let initial_report = validate_sql(
        &generated_sql,
        &filtered_catalog,
        &user.policy,
        &conn_params.allowed_schemas,
        req.requested_row_limit,
    );

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "sql_validation",
        "query",
        &query_id.to_string(),
        if initial_report.is_valid {
            "allowed"
        } else {
            "rejected"
        },
        json!({
            "sql": initial_report.original_sql,
            "referenced_tables": initial_report.referenced_tables,
            "rejection_reason": initial_report.rejection_reason
        }),
    )
    .await;

    if !initial_report.is_valid {
        let rej_reason = initial_report
            .rejection_reason
            .clone()
            .unwrap_or_else(|| "Query rejected by security validator.".to_string());
        let resp = QueryPipelineResponse {
            query_id,
            question,
            provider_used: provider.provider_id().to_string(),
            model_used: provider.model_name().to_string(),
            is_external_llm: provider.is_external_llm(),
            schema_version: filtered_catalog.version,
            linked_tables: linked.candidate_rankings,
            join_paths: linked.join_paths,
            generated_sql,
            validated_sql: String::new(),
            explanation: gen_out.explanation,
            assumptions: gen_out.assumptions,
            applied_filters: gen_out.applied_filters,
            validation_status: "validation_rejected".to_string(),
            validation_report: Some(initial_report),
            execution_status: VerifiedExecutionStatus::ValidationRejected,
            correction_attempts: vec![],
            columns: vec![],
            rows: vec![],
            result_summary: None,
            chart_spec: None,
            insight_report: None,
            clarification_needed: None,
            clarification_options: vec![],
            cannot_compute_reason: None,
            error_message: Some(rej_reason),
            created_at,
        };
        persist_execution(state, user, conn_params.id, &resp).await;
        return Ok(resp);
    }

    let adapter = PostgresAdapter::connect(conn_params.clone()).await?;
    let exec_ctx = ExecutionContext {
        timeout_ms: state.config.query_timeout_ms,
        row_limit: effective_limit,
        max_result_bytes: state.config.max_result_bytes,
        tenant_region: user.policy.region_filter.clone(),
    };

    let outcome = execute_with_self_correction(
        &adapter,
        provider.as_ref(),
        &question,
        &generated_sql,
        initial_report,
        &linked,
        &filtered_catalog,
        &semantics,
        &user.policy,
        &conn_params.allowed_schemas,
        req.requested_row_limit,
        &exec_ctx,
        state.config.self_correction_max_retries,
    )
    .await;

    let resp = build_final_response(
        state,
        user,
        query_id,
        created_at,
        question,
        provider.as_ref(),
        filtered_catalog.version,
        linked.candidate_rankings,
        linked.join_paths,
        gen_out.explanation,
        gen_out.assumptions,
        gen_out.applied_filters,
        outcome,
    )
    .await;

    persist_execution(state, user, conn_params.id, &resp).await;
    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "query_execution",
        "query",
        &query_id.to_string(),
        if matches!(
            resp.execution_status,
            VerifiedExecutionStatus::CompletedWithRows
                | VerifiedExecutionStatus::CompletedEmpty
                | VerifiedExecutionStatus::CompletedTruncated
        ) {
            "completed"
        } else {
            "failed"
        },
        json!({
            "schema_version": resp.schema_version,
            "execution_status": resp.execution_status,
            "rows_returned": resp.rows.len(),
            "correction_attempts": resp.correction_attempts.len()
        }),
    )
    .await;

    Ok(resp)
}

pub struct ExecutionWithCorrectionOutcome {
    pub final_generated_sql: String,
    pub final_report: ValidationReport,
    pub attempts: Vec<CorrectionAttemptRecord>,
    pub raw_result: Result<crate::db::RawExecutionResult, AppError>,
}

/// Runs validated SQL and performs bounded, re-validated self-correction ONLY for syntax/schema/type errors.
#[allow(clippy::too_many_arguments)]
pub async fn execute_with_self_correction(
    adapter: &dyn DatabaseAdapter,
    provider: &dyn LlmProvider,
    question: &str,
    initial_sql: &str,
    initial_report: ValidationReport,
    linked_schema: &crate::schema::LinkedSchemaContext,
    filtered_catalog: &SchemaCatalog,
    semantics: &[crate::semantic::SemanticMetric],
    policy: &crate::auth::RbacPolicy,
    connection_allowed_schemas: &[String],
    requested_row_limit: Option<u32>,
    exec_ctx: &ExecutionContext,
    max_retries: u32,
) -> ExecutionWithCorrectionOutcome {
    let mut current_sql = initial_sql.to_string();
    let mut current_report = initial_report;
    let mut seen_sqls: HashSet<String> = HashSet::new();
    seen_sqls.insert(normalize_sql(&current_sql));

    let mut attempts: Vec<CorrectionAttemptRecord> = Vec::new();

    for retry_idx in 0..=max_retries {
        let executable = match current_report.executable_sql.as_deref() {
            Some(sql) => sql,
            None => {
                return ExecutionWithCorrectionOutcome {
                    final_generated_sql: current_sql,
                    final_report: current_report,
                    attempts,
                    raw_result: Err(AppError::SqlValidationRejected(
                        "No executable SQL produced after validation.".to_string(),
                    )),
                };
            }
        };

        match adapter.execute_readonly_query(executable, exec_ctx).await {
            Ok(raw) => {
                return ExecutionWithCorrectionOutcome {
                    final_generated_sql: current_sql,
                    final_report: current_report,
                    attempts,
                    raw_result: Ok(raw),
                };
            }
            Err(err) => {
                let sanitized = sanitize_error_message(&err.to_string());
                let err_class = classify_db_error(&err, &sanitized);

                // NEVER retry permission denials, timeouts, or connection failures
                let is_retryable = matches!(err_class, "syntax" | "schema" | "type");
                if !is_retryable || retry_idx >= max_retries {
                    return ExecutionWithCorrectionOutcome {
                        final_generated_sql: current_sql,
                        final_report: current_report,
                        attempts,
                        raw_result: Err(err),
                    };
                }

                let corr_res = provider
                    .correct_sql(LlmSqlCorrectionRequest {
                        question,
                        failed_sql: &current_sql,
                        error_class: err_class,
                        sanitized_error: &sanitized,
                        linked_schema,
                        full_catalog: filtered_catalog,
                        semantics,
                        row_limit: exec_ctx.row_limit,
                    })
                    .await;

                let candidate = match corr_res {
                    Ok(out) => match out.sql {
                        Some(s) if !s.trim().is_empty() => s,
                        _ => {
                            attempts.push(CorrectionAttemptRecord {
                                attempt: retry_idx + 1,
                                previous_sql: current_sql.clone(),
                                error_class: err_class.to_string(),
                                sanitized_error: sanitized,
                                candidate_sql: None,
                                revalidation_passed: false,
                                rejection_reason: out.cannot_compute_reason,
                                outcome: "planner_declined_correction".to_string(),
                            });
                            return ExecutionWithCorrectionOutcome {
                                final_generated_sql: current_sql,
                                final_report: current_report,
                                attempts,
                                raw_result: Err(err),
                            };
                        }
                    },
                    Err(llm_err) => {
                        attempts.push(CorrectionAttemptRecord {
                            attempt: retry_idx + 1,
                            previous_sql: current_sql.clone(),
                            error_class: err_class.to_string(),
                            sanitized_error: sanitized,
                            candidate_sql: None,
                            revalidation_passed: false,
                            rejection_reason: Some(llm_err.to_string()),
                            outcome: "llm_correction_failed".to_string(),
                        });
                        return ExecutionWithCorrectionOutcome {
                            final_generated_sql: current_sql,
                            final_report: current_report,
                            attempts,
                            raw_result: Err(err),
                        };
                    }
                };

                // Prevent identical SQL repetition
                let norm = normalize_sql(&candidate);
                if seen_sqls.contains(&norm) {
                    attempts.push(CorrectionAttemptRecord {
                        attempt: retry_idx + 1,
                        previous_sql: current_sql.clone(),
                        error_class: err_class.to_string(),
                        sanitized_error: sanitized,
                        candidate_sql: Some(candidate),
                        revalidation_passed: false,
                        rejection_reason: Some("Duplicate SQL candidate rejected to prevent loop.".to_string()),
                        outcome: "duplicate_sql_aborted".to_string(),
                    });
                    return ExecutionWithCorrectionOutcome {
                        final_generated_sql: current_sql,
                        final_report: current_report,
                        attempts,
                        raw_result: Err(err),
                    };
                }
                seen_sqls.insert(norm);

                // Re-run COMPLETE deterministic AST security validation on corrected SQL!
                let reval = validate_sql(
                    &candidate,
                    filtered_catalog,
                    policy,
                    connection_allowed_schemas,
                    requested_row_limit,
                );

                if !reval.is_valid {
                    let rej = reval.rejection_reason.clone();
                    attempts.push(CorrectionAttemptRecord {
                        attempt: retry_idx + 1,
                        previous_sql: current_sql,
                        error_class: err_class.to_string(),
                        sanitized_error: sanitized,
                        candidate_sql: Some(candidate.clone()),
                        revalidation_passed: false,
                        rejection_reason: rej.clone(),
                        outcome: "revalidation_rejected".to_string(),
                    });
                    return ExecutionWithCorrectionOutcome {
                        final_generated_sql: candidate,
                        final_report: reval,
                        attempts,
                        raw_result: Err(AppError::SqlValidationRejected(
                            rej.unwrap_or_else(|| "Corrected SQL failed security validation.".to_string()),
                        )),
                    };
                }

                attempts.push(CorrectionAttemptRecord {
                    attempt: retry_idx + 1,
                    previous_sql: current_sql,
                    error_class: err_class.to_string(),
                    sanitized_error: sanitized,
                    candidate_sql: Some(candidate.clone()),
                    revalidation_passed: true,
                    rejection_reason: None,
                    outcome: "revalidated_and_retried".to_string(),
                });

                current_sql = candidate;
                current_report = reval;
            }
        }
    }

    ExecutionWithCorrectionOutcome {
        final_generated_sql: current_sql,
        final_report: current_report,
        attempts,
        raw_result: Err(AppError::DatabaseError("Max self-correction attempts exceeded.".to_string())),
    }
}

#[allow(clippy::too_many_arguments)]
async fn build_final_response(
    _state: &AppState,
    _user: &AuthUser,
    query_id: Uuid,
    created_at: DateTime<Utc>,
    question: String,
    provider: &dyn LlmProvider,
    schema_version: i32,
    linked_tables: Vec<CandidateTableMatch>,
    join_paths: Vec<ForeignKeyMeta>,
    explanation: String,
    assumptions: Vec<String>,
    applied_filters: Vec<String>,
    outcome: ExecutionWithCorrectionOutcome,
) -> QueryPipelineResponse {
    match outcome.raw_result {
        Ok(raw) => {
            let summary = verify_execution_result(&raw, "USD", "UTC");
            let chart = select_chart_spec(
                query_id,
                &raw,
                &applied_filters,
                &summary.currency_unit,
                &outcome.final_generated_sql,
            );
            let insight = generate_grounded_insights(
                query_id,
                &question,
                &outcome.final_generated_sql,
                &raw,
                &summary,
                &applied_filters,
                provider,
            )
            .await;

            QueryPipelineResponse {
                query_id,
                question,
                provider_used: provider.provider_id().to_string(),
                model_used: provider.model_name().to_string(),
                is_external_llm: provider.is_external_llm(),
                schema_version,
                linked_tables,
                join_paths,
                generated_sql: outcome.final_generated_sql,
                validated_sql: outcome.final_report.executable_sql.clone().unwrap_or_default(),
                explanation,
                assumptions,
                applied_filters,
                validation_status: "validation_passed".to_string(),
                validation_report: Some(outcome.final_report),
                execution_status: summary.status.clone(),
                correction_attempts: outcome.attempts,
                columns: raw.columns,
                rows: raw.rows,
                result_summary: Some(summary),
                chart_spec: Some(chart),
                insight_report: Some(insight),
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
                error_message: None,
                created_at,
            }
        }
        Err(err) => {
            let (exec_status, val_status) = match &err {
                AppError::SqlValidationRejected(_) => (
                    VerifiedExecutionStatus::ValidationRejected,
                    "validation_rejected".to_string(),
                ),
                AppError::Timeout => (
                    VerifiedExecutionStatus::ExecutionTimeout,
                    "validation_passed".to_string(),
                ),
                AppError::Forbidden(_) => (
                    VerifiedExecutionStatus::AuthorizationDenied,
                    "validation_passed".to_string(),
                ),
                AppError::DatabaseError(msg) if msg.to_lowercase().contains("permission denied") => (
                    VerifiedExecutionStatus::AuthorizationDenied,
                    "validation_passed".to_string(),
                ),
                AppError::DatabaseError(msg) if msg.to_lowercase().contains("connection") => (
                    VerifiedExecutionStatus::DbUnavailable,
                    "validation_passed".to_string(),
                ),
                _ => (
                    VerifiedExecutionStatus::ExecutionFailed,
                    "validation_passed".to_string(),
                ),
            };
            QueryPipelineResponse {
                query_id,
                question,
                provider_used: provider.provider_id().to_string(),
                model_used: provider.model_name().to_string(),
                is_external_llm: provider.is_external_llm(),
                schema_version,
                linked_tables,
                join_paths,
                generated_sql: outcome.final_generated_sql,
                validated_sql: outcome.final_report.executable_sql.clone().unwrap_or_default(),
                explanation,
                assumptions,
                applied_filters,
                validation_status: val_status,
                validation_report: Some(outcome.final_report),
                execution_status: exec_status,
                correction_attempts: outcome.attempts,
                columns: vec![],
                rows: vec![],
                result_summary: None,
                chart_spec: None,
                insight_report: None,
                clarification_needed: None,
                clarification_options: vec![],
                cannot_compute_reason: None,
                error_message: Some(sanitize_error_message(&err.to_string())),
                created_at,
            }
        }
    }
}

pub fn classify_db_error(err: &AppError, msg: &str) -> &'static str {
    if matches!(err, AppError::Timeout) {
        return "timeout";
    }
    let m = msg.to_lowercase();
    if m.contains("permission denied") || m.contains("read-only transaction") {
        "permission"
    } else if m.contains("does not exist") || m.contains("undefined") {
        "schema"
    } else if m.contains("syntax error") {
        "syntax"
    } else if m.contains("type") || m.contains("cannot cast") || m.contains("operator does not exist") {
        "type"
    } else if m.contains("statement timeout") {
        "timeout"
    } else {
        "execution"
    }
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub async fn load_active_provider_config(
    state: &AppState,
    tenant_id: Uuid,
) -> Result<ProviderConfigRecord, AppError> {
    let row = sqlx::query(
        r#"
        SELECT provider_type, model_name, base_url, encrypted_api_key
        FROM llm_providers
        WHERE tenant_id = $1 AND is_active = TRUE
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if let Some(r) = row {
        let enc_key: String = r.get("encrypted_api_key");
        let api_key = if enc_key.trim().is_empty() {
            String::new()
        } else {
            state
                .cipher
                .decrypt(&enc_key)
                .map_err(AppError::Internal)?
        };
        return Ok(ProviderConfigRecord {
            provider_type: r.get("provider_type"),
            model_name: r.get("model_name"),
            base_url: r.get("base_url"),
            api_key,
        });
    }

    Ok(ProviderConfigRecord {
        provider_type: "local_deterministic".to_string(),
        model_name: "schema-semantic-planner-v1".to_string(),
        base_url: String::new(),
        api_key: String::new(),
    })
}

async fn persist_execution(
    state: &AppState,
    user: &AuthUser,
    connection_id: Uuid,
    resp: &QueryPipelineResponse,
) {
    let dur = resp
        .result_summary
        .as_ref()
        .map(|s| s.duration_ms as i32)
        .unwrap_or(0);
    let rows_ret = resp.rows.len() as i32;
    let is_trunc = resp
        .result_summary
        .as_ref()
        .map(|s| s.is_truncated)
        .unwrap_or(false);
    let exec_status_str = serde_json::to_value(&resp.execution_status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());

    let _ = sqlx::query(
        r#"
        INSERT INTO query_executions (
            id, tenant_id, user_id, connection_id, question, provider_used, model_used,
            generated_sql, validated_sql, schema_version, validation_status, validation_details,
            execution_status, execution_duration_ms, rows_returned, is_truncated,
            correction_attempts, result_summary, chart_spec, insight_report, error_message, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)
        "#,
    )
    .bind(resp.query_id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(connection_id)
    .bind(&resp.question)
    .bind(&resp.provider_used)
    .bind(&resp.model_used)
    .bind(&resp.generated_sql)
    .bind(&resp.validated_sql)
    .bind(resp.schema_version)
    .bind(&resp.validation_status)
    .bind(serde_json::to_value(&resp.validation_report).unwrap_or_default())
    .bind(&exec_status_str)
    .bind(dur)
    .bind(rows_ret)
    .bind(is_trunc)
    .bind(serde_json::to_value(&resp.correction_attempts).unwrap_or_default())
    .bind(serde_json::to_value(&resp.result_summary).unwrap_or_default())
    .bind(serde_json::to_value(&resp.chart_spec).unwrap_or_default())
    .bind(serde_json::to_value(&resp.insight_report).unwrap_or_default())
    .bind(&resp.error_message)
    .bind(resp.created_at)
    .execute(&state.app_pool)
    .await;
}
