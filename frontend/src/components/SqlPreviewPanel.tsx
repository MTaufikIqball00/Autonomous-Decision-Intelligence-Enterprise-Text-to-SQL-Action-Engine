"use client";

import React from "react";
import { CheckCircle2, ShieldAlert, ShieldCheck, XCircle, Wrench } from "lucide-react";
import type { CorrectionAttemptRecord, ValidationReport } from "@/lib/types";

export function SqlPreviewPanel({
  generatedSql,
  validatedSql,
  explanation,
  assumptions,
  appliedFilters,
  validationReport,
  correctionAttempts,
  errorMessage,
}: {
  generatedSql: string;
  validatedSql: string;
  explanation: string;
  assumptions: string[];
  appliedFilters: string[];
  validationReport: ValidationReport | null;
  correctionAttempts: CorrectionAttemptRecord[];
  errorMessage: string | null;
}) {
  return (
    <div className="bg-white rounded-xl border border-slate-200 shadow-xs overflow-hidden">
      <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-4 w-4 text-sky-600" />
          <h2 className="text-sm font-semibold text-slate-900">
            SQL Preview & Deterministic AST Validation
          </h2>
        </div>
        {validationReport && (
          <span className="text-xs font-mono-tabular text-slate-500">
            Row cap: {validationReport.applied_row_limit}
          </span>
        )}
      </div>

      <div className="p-4 space-y-4">
        {explanation && (
          <p className="text-sm text-slate-700 leading-relaxed">{explanation}</p>
        )}

        {errorMessage && (
          <div className="p-3 rounded-lg bg-rose-50 border border-rose-200 flex items-start gap-2.5 text-sm text-rose-800">
            <ShieldAlert className="h-4 w-4 mt-0.5 shrink-0" />
            <div>
              <div className="font-semibold">Security / Execution Notice</div>
              <div className="text-xs mt-0.5">{errorMessage}</div>
            </div>
          </div>
        )}

        {generatedSql && (
          <div>
            <div className="text-[11px] uppercase tracking-wider font-semibold text-slate-500 mb-1.5">
              Generated PostgreSQL Query
            </div>
            <pre className="p-3.5 rounded-lg bg-slate-950 text-sky-100 text-xs font-mono-tabular overflow-x-auto leading-relaxed">
              <code>{generatedSql}</code>
            </pre>
          </div>
        )}

        {validatedSql && validatedSql !== generatedSql && (
          <div>
            <div className="text-[11px] uppercase tracking-wider font-semibold text-slate-500 mb-1.5">
              Sandboxed Executable SQL (Wrapped with Mandatory Row Cap)
            </div>
            <pre className="p-3 rounded-lg bg-slate-900 text-emerald-200 text-xs font-mono-tabular overflow-x-auto leading-relaxed">
              <code>{validatedSql}</code>
            </pre>
          </div>
        )}

        {validationReport && (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3 pt-1">
            <div className="rounded-lg border border-slate-200 p-3 space-y-2">
              <div className="text-xs font-semibold text-slate-800">
                AST Security Checks
              </div>
              <ul className="space-y-1.5">
                {validationReport.checks.map((c, idx) => (
                  <li key={idx} className="flex items-start gap-2 text-xs">
                    {c.passed ? (
                      <CheckCircle2 className="h-3.5 w-3.5 text-emerald-600 mt-0.5 shrink-0" />
                    ) : (
                      <XCircle className="h-3.5 w-3.5 text-rose-600 mt-0.5 shrink-0" />
                    )}
                    <div>
                      <span className="font-mono-tabular font-medium text-slate-800">
                        {c.check_name}:
                      </span>{" "}
                      <span className="text-slate-600">{c.detail}</span>
                    </div>
                  </li>
                ))}
              </ul>
            </div>

            <div className="rounded-lg border border-slate-200 p-3 space-y-2 text-xs">
              <div className="font-semibold text-slate-800">
                Verified References & Filters
              </div>
              <div>
                <span className="text-slate-500">Tables:</span>{" "}
                <span className="font-mono-tabular text-slate-800">
                  {validationReport.referenced_tables.join(", ") || "None"}
                </span>
              </div>
              <div>
                <span className="text-slate-500">Functions:</span>{" "}
                <span className="font-mono-tabular text-slate-800">
                  {validationReport.referenced_functions.join(", ") || "None"}
                </span>
              </div>
              {appliedFilters.length > 0 && (
                <div>
                  <span className="text-slate-500">Applied Filters:</span>
                  <ul className="list-disc list-inside text-slate-700 mt-1">
                    {appliedFilters.map((f, i) => (
                      <li key={i} className="font-mono-tabular">
                        {f}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {assumptions.length > 0 && (
                <div>
                  <span className="text-slate-500">Explicit Assumptions:</span>
                  <ul className="list-disc list-inside text-slate-700 mt-1">
                    {assumptions.map((a, i) => (
                      <li key={i}>{a}</li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          </div>
        )}

        {correctionAttempts.length > 0 && (
          <div className="rounded-lg border border-amber-200 bg-amber-50/70 p-3 space-y-2">
            <div className="flex items-center gap-1.5 text-xs font-semibold text-amber-900">
              <Wrench className="h-3.5 w-3.5" />
              Self-Correction Trace ({correctionAttempts.length} attempt(s), all re-validated via AST)
            </div>
            {correctionAttempts.map((att) => (
              <div key={att.attempt} className="text-xs text-amber-900 border-t border-amber-200/80 pt-1.5">
                <span className="font-semibold">Attempt #{att.attempt}</span> ({att.error_class}):{" "}
                {att.sanitized_error} →{" "}
                <span className="font-mono-tabular font-semibold">{att.outcome}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
