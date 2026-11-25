"use client";

import React from "react";
import { BookmarkPlus, CheckCircle2, Info, LayoutGrid, Lightbulb, MessageSquarePlus, ShieldAlert } from "lucide-react";
import type { InsightReport } from "@/lib/types";

export function InsightPanel({
  insight,
  onSaveQuery,
  onAddToDashboard,
  onProposeAction,
  onFollowUp,
  saving,
}: {
  insight: InsightReport;
  onSaveQuery?: () => void;
  onAddToDashboard?: () => void;
  onProposeAction?: () => void;
  onFollowUp?: (prompt: string) => void;
  saving?: boolean;
}) {
  return (
    <div className="bg-white rounded-xl border border-slate-200 shadow-xs overflow-hidden">
      <div className="px-4 py-3 bg-slate-50 border-b border-slate-200 flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Lightbulb className="h-4 w-4 text-amber-500" />
          <h2 className="text-sm font-semibold text-slate-900">
            Evidence-Based Business Insights
          </h2>
          <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium bg-emerald-50 text-emerald-800 border border-emerald-200">
            <CheckCircle2 className="h-3 w-3" />
            Grounded to {insight.source_query_id.slice(0, 8)}
          </span>
        </div>

        <div className="flex flex-wrap gap-2">
          {onSaveQuery && (
            <button
              type="button"
              disabled={saving}
              onClick={onSaveQuery}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium bg-white border border-slate-300 text-slate-700 hover:bg-slate-100 cursor-pointer"
            >
              <BookmarkPlus className="h-3.5 w-3.5" />
              Save Query
            </button>
          )}
          {onAddToDashboard && (
            <button
              type="button"
              onClick={onAddToDashboard}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium bg-sky-600 text-white hover:bg-sky-700 cursor-pointer"
            >
              <LayoutGrid className="h-3.5 w-3.5" />
              Pin to Dashboard
            </button>
          )}
          {onProposeAction && (
            <button
              type="button"
              onClick={onProposeAction}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium bg-amber-600 text-white hover:bg-amber-700 cursor-pointer"
            >
              <ShieldAlert className="h-3.5 w-3.5" />
              Propose Action
            </button>
          )}
          {onFollowUp && (
            <button
              type="button"
              onClick={() => onFollowUp("Compare net revenue across the two most recent quarters.")}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium bg-slate-100 text-slate-700 hover:bg-slate-200 cursor-pointer"
            >
              <MessageSquarePlus className="h-3.5 w-3.5" />
              Follow-Up Period Comparison
            </button>
          )}
        </div>
      </div>

      <div className="p-4 grid grid-cols-1 lg:grid-cols-3 gap-4 text-xs">
        <div className="space-y-2">
          <div className="font-semibold text-slate-800 uppercase tracking-wider text-[11px]">
            Verified Observations
          </div>
          <ul className="space-y-1.5 text-slate-700">
            {insight.observations.map((obs, i) => (
              <li key={i} className="p-2.5 rounded-lg bg-slate-50 border border-slate-100">
                {obs}
              </li>
            ))}
          </ul>
        </div>

        <div className="space-y-2">
          <div className="font-semibold text-slate-800 uppercase tracking-wider text-[11px]">
            Interpretations (Non-Causal)
          </div>
          {insight.interpretations.length === 0 ? (
            <div className="p-2.5 rounded-lg bg-slate-50 border border-slate-100 text-slate-500">
              No broader interpretation asserted for this single snapshot.
            </div>
          ) : (
            <ul className="space-y-1.5 text-slate-700">
              {insight.interpretations.map((interp, i) => (
                <li key={i} className="p-2.5 rounded-lg bg-sky-50/60 border border-sky-100">
                  {interp}
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="space-y-2">
          <div className="font-semibold text-slate-800 uppercase tracking-wider text-[11px]">
            Data Limitations & Grounding Checks
          </div>
          <ul className="space-y-1.5 text-slate-700">
            {insight.limitations.map((lim, i) => (
              <li key={i} className="p-2.5 rounded-lg bg-amber-50/70 border border-amber-100 flex gap-1.5">
                <Info className="h-3.5 w-3.5 text-amber-600 shrink-0 mt-0.5" />
                <span>{lim}</span>
              </li>
            ))}
            {insight.verification_checks.map((vc, i) => (
              <li key={`vc-${i}`} className="p-2 rounded-lg bg-emerald-50/60 border border-emerald-100 text-emerald-900">
                <span className="font-mono-tabular font-semibold">{vc.check}:</span>{" "}
                {vc.detail}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
