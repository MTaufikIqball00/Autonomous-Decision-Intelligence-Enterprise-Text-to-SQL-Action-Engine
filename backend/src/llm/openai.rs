use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::{
    error::{sanitize_error_message, AppError},
    llm::{
        prompt::{
            build_correction_user_prompt, build_insight_prompt, build_sql_system_prompt,
            build_sql_user_prompt,
        },
        LlmInsightOutput, LlmInsightRequest, LlmProvider, LlmSqlCorrectionRequest,
        LlmSqlGenerationRequest, LlmSqlOutput,
    },
};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: &str, model: &str, base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(25))
            .build()
            .unwrap_or_else(|_| Client::new());
        let cleaned_base = if base_url.trim().is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        Self {
            client,
            api_key: api_key.trim().to_string(),
            model: model.trim().to_string(),
            base_url: cleaned_base,
        }
    }

    async fn chat_json(&self, system_prompt: &str, user_prompt: &str) -> Result<Value, AppError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": self.model,
            "temperature": 0.0,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ]
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::LlmError(sanitize_error_message(&e.to_string())))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AppError::LlmError(sanitize_error_message(&e.to_string())))?;

        if !status.is_success() {
            return Err(AppError::LlmError(format!(
                "OpenAI-compatible endpoint returned HTTP {}: {}",
                status.as_u16(),
                sanitize_error_message(&text)
            )));
        }

        let envelope: Value = serde_json::from_str(&text)
            .map_err(|e| AppError::LlmError(format!("Invalid JSON envelope from OpenAI: {e}")))?;

        let content = envelope["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                AppError::LlmError("OpenAI response missing choices[0].message.content".to_string())
            })?;

        serde_json::from_str(content)
            .map_err(|e| AppError::LlmError(format!("OpenAI content was not valid JSON: {e}")))
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn provider_id(&self) -> &str {
        "openai"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn is_external_llm(&self) -> bool {
        true
    }

    async fn generate_sql(&self, req: LlmSqlGenerationRequest<'_>) -> Result<LlmSqlOutput, AppError> {
        let user_prompt = build_sql_user_prompt(&req);
        let val = self
            .chat_json(build_sql_system_prompt(), &user_prompt)
            .await?;
        serde_json::from_value(val)
            .map_err(|e| AppError::LlmError(format!("Malformed SQL output schema: {e}")))
    }

    async fn correct_sql(&self, req: LlmSqlCorrectionRequest<'_>) -> Result<LlmSqlOutput, AppError> {
        let user_prompt = build_correction_user_prompt(&req);
        let val = self
            .chat_json(build_sql_system_prompt(), &user_prompt)
            .await?;
        serde_json::from_value(val)
            .map_err(|e| AppError::LlmError(format!("Malformed corrected SQL schema: {e}")))
    }

    async fn summarize_results(
        &self,
        req: LlmInsightRequest<'_>,
    ) -> Result<LlmInsightOutput, AppError> {
        let prompt = build_insight_prompt(&req);
        let val = self
            .chat_json(
                "You are a factual enterprise data analyst. Never fabricate numbers.",
                &prompt,
            )
            .await?;
        serde_json::from_value(val)
            .map_err(|e| AppError::LlmError(format!("Malformed insight JSON schema: {e}")))
    }
}
