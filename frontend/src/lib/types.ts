export interface RbacPolicy {
  role: string;
  allowed_databases: string[];
  allowed_schemas: string[];
  allowed_tables: string[];
  denied_columns: string[];
  allow_export: boolean;
  allow_schema_refresh: boolean;
  allow_admin_config: boolean;
  row_limit: number;
  region_filter: string;
}

export interface UserProfile {
  user_id: string;
  tenant_id: string;
  tenant_slug: string;
  email: string;
  display_name: string;
  role: string;
  policy: RbacPolicy;
}

export interface LoginResponse {
  token: string;
  user: UserProfile;
}

export interface RecentExecutionSummary {
  query_id: string;
  question: string;
  execution_status: string;
  validation_status: string;
  rows_returned: number;
  duration_ms: number;
  created_at: string;
}

export interface OverviewResponse {
  source_db_status: string;
  source_db_identity: string;
  schema_version: number;
  permitted_table_count: number;
  permitted_column_count: number;
  active_metric_count: number;
  total_query_executions: number;
  rejected_query_count: number;
  saved_query_count: number;
  dashboard_count: number;
  active_provider_type: string;
  active_provider_model: string;
  recent_queries: RecentExecutionSummary[];
}

export interface CandidateTableMatch {
  schema: string;
  table: string;
  score: number;
  matched_terms: string[];
  reason: string;
}

export interface ForeignKeyMeta {
  constraint_name: string;
  from_schema: string;
  from_table: string;
  from_column: string;
  to_schema: string;
  to_table: string;
  to_column: string;
  source: string;
}

export interface ValidationCheck {
  check_name: string;
  passed: boolean;
  detail: string;
}

export interface ValidationReport {
  is_valid: boolean;
  original_sql: string;
  executable_sql: string | null;
  referenced_tables: string[];
  referenced_columns: string[];
  referenced_functions: string[];
  applied_row_limit: number;
  rejection_reason: string | null;
  checks: ValidationCheck[];
}

export type VerifiedExecutionStatus =
  | "completed_with_rows"
  | "completed_empty"
  | "completed_truncated"
  | "validation_rejected"
  | "execution_timeout"
  | "execution_failed"
  | "db_unavailable"
  | "authorization_denied"
  | "clarification_required"
  | "cannot_compute";

export interface CorrectionAttemptRecord {
  attempt: number;
  previous_sql: string;
  error_class: string;
  sanitized_error: string;
  candidate_sql: string | null;
  revalidation_passed: boolean;
  rejection_reason: string | null;
  outcome: string;
}

export interface ColumnMeta {
  name: string;
  type_name: string;
}

export interface ColumnStatSummary {
  column: string;
  type_name: string;
  non_null_count: number;
  null_count: number;
  numeric_sum: number | null;
  numeric_min: number | null;
  numeric_max: number | null;
  numeric_avg: number | null;
}

export interface VerifiedResultSummary {
  status: VerifiedExecutionStatus;
  row_count: number;
  column_count: number;
  is_truncated: boolean;
  truncation_reason: string | null;
  bytes_read: number;
  duration_ms: number;
  currency_unit: string;
  timezone: string;
  has_nulls: boolean;
  non_finite_numeric_count: number;
  column_stats: ColumnStatSummary[];
  db_identity: string;
}

export type ChartKind =
  | "kpi_summary"
  | "line"
  | "area"
  | "bar"
  | "pie"
  | "scatter"
  | "table_only";

export interface ChartProvenance {
  query_id: string;
  source_columns: string[];
  aggregation_method: string;
  filters: string[];
  time_range: string;
  units: string;
  result_timestamp: string;
}

export interface ChartRecommendation {
  chart_suitable: boolean;
  recommended_type: ChartKind;
  available_types: ChartKind[];
  x_axis: string | null;
  y_axes: string[];
  reason: string;
  provenance: ChartProvenance;
}

export interface InsightVerificationCheck {
  check: string;
  passed: boolean;
  detail: string;
}

