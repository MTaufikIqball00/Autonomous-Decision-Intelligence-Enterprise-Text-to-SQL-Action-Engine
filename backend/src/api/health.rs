use axum::{extract::State, Json};
use serde::Serialize;

use crate::{error::AppError, state::AppState};

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub app_database: &'static str,
    pub backend_port: u16,
}

pub async fn health_check(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.app_pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(HealthResponse {
        status: "ok",
        service: "data-to-action-backend",
        version: env!("CARGO_PKG_VERSION"),
        app_database: "connected",
        backend_port: state.config.backend_port,
    }))
}
