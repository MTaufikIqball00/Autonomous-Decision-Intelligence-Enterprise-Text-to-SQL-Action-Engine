pub mod discovery;
pub mod linker;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnCatalog {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyMeta {
    pub constraint_name: String,
    pub from_schema: String,
    pub from_table: String,
    pub from_column: String,
    pub to_schema: String,
    pub to_table: String,
    pub to_column: String,
    pub source: String, // "foreign_key" or "manual_relationship"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub index_name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub schema: String,
    pub name: String,
    pub table_type: String, // "BASE TABLE" or "VIEW"
    pub description: Option<String>,
    pub columns: Vec<ColumnCatalog>,
    pub primary_keys: Vec<String>,
    pub foreign_keys: Vec<ForeignKeyMeta>,
    pub indexes: Vec<IndexMeta>,
}

impl TableMeta {
    pub fn fq_name(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaCatalog {
    pub connection_id: Uuid,
    pub database_name: String,
    pub allowed_schemas: Vec<String>,
    pub version: i32,
    pub refreshed_at: DateTime<Utc>,
    pub tables: Vec<TableMeta>,
    pub relationships: Vec<ForeignKeyMeta>,
}

impl SchemaCatalog {
    pub fn find_table(&self, schema: Option<&str>, table: &str) -> Vec<&TableMeta> {
        self.tables
            .iter()
            .filter(|t| {
                t.name.eq_ignore_ascii_case(table)
                    && schema
                        .map(|s| t.schema.eq_ignore_ascii_case(s))
                        .unwrap_or(true)
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualRelationshipInput {
    pub from_schema: String,
    pub from_table: String,
    pub from_column: String,
    pub to_schema: String,
    pub to_table: String,
    pub to_column: String,
    pub relationship_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateTableMatch {
    pub schema: String,
    pub table: String,
    pub score: f64,
    pub matched_terms: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguityWarning {
    pub topic: String,
    pub message: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedSchemaContext {
    pub selected_tables: Vec<TableMeta>,
    pub candidate_rankings: Vec<CandidateTableMatch>,
    pub join_paths: Vec<ForeignKeyMeta>,
    pub ambiguities: Vec<AmbiguityWarning>,
    pub missing_concepts: Vec<String>,
    pub schema_version: i32,
}
