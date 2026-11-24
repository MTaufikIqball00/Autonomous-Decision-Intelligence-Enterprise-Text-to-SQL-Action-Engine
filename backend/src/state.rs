use sqlx::PgPool;

use crate::{auth::JwtService, config::AppConfig, crypto::SecretCipher};

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub app_pool: PgPool,
    pub jwt: JwtService,
    pub cipher: SecretCipher,
}
