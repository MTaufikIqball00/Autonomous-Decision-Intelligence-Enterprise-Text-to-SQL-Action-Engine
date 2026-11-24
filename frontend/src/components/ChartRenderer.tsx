"use client";

import React, { useMemo, useState } from "react";
import { BarChart3 } from "lucide-react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Line,
  LineChart,
  Pie,
  PieChart,
  ResponsiveContainer,
  Scatter,
  ScatterChart,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { ChartKind, ChartRecommendation, ColumnMeta } from "@/lib/types";

const PALETTE = ["#0284c7", "#0f766e", "#d97706", "#7c3aed", "#dc2626", "#2563eb"];

export function ChartRenderer({
  chartSpec,
  columns,
  rows,
}: {
  chartSpec: ChartRecommendation;
  columns: ColumnMeta[];
  rows: unknown[][];
}) {
  const [selectedKind, setSelectedKind] = useState<ChartKind>(
    chartSpec.recommended_type
  );

  const chartData = useMemo(() => {
    return rows.map((r) => {
      const obj: Record<string, unknown> = {};
      columns.forEach((c, idx) => {
        obj[c.name] = r[idx];
      });
      return obj;
    });
  }, [columns, rows]);

  const xKey = chartSpec.x_axis || columns[0]?.name || "label";
  const yKeys = chartSpec.y_axes.length > 0 ? chartSpec.y_axes : [];

  return (
    <div className="bg-white rounded-xl border border-slate-200 shadow-xs overflow-hidden">
      <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <BarChart3 className="h-4 w-4 text-sky-600" />
          <h2 className="text-sm font-semibold text-slate-900">
            Interactive Chart Visualization
          </h2>
        </div>

        {chartSpec.chart_suitable && (
          <div className="flex flex-wrap gap-1" role="group" aria-label="Chart Type Selector">
            {chartSpec.available_types
              .filter((t) => t !== "table_only")
              .map((kind) => (
                <button
                  key={kind}
                  type="button"
                  onClick={() => setSelectedKind(kind)}
                  className={`px-2.5 py-1 rounded-md text-xs font-medium transition-colors cursor-pointer ${
                    selectedKind === kind
                      ? "bg-sky-600 text-white"
                      : "bg-white text-slate-700 border border-slate-200 hover:bg-slate-100"
                  }`}
                >
                  {kind.replace("_", " ").toUpperCase()}
                </button>
              ))}
          </div>
        )}
      </div>

      <div className="p-4">
        {!chartSpec.chart_suitable || yKeys.length === 0 ? (
          <div className="p-6 rounded-lg bg-slate-50 border border-slate-200 text-sm text-slate-600 text-center">
            {chartSpec.reason}
          </div>
        ) : selectedKind === "kpi_summary" ? (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {columns.map((c, idx) => {
              const val = rows[0]?.[idx];
              return (
                <div
                  key={c.name}
                  className="p-4 rounded-xl border border-slate-200 bg-slate-50/70"
                >
                  <div className="text-xs font-medium text-slate-500 uppercase tracking-wider">
                    {c.name.replace(/_/g, " ")}
                  </div>
                  <div className="mt-1.5 text-2xl font-semibold font-mono-tabular text-slate-900">
                    {typeof val === "number"
                      ? val.toLocaleString("en-US", { maximumFractionDigits: 2 })
                      : String(val ?? "NULL")}
                  </div>
                  <div className="mt-1 text-[11px] text-slate-400">
                    Unit: {chartSpec.provenance.units}
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="h-80 w-full">
            <ResponsiveContainer width="100%" height="100%">
              {selectedKind === "line" ? (
                <LineChart data={chartData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis dataKey={xKey} tick={{ fontSize: 11 }} />
                  <YAxis tick={{ fontSize: 11 }} />
                  <Tooltip />
                  <Legend />
                  {yKeys.map((yk, idx) => (
                    <Line
                      key={yk}
                      type="monotone"
                      dataKey={yk}
                      stroke={PALETTE[idx % PALETTE.length]}
                      strokeWidth={2}
                      dot={{ r: 3 }}
                    />
                  ))}
                </LineChart>
              ) : selectedKind === "area" ? (
                <AreaChart data={chartData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis dataKey={xKey} tick={{ fontSize: 11 }} />
                  <YAxis tick={{ fontSize: 11 }} />
                  <Tooltip />
                  <Legend />
                  {yKeys.map((yk, idx) => (
                    <Area
                      key={yk}
                      type="monotone"
                      dataKey={yk}
                      stroke={PALETTE[idx % PALETTE.length]}
                      fill={PALETTE[idx % PALETTE.length]}
                      fillOpacity={0.18}
                    />
                  ))}
                </AreaChart>
              ) : selectedKind === "pie" ? (
                <PieChart>
                  <Tooltip />
                  <Legend />
                  <Pie
                    data={chartData}
                    dataKey={yKeys[0]}
                    nameKey={xKey}
                    cx="50%"
                    cy="50%"
                    innerRadius={55}
                    outerRadius={105}
                  >
                    {chartData.map((_, idx) => (
                      <Cell key={idx} fill={PALETTE[idx % PALETTE.length]} />
                    ))}
                  </Pie>
                </PieChart>
              ) : selectedKind === "scatter" && yKeys.length >= 1 ? (
                <ScatterChart>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis dataKey={xKey} name={xKey} tick={{ fontSize: 11 }} />
                  <YAxis dataKey={yKeys[0]} name={yKeys[0]} tick={{ fontSize: 11 }} />
                  <Tooltip />
                  <Scatter name={`${xKey} vs ${yKeys[0]}`} data={chartData} fill={PALETTE[0]} />
                </ScatterChart>
              ) : (
                <BarChart data={chartData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis dataKey={xKey} tick={{ fontSize: 11 }} />
                  <YAxis tick={{ fontSize: 11 }} />
                  <Tooltip />
                  <Legend />
                  {yKeys.map((yk, idx) => (
                    <Bar
                      key={yk}
                      dataKey={yk}
                      fill={PALETTE[idx % PALETTE.length]}
                      radius={[4, 4, 0, 0]}
                    />
                  ))}
                </BarChart>
              )}
            </ResponsiveContainer>
          </div>
        )}

        {/* Mandatory Chart Provenance Footer */}
        <div className="mt-4 pt-3 border-t border-slate-100 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2 text-[11px] text-slate-500">
          <div>
            <span className="font-semibold text-slate-700">Query ID:</span>{" "}
            <span className="font-mono-tabular">{chartSpec.provenance.query_id.slice(0, 8)}…</span>
          </div>
          <div>
            <span className="font-semibold text-slate-700">Aggregation:</span>{" "}
            {chartSpec.provenance.aggregation_method}
          </div>
          <div>
            <span className="font-semibold text-slate-700">Time Range:</span>{" "}
            {chartSpec.provenance.time_range}
          </div>
          <div>
            <span className="font-semibold text-slate-700">Units / Columns:</span>{" "}
            {chartSpec.provenance.units} ({chartSpec.provenance.source_columns.join(", ")})
          </div>
        </div>
      </div>
    </div>
  );
}
