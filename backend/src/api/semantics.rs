use axum::{extract::State, Json};
use serde_json::json;

use crate::{
    auth::{AuthUser, Role},
    db::app_db::record_audit_event,
    error::AppError,
    semantic::{
        create_new_metric_version, list_active_metrics, list_all_metric_versions, SemanticMetric,
        UpsertSemanticMetricRequest,
    },
    state::AppState,
};

pub async fn list_metrics(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<SemanticMetric>>, AppError> {
    let items = list_active_metrics(&state.app_pool, user.tenant_id).await?;
    Ok(Json(items))
}

pub async fn list_all_versions(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<SemanticMetric>>, AppError> {
    let items = list_all_metric_versions(&state.app_pool, user.tenant_id).await?;
    Ok(Json(items))
}

pub async fn upsert_metric(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<UpsertSemanticMetricRequest>,
) -> Result<Json<SemanticMetric>, AppError> {
    if !matches!(user.role, Role::Administrator | Role::DataEngineer) {
        return Err(AppError::Forbidden(
            "Only Administrators or Data Engineers may create or version semantic metrics."
                .to_string(),
        ));
    }

    let metric = create_new_metric_version(&state.app_pool, user.tenant_id, user.user_id, req).await?;

    record_audit_event(
        &state.app_pool,
        user.tenant_id,
        Some(user.user_id),
        &user.email,
        "semantic_metric_upsert",
        "semantic_metric",
        &metric.id.to_string(),
        "allowed",
        json!({"metric_key": metric.metric_key, "version": metric.version}),
    )
    .await;

    Ok(Json(metric))
}
