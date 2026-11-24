pub mod admin;
pub mod audit;
pub mod auth;
pub mod connections;
pub mod dashboards;
pub mod health;
pub mod overview;
pub mod providers;
pub mod queries;
pub mod saved_queries;
pub mod schema;
pub mod semantics;

use axum::{
    http::{header, Method},
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let origins = [
        state.config.frontend_origin.parse().expect("valid origin"),
        format!("http://localhost:{}", state.config.frontend_origin.rsplit(':').next().unwrap_or("3420"))
            .parse()
            .expect("valid localhost origin"),
    ];

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    let api_v1 = Router::new()
        .route("/health", get(health::health_check))
        .route("/auth/login", post(auth::login))
        .route("/auth/me", get(auth::me))
        .route("/overview", get(overview::get_overview))
        .route("/queries/ask", post(queries::ask_question))
        .route("/queries/validate", post(queries::validate_sql_preview))
        .route("/queries/execute-validated", post(queries::execute_validated_sql))
        .route("/queries/history", get(queries::list_history))
        .route("/queries/export", post(queries::export_csv))
        .route("/schema", get(schema::get_schema))
        .route("/schema/refresh", post(schema::refresh_schema))
        .route(
            "/schema/relationships",
            get(schema::list_manual_relationships).post(schema::add_manual_relationship),
        )
        .route(
            "/semantics",
            get(semantics::list_metrics).post(semantics::upsert_metric),
        )
        .route("/semantics/versions", get(semantics::list_all_versions))
        .route(
            "/saved-queries",
            get(saved_queries::list_saved).post(saved_queries::create_saved),
        )
        .route("/saved-queries/{id}", delete(saved_queries::delete_saved))
        .route(
            "/dashboards",
            get(dashboards::list_dashboards).post(dashboards::create_dashboard),
        )
        .route("/dashboards/{id}", delete(dashboards::delete_dashboard))
        .route(
            "/connections",
            get(connections::list_connections).post(connections::create_connection),
        )
        .route("/connections/{id}/test", post(connections::test_connection))
        .route(
            "/providers",
            get(providers::list_providers).post(providers::create_provider),
        )
        .route("/providers/{id}/activate", post(providers::activate_provider))
        .route("/admin/users", get(admin::list_users))
        .route("/admin/rbac", get(admin::list_rbac_policies))
        .route("/admin/rbac/{role}", put(admin::update_rbac_policy))
        .route("/audit", get(audit::list_audit_logs));

    Router::new()
        .nest("/api/v1", api_v1)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
