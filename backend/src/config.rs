use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_ssl_mode: String,
    pub app_database: String,
    pub app_db_user: String,
    pub app_db_password: String,
    pub analytics_database: String,
    pub readonly_db_user: String,
    pub readonly_db_password: String,
    pub backend_host: String,
    pub backend_port: u16,
    pub frontend_origin: String,
    pub jwt_secret: String,
    pub master_key_hex: String,
    pub allowed_schemas: Vec<String>,
    pub default_row_limit: u32,
    pub max_result_bytes: usize,
    pub query_timeout_ms: u64,
    pub self_correction_max_retries: u32,
    pub login_rate_limit_per_min: u32,
    pub query_rate_limit_per_min: u32,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
    pub openai_base_url: String,
    pub gemini_api_key: Option<String>,
    pub gemini_model: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        // Attempt loading .env from workspace root or parent directory
        let _ = dotenvy::dotenv();
        if env::var("DTA_APP_DB_PASSWORD").is_err() {
            let _ = dotenvy::from_path("../.env");
        }

        let pg_host = env_or("DTA_PG_HOST", "127.0.0.1");
        let pg_port = env_or("DTA_PG_PORT", "5439")
            .parse::<u16>()
            .map_err(|e| format!("Invalid DTA_PG_PORT: {e}"))?;
        let pg_ssl_mode = env_or("DTA_PG_SSL_MODE", "prefer");

        let app_database = env_or("DTA_APP_DATABASE", "data_to_action_app");
        let app_db_user = env_or("DTA_APP_DB_USER", "dta_app_user");
        let app_db_password = env_required("DTA_APP_DB_PASSWORD")?;

        let analytics_database = env_or("DTA_ANALYTICS_DATABASE", "data_to_action_analytics");
        let readonly_db_user = env_or("DTA_READONLY_DB_USER", "dta_readonly");
        let readonly_db_password = env_required("DTA_READONLY_DB_PASSWORD")?;

        let backend_host = env_or("DTA_BACKEND_HOST", "127.0.0.1");
        let backend_port = env_or("DTA_BACKEND_PORT", "8420")
            .parse::<u16>()
            .map_err(|e| format!("Invalid DTA_BACKEND_PORT: {e}"))?;
        let frontend_host = env_or("DTA_FRONTEND_HOST", "127.0.0.1");
        let frontend_port = env_or("DTA_FRONTEND_PORT", "3420");
        let frontend_origin = format!("http://{frontend_host}:{frontend_port}");

        let jwt_secret = env_required("DTA_JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            return Err("DTA_JWT_SECRET must be at least 32 characters".to_string());
        }

        let master_key_hex = env_required("DTA_MASTER_KEY")?;
        if master_key_hex.len() != 64 {
            return Err("DTA_MASTER_KEY must be a 64-character hex string (32 bytes)".to_string());
        }

        let allowed_schemas = env_or("DTA_ALLOWED_SCHEMAS", "analytics,finance")
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let default_row_limit = env_or("DTA_DEFAULT_ROW_LIMIT", "500")
            .parse::<u32>()
            .map_err(|e| format!("Invalid DTA_DEFAULT_ROW_LIMIT: {e}"))?;
        let max_result_bytes = env_or("DTA_MAX_RESULT_BYTES", "2097152")
            .parse::<usize>()
            .map_err(|e| format!("Invalid DTA_MAX_RESULT_BYTES: {e}"))?;
        let query_timeout_ms = env_or("DTA_QUERY_TIMEOUT_MS", "10000")
            .parse::<u64>()
            .map_err(|e| format!("Invalid DTA_QUERY_TIMEOUT_MS: {e}"))?;
        let self_correction_max_retries = env_or("DTA_SELF_CORRECTION_MAX_RETRIES", "2")
            .parse::<u32>()
            .map_err(|e| format!("Invalid DTA_SELF_CORRECTION_MAX_RETRIES: {e}"))?;
        let login_rate_limit_per_min = env_or("DTA_LOGIN_RATE_LIMIT", "30")
            .parse::<u32>()
            .map_err(|e| format!("Invalid DTA_LOGIN_RATE_LIMIT: {e}"))?;
        let query_rate_limit_per_min = env_or("DTA_QUERY_RATE_LIMIT", "60")
            .parse::<u32>()
            .map_err(|e| format!("Invalid DTA_QUERY_RATE_LIMIT: {e}"))?;

        Ok(Self {
            pg_host,
            pg_port,
            pg_ssl_mode,
            app_database,
            app_db_user,
            app_db_password,
            analytics_database,
            readonly_db_user,
            readonly_db_password,
            backend_host,
            backend_port,
            frontend_origin,
            jwt_secret,
            master_key_hex,
            allowed_schemas,
            default_row_limit,
            max_result_bytes,
            query_timeout_ms,
            self_correction_max_retries,
            login_rate_limit_per_min,
            query_rate_limit_per_min,
            openai_api_key: env_opt("OPENAI_API_KEY"),
            openai_model: env_opt("OPENAI_MODEL"),
            openai_base_url: env_or("OPENAI_BASE_URL", "https://api.openai.com/v1"),
            gemini_api_key: env_opt("GEMINI_API_KEY"),
            gemini_model: env_opt("GEMINI_MODEL"),
        })
    }

    pub fn app_database_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            self.app_db_user,
            self.app_db_password,
            self.pg_host,
            self.pg_port,
            self.app_database,
            self.pg_ssl_mode
        )
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_opt(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_required(key: &str) -> Result<String, String> {
    env_opt(key).ok_or_else(|| format!("Required environment variable {key} is missing or empty"))
}
