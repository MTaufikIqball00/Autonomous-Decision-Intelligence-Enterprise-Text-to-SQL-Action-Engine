use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{RbacPolicy, Role},
    db::{
        app_db::{load_active_connection_params, record_audit_event},
        DatabaseAdapter, ExecutionContext, RawExecutionResult,
    },
    engine::verifier::verify_execution_result,
    error::AppError,
    schema::{discovery::load_latest_schema_or_refresh_with_adapter, linker::filter_catalog_for_policy},
    sql_validator::validate_sql,
    state::AppState,
};

#[derive(Debug, Clone, Serialize)]
pub struct ActionConnectorDto {
    pub id: Uuid,
    pub name: String,
    pub connector_type: String,
    pub endpoint_url: String,
    pub default_headers: Value,
    pub has_secret: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionRuleDto {
    pub id: Uuid,
    pub connection_id: Option<Uuid>,
    pub saved_query_id: Option<Uuid>,
    pub connector_id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub sql_text: String,
    pub metric_column: String,
    pub comparator: String,
    pub threshold_value: f64,
    pub schedule_interval_secs: i32,
    pub is_active: bool,
    pub last_evaluated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionProposalDto {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub connector_id: Option<Uuid>,
    pub source_query_id: Option<Uuid>,
    pub title: String,
    pub rationale: String,
    pub evidence_json: Value,
    pub payload_json: Value,
    pub status: String,
    pub idempotency_key: String,
    pub proposed_by: Option<Uuid>,
    pub reviewed_by: Option<Uuid>,
    pub review_note: String,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionExecutionDto {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub connector_id: Option<Uuid>,
    pub idempotency_key: String,
    pub attempt_count: i32,
    pub http_status: Option<i32>,
    pub outcome: String,
    pub response_summary: String,
    pub executed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleEvaluationSummary {
    pub rule_id: Uuid,
    pub query_id: Uuid,
    pub triggered: bool,
    pub observed_value: Option<f64>,
    pub threshold_value: f64,
    pub comparator: String,
    pub row_count: usize,
    pub proposal: Option<ActionProposalDto>,
}

pub fn validate_connector_url(connector_type: &str, url: &str) -> Result<String, AppError> {
    let trimmed = url.trim();
    if connector_type == "email_ticket_payload" && trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            "endpoint_url is required for webhook and slack_teams connectors.".to_string(),
        ));
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::BadRequest(
            "Connector endpoint_url must use http:// or https:// scheme.".to_string(),
        ));
    }
    if lower.contains("169.254.169.254") || lower.contains("metadata.google.internal") {
        return Err(AppError::BadRequest(
            "Connector endpoint_url cannot target cloud instance metadata addresses.".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn extract_observed_metric(
    raw: &RawExecutionResult,
    metric_column: &str,
    comparator: &str,
) -> Option<f64> {
    if comparator == "row_count_gt" {
        return Some(raw.rows_returned as f64);
    }
    if raw.rows.is_empty() {
        return None;
    }
    let col_idx = if metric_column.trim().is_empty() {
        raw.columns
            .iter()
            .position(|c| {
                raw.rows[0]
                    .get(raw.columns.iter().position(|x| x.name == c.name).unwrap_or(0))
                    .and_then(value_to_f64)
                    .is_some()
            })
            .unwrap_or(0)
    } else {
        raw.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(metric_column.trim()))?
    };
    raw.rows[0].get(col_idx).and_then(value_to_f64)
}

fn value_to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

pub fn check_threshold(observed: f64, comparator: &str, threshold: f64) -> bool {
    match comparator {
        "gt" | "row_count_gt" => observed > threshold,
        "gte" => observed >= threshold,
        "lt" => observed < threshold,
        "lte" => observed <= threshold,
        "eq" => (observed - threshold).abs() < 1e-9,
        _ => false,
    }
}

/// Evaluates an `action_rules` monitor strictly read-only through the Rust AST validator,
/// creates a `query_executions` provenance entry, and opens a `pending` proposal when triggered.
pub async fn evaluate_rule_readonly(
    state: &AppState,
    tenant_id: Uuid,
    rule_id: Uuid,
    actor_user_id: Option<Uuid>,
    actor_email: &str,
) -> Result<RuleEvaluationSummary, AppError> {
    let row = sqlx::query(
        r#"
        SELECT r.id, r.connection_id, r.saved_query_id, r.connector_id, r.name, r.description,
               r.sql_text, r.metric_column, r.comparator, r.threshold_value,
               r.schedule_interval_secs, r.is_active, r.created_by,
               u.role AS creator_role
        FROM action_rules r
        LEFT JOIN users u ON u.id = r.created_by
        WHERE r.id = $1 AND r.tenant_id = $2
        "#,
    )
    .bind(rule_id)
    .bind(tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Action monitoring rule not found.".to_string()))?;

    let rule_name: String = row.get("name");
    let sql_text: String = row.get("sql_text");
    let connection_id: Option<Uuid> = row.get("connection_id");
    let connector_id: Option<Uuid> = row.get("connector_id");
    let metric_column: String = row.get("metric_column");
    let comparator: String = row.get("comparator");
    let threshold_value: f64 = row.get("threshold_value");
    let created_by: Option<Uuid> = row.get("created_by");
    let creator_role: String = row
        .try_get::<Option<String>, _>("creator_role")
        .ok()
        .flatten()
        .unwrap_or_else(|| "analyst".to_string());

    let policy = load_policy_for_role(&state.app_pool, tenant_id, &creator_role).await?;
    let conn_params =
        load_active_connection_params(&state.app_pool, tenant_id, connection_id, &state.cipher)
            .await?;

    if !policy.is_database_allowed(&conn_params.database_name) {
        return Err(AppError::Forbidden(format!(
            "Rule owner role '{}' is not authorized to query database '{}'.",
            policy.role, conn_params.database_name
        )));
    }

    let adapter = state.analytics_adapter(&conn_params).await?;
    let catalog = load_latest_schema_or_refresh_with_adapter(
        &state.app_pool,
        tenant_id,
        &conn_params,
        &adapter,
    )
    .await?;
    let filtered = filter_catalog_for_policy(&catalog, &policy);
    let report = validate_sql(
        &sql_text,
        &filtered,
        &policy,
        &conn_params.allowed_schemas,
        None,
    );

    if !report.is_valid {
        let rej = report
            .rejection_reason
            .unwrap_or_else(|| "Monitoring SQL rejected by AST validator.".to_string());
        record_audit_event(
            &state.app_pool,
            tenant_id,
            actor_user_id.or(created_by),
            actor_email,
            "action_rule_evaluate",
            "action_rule",
            &rule_id.to_string(),
            "rejected",
            json!({"rule_name": rule_name, "reason": rej}),
        )
        .await;
        return Err(AppError::SqlValidationRejected(rej));
    }

    let exec_sql = report.executable_sql.clone().unwrap_or_default();
    let exec_ctx = ExecutionContext {
        timeout_ms: state.config.query_timeout_ms,
        row_limit: report.applied_row_limit,
        max_result_bytes: state.config.max_result_bytes,
        tenant_region: policy.region_filter.clone(),
    };

    let raw = adapter.execute_readonly_query(&exec_sql, &exec_ctx).await?;
    let summary = verify_execution_result(&raw, "USD", "UTC");
    let query_id = Uuid::new_v4();
    let exec_user_id = actor_user_id.or(created_by).ok_or_else(|| {
        AppError::Internal("Rule evaluation missing user provenance id".to_string())
    })?;

    if let Err(err) = sqlx::query(
        r#"
        INSERT INTO query_executions (
            id, tenant_id, user_id, connection_id, question, provider_used, model_used,
            generated_sql, validated_sql, schema_version, validation_status, validation_details,
            execution_status, execution_duration_ms, rows_returned, is_truncated,
            correction_attempts, result_summary, chart_spec, insight_report, error_message
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, '[]'::jsonb, $17, '{}'::jsonb, '{}'::jsonb, NULL)
        "#,
    )
    .bind(query_id)
    .bind(tenant_id)
    .bind(exec_user_id)
    .bind(conn_params.id)
    .bind(format!("[Action Monitor] {rule_name}"))
    .bind("action_rule_monitor")
    .bind("sqlparser-postgresql-ast")
    .bind(&sql_text)
    .bind(&exec_sql)
    .bind(filtered.version)
    .bind("validation_passed")
    .bind(serde_json::to_value(&report).unwrap_or_default())
    .bind("completed_with_rows")
    .bind(raw.duration_ms as i32)
    .bind(raw.rows_returned as i32)
    .bind(raw.is_truncated)
    .bind(serde_json::to_value(&summary).unwrap_or_default())
    .execute(&state.app_pool)
    .await
    {
        tracing::warn!(rule_id = %rule_id, error = %err, "Failed persisting rule evaluation query_executions");
    }

    sqlx::query("UPDATE action_rules SET last_evaluated_at = NOW() WHERE id = $1")
        .bind(rule_id)
        .execute(&state.app_pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let observed = extract_observed_metric(&raw, &metric_column, &comparator);
    let triggered = observed
        .map(|v| check_threshold(v, &comparator, threshold_value))
        .unwrap_or(false);

    let mut created_proposal: Option<ActionProposalDto> = None;
    if triggered {
        let obs_val = observed.unwrap_or(0.0);
        let sample_rows: Vec<Value> = raw
            .rows
            .iter()
            .take(5)
            .map(|r| {
                let mut map = serde_json::Map::new();
                for (idx, col) in raw.columns.iter().enumerate() {
                    map.insert(col.name.clone(), r.get(idx).cloned().unwrap_or(Value::Null));
                }
                Value::Object(map)
            })
            .collect();

        let evidence = json!({
            "source_query_id": query_id,
            "rule_id": rule_id,
            "rule_name": rule_name,
            "metric_column": metric_column,
            "comparator": comparator,
            "observed_value": obs_val,
            "threshold_value": threshold_value,
            "row_count": raw.rows_returned,
            "validated_sql": exec_sql,
            "sample_rows": sample_rows,
            "evaluated_at": Utc::now(),
        });

        let payload = json!({
            "event": "data_to_action.alert_proposal",
            "title": format!("Threshold triggered: {rule_name}"),
            "observed_value": obs_val,
            "threshold_value": threshold_value,
            "comparator": comparator,
            "source_query_id": query_id,
            "sample_rows": sample_rows,
        });

        // Deduplicate pending proposals if an identical active pending proposal already exists for this rule
        let existing_pending: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM action_proposals WHERE tenant_id = $1 AND rule_id = $2 AND status = 'pending' LIMIT 1",
        )
        .bind(tenant_id)
        .bind(rule_id)
        .fetch_optional(&state.app_pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        if existing_pending.is_none() {
            let prop_id = Uuid::new_v4();
            let idem_key = format!("rule-{rule_id}-q-{query_id}");
            let rationale = format!(
                "Read-only monitor '{rule_name}' observed {metric_column} = {obs_val:.2} ({comparator} {threshold_value:.2}) across {} verified row(s) [query_id={query_id}].",
                raw.rows_returned
            );

            let p_row = sqlx::query(
                r#"
                INSERT INTO action_proposals (
                    id, tenant_id, rule_id, connector_id, source_query_id, title, rationale,
                    evidence_json, payload_json, status, idempotency_key, proposed_by
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending', $10, $11)
                RETURNING id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
                          status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
                "#,
            )
            .bind(prop_id)
            .bind(tenant_id)
            .bind(rule_id)
            .bind(connector_id)
            .bind(query_id)
            .bind(format!("Alert: {rule_name}"))
            .bind(&rationale)
            .bind(&evidence)
            .bind(&payload)
            .bind(&idem_key)
            .bind(actor_user_id.or(created_by))
            .fetch_one(&state.app_pool)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

            created_proposal = Some(row_to_proposal_dto(&p_row));
        }
    }

    record_audit_event(
        &state.app_pool,
        tenant_id,
        actor_user_id.or(created_by),
        actor_email,
        "action_rule_evaluate",
        "action_rule",
        &rule_id.to_string(),
        if triggered { "triggered" } else { "clear" },
        json!({"query_id": query_id, "observed_value": observed, "threshold_value": threshold_value}),
    )
    .await;

    Ok(RuleEvaluationSummary {
        rule_id,
        query_id,
        triggered,
        observed_value: observed,
        threshold_value,
        comparator,
        row_count: raw.rows_returned,
        proposal: created_proposal,
    })
}

/// Dispatches an APPROVED action proposal through its configured non-destructive connector
/// using AES-256-GCM decrypted credentials, idempotency key, and bounded retries.
pub async fn dispatch_approved_proposal(
    state: &AppState,
    tenant_id: Uuid,
    proposal_id: Uuid,
    actor_user_id: Uuid,
    actor_email: &str,
) -> Result<(ActionProposalDto, ActionExecutionDto), AppError> {
    let p_row = sqlx::query(
        r#"
        SELECT id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
               status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
        FROM action_proposals
        WHERE id = $1 AND tenant_id = $2
        "#,
    )
    .bind(proposal_id)
    .bind(tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Action proposal not found.".to_string()))?;

    let status: String = p_row.get("status");
    if status == "executed" {
        return Err(AppError::BadRequest(
            "Action proposal has already been executed (idempotency guard).".to_string(),
        ));
    }
    if status != "approved" {
        return Err(AppError::BadRequest(format!(
            "Action proposal must be 'approved' before execution (current status: '{status}')."
        )));
    }

    let connector_id: Option<Uuid> = p_row.get("connector_id");
    let conn_id = connector_id.ok_or_else(|| {
        AppError::BadRequest("Action proposal has no target connector_id assigned.".to_string())
    })?;

    let c_row = sqlx::query(
        r#"
        SELECT id, name, connector_type, endpoint_url, encrypted_secret, default_headers, is_active
        FROM action_connectors
        WHERE id = $1 AND tenant_id = $2 AND is_active = TRUE
        "#,
    )
    .bind(conn_id)
    .bind(tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Assigned active action connector not found.".to_string()))?;

    let connector_type: String = c_row.get("connector_type");
    let endpoint_url: String = c_row.get("endpoint_url");
    let enc_secret: String = c_row.get("encrypted_secret");
    let default_headers: HashMap<String, String> =
        serde_json::from_value(c_row.get("default_headers")).unwrap_or_default();

    let secret = if enc_secret.trim().is_empty() {
        String::new()
    } else {
        state.cipher.decrypt(&enc_secret).map_err(AppError::Internal)?
    };

    let proposal_title: String = p_row.get("title");
    let rationale: String = p_row.get("rationale");
    let source_query_id: Option<Uuid> = p_row.get("source_query_id");
    let evidence_json: Value = p_row.get("evidence_json");
    let payload_json: Value = p_row.get("payload_json");
    let idempotency_key: String = p_row.get("idempotency_key");

    let outbound_body = match connector_type.as_str() {
        "slack_teams" => json!({
            "text": format!("[DATA-TO-ACTION] {proposal_title}\n{rationale}"),
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!("*{}*\n{}\n_Verified Query ID: {}_", proposal_title, rationale, source_query_id.map(|u| u.to_string()).unwrap_or_else(|| "n/a".to_string()))
                    }
                }
            ],
            "evidence": evidence_json,
            "idempotency_key": idempotency_key,
        }),
        "email_ticket_payload" => json!({
            "channel": "email_ticket_payload",
            "subject": format!("[DATA-TO-ACTION] {proposal_title}"),
            "summary": rationale,
            "source_query_id": source_query_id,
            "evidence": evidence_json,
            "payload": payload_json,
            "idempotency_key": idempotency_key,
        }),
        _ => json!({
            "proposal_id": proposal_id,
            "title": proposal_title,
            "rationale": rationale,
            "source_query_id": source_query_id,
            "evidence": evidence_json,
            "payload": payload_json,
            "idempotency_key": idempotency_key,
            "dispatched_at": Utc::now(),
        }),
    };

    let mut attempt_count = 0_i32;
    let mut http_status: Option<i32> = None;
    let outcome: String;
    let response_summary: String;

    if connector_type == "email_ticket_payload" && endpoint_url.trim().is_empty() {
        attempt_count = 1;
        outcome = "staged_payload_ready".to_string();
        response_summary = format!(
            "Structured non-destructive email/ticket payload generated and audited (no SMTP/webhook URL configured): {}",
            outbound_body
        );
    } else {
        let valid_url = validate_connector_url(&connector_type, &endpoint_url)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| AppError::Internal(format!("HTTP client init failed: {e}")))?;

        let mut last_err = String::new();
        let mut delivered = false;
        let mut final_summary = String::new();

        for attempt in 1..=3 {
            attempt_count = attempt;
            let mut req = client
                .post(&valid_url)
                .header("Content-Type", "application/json")
                .header("X-Idempotency-Key", &idempotency_key);
            if !secret.is_empty() {
                req = req.header("X-Action-Secret", &secret);
            }
            for (k, v) in &default_headers {
                req = req.header(k.as_str(), v.as_str());
            }

            match req.json(&outbound_body).send().await {
                Ok(resp) => {
                    let code = resp.status().as_u16() as i32;
                    http_status = Some(code);
                    let body_txt = resp.text().await.unwrap_or_default();
                    let clipped: String = body_txt.chars().take(300).collect();
                    if (200..300).contains(&code) {
                        delivered = true;
                        final_summary = format!("HTTP {code}: {clipped}");
                        break;
                    }
                    last_err = format!("HTTP {code}: {clipped}");
                    if code < 500 && code != 429 {
                        break;
                    }
                }
                Err(e) => {
                    last_err = format!("Transport error: {e}");
                }
            }

            if attempt < 3 {
                tokio::time::sleep(Duration::from_millis((attempt as u64) * 150)).await;
            }
        }

        if delivered {
            outcome = "executed".to_string();
            response_summary = final_summary;
        } else {
            outcome = "failed".to_string();
            response_summary = last_err;
        }
    }

    let next_proposal_status = if outcome == "failed" {
        "failed"
    } else {
        "executed"
    };

    let updated_p = sqlx::query(
        r#"
        UPDATE action_proposals
        SET status = $3, executed_at = NOW()
        WHERE id = $1 AND tenant_id = $2
        RETURNING id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
                  status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
        "#,
    )
    .bind(proposal_id)
    .bind(tenant_id)
    .bind(next_proposal_status)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let exec_id = Uuid::new_v4();
    let e_row = sqlx::query(
        r#"
        INSERT INTO action_executions (
            id, tenant_id, proposal_id, connector_id, idempotency_key,
            attempt_count, http_status, outcome, response_summary, executed_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING id, proposal_id, connector_id, idempotency_key, attempt_count, http_status, outcome, response_summary, executed_by, created_at
        "#,
    )
    .bind(exec_id)
    .bind(tenant_id)
    .bind(proposal_id)
    .bind(conn_id)
    .bind(&idempotency_key)
    .bind(attempt_count)
    .bind(http_status)
    .bind(&outcome)
    .bind(&response_summary)
    .bind(actor_user_id)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        tenant_id,
        Some(actor_user_id),
        actor_email,
        "action_execute",
        "action_proposal",
        &proposal_id.to_string(),
        &outcome,
        json!({
            "connector_id": conn_id,
            "connector_type": connector_type,
            "attempt_count": attempt_count,
            "http_status": http_status,
            "idempotency_key": idempotency_key,
        }),
    )
    .await;

    Ok((row_to_proposal_dto(&updated_p), row_to_execution_dto(&e_row)))
}

/// Spawns a background Tokio scheduler that evaluates due active `action_rules`
/// strictly read-only and raises `pending` proposals when thresholds trigger.
pub fn spawn_action_scheduler(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.tick().await; // skip immediate first tick at boot
        loop {
            ticker.tick().await;
            let due_rows = sqlx::query(
                r#"
                SELECT id, tenant_id
                FROM action_rules
                WHERE is_active = TRUE
                  AND (last_evaluated_at IS NULL OR last_evaluated_at + (schedule_interval_secs * INTERVAL '1 second') <= NOW())
                ORDER BY created_at ASC
                LIMIT 20
                "#,
            )
            .fetch_all(&state.app_pool)
            .await;

            match due_rows {
                Ok(rows) => {
                    for row in rows {
                        let rule_id: Uuid = row.get("id");
                        let tenant_id: Uuid = row.get("tenant_id");
                        if let Err(err) = evaluate_rule_readonly(
                            &state,
                            tenant_id,
                            rule_id,
                            None,
                            "system-scheduler@dta.local",
                        )
                        .await
                        {
                            tracing::warn!(rule_id = %rule_id, error = %err, "Scheduled action rule evaluation failed");
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "Action scheduler failed querying due rules");
                }
            }
        }
    });
}

async fn load_policy_for_role(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    role: &str,
) -> Result<RbacPolicy, AppError> {
    let role_clean = Role::parse(role)?.as_str().to_string();
    let r = sqlx::query(
        r#"
        SELECT role, allowed_databases, allowed_schemas, allowed_tables, denied_columns,
               allow_export, allow_schema_refresh, allow_admin_config, row_limit, region_filter
        FROM rbac_policies
        WHERE tenant_id = $1 AND role = $2
        "#,
    )
    .bind(tenant_id)
    .bind(&role_clean)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound(format!("RBAC policy '{role_clean}' not found")))?;

    let row_limit: i32 = r.get("row_limit");
    Ok(RbacPolicy {
        role: r.get("role"),
        allowed_databases: serde_json::from_value(r.get("allowed_databases")).unwrap_or_default(),
        allowed_schemas: serde_json::from_value(r.get("allowed_schemas")).unwrap_or_default(),
        allowed_tables: serde_json::from_value(r.get("allowed_tables")).unwrap_or_default(),
        denied_columns: serde_json::from_value(r.get("denied_columns")).unwrap_or_default(),
        allow_export: r.get("allow_export"),
        allow_schema_refresh: r.get("allow_schema_refresh"),
        allow_admin_config: r.get("allow_admin_config"),
        row_limit: row_limit.max(1) as u32,
        region_filter: r.get("region_filter"),
    })
}

pub fn row_to_proposal_dto(r: &sqlx::postgres::PgRow) -> ActionProposalDto {
    ActionProposalDto {
        id: r.get("id"),
        rule_id: r.get("rule_id"),
        connector_id: r.get("connector_id"),
        source_query_id: r.get("source_query_id"),
        title: r.get("title"),
        rationale: r.get("rationale"),
        evidence_json: r.get("evidence_json"),
        payload_json: r.get("payload_json"),
        status: r.get("status"),
        idempotency_key: r.get("idempotency_key"),
        proposed_by: r.get("proposed_by"),
        reviewed_by: r.get("reviewed_by"),
        review_note: r.get("review_note"),
        reviewed_at: r.get("reviewed_at"),
        executed_at: r.get("executed_at"),
        created_at: r.get("created_at"),
    }
}

pub fn row_to_execution_dto(r: &sqlx::postgres::PgRow) -> ActionExecutionDto {
    ActionExecutionDto {
        id: r.get("id"),
        proposal_id: r.get("proposal_id"),
        connector_id: r.get("connector_id"),
        idempotency_key: r.get("idempotency_key"),
        attempt_count: r.get("attempt_count"),
        http_status: r.get("http_status"),
        outcome: r.get("outcome"),
        response_summary: r.get("response_summary"),
        executed_by: r.get("executed_by"),
        created_at: r.get("created_at"),
    }
}