export interface InsightReport {
  source_query_id: string;
  observations: string[];
  interpretations: string[];
  limitations: string[];
  verification_checks: InsightVerificationCheck[];
  grounded: boolean;
}

export interface QueryPipelineResponse {
  query_id: string;
  question: string;
  provider_used: string;
  model_used: string;
  is_external_llm: boolean;
  schema_version: number;
  linked_tables: CandidateTableMatch[];
  join_paths: ForeignKeyMeta[];
  generated_sql: string;
  validated_sql: string;
  explanation: string;
  assumptions: string[];
  applied_filters: string[];
  validation_status: string;
  validation_report: ValidationReport | null;
  execution_status: VerifiedExecutionStatus;
  correction_attempts: CorrectionAttemptRecord[];
  columns: ColumnMeta[];
  rows: unknown[][];
  result_summary: VerifiedResultSummary | null;
  chart_spec: ChartRecommendation | null;
  insight_report: InsightReport | null;
  clarification_needed: string | null;
  clarification_options: string[];
  cannot_compute_reason: string | null;
  error_message: string | null;
  created_at: string;
}

export interface ColumnCatalog {
  name: string;
  data_type: string;
  is_nullable: boolean;
  is_primary_key: boolean;
  description: string | null;
}

export interface IndexMeta {
  index_name: string;
  columns: string[];
  is_unique: boolean;
}

export interface TableMeta {
  schema: string;
  name: string;
  table_type: string;
  description: string | null;
  columns: ColumnCatalog[];
  primary_keys: string[];
  foreign_keys: ForeignKeyMeta[];
  indexes: IndexMeta[];
}

export interface SchemaCatalog {
  connection_id: string;
  database_name: string;
  allowed_schemas: string[];
  version: number;
  refreshed_at: string;
  tables: TableMeta[];
  relationships: ForeignKeyMeta[];
}

export interface SchemaResponse {
  catalog: SchemaCatalog;
  examples: string[];
}

export interface SemanticMetric {
  id: string;
  tenant_id: string;
  metric_key: string;
  display_name: string;
  version: number;
  definition: string;
  sql_formula: string;
  source_table: string;
  approved_dimensions: string[];
  approved_filters: string[];
  currency_unit: string;
  timezone_convention: string;
  owner_name: string;
  freshness_sla: string;
  is_active: boolean;
  updated_at: string;
}

export interface QueryHistoryItem {
  query_id: string;
  user_id: string;
  question: string;
  provider_used: string;
  model_used: string;
  generated_sql: string;
  schema_version: number;
  validation_status: string;
  execution_status: string;
  execution_duration_ms: number;
  rows_returned: number;
  is_truncated: boolean;
  error_message: string | null;
  created_at: string;
}

export interface SavedQueryDto {
  id: string;
  user_id: string;
  query_execution_id: string | null;
  title: string;
  question: string;
  sql_text: string;
  notes: string;
  is_shared: boolean;
  created_at: string;
}

export interface DashboardWidget {
  title: string;
  question: string;
  sql_text: string;
  chart_type: ChartKind;
  columns: ColumnMeta[];
  rows: unknown[][];
  query_id?: string;
}

export interface DashboardDto {
  id: string;
  user_id: string;
  title: string;
  description: string;
  widgets: DashboardWidget[];
  is_shared: boolean;
  created_at: string;
  updated_at: string;
}

export interface ConnectionDto {
  id: string;
  name: string;
  adapter_type: string;
  host: string;
  port: number;
  database_name: string;
  username: string;
  ssl_mode: string;
  allowed_schemas: string[];
  is_readonly: boolean;
  is_active: boolean;
  has_encrypted_secret: boolean;
  created_at: string;
}

export interface ProviderDto {
  id: string;
  name: string;
  provider_type: string;
  model_name: string;
  base_url: string;
  has_api_key: boolean;
  is_active: boolean;
  updated_at: string;
}

export interface AuditLogDto {
  id: string;
  user_email: string;
  event_type: string;
  resource_type: string;
  resource_id: string;
  decision: string;
  details: Record<string, unknown>;
  created_at: string;
}
