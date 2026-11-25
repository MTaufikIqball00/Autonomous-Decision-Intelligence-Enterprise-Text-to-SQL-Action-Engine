"use client";

import React, { useEffect, useState } from "react";
import {
  Activity,
  CheckCircle2,
  Play,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { api, getStoredUser } from "@/lib/api";
import type {
  ActionConnectorDto,
  ActionExecutionDto,
  ActionProposalDto,
  ActionRuleDto,
  UserProfile,
} from "@/lib/types";

export default function ActionCenterPage() {
  const [user, setUser] = useState<UserProfile | null>(null);
  const [connectors, setConnectors] = useState<ActionConnectorDto[]>([]);
  const [rules, setRules] = useState<ActionRuleDto[]>([]);
  const [proposals, setProposals] = useState<ActionProposalDto[]>([]);
  const [executions, setExecutions] = useState<ActionExecutionDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  // New connector form
  const [connName, setConnName] = useState("");
  const [connType, setConnType] = useState<"webhook" | "slack_teams" | "email_ticket_payload">("webhook");
  const [connUrl, setConnUrl] = useState("");
  const [connSecret, setConnSecret] = useState("");

  // New monitoring rule form
  const [ruleName, setRuleName] = useState("");
  const [ruleSql, setRuleSql] = useState(
    "SELECT COUNT(*) AS completed_orders FROM analytics.orders WHERE status IN ('COMPLETED', 'SHIPPED')"
  );
  const [ruleMetricCol, setRuleMetricCol] = useState("completed_orders");
  const [ruleComparator, setRuleComparator] = useState("gt");
  const [ruleThreshold, setRuleThreshold] = useState("10");
  const [ruleConnectorId, setRuleConnectorId] = useState("");
  const [ruleInterval, setRuleInterval] = useState("300");

  const isOperator =
    user?.role === "administrator" || user?.role === "data_engineer";

  async function load() {
    try {
      const [c, r, p, e] = await Promise.all([
        api.actionConnectors(),
        api.actionRules(),
        api.actionProposals(),
        api.actionExecutions(),
      ]);
      setConnectors(c);
      setRules(r);
      setProposals(p);
      setExecutions(e);
      if (!ruleConnectorId && c.length > 0) {
        setRuleConnectorId(c[0].id);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed loading Action Center");
    }
  }

  useEffect(() => {
    setUser(getStoredUser());
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleCreateConnector(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setStatusMsg(null);
    try {
      const created = await api.createActionConnector({
        name: connName,
        connector_type: connType,
        endpoint_url: connUrl,
        secret: connSecret,
      });
      setConnName("");
      setConnUrl("");
      setConnSecret("");
      setRuleConnectorId(created.id);
      setStatusMsg(`Registered encrypted connector "${created.name}".`);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed creating connector");
    }
  }

  async function handleCreateRule(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setStatusMsg(null);
    try {
      const created = await api.createActionRule({
        name: ruleName,
        sql_text: ruleSql,
        metric_column: ruleMetricCol,
        comparator: ruleComparator,
        threshold_value: Number(ruleThreshold) || 0,
        connector_id: ruleConnectorId || null,
        schedule_interval_secs: Number(ruleInterval) || 300,
      });
      setRuleName("");
      setStatusMsg(`Created AST-validated monitoring rule "${created.name}".`);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed creating monitoring rule");
    }
  }

  async function handleEvaluateRule(id: string) {
    setBusyId(id);
    setError(null);
    setStatusMsg(null);
    try {
      const res = await api.evaluateActionRule(id);
      setStatusMsg(
        `Evaluated read-only (query_id=${res.query_id.slice(0, 8)}): observed=${res.observed_value ?? "n/a"}, triggered=${res.triggered}.`
      );
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Rule evaluation failed");
    } finally {
      setBusyId(null);
    }
  }

  async function handleApprove(p: ActionProposalDto) {
    setBusyId(p.id);
    setError(null);
    setStatusMsg(null);
    try {
      const connectorId = p.connector_id || ruleConnectorId || connectors[0]?.id;
      await api.approveActionProposal(p.id, {
        review_note: "Approved in Action Center after evidence review",
        connector_id: connectorId,
      });
      setStatusMsg(`Approved proposal "${p.title}". Ready for idempotent dispatch.`);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Approve failed");
    } finally {
      setBusyId(null);
    }
  }

  async function handleReject(p: ActionProposalDto) {
    setBusyId(p.id);
    setError(null);
    setStatusMsg(null);
    try {
      await api.rejectActionProposal(p.id, {
        review_note: "Rejected in Action Center review",
      });
      setStatusMsg(`Rejected proposal "${p.title}".`);
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Reject failed");
    } finally {
      setBusyId(null);
    }
  }

  async function handleExecute(p: ActionProposalDto) {
    setBusyId(p.id);
    setError(null);
    setStatusMsg(null);
    try {
      const res = await api.executeActionProposal(p.id);
      setStatusMsg(
        `Execution outcome="${res.execution.outcome}" (${res.execution.attempt_count} attempt(s)).`
      );
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Action execution failed");
    } finally {
      setBusyId(null);
    }
  }

  return (
    <AppShell
      title="Autonomous Decision Intelligence & Action Center"
      subtitle="AST-validated read-only monitors -> evidence-backed proposals -> Human-in-the-Loop approval -> non-destructive connectors"
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

        {/* Action Proposals (Human-in-the-Loop Queue) */}
        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <ShieldAlert className="h-4 w-4 text-amber-600" />
              <h2 className="text-sm font-semibold text-slate-900">
                Action Proposals Queue — Human-in-the-Loop ({proposals.length})
              </h2>
            </div>
            <span className="text-[11px] text-slate-500">
              {isOperator
                ? "Operator permissions active (Approve / Reject / Execute)"
                : "Read/Propose mode (Administrator or Data Engineer required to Approve/Execute)"}
            </span>
          </div>

          {proposals.length === 0 ? (
            <div className="p-6 text-sm text-slate-500 text-center">
              No action proposals yet. Evaluate a monitoring rule below or propose an action from the NL-to-SQL Workspace.
            </div>
          ) : (
            <div className="divide-y divide-slate-100">
              {proposals.map((p) => (
                <div key={p.id} className="p-4 space-y-2.5">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-semibold text-slate-900">{p.title}</span>
                        <span
                          className={`px-2 py-0.5 rounded-full text-[10px] font-semibold uppercase ${
                            p.status === "executed"
                              ? "bg-emerald-50 text-emerald-800 border border-emerald-200"
                              : p.status === "approved"
                              ? "bg-sky-50 text-sky-800 border border-sky-200"
                              : p.status === "rejected" || p.status === "failed"
                              ? "bg-rose-50 text-rose-800 border border-rose-200"
                              : "bg-amber-50 text-amber-800 border border-amber-200"
                          }`}
                        >
                          {p.status}
                        </span>
                      </div>
                      <p className="text-xs text-slate-700">{p.rationale}</p>
                      <div className="text-[11px] font-mono-tabular text-slate-500">
                        query_id={p.source_query_id || "n/a"} · idempotency={p.idempotency_key}
                      </div>
                    </div>

                    {isOperator && (
                      <div className="flex flex-wrap gap-2">
                        {p.status === "pending" && (
                          <>
                            <button
                              type="button"
                              disabled={busyId === p.id}
                              onClick={() => handleApprove(p)}
                              className="inline-flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-semibold bg-sky-600 text-white hover:bg-sky-700 cursor-pointer"
                            >
                              <CheckCircle2 className="h-3.5 w-3.5" /> Approve
                            </button>
                            <button
                              type="button"
                              disabled={busyId === p.id}
                              onClick={() => handleReject(p)}
                              className="inline-flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-medium border border-slate-300 text-slate-700 hover:bg-slate-100 cursor-pointer"
                            >
                              <X className="h-3.5 w-3.5" /> Reject
                            </button>
                          </>
                        )}
                        {p.status === "approved" && (
                          <button
                            type="button"
                            disabled={busyId === p.id}
                            onClick={() => handleExecute(p)}
                            className="inline-flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-semibold bg-emerald-600 text-white hover:bg-emerald-700 cursor-pointer"
                          >
                            <Play className="h-3.5 w-3.5" /> Execute Connector
                          </button>
                        )}
                      </div>
                    )}
                  </div>
                  <pre className="p-2.5 rounded bg-slate-950 text-sky-200 text-[11px] font-mono-tabular overflow-x-auto">
                    <code>{JSON.stringify(p.evidence_json, null, 2)}</code>
                  </pre>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Monitoring Rules */}
        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
            <Activity className="h-4 w-4 text-sky-600" />
            <h2 className="text-sm font-semibold text-slate-900">
              Scheduled Read-Only Anomaly & Threshold Monitors ({rules.length})
            </h2>
          </div>
          <div className="divide-y divide-slate-100">
            {rules.map((r) => (
              <div key={r.id} className="p-4 flex flex-wrap items-center justify-between gap-3">
                <div className="space-y-1 max-w-3xl">
                  <div className="text-sm font-semibold text-slate-900">
                    {r.name}{" "}
                    <span className="text-xs font-mono-tabular text-sky-700">
                      ({r.metric_column} {r.comparator} {r.threshold_value} · every {r.schedule_interval_secs}s)
                    </span>
                  </div>
                  <div className="text-[11px] font-mono-tabular text-slate-600 break-all">
                    {r.sql_text}
                  </div>
                </div>
                {isOperator && (
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      disabled={busyId === r.id}
                      onClick={() => handleEvaluateRule(r.id)}
                      className="inline-flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-semibold bg-slate-900 text-white hover:bg-slate-800 cursor-pointer"
                    >
                      <Play className="h-3.5 w-3.5" /> Evaluate Now
                    </button>
                    <button
                      type="button"
                      onClick={() => api.deleteActionRule(r.id).then(load)}
                      aria-label={`Delete rule ${r.name}`}
                      className="p-1.5 text-slate-400 hover:text-rose-600 cursor-pointer"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                )}
              </div>
            ))}
          </div>

          {isOperator && (
            <form onSubmit={handleCreateRule} className="p-4 bg-slate-50 border-t border-slate-200 space-y-2 text-xs">
              <div className="font-semibold text-slate-800">Add Read-Only Threshold Monitor</div>
              <div className="grid grid-cols-1 sm:grid-cols-5 gap-2">
                <input required value={ruleName} onChange={(e) => setRuleName(e.target.value)} placeholder="Monitor rule name" className="px-2.5 py-2 border rounded bg-white" />
                <input required value={ruleMetricCol} onChange={(e) => setRuleMetricCol(e.target.value)} placeholder="Numeric column" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
                <select value={ruleComparator} onChange={(e) => setRuleComparator(e.target.value)} className="px-2.5 py-2 border rounded bg-white">
                  <option value="gt">&gt; (gt)</option>
                  <option value="gte">&gt;= (gte)</option>
                  <option value="lt">&lt; (lt)</option>
                  <option value="lte">&lt;= (lte)</option>
                  <option value="row_count_gt">row_count &gt;</option>
                </select>
                <input required type="number" step="any" value={ruleThreshold} onChange={(e) => setRuleThreshold(e.target.value)} placeholder="Threshold" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
                <select value={ruleConnectorId} onChange={(e) => setRuleConnectorId(e.target.value)} className="px-2.5 py-2 border rounded bg-white">
                  <option value="">Select connector...</option>
                  {connectors.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} ({c.connector_type})
                    </option>
                  ))}
                </select>
              </div>
              <div className="flex flex-col sm:flex-row gap-2">
                <input required value={ruleSql} onChange={(e) => setRuleSql(e.target.value)} placeholder="AST-validated SELECT SQL" className="flex-1 px-2.5 py-2 border rounded bg-white font-mono-tabular" />
                <input type="number" min={30} value={ruleInterval} onChange={(e) => setRuleInterval(e.target.value)} placeholder="Interval (s)" className="w-28 px-2.5 py-2 border rounded bg-white font-mono-tabular" />
                <button type="submit" className="px-4 py-2 rounded bg-sky-600 text-white font-semibold cursor-pointer">
                  Create Rule
                </button>
              </div>
            </form>
          )}
        </section>

        {/* Non-Destructive Action Connectors */}
        <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-sky-600" />
            <h2 className="text-sm font-semibold text-slate-900">
              Non-Destructive Outbound Connectors ({connectors.length})
            </h2>
          </div>
          <div className="divide-y divide-slate-100">
            {connectors.map((c) => (
              <div key={c.id} className="p-4 flex flex-wrap items-center justify-between gap-2 text-xs">
                <div>
                  <div className="font-semibold text-slate-900">{c.name}</div>
                  <div className="font-mono-tabular text-slate-500">
                    type={c.connector_type} · url={c.endpoint_url || "(local staged payload)"} · secret_encrypted={String(c.has_secret)}
                  </div>
                </div>
                {isOperator && (
                  <button
                    type="button"
                    onClick={() => api.deleteActionConnector(c.id).then(load)}
                    className="p-1.5 text-slate-400 hover:text-rose-600 cursor-pointer"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                )}
              </div>
            ))}
          </div>
          {isOperator && (
            <form onSubmit={handleCreateConnector} className="p-4 bg-slate-50 border-t border-slate-200 grid grid-cols-1 sm:grid-cols-4 gap-2 text-xs">
              <input required value={connName} onChange={(e) => setConnName(e.target.value)} placeholder="Connector name" className="px-2.5 py-2 border rounded bg-white" />
              <select value={connType} onChange={(e) => setConnType(e.target.value as typeof connType)} className="px-2.5 py-2 border rounded bg-white">
                <option value="webhook">webhook</option>
                <option value="slack_teams">slack_teams</option>
                <option value="email_ticket_payload">email_ticket_payload</option>
              </select>
              <input value={connUrl} onChange={(e) => setConnUrl(e.target.value)} placeholder="https://... (optional for email_ticket)" className="px-2.5 py-2 border rounded bg-white font-mono-tabular" />
              <div className="flex gap-2">
                <input type="password" value={connSecret} onChange={(e) => setConnSecret(e.target.value)} placeholder="Signing/Auth Secret (AES-GCM)" className="flex-1 px-2.5 py-2 border rounded bg-white" />
                <button type="submit" className="px-3 py-2 rounded bg-sky-600 text-white font-semibold cursor-pointer">
                  Add
                </button>
              </div>
            </form>
          )}
        </section>

        {/* Action Executions Audit */}
        {executions.length > 0 && (
          <section className="bg-white rounded-xl border border-slate-200 overflow-hidden">
            <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 text-sm font-semibold text-slate-900">
              Action Execution History ({executions.length})
            </div>
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-slate-200 text-xs">
                <thead className="bg-slate-50">
                  <tr>
                    <th className="px-3 py-2 text-left">Timestamp</th>
                    <th className="px-3 py-2 text-left">Outcome</th>
                    <th className="px-3 py-2 text-left">Attempts</th>
                    <th className="px-3 py-2 text-left">HTTP</th>
                    <th className="px-3 py-2 text-left">Summary</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {executions.map((ex) => (
                    <tr key={ex.id}>
                      <td className="px-3 py-2 font-mono-tabular whitespace-nowrap">
                        {new Date(ex.created_at).toLocaleString()}
                      </td>
                      <td className="px-3 py-2 font-semibold">{ex.outcome}</td>
                      <td className="px-3 py-2 font-mono-tabular">{ex.attempt_count}</td>
                      <td className="px-3 py-2 font-mono-tabular">{ex.http_status ?? "-"}</td>
                      <td className="px-3 py-2 font-mono-tabular max-w-lg truncate">
                        {ex.response_summary}
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
