import React from "react";
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ResultTable } from "@/components/ResultTable";
import { ChartRenderer } from "@/components/ChartRenderer";
import type { ChartRecommendation, VerifiedResultSummary } from "@/lib/types";

vi.mock("recharts", async () => {
  const actual = await vi.importActual<typeof import("recharts")>("recharts");
  return {
    ...actual,
    ResponsiveContainer: ({ children }: { children: React.ReactNode }) => (
      <div data-testid="recharts-responsive" style={{ width: 640, height: 320 }}>
        {children}
      </div>
    ),
  };
});

describe("ResultTable & ChartRenderer", () => {
  it("renders NULL cells, truncation banner, and RBAC export disablement", () => {
    const summary: VerifiedResultSummary = {
      status: "completed_truncated",
      row_count: 2,
      column_count: 2,
      is_truncated: true,
      truncation_reason: "Row count exceeded limit of 2 rows",
      bytes_read: 120,
      duration_ms: 14,
      currency_unit: "USD",
      timezone: "UTC",
      has_nulls: true,
      non_finite_numeric_count: 0,
      column_stats: [],
      db_identity: "postgresql://dta_readonly:***@127.0.0.1:5439/data_to_action_analytics",
    };

    render(
      <ResultTable
        columns={[
          { name: "region_name", type_name: "VARCHAR" },
          { name: "net_revenue", type_name: "NUMERIC" },
        ]}
        rows={[
          ["North America", 125000.5],
          ["EMEA", null],
        ]}
        summary={summary}
        allowExport={false}
        onExportCsv={vi.fn()}
      />
    );

    expect(screen.getByText("North America")).toBeInTheDocument();
    expect(screen.getByText("125,000.5")).toBeInTheDocument();
    expect(screen.getByText("NULL")).toBeInTheDocument();
    expect(screen.getByText(/Row count exceeded limit of 2 rows/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Export Denied/i })).toBeDisabled();
  });

  it("renders empty result table cleanly", () => {
    render(
      <ResultTable
        columns={[{ name: "region_name", type_name: "VARCHAR" }]}
        rows={[]}
        summary={null}
        allowExport={true}
      />
    );
    expect(screen.getByText(/returned 0 matching rows/i)).toBeInTheDocument();
  });

  it("renders KPI summary and chart provenance + chart switcher", () => {
    const spec: ChartRecommendation = {
      chart_suitable: true,
      recommended_type: "bar",
      available_types: ["bar", "line", "kpi_summary"],
      x_axis: "region_code",
      y_axes: ["net_revenue"],
      reason: "Categorical dimension paired with numeric metrics.",
      provenance: {
        query_id: "11111111-2222-3333-4444-555555555555",
        source_columns: ["region_code", "net_revenue"],
        aggregation_method: "SUM",
        filters: ["status IN ('COMPLETED', 'SHIPPED')"],
        time_range: "2026-Q3",
        units: "USD",
        result_timestamp: "2026-11-24T12:00:00Z",
      },
    };

    render(
      <ChartRenderer
        chartSpec={spec}
        columns={[
          { name: "region_code", type_name: "VARCHAR" },
          { name: "net_revenue", type_name: "NUMERIC" },
        ]}
        rows={[
          ["NA", 200000],
          ["APAC", 150000],
        ]}
      />
    );

    expect(screen.getByText(/11111111/)).toBeInTheDocument();
    expect(screen.getByText(/USD/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "KPI SUMMARY" }));
    expect(screen.getByText("region code")).toBeInTheDocument();
  });
});
