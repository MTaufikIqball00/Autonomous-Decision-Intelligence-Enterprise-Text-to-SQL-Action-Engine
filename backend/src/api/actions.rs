use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{AuthUser, Role},
    db::app_db::{load_active_connection_params, record_audit_event},
    engine::actions::{
        dispatch_approved_proposal, evaluate_rule_readonly, row_to_execution_dto,
        row_to_proposal_dto, validate_connector_url, ActionConnectorDto, ActionExecutionDto,
        ActionProposalDto, ActionRuleDto, RuleEvaluationSummary,
    },
    error::AppError,
    schema::{discovery::load_latest_schema_or_refresh_with_adapter, linker::filter_catalog_for_policy},
    sql_validator::validate_sql,
    state::AppState,
};

fn require_action_operator(user: &AuthUser) -> Result<(), AppError> {
    if matches!(user.role, Role::Administrator | Role::DataEngineer) {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!(
            "Role '{}' is not authorized to configure or approve Action Engine resources (requires Administrator or Data Engineer).",
            user.role.as_str()
        )))
    }
}

pub async fn list_connectors(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ActionConnectorDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, connector_type, endpoint_url, encrypted_secret, default_headers, is_active, created_at
        FROM action_connectors
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let enc: String = r.get("encrypted_secret");
                ActionConnectorDto {
                    id: r.get("id"),
                    name: r.get("name"),
                    connector_type: r.get("connector_type"),
                    endpoint_url: r.get("endpoint_url"),
                    default_headers: r.get("default_headers"),
                    has_secret: !enc.is_empty(),
                    is_active: r.get("is_active"),
                    created_at: r.get("created_at"),
                }
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct CreateConnectorRequest {
    pub name: String,
    pub connector_type: String,
    #[serde(default)]
    pub endpoint_url: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub default_headers: HashMap<String, String>,
}

pub async fn create_connector(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateConnectorRequest>,
) -> Result<Json<ActionConnectorDto>, AppError> {
    require_action_operator(&user)?;
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Connector name is required.".to_string()));
    }
    let ctype = req.connector_type.trim().to_lowercase();
    if !matches!(ctype.as_str(), "webhook" | "slack_teams" | "email_ticket_payload") {
        return Err(AppError::BadRequest(
            "connector_type must be 'webhook', 'slack_teams', or 'email_ticket_payload'.".to_string(),
        ));
    }
    let clean_url = validate_connector_url(&ctype, &req.endpoint_url)?;
    let enc_secret = if req.secret.is_empty() {
        String::new()
    } else {
        state
            .cipher
            .encrypt(&req.secret)
            .map_err(AppError::Internal)?
    };

    let id = Uuid::new_v4();
    let row = sqlx::query(
        r#"
        INSERT INTO action_connectors (
            id, tenant_id, name, connector_type, endpoint_url, encrypted_secret, default_headers, is_active, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, $8)
        RETURNING id, name, connector_type, endpoint_url, encrypted_secret, default_headers, is_active, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.name.trim())
    .bind(&ctype)
    .bind(&clean_url)
    .bind(&enc_secret)
    .bind(json!(req.default_headers))
    .bind(user.user_id)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "action_connector_create",
        "action_connector",
        &id.to_string(),
        "allowed",
        json!({"name": req.name.trim(), "connector_type": ctype}),
    )
    .await;

    let enc: String = row.get("encrypted_secret");
    Ok(Json(ActionConnectorDto {
        id: row.get("id"),
        name: row.get("name"),
        connector_type: row.get("connector_type"),
        endpoint_url: row.get("endpoint_url"),
        default_headers: row.get("default_headers"),
        has_secret: !enc.is_empty(),
        is_active: row.get("is_active"),
        created_at: row.get("created_at"),
    }))
}

