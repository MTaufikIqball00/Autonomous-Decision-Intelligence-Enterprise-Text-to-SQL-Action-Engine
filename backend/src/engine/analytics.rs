use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::RawExecutionResult,
    engine::verifier::{is_numeric_type, is_temporal_type_or_name},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    KpiSummary,
    Line,
    Area,
    Bar,
    Pie,
    Scatter,
    TableOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartProvenance {
    pub query_id: Uuid,
    pub source_columns: Vec<String>,
    pub aggregation_method: String,
    pub filters: Vec<String>,
    pub time_range: String,
    pub units: String,
    pub result_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartRecommendation {
    pub chart_suitable: bool,
    pub recommended_type: ChartKind,
    pub available_types: Vec<ChartKind>,
    pub x_axis: Option<String>,
    pub y_axes: Vec<String>,
    pub reason: String,
    pub provenance: ChartProvenance,
}

pub fn select_chart_spec(
    query_id: Uuid,
    raw: &RawExecutionResult,
    filters: &[String],
    units: &str,
    sql: &str,
) -> ChartRecommendation {
    let provenance = ChartProvenance {
        query_id,
        source_columns: raw.columns.iter().map(|c| c.name.clone()).collect(),
        aggregation_method: infer_aggregation_method(sql),
        filters: filters.to_vec(),
        time_range: infer_time_range(sql),
        units: units.to_string(),
        result_timestamp: Utc::now(),
    };

    if raw.rows_returned == 0 || raw.columns.is_empty() {
        return ChartRecommendation {
            chart_suitable: false,
            recommended_type: ChartKind::TableOnly,
            available_types: vec![ChartKind::TableOnly],
            x_axis: None,
            y_axes: vec![],
            reason: "Query returned 0 rows; no chart can be plotted without verified records.".to_string(),
            provenance,
        };
    }

    let numeric_cols: Vec<String> = raw
        .columns
        .iter()
        .filter(|c| is_numeric_type(&c.type_name))
        .map(|c| c.name.clone())
        .collect();

    let non_numeric_cols: Vec<String> = raw
        .columns
        .iter()
        .filter(|c| !is_numeric_type(&c.type_name))
        .map(|c| c.name.clone())
        .collect();

    let temporal_col = raw
        .columns
        .iter()
        .find(|c| is_temporal_type_or_name(&c.type_name, &c.name))
        .map(|c| c.name.clone());

    if numeric_cols.is_empty() {
        return ChartRecommendation {
            chart_suitable: false,
            recommended_type: ChartKind::TableOnly,
            available_types: vec![ChartKind::TableOnly],
            x_axis: None,
            y_axes: vec![],
            reason: "Result columns contain no numeric measurements suitable for charting; displaying data table.".to_string(),
            provenance,
        };
    }

    if raw.rows_returned == 1 {
        return ChartRecommendation {
            chart_suitable: true,
            recommended_type: ChartKind::KpiSummary,
            available_types: vec![ChartKind::KpiSummary, ChartKind::Bar, ChartKind::TableOnly],
            x_axis: non_numeric_cols.first().cloned(),
            y_axes: numeric_cols,
            reason: "Single aggregated row is best represented as a KPI summary card.".to_string(),
            provenance,
        };
    }

    if let Some(time_col) = temporal_col {
        let mut avail = vec![ChartKind::Line, ChartKind::Area, ChartKind::Bar, ChartKind::TableOnly];
        if numeric_cols.len() >= 2 {
            avail.push(ChartKind::Scatter);
        }
        return ChartRecommendation {
            chart_suitable: true,
            recommended_type: ChartKind::Line,
            available_types: avail,
            x_axis: Some(time_col),
            y_axes: numeric_cols,
            reason: "Time-series dimension detected; line or area chart preserves chronological ordering.".to_string(),
            provenance,
        };
    }

    if let Some(dim_col) = non_numeric_cols.first() {
        let mut avail = vec![ChartKind::Bar, ChartKind::TableOnly];
        if raw.rows_returned <= 8 && numeric_cols.len() == 1 {
            avail.push(ChartKind::Pie);
        }
        if numeric_cols.len() >= 2 {
            avail.push(ChartKind::Scatter);
        }
        return ChartRecommendation {
            chart_suitable: true,
            recommended_type: ChartKind::Bar,
            available_types: avail,
            x_axis: Some(dim_col.clone()),
            y_axes: numeric_cols,
            reason: "Categorical dimension paired with numeric metrics; bar chart is recommended.".to_string(),
            provenance,
        };
    }

    if numeric_cols.len() >= 2 {
        return ChartRecommendation {
            chart_suitable: true,
            recommended_type: ChartKind::Scatter,
            available_types: vec![ChartKind::Scatter, ChartKind::Bar, ChartKind::TableOnly],
            x_axis: Some(numeric_cols[0].clone()),
            y_axes: numeric_cols[1..].to_vec(),
            reason: "Multiple numeric columns without categorical labels; scatter plot compares distributions.".to_string(),
            provenance,
        };
    }

    ChartRecommendation {
        chart_suitable: false,
        recommended_type: ChartKind::TableOnly,
        available_types: vec![ChartKind::TableOnly],
        x_axis: None,
        y_axes: vec![],
        reason: "Result structure does not match a supported chart pattern.".to_string(),
        provenance,
    }
}

fn infer_aggregation_method(sql: &str) -> String {
    let s = sql.to_uppercase();
    let mut parts = Vec::new();
    for fn_name in ["SUM(", "COUNT(", "AVG(", "STDDEV_POP(", "LAG(", "MIN(", "MAX("] {
        if s.contains(fn_name) {
            parts.push(fn_name.trim_end_matches('(').to_string());
        }
    }
    if parts.is_empty() {
        "Row-level projection".to_string()
    } else {
        parts.join(", ")
    }
}

fn infer_time_range(sql: &str) -> String {
    let s = sql.to_lowercase();
    if s.contains("date_trunc('quarter', current_date) - interval '3 months'") {
        "Previous full calendar quarter".to_string()
    } else if s.contains("date '2026-04-01'") && s.contains("2026-10-01") {
        "2026-04-01 to 2026-09-30 (Q2-Q3 2026)".to_string()
    } else if s.contains("2026-01-01") {
        "From 2026-01-01 to latest completed quarter".to_string()
    } else {
        "All available records matching query filters".to_string()
    }
}
