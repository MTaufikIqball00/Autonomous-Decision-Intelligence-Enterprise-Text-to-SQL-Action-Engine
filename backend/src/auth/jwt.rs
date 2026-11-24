use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Clone)]
pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtService {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    pub fn issue_token(&self, user_id: Uuid, tenant_id: Uuid, email: &str) -> Result<String, String> {
        let now = Utc::now();
        let exp = (now + Duration::hours(12)).timestamp() as usize;
        let iat = now.timestamp() as usize;
        let claims = Claims {
            sub: user_id,
            tenant_id,
            email: email.to_string(),
            exp,
            iat,
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| format!("JWT encoding failed: {e}"))
    }

    pub fn verify_token(&self, token: &str) -> Result<Claims, String> {
        let validation = Validation::default();
        decode::<Claims>(token, &self.decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(|e| format!("JWT verification failed: {e}"))
    }
}
