"use client";

import React, { useEffect, useMemo, useState } from "react";
import { CheckCircle2, Cpu, Database, FileText, ShieldCheck } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { api, getStoredUser } from "@/lib/api";
import type {
  AdminUserDto,
  AuditLogDto,
  ConnectionDto,
  ProviderDto,
  RbacPolicy,
  UserProfile,
} from "@/lib/types";

export default function AdminGovernancePage() {
  const [user, setUser] = useState<UserProfile | null>(null);
  const [connections, setConnections] = useState<ConnectionDto[]>([]);
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [policies, setPolicies] = useState<RbacPolicy[]>([]);
  const [users, setUsers] = useState<AdminUserDto[]>([]);
  const [audits, setAudits] = useState<AuditLogDto[]>([]);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Add DB Connection form
  const [cName, setCName] = useState("");
  const [cAdapter, setCAdapter] = useState("postgresql");
  const [cHost, setCHost] = useState("127.0.0.1");
  const [cPort, setCPort] = useState("5439");
  const [cDb, setCDb] = useState("data_to_action_analytics");
  const [cUser, setCUser] = useState("dta_readonly");
  const [cPass, setCPass] = useState("");
  const [cSchemas, setCSchemas] = useState("analytics, finance");

  // Add LLM Provider form
  const [pName, setPName] = useState("");
  const [pType, setPType] = useState("openai");
  const [pModel, setPModel] = useState("");
  const [pBaseUrl, setPBaseUrl] = useState("https://api.openai.com/v1");
  const [pKey, setPKey] = useState("");

  // RBAC Policy editor state
  const [editingRole, setEditingRole] = useState<string | null>(null);
  const [editSchemas, setEditSchemas] = useState("");
  const [editTables, setEditTables] = useState("");
  const [editDeniedCols, setEditDeniedCols] = useState("");
  const [editRowLimit, setEditRowLimit] = useState("500");
  const [editRegion, setEditRegion] = useState("ALL");
  const [editExport, setEditExport] = useState(false);
  const [editSchemaRefresh, setEditSchemaRefresh] = useState(false);

  // Audit log filters
  const [auditSearch, setAuditSearch] = useState("");
  const [auditDecision, setAuditDecision] = useState("all");
  const [auditEvent, setAuditEvent] = useState("all");

  async function load() {
    try {
      const [conns, provs] = await Promise.all([api.connections(), api.providers()]);
      setConnections(conns);
      setProviders(provs);
      const u = getStoredUser();
      setUser(u);
      if (u?.role === "administrator" || u?.role === "auditor") {
        const [rbacs, logs, tenantUsers] = await Promise.all([
          api.rbacPolicies(),
          api.auditLogs(),
          api.adminUsers(),
        ]);
        setPolicies(rbacs);
        setAudits(logs);
        setUsers(tenantUsers);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed loading governance data");
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function handleTestConn(id: string) {
    setStatusMsg(null);
    setError(null);
    try {
      const res = await api.testConnection(id);
      setStatusMsg(`${res.identity}: ${res.status}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Connection test failed");
    }
  }

  async function handleAddConnection(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setStatusMsg(null);
    try {
      const created = await api.createConnection({
        name: cName,
        adapter_type: cAdapter,
        host: cHost,
        port: Number(cPort) || 5439,
        database_name: cDb,
        username: cUser,
        password: cPass,
        ssl_mode: "prefer",
        allowed_schemas: cSchemas
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      });
      setCName("");
      setCPass("");
      setStatusMsg(`Registered encrypted connection "${created.name}".`);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed registering database connection");
    }
  }

  async function handleActivateProvider(id: string) {
    setError(null);
    try {
      await api.activateProvider(id);
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Activation failed");
    }
  }

  async function handleAddProvider(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      await api.createProvider({
        name: pName,
        provider_type: pType,
        model_name: pModel,
        base_url: pBaseUrl,
        api_key: pKey,
      });
      setPName("");
      setPModel("");
      setPKey("");
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed registering provider");
    }
  }

  function startEditPolicy(p: RbacPolicy) {
    setEditingRole(p.role);
    setEditSchemas(p.allowed_schemas.join(", "));
    setEditTables(p.allowed_tables.join(", "));
    setEditDeniedCols(p.denied_columns.join(", "));
    setEditRowLimit(String(p.row_limit));
    setEditRegion(p.region_filter);
    setEditExport(p.allow_export);
    setEditSchemaRefresh(p.allow_schema_refresh);
  }

  async function handleSavePolicy(e: React.FormEvent) {
    e.preventDefault();
    if (!editingRole) return;
    setError(null);
    setStatusMsg(null);
    const splitCsv = (s: string) =>
      s
        .split(",")
        .map((v) => v.trim())
        .filter(Boolean);
    try {
      const updated = await api.updateRbacPolicy(editingRole, {
        allowed_schemas: splitCsv(editSchemas),
        allowed_tables: splitCsv(editTables),
        denied_columns: splitCsv(editDeniedCols),
        allow_export: editExport,
        allow_schema_refresh: editSchemaRefresh,
        row_limit: Number(editRowLimit) || 500,
        region_filter: editRegion || "ALL",
      });
      setPolicies((prev) => prev.map((p) => (p.role === updated.role ? updated : p)));
      setEditingRole(null);
      setStatusMsg(`Updated RBAC policy for role "${updated.role}".`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed updating RBAC policy");
    }
  }

  async function handleUpdateUser(u: AdminUserDto, patch: { role?: string; is_active?: boolean }) {
    setError(null);
    setStatusMsg(null);
    try {
      const updated = await api.updateAdminUser(u.id, patch);
      setUsers((prev) => prev.map((row) => (row.id === updated.id ? updated : row)));
      setStatusMsg(`Updated user "${updated.email}" (role=${updated.role}, active=${updated.is_active}).`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed updating user");
    }
  }

  const distinctEvents = useMemo(
    () => Array.from(new Set(audits.map((a) => a.event_type))).sort(),
    [audits]
  );

  const filteredAudits = useMemo(() => {
    const q = auditSearch.trim().toLowerCase();
    return audits.filter((a) => {
      if (auditDecision !== "all" && a.decision !== auditDecision) return false;
      if (auditEvent !== "all" && a.event_type !== auditEvent) return false;
      if (!q) return true;
      return (
        a.user_email.toLowerCase().includes(q) ||
        a.event_type.toLowerCase().includes(q) ||
        a.resource_id.toLowerCase().includes(q) ||
        JSON.stringify(a.details).toLowerCase().includes(q)
      );
    });
  }, [audits, auditSearch, auditDecision, auditEvent]);

  return (
    <AppShell
      title="Governance, Connections, LLM Providers & Audit Logs"
      subtitle="Encrypted connection secrets, provider-independent LLM settings, RBAC policies, users, and audit trails"
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

        {/* Database Connections */}
        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
            <Database className="h-4 w-4 text-sky-600" />
            <h2 className="text-sm font-semibold text-slate-900">
              Configured Analytics Database Connections (Credentials Encrypted & Hidden)
            </h2>
          </div>
          <div className="divide-y divide-slate-100">
            {connections.map((c) => (
              <div key={c.id} className="p-4 flex flex-wrap items-center justify-between gap-3">
                <div className="space-y-1">
                  <div className="text-sm font-semibold text-slate-900">{c.name}</div>
                  <div className="text-xs font-mono-tabular text-slate-500">
                    {c.adapter_type} · {c.username}@{c.host}:{c.port}/{c.database_name} ·
                    schemas: [{c.allowed_schemas.join(", ")}] · read_only={String(c.is_readonly)}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleTestConn(c.id)}
                  className="px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-900 text-white hover:bg-slate-800 cursor-pointer"
                >
                  Test Read-Only Connection
                </button>
              </div>
            ))}
          </div>

          {user?.policy.allow_admin_config && (
            <form
              onSubmit={handleAddConnection}
              className="p-4 bg-slate-50 border-t border-slate-200 grid grid-cols-1 sm:grid-cols-4 gap-2 text-xs"
            >
              <input required value={cName} onChange={(e) => setCName(e.target.value)} placeholder="Connection label" className="px-2.5 py-2 border rounded bg-white" />
              <select value={cAdapter} onChange={(e) => setCAdapter(e.target.value)} className="px-2.5 py-2 border rounded bg-white">
                <option value="postgresql">postgresql</option>
                <option value="bigquery">bigquery (stub)</option>
              </select>
              <input required value={cHost} onChange={(e) => setCHost(e.target.value)} placeholder="Host" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <input required value={cPort} onChange={(e) => setCPort(e.target.value)} placeholder="Port" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <input required value={cDb} onChange={(e) => setCDb(e.target.value)} placeholder="Database name" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <input required value={cUser} onChange={(e) => setCUser(e.target.value)} placeholder="Read-only DB user" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <input required type="password" value={cPass} onChange={(e) => setCPass(e.target.value)} placeholder="Read-only password (encrypted)" className="px-2.5 py-2 border rounded bg-white" />
              <div className="flex gap-2">
                <input value={cSchemas} onChange={(e) => setCSchemas(e.target.value)} placeholder="Allowed schemas CSV" className="flex-1 px-2.5 py-2 border rounded bg-white font-mono-tabular" />
                <button type="submit" className="px-3 py-2 rounded bg-sky-600 text-white font-semibold cursor-pointer">
                  Add DB
                </button>
              </div>
            </form>
          )}
        </section>

        {/* LLM Providers */}
        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
            <Cpu className="h-4 w-4 text-sky-600" />
            <h2 className="text-sm font-semibold text-slate-900">
              Model Provider Configuration (No Hardcoded Model Versions)
            </h2>
          </div>
          <div className="divide-y divide-slate-100">
            {providers.map((p) => (
              <div key={p.id} className="p-4 flex flex-wrap items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-semibold text-slate-900 flex items-center gap-2">
                    {p.name}
                    {p.is_active && (
                      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] bg-emerald-50 text-emerald-800 border border-emerald-200">
                        <CheckCircle2 className="h-3 w-3" /> ACTIVE
                      </span>
                    )}
                  </div>
                  <div className="text-xs font-mono-tabular text-slate-500">
                    type={p.provider_type} · model={p.model_name} · api_key_stored={String(p.has_api_key)}
                  </div>
                </div>
                {user?.policy.allow_admin_config && !p.is_active && (
                  <button
                    type="button"
                    onClick={() => handleActivateProvider(p.id)}
                    className="px-3 py-1.5 rounded-lg text-xs font-medium border border-slate-300 hover:bg-slate-100 cursor-pointer"
                  >
                    Activate
                  </button>
                )}
              </div>
            ))}
          </div>

          {user?.policy.allow_admin_config && (
            <form onSubmit={handleAddProvider} className="p-4 bg-slate-50 border-t border-slate-200 grid grid-cols-1 sm:grid-cols-5 gap-2 text-xs">
              <input required value={pName} onChange={(e) => setPName(e.target.value)} placeholder="Provider label" className="px-2.5 py-2 border rounded bg-white" />
              <select value={pType} onChange={(e) => setPType(e.target.value)} className="px-2.5 py-2 border rounded bg-white">
                <option value="openai">openai</option>
                <option value="gemini">gemini</option>
                <option value="local_deterministic">local_deterministic</option>
              </select>
              <input required value={pModel} onChange={(e) => setPModel(e.target.value)} placeholder="Explicit model ID" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <input value={pBaseUrl} onChange={(e) => setPBaseUrl(e.target.value)} placeholder="Base URL" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <div className="flex gap-2">
                <input type="password" value={pKey} onChange={(e) => setPKey(e.target.value)} placeholder="API Key (encrypted)" className="flex-1 px-2.5 py-2 border rounded bg-white" />
                <button type="submit" className="px-3 py-2 rounded bg-sky-600 text-white font-semibold cursor-pointer">
                  Add
                </button>
              </div>
            </form>
          )}
        </section>

        {/* RBAC Policies */}
        {policies.length > 0 && (
          <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
            <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-sky-600" />
              <h2 className="text-sm font-semibold text-slate-900">
                RBAC Role Enforcement Policies
              </h2>
            </div>
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-slate-200 text-xs">
                <thead className="bg-slate-50">
                  <tr>
                    <th className="px-3 py-2 text-left">Role</th>
                    <th className="px-3 py-2 text-left">Allowed Schemas</th>
                    <th className="px-3 py-2 text-left">Denied Columns</th>
                    <th className="px-3 py-2 text-left">Export</th>
                    <th className="px-3 py-2 text-left">Refresh</th>
                    <th className="px-3 py-2 text-left">Row Limit</th>
                    <th className="px-3 py-2 text-left">PG RLS Region</th>
                    {user?.policy.allow_admin_config && (
                      <th className="px-3 py-2 text-right">Actions</th>
                    )}
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {policies.map((p) => (
                    <tr key={p.role}>
                      <td className="px-3 py-2 font-semibold">{p.role}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.allowed_schemas.join(", ")}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.denied_columns.join(", ") || "None"}</td>
                      <td className="px-3 py-2">{String(p.allow_export)}</td>
                      <td className="px-3 py-2">{String(p.allow_schema_refresh)}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.row_limit}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.region_filter}</td>
                      {user?.policy.allow_admin_config && (
                        <td className="px-3 py-2 text-right">
                          <button
                            type="button"
                            onClick={() => startEditPolicy(p)}
                            className="inline-flex items-center gap-1 px-2 py-1 rounded border border-slate-200 hover:bg-slate-100 cursor-pointer"
                          >
                            <ShieldCheck className="h-3 w-3" /> Edit
                          </button>
                        </td>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {editingRole && user?.policy.allow_admin_config && (
              <form
                onSubmit={handleSavePolicy}
                className="p-4 bg-sky-50/50 border-t border-sky-200 space-y-3 text-xs"
              >
                <div className="flex items-center justify-between">
                  <span className="font-semibold text-slate-900">
                    Edit RBAC Policy: <code className="font-mono-tabular">{editingRole}</code>
                  </span>
                  <button
                    type="button"
                    onClick={() => setEditingRole(null)}
                    className="text-slate-500 hover:text-slate-800 cursor-pointer"
                  >
                    Cancel
                  </button>
                </div>
                <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
                  <label className="space-y-1">
                    <span className="text-[11px] text-slate-600">Allowed Schemas (CSV)</span>
                    <input value={editSchemas} onChange={(e) => setEditSchemas(e.target.value)} className="w-full px-2.5 py-1.5 border rounded bg-white font-mono-tabular" />
                  </label>
                  <label className="space-y-1">
                    <span className="text-[11px] text-slate-600">Allowed Tables (empty = all)</span>
                    <input value={editTables} onChange={(e) => setEditTables(e.target.value)} className="w-full px-2.5 py-1.5 border rounded bg-white font-mono-tabular" />
                  </label>
                  <label className="space-y-1">
                    <span className="text-[11px] text-slate-600">Denied Columns (schema.table.col)</span>
                    <input value={editDeniedCols} onChange={(e) => setEditDeniedCols(e.target.value)} className="w-full px-2.5 py-1.5 border rounded bg-white font-mono-tabular" />
                  </label>
                  <label className="space-y-1">
                    <span className="text-[11px] text-slate-600">Row Limit</span>
                    <input type="number" min={1} max={5000} value={editRowLimit} onChange={(e) => setEditRowLimit(e.target.value)} className="w-full px-2.5 py-1.5 border rounded bg-white font-mono-tabular" />
                  </label>
                  <label className="space-y-1">
                    <span className="text-[11px] text-slate-600">PG RLS Region Filter</span>
                    <input value={editRegion} onChange={(e) => setEditRegion(e.target.value)} className="w-full px-2.5 py-1.5 border rounded bg-white font-mono-tabular" />
                  </label>
                  <div className="flex items-end gap-4 pb-1">
                    <label className="inline-flex items-center gap-1.5 cursor-pointer">
                      <input type="checkbox" checked={editExport} onChange={(e) => setEditExport(e.target.checked)} />
                      Allow Export
                    </label>
                    <label className="inline-flex items-center gap-1.5 cursor-pointer">
                      <input type="checkbox" checked={editSchemaRefresh} onChange={(e) => setEditSchemaRefresh(e.target.checked)} />
                      Allow Refresh
                    </label>
                  </div>
                </div>
                <div className="flex justify-end">
                  <button type="submit" className="px-4 py-1.5 rounded-lg bg-sky-600 text-white font-semibold hover:bg-sky-700 cursor-pointer">
                    Save Policy
                  </button>
                </div>
              </form>
            )}
          </section>
        )}

        {/* Tenant Users */}
        {users.length > 0 && (
          <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
            <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-sky-600" />
              <h2 className="text-sm font-semibold text-slate-900">
                Tenant Users & Access Control ({users.length})
              </h2>
            </div>
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-slate-200 text-xs">
                <thead className="bg-slate-50">
                  <tr>
                    <th className="px-3 py-2 text-left">Name</th>
                    <th className="px-3 py-2 text-left">Email</th>
                    <th className="px-3 py-2 text-left">Role</th>
                    <th className="px-3 py-2 text-left">Status</th>
                    {user?.policy.allow_admin_config && (
                      <th className="px-3 py-2 text-right">Actions</th>
                    )}
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {users.map((u) => (
                    <tr key={u.id}>
                      <td className="px-3 py-2 font-medium">{u.display_name}</td>
                      <td className="px-3 py-2 font-mono-tabular">{u.email}</td>
                      <td className="px-3 py-2">
                        {user?.policy.allow_admin_config ? (
                          <select
                            aria-label={`Role for ${u.email}`}
                            value={u.role}
                            onChange={(e) => handleUpdateUser(u, { role: e.target.value })}
                            className="px-2 py-1 border rounded bg-white text-xs"
                          >
                            <option value="administrator">administrator</option>
                            <option value="data_engineer">data_engineer</option>
                            <option value="analyst">analyst</option>
                            <option value="business_user">business_user</option>
                            <option value="auditor">auditor</option>
                          </select>
                        ) : (
                          u.role
                        )}
                      </td>
                      <td className="px-3 py-2">
                        <span
                          className={`px-2 py-0.5 rounded-full text-[10px] font-semibold ${
                            u.is_active
                              ? "bg-emerald-50 text-emerald-800 border border-emerald-200"
                              : "bg-rose-50 text-rose-800 border border-rose-200"
                          }`}
                        >
                          {u.is_active ? "active" : "disabled"}
                        </span>
                      </td>
                      {user?.policy.allow_admin_config && (
                        <td className="px-3 py-2 text-right">
                          <button
                            type="button"
                            disabled={u.id === user.user_id && u.is_active}
                            onClick={() => handleUpdateUser(u, { is_active: !u.is_active })}
                            className="px-2.5 py-1 rounded border border-slate-200 hover:bg-slate-100 disabled:opacity-40 cursor-pointer"
                          >
                            {u.is_active ? "Disable" : "Enable"}
                          </button>
                        </td>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        )}

        {/* Audit Logs */}
        {audits.length > 0 && (
          <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
            <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <FileText className="h-4 w-4 text-sky-600" />
                <h2 className="text-sm font-semibold text-slate-900">
                  Security Audit Logs ({filteredAudits.length} / {audits.length})
                </h2>
              </div>
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <input
                  type="text"
                  value={auditSearch}
                  onChange={(e) => setAuditSearch(e.target.value)}
                  placeholder="Search actor, resource, or details..."
                  className="px-2.5 py-1 rounded border border-slate-300 bg-white"
                />
                <select
                  aria-label="Filter audit event type"
                  value={auditEvent}
                  onChange={(e) => setAuditEvent(e.target.value)}
                  className="px-2 py-1 rounded border border-slate-300 bg-white"
                >
                  <option value="all">All events</option>
                  {distinctEvents.map((ev) => (
                    <option key={ev} value={ev}>
                      {ev}
                    </option>
                  ))}
                </select>
                <select
                  aria-label="Filter audit decision"
                  value={auditDecision}
                  onChange={(e) => setAuditDecision(e.target.value)}
                  className="px-2 py-1 rounded border border-slate-300 bg-white"
                >
                  <option value="all">All decisions</option>
                  <option value="allowed">allowed</option>
                  <option value="completed">completed</option>
                  <option value="denied">denied</option>
                  <option value="rejected">rejected</option>
                </select>
              </div>
            </div>
            <div className="overflow-x-auto max-h-96">
              <table className="min-w-full divide-y divide-slate-200 text-xs">
                <thead className="bg-slate-50 sticky top-0">
                  <tr>
                    <th className="px-3 py-2 text-left">Timestamp</th>
                    <th className="px-3 py-2 text-left">Actor</th>
                    <th className="px-3 py-2 text-left">Event</th>
                    <th className="px-3 py-2 text-left">Decision</th>
                    <th className="px-3 py-2 text-left">Details</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {filteredAudits.map((a) => (
                    <tr key={a.id}>
                      <td className="px-3 py-2 font-mono-tabular whitespace-nowrap">
                        {new Date(a.created_at).toLocaleString()}
                      </td>
                      <td className="px-3 py-2">{a.user_email}</td>
                      <td className="px-3 py-2 font-mono-tabular">{a.event_type}</td>
                      <td className="px-3 py-2">
                        <span
                          className={`px-2 py-0.5 rounded-full text-[10px] font-semibold ${
                            a.decision === "denied" || a.decision === "rejected"
                              ? "bg-rose-50 text-rose-800 border border-rose-200"
                              : "bg-emerald-50 text-emerald-800 border border-emerald-200"
                          }`}
                        >
                          {a.decision}
                        </span>
                      </td>
                      <td className="px-3 py-2 font-mono-tabular text-[11px] max-w-md truncate">
                        {JSON.stringify(a.details)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        )}
      </div>
    </AppShell>
  );
}
