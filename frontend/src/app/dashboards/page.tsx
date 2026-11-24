"use client";

import React, { useEffect, useState } from "react";
import { LayoutDashboard, Trash2 } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { ResultTable } from "@/components/ResultTable";
import { api } from "@/lib/api";
import type { DashboardDto } from "@/lib/types";

export default function DashboardsPage() {
  const [dashboards, setDashboards] = useState<DashboardDto[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .dashboards()
      .then(setDashboards)
      .catch((e) => setError(e instanceof Error ? e.message : "Failed loading dashboards"));
  }, []);

  async function handleDelete(id: string) {
    try {
      await api.deleteDashboard(id);
      setDashboards((prev) => prev.filter((d) => d.id !== id));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed deleting dashboard");
    }
  }

  return (
    <AppShell
      title="Enterprise Dashboard Builder"
      subtitle="Composed strictly from verified query execution outputs stored separately from source DB"
    >
      <div className="space-y-6">
        {error && (
          <div className="p-4 rounded-xl bg-rose-50 border border-rose-200 text-sm text-rose-800">
            {error}
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
              <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <LayoutDashboard className="h-4 w-4 text-sky-600" />
                  <div>
                    <div className="text-sm font-semibold text-slate-900">{d.title}</div>
                    <div className="text-xs text-slate-500">{d.description}</div>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleDelete(d.id)}
                  className="p-1.5 text-slate-400 hover:text-rose-600 cursor-pointer"
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>
              <div className="p-4 space-y-4">
                {(Array.isArray(d.widgets) ? d.widgets : []).map((w, idx) => (
                  <div key={idx} className="space-y-2">
                    <div className="text-xs font-semibold text-slate-800">
                      {w.title} ({w.chart_type})
                    </div>
                    <ResultTable
                      columns={w.columns || []}
                      rows={w.rows || []}
                      summary={null}
                      allowExport={false}
                    />
                  </div>
                ))}
              </div>
            </div>
          ))
        )}
      </div>
    </AppShell>
  );
}
