use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use dta_backend::{
    api::build_router,
    auth::JwtService,
    config::AppConfig,
    crypto::SecretCipher,
    db::app_db::{bootstrap_app_database, connect_app_pool, load_active_connection_params},
    schema::discovery::load_latest_schema_or_refresh,
    state::AppState,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dta_backend=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = AppConfig::from_env().unwrap_or_else(|e| {
        tracing::error!("Configuration error: {e}");
        std::process::exit(1);
    });

    let cipher = SecretCipher::from_hex(&config.master_key_hex).unwrap_or_else(|e| {
        tracing::error!("Master key error: {e}");
        std::process::exit(1);
    });

    let app_pool = connect_app_pool(&config).await.unwrap_or_else(|e| {
        tracing::error!("{e}");
        std::process::exit(1);
    });

    let (tenant_id, connection_id) = bootstrap_app_database(&app_pool, &config, &cipher)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Application database bootstrap error: {e}");
            std::process::exit(1);
        });

    if let Ok(conn_params) =
        load_active_connection_params(&app_pool, tenant_id, Some(connection_id), &cipher).await
    {
        match load_latest_schema_or_refresh(&app_pool, tenant_id, &conn_params).await {
            Ok(cat) => tracing::info!(
                "Schema catalog ready: version={}, tables={}, database={}",
                cat.version,
                cat.tables.len(),
                cat.database_name
            ),
            Err(e) => tracing::warn!("Initial schema discovery deferred: {e}"),
        }
    }

    let state = AppState {
        jwt: JwtService::new(&config.jwt_secret),
        cipher,
        app_pool,
        config: config.clone(),
    };

    let router = build_router(state);
    let addr: SocketAddr = format!("{}:{}", config.backend_host, config.backend_port)
        .parse()
        .expect("valid backend bind address");

    tracing::info!("DATA-TO-ACTION Rust backend listening on http://{addr}");
    let listener = TcpListener::bind(addr)
        .await
        .expect("bind backend listener");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("serve backend");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("Shutdown signal received, closing server cleanly.");
}
