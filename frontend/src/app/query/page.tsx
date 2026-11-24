"use client";

import React, { useEffect, useState } from "react";
import {
  AlertTriangle,
  Database,
  HelpCircle,
  Play,
  Send,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { ChartRenderer } from "@/components/ChartRenderer";
import { InsightPanel } from "@/components/InsightPanel";
import { ResultTable } from "@/components/ResultTable";
import { SecurityStateBadge } from "@/components/SecurityStateBadge";
import { SqlPreviewPanel } from "@/components/SqlPreviewPanel";
import { api, getStoredUser } from "@/lib/api";
import type { QueryPipelineResponse, UserProfile, ValidationReport } from "@/lib/types";

export default function QueryWorkspacePage() {
  const [user, setUser] = useState<UserProfile | null>(null);
  const [question, setQuestion] = useState("");
  const [examples, setExamples] = useState<string[]>([]);
  const [clarifyMode, setClarifyMode] = useState(true);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [result, setResult] = useState<QueryPipelineResponse | null>(null);

  // Manual SQL inspection / sandboxed verification tab
  const [showManualSql, setShowManualSql] = useState(false);
  const [manualSql, setManualSql] = useState("");
  const [manualReport, setManualReport] = useState<ValidationReport | null>(null);

  useEffect(() => {
    setUser(getStoredUser());
    api
      .schema()
      .then((res) => setExamples(res.examples))
      .catch(() => {});
  }, []);

  async function submitQuestion(qText: string, preferredMetric?: string) {
    if (!qText.trim()) return;
    setLoading(true);
    setError(null);
    setActionMsg(null);
    try {
      const resp = await api.askQuery({
        question: qText,
        preferred_metric: preferredMetric,
        clarify_ambiguity: clarifyMode,
      });
      setResult(resp);
      if (resp.generated_sql) {
        setManualSql(resp.generated_sql);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Query execution failed");
    } finally {
      setLoading(false);
    }
  }

  async function handleValidateManualSql() {
    setError(null);
    try {
      const report = await api.validateSql(manualSql);
      setManualReport(report);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Validation failed");
    }
  }

  async function handleExecuteManualSql() {
    setLoading(true);
    setError(null);
    try {
      const resp = await api.executeValidatedSql(
        manualSql,
        question || "Manual validated SQL query"
      );
      setResult(resp);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Execution failed");
    } finally {
      setLoading(false);
    }
  }

  async function handleExportCsv() {
    if (!result) return;
    setExporting(true);
    setError(null);
    try {
      const csv = await api.exportCsv(result.query_id);
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `dta_query_${result.query_id.slice(0, 8)}.csv`;
      link.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : "CSV export denied");
    } finally {
      setExporting(false);
    }
  }

  async function handleSaveQuery() {
    if (!result || !result.generated_sql) return;
    setSaving(true);
    setActionMsg(null);
    try {
      await api.createSavedQuery({
        query_execution_id: result.query_id,
        title: result.question.slice(0, 80),
        question: result.question,
        sql_text: result.generated_sql,
        is_shared: true,
      });
      setActionMsg("Saved query to shared enterprise catalog.");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed saving query");
    } finally {
      setSaving(false);
    }
  }

  async function handlePinToDashboard() {
    if (!result || !result.chart_spec) return;
    setActionMsg(null);
    try {
      await api.createDashboard({
        title: `Analytics: ${result.question.slice(0, 48)}`,
        description: `Pinned from verified query ${result.query_id}`,
        is_shared: true,
        widgets: [
          {
            title: result.question,
            question: result.question,
            sql_text: result.generated_sql,
            chart_type: result.chart_spec.recommended_type,
            columns: result.columns,
            rows: result.rows,
            query_id: result.query_id,
          },
        ],
      });
      setActionMsg("Created dashboard widget from verified query output.");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed creating dashboard widget");
    }
  }

  return (
    <AppShell
      title="Natural Language to SQL Workspace"
      subtitle="Schema-linked generation -> deterministic AST security gate -> read-only PostgreSQL -> grounded insights"
    >
      <div className="space-y-6">
        {/* Query Input Box */}
        <section className="bg-white rounded-xl border border-slate-200 shadow-xs p-4 sm:p-5 space-y-4">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              submitQuestion(question);
            }}
            className="space-y-3"
          >
            <div className="flex items-center justify-between gap-2">
              <label
                htmlFor="nl-question-input"
                className="text-xs font-semibold uppercase tracking-wider text-slate-600"
              >
                Ask an Enterprise Analytics Question
              </label>
              <label className="inline-flex items-center gap-2 text-xs text-slate-600 cursor-pointer">
                <input
                  type="checkbox"
                  checked={clarifyMode}
                  onChange={(e) => setClarifyMode(e.target.checked)}
                  className="rounded border-slate-300"
                />
                Strict Ambiguity Disambiguation
              </label>
            </div>

            <div className="flex flex-col sm:flex-row gap-2.5">
              <input
                id="nl-question-input"
                type="text"
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
                placeholder="e.g., Show the top ten products by gross profit, or Which departments have unusual expense patterns?"
                className="flex-1 px-4 py-2.5 rounded-lg border border-slate-300 text-sm focus:outline-none focus:ring-2 focus:ring-sky-500"
              />
              <button
                type="submit"
                disabled={loading || !question.trim()}
                className="inline-flex items-center justify-center gap-2 px-5 py-2.5 rounded-lg bg-sky-600 hover:bg-sky-700 text-white text-sm font-semibold disabled:opacity-60 transition-colors cursor-pointer shrink-0"
              >
                <Send className="h-4 w-4" />
                {loading ? "Validating & Running..." : "Run Secure Query"}
              </button>
            </div>
          </form>

          {/* Schema-Grounded Example Chips */}
          {examples.length > 0 && (
            <div className="space-y-1.5">
              <div className="text-[11px] font-medium text-slate-500 flex items-center gap-1">
                <Sparkles className="h-3.5 w-3.5 text-sky-600" />
                Schema-grounded example questions from live PostgreSQL metadata:
              </div>
              <div className="flex flex-wrap gap-1.5">
                {examples.map((ex) => (
                  <button
                    key={ex}
                    type="button"
                    onClick={() => {
                      setQuestion(ex);
                      submitQuestion(ex);
                    }}
                    className="px-2.5 py-1 rounded-full text-xs bg-slate-100 hover:bg-sky-50 text-slate-700 hover:text-sky-800 border border-slate-200 hover:border-sky-200 transition-colors cursor-pointer"
                  >
                    {ex}
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="pt-2 border-t border-slate-100 flex items-center justify-between">
            <button
              type="button"
              onClick={() => setShowManualSql((v) => !v)}
              className="text-xs font-medium text-slate-600 hover:text-sky-700 cursor-pointer"
            >
              {showManualSql ? "Hide Manual SQL Security Tester" : "Open Manual SQL AST Security Tester"}
            </button>
          </div>

          {showManualSql && (
            <div className="p-3.5 rounded-lg bg-slate-50 border border-slate-200 space-y-2.5">
              <div className="text-xs font-semibold text-slate-700">
                Test Any PostgreSQL SQL Through the Deterministic AST Validator
              </div>
              <textarea
                rows={3}
                value={manualSql}
                onChange={(e) => setManualSql(e.target.value)}
                placeholder="SELECT region_code, region_name FROM analytics.regions;"
                className="w-full p-2.5 rounded-lg border border-slate-300 font-mono-tabular text-xs bg-white focus:outline-none focus:ring-2 focus:ring-sky-500"
              />
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  onClick={handleValidateManualSql}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-white border border-slate-300 text-slate-800 hover:bg-slate-100 cursor-pointer"
                >
                  <ShieldCheck className="h-3.5 w-3.5 text-sky-600" />
                  AST Validate Only
                </button>
                <button
                  type="button"
                  onClick={handleExecuteManualSql}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-900 text-white hover:bg-slate-800 cursor-pointer"
                >
                  <Play className="h-3.5 w-3.5" />
                  Validate & Execute Read-Only
                </button>
              </div>
              {manualReport && (
                <div
                  className={`p-2.5 rounded text-xs border ${
                    manualReport.is_valid
                      ? "bg-emerald-50 border-emerald-200 text-emerald-900"
                      : "bg-rose-50 border-rose-200 text-rose-900"
                  }`}
                >
                  {manualReport.is_valid
                    ? `Validation Passed. Executable SQL capped at ${manualReport.applied_row_limit} rows.`
                    : `Validation Rejected: ${manualReport.rejection_reason}`}
                </div>
              )}
            </div>
          )}
        </section>

        {error && (
          <div
            role="alert"
            className="p-4 rounded-xl bg-rose-50 border border-rose-200 text-sm text-rose-800"
          >
            {error}
          </div>
        )}

        {actionMsg && (
          <div className="p-3 rounded-xl bg-emerald-50 border border-emerald-200 text-xs font-medium text-emerald-800">
            {actionMsg}
          </div>
        )}

        {/* Loading Banner — NEVER displays premature success */}
        {loading && (
          <div className="bg-white rounded-xl border border-sky-200 p-5 flex items-center justify-between">
            <SecurityStateBadge isLoading={true} />
            <span className="text-xs text-slate-500">
              AST verification & read-only PostgreSQL execution in progress...
            </span>
          </div>
        )}

        {/* Pipeline Result */}
        {!loading && result && (
          <div className="space-y-6">
            {/* Status + Provenance Header */}
            <div className="bg-white rounded-xl border border-slate-200 p-4 flex flex-wrap items-center justify-between gap-3">
              <SecurityStateBadge
                validationStatus={result.validation_status}
                executionStatus={result.execution_status}
              />
              <div className="flex flex-wrap items-center gap-3 text-xs text-slate-600 font-mono-tabular">
                <span>
                  Planner:{" "}
                  <strong className="text-slate-900">
                    {result.provider_used} ({result.model_used})
                  </strong>
                </span>
                <span>Schema v{result.schema_version}</span>
                <span>Query ID: {result.query_id.slice(0, 8)}</span>
              </div>
            </div>

            {/* Clarification Dialog when question is ambiguous */}
            {result.clarification_needed && (
              <div className="bg-amber-50 border border-amber-200 rounded-xl p-5 space-y-3">
                <div className="flex items-start gap-2.5">
                  <HelpCircle className="h-5 w-5 text-amber-700 shrink-0 mt-0.5" />
                  <div>
                    <h2 className="text-sm font-semibold text-amber-950">
                      Business Definition Clarification Required
                    </h2>
                    <p className="text-sm text-amber-900 mt-1">
                      {result.clarification_needed}
                    </p>
                  </div>
                </div>
                <div className="flex flex-wrap gap-2 pl-7">
                  {result.clarification_options.map((opt) => (
                    <button
                      key={opt}
                      type="button"
                      onClick={() => {
                        const metricHint = opt.toLowerCase().includes("gross")
                          ? "gross_revenue"
                          : opt.toLowerCase().includes("net")
                          ? "net_revenue"
                          : undefined;
                        if (opt.toLowerCase().startsWith("compare ")) {
                          setQuestion(opt);
                          submitQuestion(opt, metricHint);
                        } else {
                          submitQuestion(question, metricHint || opt);
                        }
                      }}
                      className="px-3 py-1.5 rounded-lg bg-white border border-amber-300 text-xs font-medium text-amber-950 hover:bg-amber-100 cursor-pointer"
                    >
                      {opt}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {/* Cannot Compute Limitation Panel */}
            {result.cannot_compute_reason && (
              <div className="bg-amber-50 border border-amber-200 rounded-xl p-5 flex items-start gap-3">
                <AlertTriangle className="h-5 w-5 text-amber-700 shrink-0 mt-0.5" />
                <div>
                  <h2 className="text-sm font-semibold text-amber-950">
                    Requested Metric Unavailable in Verified Schema
                  </h2>
                  <p className="text-sm text-amber-900 mt-1">
                    {result.cannot_compute_reason}
                  </p>
                </div>
              </div>
            )}

            {/* Linked Schema Tables */}
            {result.linked_tables.length > 0 && (
              <div className="bg-white rounded-xl border border-slate-200 p-4 space-y-2">
                <div className="flex items-center gap-2 text-xs font-semibold text-slate-800">
                  <Database className="h-4 w-4 text-sky-600" />
                  Schema Linking Evidence ({result.linked_tables.length} ranked table(s))
                </div>
                <div className="flex flex-wrap gap-2">
                  {result.linked_tables.map((t) => (
                    <span
                      key={`${t.schema}.${t.table}`}
                      title={t.reason}
                      className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-slate-100 text-xs text-slate-800 border border-slate-200 font-mono-tabular"
                    >
                      {t.schema}.{t.table}
                      <span className="text-[10px] text-slate-500">({t.score})</span>
                    </span>
                  ))}
                </div>
              </div>
            )}

            {/* SQL Preview & AST Validation */}
            {(result.generated_sql || result.error_message) && (
              <SqlPreviewPanel
                generatedSql={result.generated_sql}
                validatedSql={result.validated_sql}
                explanation={result.explanation}
                assumptions={result.assumptions}
                appliedFilters={result.applied_filters}
                validationReport={result.validation_report}
                correctionAttempts={result.correction_attempts}
                errorMessage={result.error_message}
              />
            )}

            {/* Verified Insights */}
            {result.insight_report && (
              <InsightPanel
                insight={result.insight_report}
                onSaveQuery={handleSaveQuery}
                onAddToDashboard={handlePinToDashboard}
                onFollowUp={(nextQ) => {
                  setQuestion(nextQ);
                  submitQuestion(nextQ, "net_revenue");
                }}
                saving={saving}
              />
            )}

            {/* Chart Visualization */}
            {result.chart_spec && (
              <ChartRenderer
                chartSpec={result.chart_spec}
                columns={result.columns}
                rows={result.rows}
              />
            )}

            {/* Result Table */}
            {result.result_summary && (
              <ResultTable
                columns={result.columns}
                rows={result.rows}
                summary={result.result_summary}
                allowExport={Boolean(user?.policy.allow_export)}
                onExportCsv={handleExportCsv}
                exporting={exporting}
              />
            )}
          </div>
        )}
      </div>
    </AppShell>
  );
}
