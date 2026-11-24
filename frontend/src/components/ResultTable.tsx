"use client";

import React from "react";
import { AlertTriangle, Download, Table2 } from "lucide-react";
import type { ColumnMeta, VerifiedResultSummary } from "@/lib/types";

export function ResultTable({
  columns,
  rows,
  summary,
  allowExport,
  onExportCsv,
  exporting,
}: {
  columns: ColumnMeta[];
  rows: unknown[][];
  summary: VerifiedResultSummary | null;
  allowExport: boolean;
  onExportCsv?: () => void;
  exporting?: boolean;
}) {
  return (
    <div className="bg-white rounded-xl border border-slate-200 shadow-xs overflow-hidden">
      <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Table2 className="h-4 w-4 text-sky-600" />
          <h2 className="text-sm font-semibold text-slate-900">
            Verified Query Result Table
          </h2>
          {summary && (
            <span className="text-xs font-mono-tabular text-slate-500">
              ({summary.row_count} row{summary.row_count === 1 ? "" : "s"} ·{" "}
              {summary.duration_ms} ms)
            </span>
          )}
        </div>

        {onExportCsv && (
          <button
            type="button"
            disabled={!allowExport || exporting || rows.length === 0}
            onClick={onExportCsv}
            title={
              allowExport
                ? "Export verified query rows as CSV"
                : "Export restricted by your RBAC role policy"
            }
            className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium border transition-colors ${
              allowExport && rows.length > 0
                ? "bg-white border-slate-300 text-slate-700 hover:bg-slate-100 cursor-pointer"
                : "bg-slate-100 border-slate-200 text-slate-400 cursor-not-allowed"
            }`}
          >
            <Download className="h-3.5 w-3.5" />
            {exporting ? "Exporting..." : allowExport ? "Export CSV" : "Export Denied (RBAC)"}
          </button>
        )}
      </div>

      {summary?.is_truncated && (
        <div className="px-4 py-2.5 bg-amber-50 border-b border-amber-200 flex items-center gap-2 text-xs text-amber-900">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          <span>
            {summary.truncation_reason ||
              "Result set truncated to configured row/byte limit."}
          </span>
        </div>
      )}

      {rows.length === 0 ? (
        <div className="p-8 text-center text-sm text-slate-500">
          Query executed cleanly and returned 0 matching rows.
        </div>
      ) : (
        <div className="overflow-x-auto max-h-[440px]">
          <table className="min-w-full divide-y divide-slate-200 text-xs">
            <thead className="bg-slate-50 sticky top-0 z-10">
              <tr>
                {columns.map((col, idx) => (
                  <th
                    key={idx}
                    scope="col"
                    className="px-3.5 py-2.5 text-left font-semibold text-slate-700 whitespace-nowrap border-b border-slate-200"
                  >
                    <div>{col.name}</div>
                    <div className="text-[10px] font-mono-tabular font-normal text-slate-400">
                      {col.type_name}
                    </div>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 bg-white">
              {rows.map((row, rIdx) => (
                <tr key={rIdx} className="hover:bg-slate-50/80">
                  {columns.map((col, cIdx) => {
                    const cell = row[cIdx];
                    const isNum = typeof cell === "number";
                    return (
                      <td
                        key={cIdx}
                        className={`px-3.5 py-2 whitespace-nowrap text-slate-800 ${
                          isNum ? "font-mono-tabular text-right" : ""
                        }`}
                      >
                        {cell === null || cell === undefined ? (
                          <span className="inline-block px-1.5 py-0.5 rounded bg-slate-100 text-slate-400 font-mono-tabular text-[10px]">
                            NULL
                          </span>
                        ) : typeof cell === "boolean" ? (
                          <span className="font-mono-tabular">
                            {cell ? "true" : "false"}
                          </span>
                        ) : typeof cell === "number" ? (
                          formatNumber(cell, col.name)
                        ) : (
                          String(cell)
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function formatNumber(n: number, colName: string): string {
  const lower = colName.toLowerCase();
  if (lower.includes("year") || lower.includes("id") || lower.includes("count")) {
    return Number.isInteger(n) ? n.toLocaleString("en-US") : n.toFixed(2);
  }
  return n.toLocaleString("en-US", { maximumFractionDigits: 2 });
}
