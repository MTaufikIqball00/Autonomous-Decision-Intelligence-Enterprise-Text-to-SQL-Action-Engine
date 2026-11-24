use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    db::RawExecutionResult,
    llm::ColumnStatSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedExecutionStatus {
    CompletedWithRows,
    CompletedEmpty,
    CompletedTruncated,
    ValidationRejected,
    ExecutionTimeout,
    ExecutionFailed,
    DbUnavailable,
    AuthorizationDenied,
    ClarificationRequired,
    CannotCompute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedResultSummary {
    pub status: VerifiedExecutionStatus,
    pub row_count: usize,
    pub column_count: usize,
    pub is_truncated: bool,
    pub truncation_reason: Option<String>,
    pub bytes_read: usize,
    pub duration_ms: u64,
    pub currency_unit: String,
    pub timezone: String,
    pub has_nulls: bool,
    pub non_finite_numeric_count: usize,
    pub column_stats: Vec<ColumnStatSummary>,
    pub db_identity: String,
}

pub fn verify_execution_result(
    raw: &RawExecutionResult,
    currency_unit: &str,
    timezone: &str,
) -> VerifiedResultSummary {
    let mut stats = Vec::with_capacity(raw.columns.len());
    let mut has_nulls = false;
    let mut non_finite_numeric_count = 0usize;

    for (col_idx, col_meta) in raw.columns.iter().enumerate() {
        let mut non_null = 0usize;
        let mut null_count = 0usize;
        let mut nums: Vec<f64> = Vec::new();

        for row in &raw.rows {
            match row.get(col_idx) {
                Some(Value::Null) | None => {
                    null_count += 1;
                    has_nulls = true;
                }
                Some(Value::Number(n)) => {
                    non_null += 1;
                    if let Some(f) = n.as_f64() {
                        if f.is_finite() {
                            nums.push(f);
                        } else {
                            non_finite_numeric_count += 1;
                        }
                    }
                }
                Some(_) => {
                    non_null += 1;
                }
            }
        }

        let (sum, min, max, avg) = if !nums.is_empty() && is_numeric_type(&col_meta.type_name) {
            let sum: f64 = nums.iter().sum();
            let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
            let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let avg = sum / (nums.len() as f64);
            (
                Some((sum * 100.0).round() / 100.0),
                Some((min * 100.0).round() / 100.0),
                Some((max * 100.0).round() / 100.0),
                Some((avg * 100.0).round() / 100.0),
            )
        } else {
            (None, None, None, None)
        };

        stats.push(ColumnStatSummary {
            column: col_meta.name.clone(),
            type_name: col_meta.type_name.clone(),
            non_null_count: non_null,
            null_count,
            numeric_sum: sum,
            numeric_min: min,
            numeric_max: max,
            numeric_avg: avg,
        });
    }

    let status = if raw.is_truncated {
        VerifiedExecutionStatus::CompletedTruncated
    } else if raw.rows_returned == 0 {
        VerifiedExecutionStatus::CompletedEmpty
    } else {
        VerifiedExecutionStatus::CompletedWithRows
    };

    VerifiedResultSummary {
        status,
        row_count: raw.rows_returned,
        column_count: raw.columns.len(),
        is_truncated: raw.is_truncated,
        truncation_reason: raw.truncation_reason.clone(),
        bytes_read: raw.bytes_read,
        duration_ms: raw.duration_ms,
        currency_unit: currency_unit.to_string(),
        timezone: timezone.to_string(),
        has_nulls,
        non_finite_numeric_count,
        column_stats: stats,
        db_identity: raw.db_identity.clone(),
    }
}

pub fn is_numeric_type(type_name: &str) -> bool {
    matches!(
        type_name.to_uppercase().as_str(),
        "INT2" | "INT4" | "INT8" | "FLOAT4" | "FLOAT8" | "NUMERIC"
    )
}

pub fn is_temporal_type_or_name(type_name: &str, col_name: &str) -> bool {
    let t = type_name.to_uppercase();
    let n = col_name.to_lowercase();
    matches!(t.as_str(), "DATE" | "TIMESTAMP" | "TIMESTAMPTZ")
        || n.contains("month")
        || n.contains("date")
        || n.contains("quarter")
        || n.contains("year")
        || n.contains("period")
}
