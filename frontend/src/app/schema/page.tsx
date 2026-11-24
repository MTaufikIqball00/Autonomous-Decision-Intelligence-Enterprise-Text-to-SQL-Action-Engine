"use client";

import React, { useEffect, useState } from "react";
import { Database, GitMerge, Key, RefreshCw, Search } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { api, getStoredUser } from "@/lib/api";
import type { SchemaCatalog, UserProfile } from "@/lib/types";

export default function SchemaExplorerPage() {
  const [user, setUser] = useState<UserProfile | null>(null);
  const [catalog, setCatalog] = useState<SchemaCatalog | null>(null);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Manual relationship state
  const [fromSchema, setFromSchema] = useState("analytics");
  const [fromTable, setFromTable] = useState("orders");
  const [fromColumn, setFromColumn] = useState("region_id");
  const [toSchema, setToSchema] = useState("analytics");
  const [toTable, setToTable] = useState("regions");
  const [toColumn, setToColumn] = useState("region_id");
  const [relDesc, setRelDesc] = useState("Verified operational region join");
  const [relMsg, setRelMsg] = useState<string | null>(null);

  useEffect(() => {
    setUser(getStoredUser());
    api
      .schema()
      .then((res) => setCatalog(res.catalog))
      .catch((e) => setError(e instanceof Error ? e.message : "Failed loading schema"))
      .finally(() => setLoading(false));
  }, []);

  async function handleRefresh() {
    setRefreshing(true);
    setError(null);
    try {
      const res = await api.refreshSchema();
      setCatalog(res.catalog);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Schema refresh failed");
    } finally {
      setRefreshing(false);
    }
  }

  async function handleAddRel(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setRelMsg(null);
    try {
      await api.addManualRelationship({
        from_schema: fromSchema,
        from_table: fromTable,
        from_column: fromColumn,
        to_schema: toSchema,
        to_table: toTable,
        to_column: toColumn,
        relationship_type: "many_to_one",
        description: relDesc,
      });
      const res = await api.schema();
      setCatalog(res.catalog);
      setRelMsg("Documented relationship persisted and merged into schema snapshot.");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed adding relationship");
    }
  }

  const filteredTables =
    catalog?.tables.filter((t) => {
      const q = search.trim().toLowerCase();
      if (!q) return true;
      return (
        t.name.toLowerCase().includes(q) ||
        t.schema.toLowerCase().includes(q) ||
        (t.description || "").toLowerCase().includes(q) ||
        t.columns.some((c) => c.name.toLowerCase().includes(q))
      );
    }) || [];

  return (
    <AppShell
      title="Schema Explorer & Relationship Viewer"
      subtitle="Live PostgreSQL catalog metadata filtered strictly by your RBAC policy"
    >
      <div className="space-y-6">
        {error && (
          <div className="p-4 rounded-xl bg-rose-50 border border-rose-200 text-sm text-rose-800">
            {error}
          </div>
        )}

        <div className="bg-white rounded-xl border border-slate-200 p-4 flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2 flex-1 min-w-[240px]">
            <Search className="h-4 w-4 text-slate-400" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search permitted schemas, tables, columns, or descriptions..."
              className="w-full text-sm focus:outline-none"
            />
          </div>

          <div className="flex items-center gap-3">
            {catalog && (
              <span className="text-xs font-mono-tabular text-slate-500">
                DB: {catalog.database_name} · Version v{catalog.version}
              </span>
            )}
            <button
              type="button"
              disabled={refreshing || !user?.policy.allow_schema_refresh}
              onClick={handleRefresh}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold bg-slate-900 text-white hover:bg-slate-800 disabled:opacity-50 cursor-pointer"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} />
              Refresh Catalog
            </button>
          </div>
        </div>

        {loading && (
          <div className="bg-white rounded-xl border border-slate-200 p-8 text-sm text-slate-500">
            Discovering live PostgreSQL metadata...
          </div>
        )}

        {catalog && (
          <>
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
              {filteredTables.map((t) => (
                <div
                  key={`${t.schema}.${t.name}`}
                  className="bg-white rounded-xl border border-slate-200 overflow-hidden"
                >
                  <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <Database className="h-4 w-4 text-sky-600" />
                      <span className="font-mono-tabular text-sm font-semibold text-slate-900">
                        {t.schema}.{t.name}
                      </span>
                    </div>
                    <span className="px-2 py-0.5 rounded text-[10px] font-semibold bg-slate-200 text-slate-700">
                      {t.table_type}
                    </span>
                  </div>
                  <div className="p-4 space-y-3">
                    {t.description && (
                      <p className="text-xs text-slate-600">{t.description}</p>
                    )}
                    <div className="divide-y divide-slate-100 border border-slate-100 rounded-lg">
                      {t.columns.map((c) => (
                        <div
                          key={c.name}
                          className="px-3 py-1.5 flex items-center justify-between text-xs gap-2"
                        >
                          <div className="flex items-center gap-1.5 min-w-0">
                            {c.is_primary_key && (
                              <Key className="h-3 w-3 text-amber-600 shrink-0" />
                            )}
                            <span className="font-mono-tabular font-medium text-slate-800 truncate">
                              {c.name}
                            </span>
                          </div>
                          <span className="font-mono-tabular text-[11px] text-slate-500 shrink-0">
                            {c.data_type}
                            {c.is_nullable ? " (null)" : ""}
                          </span>
                        </div>
                      ))}
                    </div>
                    {t.foreign_keys.length > 0 && (
                      <div className="text-[11px] text-slate-600 space-y-1">
                        <div className="font-semibold text-slate-700 flex items-center gap-1">
                          <GitMerge className="h-3 w-3" /> Verified Relationships
                        </div>
                        {t.foreign_keys.map((fk) => (
                          <div key={fk.constraint_name} className="font-mono-tabular">
                            {fk.from_column} → {fk.to_schema}.{fk.to_table}.{fk.to_column} ({fk.source})
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>

            {user?.policy.allow_schema_refresh && (
              <form
                onSubmit={handleAddRel}
                className="bg-white rounded-xl border border-slate-200 p-5 space-y-3"
              >
                <h2 className="text-sm font-semibold text-slate-900">
                  Document Verified Relationship (Administrator / Data Engineer)
                </h2>
                {relMsg && (
                  <div className="p-2.5 rounded bg-emerald-50 border border-emerald-200 text-xs text-emerald-800">
                    {relMsg}
                  </div>
                )}
                <div className="grid grid-cols-2 sm:grid-cols-6 gap-2 text-xs">
                  <input value={fromSchema} onChange={(e) => setFromSchema(e.target.value)} placeholder="from_schema" className="px-2.5 py-2 border rounded" />
                  <input value={fromTable} onChange={(e) => setFromTable(e.target.value)} placeholder="from_table" className="px-2.5 py-2 border rounded" />
                  <input value={fromColumn} onChange={(e) => setFromColumn(e.target.value)} placeholder="from_column" className="px-2.5 py-2 border rounded" />
                  <input value={toSchema} onChange={(e) => setToSchema(e.target.value)} placeholder="to_schema" className="px-2.5 py-2 border rounded" />
                  <input value={toTable} onChange={(e) => setToTable(e.target.value)} placeholder="to_table" className="px-2.5 py-2 border rounded" />
                  <input value={toColumn} onChange={(e) => setToColumn(e.target.value)} placeholder="to_column" className="px-2.5 py-2 border rounded" />
                </div>
                <div className="flex gap-2">
                  <input value={relDesc} onChange={(e) => setRelDesc(e.target.value)} placeholder="Relationship documentation" className="flex-1 px-3 py-2 text-xs border rounded" />
                  <button type="submit" className="px-4 py-2 rounded-lg bg-sky-600 text-white text-xs font-semibold cursor-pointer">
                    Save Relationship
                  </button>
                </div>
              </form>
            )}
          </>
        )}
      </div>
    </AppShell>
  );
}
