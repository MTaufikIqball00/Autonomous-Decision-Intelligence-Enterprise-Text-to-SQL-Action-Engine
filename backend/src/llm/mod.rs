pub mod gemini;
pub mod local_deterministic;
pub mod openai;
pub mod prompt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    error::AppError,
    schema::{LinkedSchemaContext, SchemaCatalog},
    semantic::SemanticMetric,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub question: String,
    #[serde(default)]
    pub sql: String,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct LlmSqlGenerationRequest<'a> {
    pub question: &'a str,
    pub linked_schema: &'a LinkedSchemaContext,
    pub full_catalog: &'a SchemaCatalog,
    pub semantics: &'a [SemanticMetric],
    pub preferred_metric: Option<&'a str>,
    pub row_limit: u32,
    pub conversation_history: &'a [ConversationTurn],
}

#[derive(Debug, Clone)]
pub struct LlmSqlCorrectionRequest<'a> {
    pub question: &'a str,
    pub failed_sql: &'a str,
    pub error_class: &'a str,
    pub sanitized_error: &'a str,
    pub linked_schema: &'a LinkedSchemaContext,
    pub full_catalog: &'a SchemaCatalog,
    pub semantics: &'a [SemanticMetric],
    pub row_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSqlOutput {
    pub sql: Option<String>,
    pub explanation: String,
    pub assumptions: Vec<String>,
    pub applied_filters: Vec<String>,
    pub clarification_needed: Option<String>,
    pub clarification_options: Vec<String>,
    pub cannot_compute_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnStatSummary {
    pub column: String,
    pub type_name: String,
    pub non_null_count: usize,
    pub null_count: usize,
    pub numeric_sum: Option<f64>,
    pub numeric_min: Option<f64>,
    pub numeric_max: Option<f64>,
    pub numeric_avg: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct LlmInsightRequest<'a> {
    pub question: &'a str,
    pub sql: &'a str,
    pub columns: &'a [String],
    pub rows: &'a [Vec<Value>],
    pub stats: &'a [ColumnStatSummary],
    pub rows_returned: usize,
    pub is_truncated: bool,
    pub currency_unit: &'a str,
    pub applied_filters: &'a [String],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInsightOutput {
    pub observations: Vec<String>,
    pub interpretations: Vec<String>,
    pub limitations: Vec<String>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn model_name(&self) -> &str;
    fn is_external_llm(&self) -> bool;

    async fn generate_sql(&self, req: LlmSqlGenerationRequest<'_>) -> Result<LlmSqlOutput, AppError>;
    async fn correct_sql(&self, req: LlmSqlCorrectionRequest<'_>) -> Result<LlmSqlOutput, AppError>;
    async fn summarize_results(
        &self,
        req: LlmInsightRequest<'_>,
    ) -> Result<LlmInsightOutput, AppError>;
}

#[derive(Debug, Clone)]
pub struct ProviderConfigRecord {
    pub provider_type: String,
    pub model_name: String,
    pub base_url: String,
    pub api_key: String,
}

pub fn create_provider(record: &ProviderConfigRecord) -> Result<Box<dyn LlmProvider>, AppError> {
    match record.provider_type.to_lowercase().as_str() {
        "local_deterministic" => Ok(Box::new(local_deterministic::LocalDeterministicPlanner::new(
            &record.model_name,
        ))),
        "openai" => {
            if record.api_key.trim().is_empty() || record.model_name.trim().is_empty() {
                return Err(AppError::LlmError(
                    "OpenAI provider requires both an API key and an explicit model name."
                        .to_string(),
                ));
            }
            Ok(Box::new(openai::OpenAiProvider::new(
                &record.api_key,
                &record.model_name,
                &record.base_url,
            )))
        }
        "gemini" => {
            if record.api_key.trim().is_empty() || record.model_name.trim().is_empty() {
                return Err(AppError::LlmError(
                    "Gemini provider requires both an API key and an explicit model name."
                        .to_string(),
                ));
            }
            Ok(Box::new(gemini::GeminiProvider::new(
                &record.api_key,
                &record.model_name,
                &record.base_url,
            )))
        }
        other => Err(AppError::BadRequest(format!(
            "Unsupported LLM provider type: {other}"
        ))),
    }
}