pub async fn delete_connector(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    require_action_operator(&user)?;
    let res = sqlx::query("DELETE FROM action_connectors WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(user.tenant_id)
        .execute(&state.app_pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Connector not found.".to_string()));
    }
    Ok(Json(json!({"deleted": true, "id": id})))
}

pub async fn list_rules(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ActionRuleDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, connection_id, saved_query_id, connector_id, name, description,
               sql_text, metric_column, comparator, threshold_value, schedule_interval_secs,
               is_active, last_evaluated_at, created_at
        FROM action_rules
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| ActionRuleDto {
                id: r.get("id"),
                connection_id: r.get("connection_id"),
                saved_query_id: r.get("saved_query_id"),
                connector_id: r.get("connector_id"),
                name: r.get("name"),
                description: r.get("description"),
                sql_text: r.get("sql_text"),
                metric_column: r.get("metric_column"),
                comparator: r.get("comparator"),
                threshold_value: r.get("threshold_value"),
                schedule_interval_secs: r.get("schedule_interval_secs"),
                is_active: r.get("is_active"),
                last_evaluated_at: r.get("last_evaluated_at"),
                created_at: r.get("created_at"),
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub connection_id: Option<Uuid>,
    pub saved_query_id: Option<Uuid>,
    pub connector_id: Option<Uuid>,
    #[serde(default)]
    pub sql_text: String,
    pub metric_column: String,
    pub comparator: String,
    pub threshold_value: f64,
    pub schedule_interval_secs: Option<i32>,
}

pub async fn create_rule(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateRuleRequest>,
) -> Result<Json<ActionRuleDto>, AppError> {
    require_action_operator(&user)?;
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Rule name is required.".to_string()));
    }
    let comp = req.comparator.trim().to_lowercase();
    if !matches!(comp.as_str(), "gt" | "gte" | "lt" | "lte" | "eq" | "row_count_gt") {
        return Err(AppError::BadRequest(
            "comparator must be gt, gte, lt, lte, eq, or row_count_gt.".to_string(),
        ));
    }

    let mut sql_text = req.sql_text.trim().to_string();
    if sql_text.is_empty() {
        if let Some(sq_id) = req.saved_query_id {
            sql_text = sqlx::query_scalar::<_, String>(
                "SELECT sql_text FROM saved_queries WHERE id = $1 AND tenant_id = $2",
            )
            .bind(sq_id)
            .bind(user.tenant_id)
            .fetch_optional(&state.app_pool)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Referenced saved_query_id not found.".to_string()))?;
        }
    }
    if sql_text.is_empty() {
        return Err(AppError::BadRequest(
            "Either sql_text or saved_query_id is required.".to_string(),
        ));
    }

    // Validate SQL through the deterministic Rust AST validator BEFORE persisting!
    let conn_params = load_active_connection_params(
        &state.app_pool,
        user.tenant_id,
        req.connection_id,
        &state.cipher,
    )
    .await?;
    let adapter = state.analytics_adapter(&conn_params).await?;
    let catalog = load_latest_schema_or_refresh_with_adapter(
        &state.app_pool,
        user.tenant_id,
        &conn_params,
        &adapter,
    )
    .await?;
    let filtered = filter_catalog_for_policy(&catalog, &user.policy);
    let report = validate_sql(
        &sql_text,
        &filtered,
        &user.policy,
        &conn_params.allowed_schemas,
        None,
    );
    if !report.is_valid {
        return Err(AppError::SqlValidationRejected(
            report
                .rejection_reason
                .unwrap_or_else(|| "Rule SQL failed AST validation.".to_string()),
        ));
    }

    let interval = req.schedule_interval_secs.unwrap_or(300).clamp(30, 86400);
    let id = Uuid::new_v4();
    let row = sqlx::query(
        r#"
        INSERT INTO action_rules (
            id, tenant_id, connection_id, saved_query_id, connector_id, name, description,
            sql_text, metric_column, comparator, threshold_value, schedule_interval_secs, is_active, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, TRUE, $13)
        RETURNING id, connection_id, saved_query_id, connector_id, name, description,
                  sql_text, metric_column, comparator, threshold_value, schedule_interval_secs,
                  is_active, last_evaluated_at, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(Some(conn_params.id))
    .bind(req.saved_query_id)
    .bind(req.connector_id)
    .bind(req.name.trim())
    .bind(req.description.trim())
    .bind(&sql_text)
    .bind(req.metric_column.trim())
    .bind(&comp)
    .bind(req.threshold_value)
    .bind(interval)
    .bind(user.user_id)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "action_rule_create",
        "action_rule",
        &id.to_string(),
        "allowed",
        json!({"name": req.name.trim(), "comparator": comp, "threshold_value": req.threshold_value}),
    )
    .await;

    Ok(Json(ActionRuleDto {
        id: row.get("id"),
        connection_id: row.get("connection_id"),
        saved_query_id: row.get("saved_query_id"),
        connector_id: row.get("connector_id"),
        name: row.get("name"),
        description: row.get("description"),
        sql_text: row.get("sql_text"),
        metric_column: row.get("metric_column"),
        comparator: row.get("comparator"),
        threshold_value: row.get("threshold_value"),
        schedule_interval_secs: row.get("schedule_interval_secs"),
        is_active: row.get("is_active"),
        last_evaluated_at: row.get("last_evaluated_at"),
        created_at: row.get("created_at"),
    }))
}

pub async fn evaluate_rule(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<RuleEvaluationSummary>, AppError> {
    require_action_operator(&user)?;
    let summary = evaluate_rule_readonly(
        &state,
        user.tenant_id,
        id,
        Some(user.user_id),
        &user.email,
    )
    .await?;
    Ok(Json(summary))
}

pub async fn delete_rule(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    require_action_operator(&user)?;
    let res = sqlx::query("DELETE FROM action_rules WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(user.tenant_id)
        .execute(&state.app_pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Rule not found.".to_string()));
    }
    Ok(Json(json!({"deleted": true, "id": id})))
}

pub async fn list_proposals(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ActionProposalDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
               status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
        FROM action_proposals
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(rows.iter().map(row_to_proposal_dto).collect()))
}

#[derive(Deserialize)]
pub struct CreateProposalRequest {
    pub source_query_id: Uuid,
    pub connector_id: Option<Uuid>,
    pub title: String,
    pub rationale: String,
    #[serde(default)]
    pub evidence_json: Value,
    #[serde(default)]
    pub payload_json: Value,
    pub idempotency_key: Option<String>,
}

pub async fn create_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateProposalRequest>,
) -> Result<Json<ActionProposalDto>, AppError> {
    if !matches!(
        user.role,
        Role::Administrator | Role::DataEngineer | Role::Analyst
    ) {
        return Err(AppError::Forbidden(
            "Your role cannot propose operational actions.".to_string(),
        ));
    }
    if req.title.trim().is_empty() || req.rationale.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Proposal title and rationale citing verified query evidence are required.".to_string(),
        ));
    }

    // Verify source_query_id actually belongs to a validated query execution in this tenant
    let q_exists = sqlx::query(
        "SELECT question, validated_sql, result_summary FROM query_executions WHERE id = $1 AND tenant_id = $2 AND validation_status = 'validation_passed'",
    )
    .bind(req.source_query_id)
    .bind(user.tenant_id)
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| {
        AppError::BadRequest(
            "source_query_id must reference a verified, AST-validated query execution.".to_string(),
        )
    })?;

    let id = Uuid::new_v4();
    let idem_key = req
        .idempotency_key
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("manual-{id}"));

    let evidence = if req.evidence_json.is_object() {
        req.evidence_json
    } else {
        json!({
            "source_query_id": req.source_query_id,
            "question": q_exists.get::<String, _>("question"),
            "validated_sql": q_exists.get::<String, _>("validated_sql"),
            "result_summary": q_exists.get::<Value, _>("result_summary"),
        })
    };

    let row = sqlx::query(
        r#"
        INSERT INTO action_proposals (
            id, tenant_id, connector_id, source_query_id, title, rationale,
            evidence_json, payload_json, status, idempotency_key, proposed_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', $9, $10)
        RETURNING id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
                  status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.connector_id)
    .bind(req.source_query_id)
    .bind(req.title.trim())
    .bind(req.rationale.trim())
    .bind(&evidence)
    .bind(if req.payload_json.is_object() { req.payload_json } else { json!({}) })
    .bind(&idem_key)
    .bind(user.user_id)
    .fetch_one(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "action_proposal_create",
        "action_proposal",
        &id.to_string(),
        "allowed",
        json!({"source_query_id": req.source_query_id, "title": req.title.trim()}),
    )
    .await;

    Ok(Json(row_to_proposal_dto(&row)))
}

