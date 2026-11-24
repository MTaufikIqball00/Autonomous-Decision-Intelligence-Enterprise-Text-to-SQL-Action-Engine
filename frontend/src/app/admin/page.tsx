"use client";

import React, { useEffect, useState } from "react";
import { CheckCircle2, Cpu, Database, FileText, ShieldCheck } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { api, getStoredUser } from "@/lib/api";
import type {
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
  const [audits, setAudits] = useState<AuditLogDto[]>([]);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Add LLM Provider form
  const [pName, setPName] = useState("");
  const [pType, setPType] = useState("openai");
  const [pModel, setPModel] = useState("");
  const [pBaseUrl, setPBaseUrl] = useState("https://api.openai.com/v1");
  const [pKey, setPKey] = useState("");

  async function load() {
    try {
      const [conns, provs] = await Promise.all([api.connections(), api.providers()]);
      setConnections(conns);
      setProviders(provs);
      const u = getStoredUser();
      setUser(u);
      if (u?.role === "administrator" || u?.role === "auditor") {
        const [rbacs, logs] = await Promise.all([api.rbacPolicies(), api.auditLogs()]);
        setPolicies(rbacs);
        setAudits(logs);
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

  return (
    <AppShell
      title="Governance, Connections, LLM Providers & Audit Logs"
      subtitle="Encrypted connection secrets, provider-independent LLM settings, RBAC policies, and audit trails"
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
                    <th className="px-3 py-2 text-left">Row Limit</th>
                    <th className="px-3 py-2 text-left">PG RLS Region</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {policies.map((p) => (
                    <tr key={p.role}>
                      <td className="px-3 py-2 font-semibold">{p.role}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.allowed_schemas.join(", ")}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.denied_columns.join(", ") || "None"}</td>
                      <td className="px-3 py-2">{String(p.allow_export)}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.row_limit}</td>
                      <td className="px-3 py-2 font-mono-tabular">{p.region_filter}</td>
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
            <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
              <FileText className="h-4 w-4 text-sky-600" />
              <h2 className="text-sm font-semibold text-slate-900">
                Security Audit Logs ({audits.length})
              </h2>
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
                  {audits.map((a) => (
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
