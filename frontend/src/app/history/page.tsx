"use client";

import React, { useEffect, useState } from "react";
import { Bookmark, History, Trash2 } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { SecurityStateBadge } from "@/components/SecurityStateBadge";
import { api } from "@/lib/api";
import type { QueryHistoryItem, SavedQueryDto, VerifiedExecutionStatus } from "@/lib/types";

export default function HistoryAndSavedPage() {
  const [history, setHistory] = useState<QueryHistoryItem[]>([]);
  const [saved, setSaved] = useState<SavedQueryDto[]>([]);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    try {
      const [h, s] = await Promise.all([api.queryHistory(), api.savedQueries()]);
      setHistory(h);
      setSaved(s);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed loading query history");
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function handleDeleteSaved(id: string) {
    try {
      await api.deleteSavedQuery(id);
      setSaved((prev) => prev.filter((item) => item.id !== id));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Delete failed");
    }
  }

  return (
    <AppShell
      title="Query History & Saved Queries"
      subtitle="Auditable query execution trail and reusable verified analytical queries"
    >
      <div className="space-y-6">
        {error && (
          <div className="p-4 rounded-xl bg-rose-50 border border-rose-200 text-sm text-rose-800">
            {error}
          </div>
        )}

        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
            <Bookmark className="h-4 w-4 text-sky-600" />
            <h2 className="text-sm font-semibold text-slate-900">
              Saved Queries ({saved.length})
            </h2>
          </div>
          {saved.length === 0 ? (
            <div className="p-6 text-sm text-slate-500 text-center">
              No saved queries yet. Save any verified query from the NL-to-SQL Workspace.
            </div>
          ) : (
            <div className="divide-y divide-slate-100">
              {saved.map((item) => (
                <div key={item.id} className="p-4 space-y-2">
                  <div className="flex items-start justify-between gap-2">
                    <div>
                      <div className="text-sm font-semibold text-slate-900">{item.title}</div>
                      <div className="text-xs text-slate-500">{item.question}</div>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleDeleteSaved(item.id)}
                      aria-label={`Delete saved query ${item.title}`}
                      className="p-1.5 text-slate-400 hover:text-rose-600 cursor-pointer"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                  <pre className="p-2.5 rounded bg-slate-950 text-sky-200 text-[11px] font-mono-tabular overflow-x-auto">
                    <code>{item.sql_text}</code>
                  </pre>
                </div>
              ))}
            </div>
          )}
        </section>

        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
            <History className="h-4 w-4 text-sky-600" />
            <h2 className="text-sm font-semibold text-slate-900">
              Execution History ({history.length})
            </h2>
          </div>
          {history.length === 0 ? (
            <div className="p-6 text-sm text-slate-500 text-center">
              No query executions recorded yet.
            </div>
          ) : (
            <div className="divide-y divide-slate-100">
              {history.map((h) => (
                <div key={h.query_id} className="p-4 space-y-2">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="text-sm font-medium text-slate-900">{h.question}</div>
                    <SecurityStateBadge
                      validationStatus={h.validation_status}
                      executionStatus={h.execution_status as VerifiedExecutionStatus}
                    />
                  </div>
                  <div className="text-[11px] font-mono-tabular text-slate-500">
                    ID: {h.query_id} · Planner: {h.provider_used}/{h.model_used} · Schema v{h.schema_version} ·{" "}
                    {h.rows_returned} row(s) · {h.execution_duration_ms} ms
                  </div>
                  {h.generated_sql && (
                    <pre className="p-2.5 rounded bg-slate-900 text-slate-100 text-[11px] font-mono-tabular overflow-x-auto">
                      <code>{h.generated_sql}</code>
                    </pre>
                  )}
                  {h.error_message && (
                    <div className="text-xs text-rose-700">{h.error_message}</div>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </AppShell>
  );
}
