"use client";

import React, { useEffect, useState } from "react";
import Link from "next/link";
import { Activity, ArrowRight, Database, ShieldAlert, ShieldCheck } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { SecurityStateBadge } from "@/components/SecurityStateBadge";
import { api } from "@/lib/api";
import type { OverviewResponse, VerifiedExecutionStatus } from "@/lib/types";

export default function OverviewPage() {
  const [data, setData] = useState<OverviewResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .overview()
      .then(setData)
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load overview"))
      .finally(() => setLoading(false));
  }, []);

  return (
    <AppShell
      title="Executive Analytics Overview"
      subtitle="Live telemetry from separated PostgreSQL metadata & read-only analytics source"
    >
      {loading && (
        <div className="bg-white rounded-xl border border-slate-200 p-8 text-sm text-slate-600">
          Verifying live PostgreSQL health and loading workspace telemetry...
        </div>
      )}

      {error && (
        <div className="bg-rose-50 border border-rose-200 rounded-xl p-4 text-sm text-rose-800">
          {error}
        </div>
      )}

      {data && (
        <div className="space-y-6">
          {/* Live Telemetry Cards (strictly from real API/DB counts) */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            <div className="bg-white rounded-xl border border-slate-200 p-4">
              <div className="flex items-center justify-between text-xs text-slate-500">
                <span>Source DB Health</span>
                <Database className="h-4 w-4 text-emerald-600" />
              </div>
              <div className="mt-2 text-sm font-semibold text-emerald-800 break-words">
                {data.source_db_status}
              </div>
              <div className="mt-1 text-[11px] font-mono-tabular text-slate-400 truncate">
                {data.source_db_identity}
              </div>
            </div>

            <div className="bg-white rounded-xl border border-slate-200 p-4">
              <div className="flex items-center justify-between text-xs text-slate-500">
                <span>Permitted Schema Catalog</span>
                <ShieldCheck className="h-4 w-4 text-sky-600" />
              </div>
              <div className="mt-2 text-2xl font-bold font-mono-tabular text-slate-900">
                {data.permitted_table_count} tables
              </div>
              <div className="mt-1 text-xs text-slate-500">
                v{data.schema_version} · {data.permitted_column_count} columns ·{" "}
                {data.active_metric_count} semantic metrics
              </div>
            </div>

            <div className="bg-white rounded-xl border border-slate-200 p-4">
              <div className="flex items-center justify-between text-xs text-slate-500">
                <span>Executed / Blocked Queries</span>
                <Activity className="h-4 w-4 text-sky-600" />
              </div>
              <div className="mt-2 text-2xl font-bold font-mono-tabular text-slate-900">
                {data.total_query_executions} total
              </div>
              <div className="mt-1 text-xs text-rose-700 font-medium">
                {data.rejected_query_count} blocked by AST validator
              </div>
            </div>

            <div className="bg-white rounded-xl border border-slate-200 p-4">
              <div className="flex items-center justify-between text-xs text-slate-500">
                <span>Active SQL Planner</span>
                <ShieldAlert className="h-4 w-4 text-amber-600" />
              </div>
              <div className="mt-2 text-sm font-semibold text-slate-900">
                {data.active_provider_type}
              </div>
              <div className="mt-1 text-xs font-mono-tabular text-slate-500 truncate">
                {data.active_provider_model}
              </div>
            </div>
          </div>

          <div className="bg-slate-900 text-white rounded-xl p-6 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
            <div>
              <h2 className="text-base font-semibold">
                Ask Natural Language Questions Against Verified Enterprise Schemas
              </h2>
              <p className="text-xs text-slate-300 mt-1">
                Every query is AST-parsed, schema-checked, row-capped, and executed in a read-only PostgreSQL transaction.
              </p>
            </div>
            <Link
              href="/query"
              className="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-sky-500 hover:bg-sky-400 text-slate-950 font-semibold text-sm shrink-0 transition-colors"
            >
              Open NL-to-SQL Workspace
              <ArrowRight className="h-4 w-4" />
            </Link>
          </div>

          {/* Recent Audited Executions */}
          <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
            <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between">
              <h2 className="text-sm font-semibold text-slate-900">
                Recent Audited Query Executions
              </h2>
              <Link href="/history" className="text-xs font-medium text-sky-600 hover:underline">
                View Full History
              </Link>
            </div>
            {data.recent_queries.length === 0 ? (
              <div className="p-6 text-sm text-slate-500 text-center">
                No queries executed yet in this tenant. Launch a query in the NL-to-SQL Workspace.
              </div>
            ) : (
              <div className="divide-y divide-slate-100">
                {data.recent_queries.map((q) => (
                  <div
                    key={q.query_id}
                    className="p-4 flex flex-col sm:flex-row sm:items-center justify-between gap-3"
                  >
                    <div className="min-w-0">
                      <div className="text-sm font-medium text-slate-900 truncate">
                        {q.question}
                      </div>
                      <div className="text-[11px] font-mono-tabular text-slate-500 mt-0.5">
                        ID: {q.query_id} · {q.rows_returned} row(s) · {q.duration_ms} ms
                      </div>
                    </div>
                    <SecurityStateBadge
                      validationStatus={q.validation_status}
                      executionStatus={q.execution_status as VerifiedExecutionStatus}
                    />
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </AppShell>
  );
}
