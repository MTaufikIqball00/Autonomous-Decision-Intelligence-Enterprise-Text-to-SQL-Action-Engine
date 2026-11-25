"use client";

import React, { useEffect, useState } from "react";
import Link from "next/link";
import { LayoutDashboard, Play, RefreshCw, Trash2 } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { ChartRenderer } from "@/components/ChartRenderer";
import { ResultTable } from "@/components/ResultTable";
import { api } from "@/lib/api";
import type { ChartRecommendation, DashboardDto, DashboardWidget } from "@/lib/types";

function resolveWidgetChartSpec(w: DashboardWidget): ChartRecommendation | null {
  if (w.chart_spec) return w.chart_spec;
  if (!w.columns || w.columns.length === 0 || !w.rows || w.rows.length === 0) return null;

  const isNumericType = (t: string) =>
    /int|numeric|decimal|float|double|real|money/i.test(t);
  const numericCols = w.columns.filter((c) => isNumericType(c.type_name)).map((c) => c.name);
  const nonNumericCols = w.columns.filter((c) => !isNumericType(c.type_name)).map((c) => c.name);

  if (numericCols.length === 0) return null;

  return {
    chart_suitable: w.chart_type !== "table_only",
    recommended_type: w.chart_type || "bar",
    available_types: Array.from(
      new Set([w.chart_type || "bar", "bar", "line", "area", "table_only"])
    ),
    x_axis: nonNumericCols[0] || w.columns[0]?.name || null,
    y_axes: numericCols,
    reason: "Dashboard widget chart synthesized from verified column metadata.",
    provenance: {
      query_id: w.query_id || "dashboard-widget",
      source_columns: w.columns.map((c) => c.name),
      aggregation_method: "Saved/Live Query",
      filters: [],
      time_range: "Configured query scope",
      units: "USD",
      result_timestamp: w.refreshed_at || new Date().toISOString(),
    },
  };
}

export default function DashboardsPage() {
  const [dashboards, setDashboards] = useState<DashboardDto[]>([]);
  const [refreshingId, setRefreshingId] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .dashboards()
      .then(setDashboards)
      .catch((e) => setError(e instanceof Error ? e.message : "Failed loading dashboards"));
  }, []);

  async function handleDelete(id: string) {
    setError(null);
    try {
      await api.deleteDashboard(id);
      setDashboards((prev) => prev.filter((d) => d.id !== id));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed deleting dashboard");
    }
  }

  async function handleRefresh(id: string) {
    setRefreshingId(id);
    setError(null);
    setStatusMsg(null);
    try {
      const updated = await api.refreshDashboard(id);
      setDashboards((prev) => prev.map((d) => (d.id === id ? updated : d)));
      setStatusMsg(
        `Dashboard "${updated.title}" refreshed through AST validator & RBAC policy.`
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : "Live dashboard refresh failed");
    } finally {
      setRefreshingId(null);
    }
  }

  return (
    <AppShell
      title="Enterprise Dashboard Builder"
      subtitle="Live interactive dashboards re-validated per caller RBAC policy & executed read-only"
    >
      <div className="space-y-6">
        {error && (
          <div className="p-4 rounded-xl bg-rose-50 border border-rose-200 text-sm text-rose-800">
            {error}
          </div>
        )}
        {statusMsg && (
          <div className="p-3 rounded-xl bg-emerald-50 border border-emerald-200 text-xs font-medium text-emerald-800">
            {statusMsg}
          </div>
        )}

        {dashboards.length === 0 ? (
          <div className="bg-white rounded-xl border border-slate-200 p-8 text-center text-sm text-slate-500">
            No dashboards created yet. Run a query in the NL-to-SQL Workspace and click{" "}
            <strong>Pin to Dashboard</strong>.
          </div>
        ) : (
          dashboards.map((d) => (
            <div key={d.id} className="bg-white rounded-xl border border-slate-200 overflow-hidden">
              <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <LayoutDashboard className="h-4 w-4 text-sky-600" />
                  <div>
                    <div className="text-sm font-semibold text-slate-900">{d.title}</div>
                    <div className="text-xs text-slate-500">
                      {d.description} · Updated {new Date(d.updated_at).toLocaleString()}
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    disabled={refreshingId === d.id}
                    onClick={() => handleRefresh(d.id)}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold bg-sky-600 text-white hover:bg-sky-700 disabled:opacity-60 cursor-pointer"
                  >
                    <RefreshCw
                      className={`h-3.5 w-3.5 ${refreshingId === d.id ? "animate-spin" : ""}`}
                    />
                    {refreshingId === d.id ? "Refreshing..." : "Refresh Live"}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDelete(d.id)}
                    aria-label={`Delete dashboard ${d.title}`}
                    className="p-1.5 text-slate-400 hover:text-rose-600 cursor-pointer"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              </div>
              <div className="p-4 space-y-6">
                {(Array.isArray(d.widgets) ? d.widgets : []).map((w, idx) => {
                  const chartSpec = resolveWidgetChartSpec(w);
                  return (
                    <div
                      key={idx}
                      className="p-4 rounded-xl border border-slate-200 bg-slate-50/40 space-y-4"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div>
                          <div className="text-sm font-semibold text-slate-900">
                            {w.title}
                          </div>
                          <div className="text-[11px] text-slate-500 font-mono-tabular">
                            Chart: {w.chart_type}
                            {w.refreshed_at
                              ? ` · Refreshed ${new Date(w.refreshed_at).toLocaleString()}`
                              : ""}
                          </div>
                        </div>
                        {w.sql_text && (
                          <Link
                            href={`/query?q=${encodeURIComponent(w.question || w.title)}&sql=${encodeURIComponent(w.sql_text)}`}
                            className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium bg-white border border-slate-200 text-slate-700 hover:bg-slate-100"
                          >
                            <Play className="h-3 w-3" />
                            Open in Workspace
                          </Link>
                        )}
                      </div>

                      {w.refresh_error && (
                        <div className="p-2.5 rounded-lg bg-rose-50 border border-rose-200 text-xs text-rose-800">
                          Refresh blocked/failed: {w.refresh_error}
                        </div>
                      )}

                      {chartSpec && (
                        <ChartRenderer
                          chartSpec={chartSpec}
                          columns={w.columns || []}
                          rows={w.rows || []}
                        />
                      )}

                      <ResultTable
                        columns={w.columns || []}
                        rows={w.rows || []}
                        summary={w.result_summary || null}
                        allowExport={false}
                      />
                    </div>
                  );
                })}
              </div>
            </div>
          ))
        )}
      </div>
    </AppShell>
  );
}