#[derive(Deserialize, Default)]
pub struct ReviewProposalRequest {
    #[serde(default)]
    pub review_note: String,
    pub connector_id: Option<Uuid>,
}

pub async fn approve_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    req_opt: Option<Json<ReviewProposalRequest>>,
) -> Result<Json<ActionProposalDto>, AppError> {
    require_action_operator(&user)?;
    let req = req_opt.map(|Json(r)| r).unwrap_or_default();

    let row = sqlx::query(
        r#"
        UPDATE action_proposals
        SET status = 'approved',
            connector_id = COALESCE($3, connector_id),
            reviewed_by = $4,
            review_note = $5,
            reviewed_at = NOW()
        WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
        RETURNING id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
                  status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.connector_id)
    .bind(user.user_id)
    .bind(req.review_note.trim())
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| {
        AppError::BadRequest("Proposal not found or is not in 'pending' status.".to_string())
    })?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "action_proposal_approve",
        "action_proposal",
        &id.to_string(),
        "approved",
        json!({"review_note": req.review_note.trim()}),
    )
    .await;

    Ok(Json(row_to_proposal_dto(&row)))
}

pub async fn reject_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    req_opt: Option<Json<ReviewProposalRequest>>,
) -> Result<Json<ActionProposalDto>, AppError> {
    require_action_operator(&user)?;
    let req = req_opt.map(|Json(r)| r).unwrap_or_default();

    let row = sqlx::query(
        r#"
        UPDATE action_proposals
        SET status = 'rejected',
            reviewed_by = $3,
            review_note = $4,
            reviewed_at = NOW()
        WHERE id = $1 AND tenant_id = $2 AND status IN ('pending', 'approved')
        RETURNING id, rule_id, connector_id, source_query_id, title, rationale, evidence_json, payload_json,
                  status, idempotency_key, proposed_by, reviewed_by, review_note, reviewed_at, executed_at, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(req.review_note.trim())
    .fetch_optional(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .ok_or_else(|| {
        AppError::BadRequest("Proposal not found or cannot be rejected in its current status.".to_string())
    })?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "action_proposal_reject",
        "action_proposal",
        &id.to_string(),
        "rejected",
        json!({"review_note": req.review_note.trim()}),
    )
    .await;

    Ok(Json(row_to_proposal_dto(&row)))
}

pub async fn execute_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    require_action_operator(&user)?;
    let (proposal, execution) = dispatch_approved_proposal(
        &state,
        user.tenant_id,
        id,
        user.user_id,
        &user.email,
    )
    .await?;
    Ok(Json(json!({
        "proposal": proposal,
        "execution": execution,
    })))
}

pub async fn list_executions(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ActionExecutionDto>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT id, proposal_id, connector_id, idempotency_key, attempt_count, http_status,
               outcome, response_summary, executed_by, created_at
        FROM action_executions
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&state.app_pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(rows.iter().map(row_to_execution_dto).collect()))
}
