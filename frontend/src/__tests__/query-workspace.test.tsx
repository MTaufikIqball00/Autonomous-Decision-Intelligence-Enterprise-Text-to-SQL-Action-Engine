import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import QueryWorkspacePage from "@/app/query/page";
import * as apiMod from "@/lib/api";

describe("QueryWorkspacePage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    window.localStorage.setItem(
      "dta_auth_user",
      JSON.stringify({
        user_id: "u1",
        tenant_id: "t1",
        tenant_slug: "acme",
        email: "analyst@enterprise.local",
        display_name: "Lead Analyst",
        role: "analyst",
        policy: {
          role: "analyst",
          allowed_databases: ["data_to_action_analytics"],
          allowed_schemas: ["analytics", "finance"],
          allowed_tables: [],
          denied_columns: ["analytics.customers.tax_id"],
          allow_export: true,
          allow_schema_refresh: false,
          allow_admin_config: false,
          row_limit: 500,
          region_filter: "ALL",
        },
      })
    );

    vi.spyOn(apiMod.api, "schema").mockResolvedValue({
      catalog: {
        connection_id: "c1",
        database_name: "data_to_action_analytics",
        allowed_schemas: ["analytics", "finance"],
        version: 1,
        refreshed_at: "2026-11-24T00:00:00Z",
        tables: [],
        relationships: [],
      },
      examples: ["What was the total net revenue last quarter?"],
    });
  });

  it("submits question and displays SQL preview, validation passed, and result rows", async () => {
    vi.spyOn(apiMod.api, "askQuery").mockResolvedValue({
      query_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      question: "What was the total net revenue last quarter?",
      provider_used: "local_deterministic",
      model_used: "schema-semantic-planner-v1",
      is_external_llm: false,
      schema_version: 1,
      linked_tables: [
        {
          schema: "analytics",
          table: "orders",
          score: 14,
          matched_terms: ["revenue", "quarter"],
          reason: "mapped by semantic metric net_revenue",
        },
      ],
      join_paths: [],
      generated_sql: "SELECT COUNT(order_id) AS order_count, SUM(net_amount) AS total_net_revenue FROM analytics.orders",
      validated_sql: "SELECT * FROM (SELECT COUNT(order_id) AS order_count, SUM(net_amount) AS total_net_revenue FROM analytics.orders) AS _dta_safe_result LIMIT 501",
      explanation: "Computes total net revenue from analytics.orders.",
      assumptions: ["Uses net_amount in USD."],
      applied_filters: ["status IN ('COMPLETED', 'SHIPPED')"],
      validation_status: "validation_passed",
      validation_report: {
        is_valid: true,
        original_sql: "SELECT ...",
        executable_sql: "SELECT ...",
        referenced_tables: ["analytics.orders"],
        referenced_columns: ["analytics.orders.order_id", "analytics.orders.net_amount"],
        referenced_functions: ["count", "sum"],
        applied_row_limit: 500,
        rejection_reason: null,
        checks: [
          { check_name: "single_statement", passed: true, detail: "Single query" },
        ],
      },
      execution_status: "completed_with_rows",
      correction_attempts: [],
      columns: [
        { name: "order_count", type_name: "INT8" },
        { name: "total_net_revenue", type_name: "NUMERIC" },
      ],
      rows: [[98, 412345.67]],
      result_summary: {
        status: "completed_with_rows",
        row_count: 1,
        column_count: 2,
        is_truncated: false,
        truncation_reason: null,
        bytes_read: 32,
        duration_ms: 8,
        currency_unit: "USD",
        timezone: "UTC",
        has_nulls: false,
        non_finite_numeric_count: 0,
        column_stats: [],
        db_identity: "postgresql://dta_readonly:***@127.0.0.1:5439/data_to_action_analytics",
      },
      chart_spec: {
        chart_suitable: true,
        recommended_type: "kpi_summary",
        available_types: ["kpi_summary", "table_only"],
        x_axis: null,
        y_axes: ["order_count", "total_net_revenue"],
        reason: "Single aggregated row.",
        provenance: {
          query_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
          source_columns: ["order_count", "total_net_revenue"],
          aggregation_method: "COUNT, SUM",
          filters: [],
          time_range: "Previous quarter",
          units: "USD",
          result_timestamp: "2026-11-24T12:00:00Z",
        },
      },
      insight_report: {
        source_query_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        observations: ["Query returned 1 verified row."],
        interpretations: [],
        limitations: [],
        verification_checks: [
          { check: "numeric_grounding", passed: true, detail: "Verified." },
        ],
        grounded: true,
      },
      clarification_needed: null,
      clarification_options: [],
      cannot_compute_reason: null,
      error_message: null,
      created_at: "2026-11-24T12:00:00Z",
    });

    render(<QueryWorkspacePage />);
    const input = await screen.findByLabelText(/Ask an Enterprise Analytics Question/i);
    fireEvent.change(input, {
      target: { value: "What was the total net revenue last quarter?" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Run Secure Query/i }));

    await waitFor(() => {
      expect(screen.getByText("Validation Passed")).toBeInTheDocument();
      expect(screen.getByText("Execution Completed")).toBeInTheDocument();
      expect(screen.getAllByText("analytics.orders").length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText(/Query returned 1 verified row/i)).toBeInTheDocument();
    });
  });

  it("displays Validation Rejected safely when AST validator blocks unsafe SQL", async () => {
    vi.spyOn(apiMod.api, "askQuery").mockResolvedValue({
      query_id: "99999999-bbbb-cccc-dddd-eeeeeeeeeeee",
      question: "Drop table orders",
      provider_used: "local_deterministic",
      model_used: "schema-semantic-planner-v1",
      is_external_llm: false,
      schema_version: 1,
      linked_tables: [],
      join_paths: [],
      generated_sql: "DROP TABLE analytics.orders",
      validated_sql: "",
      explanation: "Blocked",
      assumptions: [],
      applied_filters: [],
      validation_status: "validation_rejected",
      validation_report: null,
      execution_status: "validation_rejected",
      correction_attempts: [],
      columns: [],
      rows: [],
      result_summary: null,
      chart_spec: null,
      insight_report: null,
      clarification_needed: null,
      clarification_options: [],
      cannot_compute_reason: null,
      error_message: "Statement type 'Drop' is not permitted.",
      created_at: "2026-11-24T12:00:00Z",
    });

    render(<QueryWorkspacePage />);
    const input = await screen.findByLabelText(/Ask an Enterprise Analytics Question/i);
    fireEvent.change(input, { target: { value: "Drop table orders" } });
    fireEvent.click(screen.getByRole("button", { name: /Run Secure Query/i }));

    await waitFor(() => {
      expect(screen.getByText("Validation Rejected")).toBeInTheDocument();
      expect(screen.getByText(/Statement type 'Drop' is not permitted/i)).toBeInTheDocument();
    });
  });
});
