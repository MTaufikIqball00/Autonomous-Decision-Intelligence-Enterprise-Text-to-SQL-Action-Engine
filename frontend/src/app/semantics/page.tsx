"use client";

import React, { useEffect, useState } from "react";
import { BookOpen, Plus } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { api, getStoredUser } from "@/lib/api";
import type { SemanticMetric, UserProfile } from "@/lib/types";

export default function SemanticCatalogPage() {
  const [user, setUser] = useState<UserProfile | null>(null);
  const [metrics, setMetrics] = useState<SemanticMetric[]>([]);
  const [showAllVersions, setShowAllVersions] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [metricKey, setMetricKey] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [definition, setDefinition] = useState("");
  const [sqlFormula, setSqlFormula] = useState("");
  const [sourceTable, setSourceTable] = useState("analytics.orders");
  const [dimensions, setDimensions] = useState("order_date, region_name");
  const [filters, setFilters] = useState("analytics.orders.status IN ('COMPLETED', 'SHIPPED')");

  async function load(allVersions: boolean) {
    try {
      const list = allVersions ? await api.semanticVersions() : await api.semantics();
      setMetrics(list);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed loading semantic catalog");
    }
  }

  useEffect(() => {
    setUser(getStoredUser());
    load(false);
  }, []);

  async function handleCreateVersion(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      await api.upsertSemantic({
        metric_key: metricKey,
        display_name: displayName,
        definition,
        sql_formula: sqlFormula,
        source_table: sourceTable,
        approved_dimensions: dimensions.split(",").map((s) => s.trim()).filter(Boolean),
        approved_filters: filters.split(",").map((s) => s.trim()).filter(Boolean),
        currency_unit: "USD",
        timezone_convention: "UTC",
        owner_name: user?.display_name || "Admin",
        freshness_sla: "Daily 02:00 UTC",
      });
      setMetricKey("");
      setDisplayName("");
      setDefinition("");
      setSqlFormula("");
      load(showAllVersions);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed saving semantic metric");
    }
  }

  const canEdit = user?.role === "administrator" || user?.role === "data_engineer";

  return (
    <AppShell
      title="Business Semantic Catalog"
      subtitle="Versioned, auditable definitions for approved business metrics, formulas, dimensions, and units"
    >
      <div className="space-y-6">
        {error && (
          <div className="p-4 rounded-xl bg-rose-50 border border-rose-200 text-sm text-rose-800">
            {error}
          </div>
        )}

        <div className="flex items-center justify-between">
          <div className="text-xs text-slate-600">
            Showing {metrics.length} semantic definition(s)
          </div>
          <label className="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
            <input
              type="checkbox"
              checked={showAllVersions}
              onChange={(e) => {
                setShowAllVersions(e.target.checked);
                load(e.target.checked);
              }}
            />
            Show Historical Versions
          </label>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {metrics.map((m) => (
            <div key={m.id} className="bg-white rounded-xl border border-slate-200 p-4 space-y-2.5">
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2">
                  <BookOpen className="h-4 w-4 text-sky-600" />
                  <div>
                    <div className="text-sm font-semibold text-slate-900">{m.display_name}</div>
                    <div className="text-[11px] font-mono-tabular text-slate-500">
                      {m.metric_key} · v{m.version} · {m.source_table}
                    </div>
                  </div>
                </div>
                <span
                  className={`px-2 py-0.5 rounded-full text-[10px] font-semibold ${
                    m.is_active
                      ? "bg-emerald-50 text-emerald-800 border border-emerald-200"
                      : "bg-slate-100 text-slate-500"
                  }`}
                >
                  {m.is_active ? "ACTIVE" : "SUPERSEDED"}
                </span>
              </div>
              <p className="text-xs text-slate-700">{m.definition}</p>
              <div className="p-2 rounded bg-slate-950 text-sky-200 text-[11px] font-mono-tabular">
                {m.sql_formula}
              </div>
              <div className="text-[11px] text-slate-500 flex flex-wrap gap-3">
                <span>Currency: {m.currency_unit}</span>
                <span>TZ: {m.timezone_convention}</span>
                <span>Owner: {m.owner_name}</span>
                <span>SLA: {m.freshness_sla}</span>
              </div>
            </div>
          ))}
        </div>

        {canEdit && (
          <form
            onSubmit={handleCreateVersion}
            className="bg-white rounded-xl border border-slate-200 p-5 space-y-3"
          >
            <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
              <Plus className="h-4 w-4 text-sky-600" />
              Publish New Versioned Semantic Metric
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 text-xs">
              <input required value={metricKey} onChange={(e) => setMetricKey(e.target.value)} placeholder="metric_key (e.g. avg_order_value)" className="px-3 py-2 border rounded-lg" />
              <input required value={displayName} onChange={(e) => setDisplayName(e.target.value)} placeholder="Display Name" className="px-3 py-2 border rounded-lg" />
              <input required value={sourceTable} onChange={(e) => setSourceTable(e.target.value)} placeholder="Source Table" className="px-3 py-2 border rounded-lg" />
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs">
              <input required value={sqlFormula} onChange={(e) => setSqlFormula(e.target.value)} placeholder="SQL Formula (e.g. AVG(analytics.orders.net_amount))" className="px-3 py-2 border rounded-lg font-mono-tabular" />
              <input required value={definition} onChange={(e) => setDefinition(e.target.value)} placeholder="Business definition" className="px-3 py-2 border rounded-lg" />
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs">
              <input value={dimensions} onChange={(e) => setDimensions(e.target.value)} placeholder="Approved dimensions (comma-separated)" className="px-3 py-2 border rounded-lg" />
              <input value={filters} onChange={(e) => setFilters(e.target.value)} placeholder="Approved filters" className="px-3 py-2 border rounded-lg" />
            </div>
            <button type="submit" className="px-4 py-2 rounded-lg bg-sky-600 text-white text-xs font-semibold cursor-pointer">
              Save Versioned Definition
            </button>
          </form>
        )}
      </div>
    </AppShell>
  );
}
