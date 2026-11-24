"use client";

import React from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Clock,
  HelpCircle,
  Loader2,
  ShieldAlert,
  ShieldCheck,
  XCircle,
} from "lucide-react";
import type { VerifiedExecutionStatus } from "@/lib/types";

export function SecurityStateBadge({
  validationStatus,
  executionStatus,
  isLoading,
}: {
  validationStatus?: string;
  executionStatus?: VerifiedExecutionStatus;
  isLoading?: boolean;
}) {
  if (isLoading) {
    return (
      <div
        role="status"
        aria-live="polite"
        className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-xs font-semibold bg-sky-50 text-sky-800 border border-sky-200"
      >
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        <span>Validation Pending / Execution Running</span>
      </div>
    );
  }

  if (validationStatus === "validation_rejected" || executionStatus === "validation_rejected") {
    return (
      <div className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-rose-50 text-rose-800 border border-rose-200">
        <ShieldAlert className="h-3.5 w-3.5" />
        <span>Validation Rejected</span>
      </div>
    );
  }

  if (executionStatus === "authorization_denied") {
    return (
      <div className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-rose-50 text-rose-800 border border-rose-200">
        <ShieldAlert className="h-3.5 w-3.5" />
        <span>Authorization Denied</span>
      </div>
    );
  }

  if (executionStatus === "clarification_required") {
    return (
      <div className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-amber-50 text-amber-800 border border-amber-200">
        <HelpCircle className="h-3.5 w-3.5" />
        <span>Clarification Required</span>
      </div>
    );
  }

  if (executionStatus === "cannot_compute") {
    return (
      <div className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-amber-50 text-amber-800 border border-amber-200">
        <AlertTriangle className="h-3.5 w-3.5" />
        <span>Unavailable Metric / Schema Gap</span>
      </div>
    );
  }

  if (executionStatus === "execution_timeout") {
    return (
      <div className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-rose-50 text-rose-800 border border-rose-200">
        <Clock className="h-3.5 w-3.5" />
        <span>Execution Timed Out</span>
      </div>
    );
  }

  if (executionStatus === "execution_failed" || executionStatus === "db_unavailable") {
    return (
      <div className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-rose-50 text-rose-800 border border-rose-200">
        <XCircle className="h-3.5 w-3.5" />
        <span>Execution Failed</span>
      </div>
    );
  }

  if (executionStatus === "completed_truncated") {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-emerald-50 text-emerald-800 border border-emerald-200">
          <ShieldCheck className="h-3.5 w-3.5" />
          Validation Passed
        </span>
        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-amber-50 text-amber-800 border border-amber-200">
          <AlertTriangle className="h-3.5 w-3.5" />
          Results Truncated
        </span>
      </div>
    );
  }

  if (executionStatus === "completed_empty") {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-emerald-50 text-emerald-800 border border-emerald-200">
          <ShieldCheck className="h-3.5 w-3.5" />
          Validation Passed
        </span>
        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-amber-50 text-amber-800 border border-amber-200">
          <CheckCircle2 className="h-3.5 w-3.5" />
          Execution Completed (0 Rows)
        </span>
      </div>
    );
  }

  if (executionStatus === "completed_with_rows") {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-emerald-50 text-emerald-800 border border-emerald-200">
          <ShieldCheck className="h-3.5 w-3.5" />
          Validation Passed
        </span>
        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-emerald-50 text-emerald-800 border border-emerald-200">
          <CheckCircle2 className="h-3.5 w-3.5" />
          Execution Completed
        </span>
      </div>
    );
  }

  return (
    <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium bg-slate-100 text-slate-700 border border-slate-200">
      SQL Ready
    </span>
  );
}
