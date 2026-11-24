"use client";

import React, { useState } from "react";
import { useRouter } from "next/navigation";
import { Lock, ShieldCheck } from "lucide-react";
import { api, setAuthSession } from "@/lib/api";

const DEMO_ACCOUNTS = [
  {
    label: "Administrator",
    email: "admin@enterprise.local",
    pw: "AdminPass!2026",
    scope: "All schemas, connections, providers, RBAC, audit logs, CSV export",
  },
  {
    label: "Analyst",
    email: "analyst@enterprise.local",
    pw: "AnalystPass!2026",
    scope: "analytics + finance (tax_id masked), CSV export permitted",
  },
  {
    label: "Business User",
    email: "business@enterprise.local",
    pw: "BusinessPass!2026",
    scope: "analytics only (tax_id masked), CSV export denied, 250 row cap",
  },
  {
    label: "Security Auditor",
    email: "auditor@enterprise.local",
    pw: "AuditorPass!2026",
    scope: "analytics + finance + audit logs & RBAC inspection (no admin mutation)",
  },
];

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("analyst@enterprise.local");
  const [password, setPassword] = useState("AnalystPass!2026");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      const resp = await api.login(email, password);
      setAuthSession(resp);
      router.push("/query");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Authentication failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen bg-slate-950 flex items-center justify-center p-4">
      <div className="max-w-4xl w-full grid grid-cols-1 lg:grid-cols-2 bg-white rounded-2xl shadow-xl overflow-hidden border border-slate-200">
        <div className="bg-slate-900 text-slate-100 p-8 flex flex-col justify-between">
          <div className="space-y-4">
            <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-sky-500/15 border border-sky-400/30 text-sky-300 text-xs font-medium">
              <ShieldCheck className="h-4 w-4" />
              DATA-TO-ACTION ENTERPRISE
            </div>
            <h1 className="text-2xl font-bold tracking-tight text-white">
              Secure, Read-Only, Evidence-Based Analytics
            </h1>
            <p className="text-sm text-slate-300 leading-relaxed">
              Natural language questions are linked against live PostgreSQL metadata,
              validated deterministically via Rust AST security guards, and executed under
              a least-privilege read-only database role.
            </p>
          </div>

          <div className="mt-8 space-y-2.5">
            <div className="text-xs font-semibold uppercase tracking-wider text-slate-400">
              Seeded RBAC Demo Identities (Local Dev Environment)
            </div>
            <div className="space-y-2">
              {DEMO_ACCOUNTS.map((acc) => (
                <button
                  key={acc.email}
                  type="button"
                  onClick={() => {
                    setEmail(acc.email);
                    setPassword(acc.pw);
                  }}
                  className="w-full text-left p-2.5 rounded-lg bg-slate-800/80 hover:bg-slate-800 border border-slate-700 transition-colors cursor-pointer"
                >
                  <div className="flex items-center justify-between text-xs font-semibold text-white">
                    <span>{acc.label}</span>
                    <span className="font-mono-tabular text-sky-300">{acc.email}</span>
                  </div>
                  <div className="text-[11px] text-slate-400 mt-0.5">{acc.scope}</div>
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="p-8 flex flex-col justify-center">
          <div className="flex items-center gap-2 text-slate-900 mb-6">
            <Lock className="h-5 w-5 text-sky-600" />
            <h2 className="text-xl font-semibold">Enterprise Sign In</h2>
          </div>

          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label
                htmlFor="email"
                className="block text-xs font-semibold text-slate-700 mb-1"
              >
                Work Email
              </label>
              <input
                id="email"
                type="email"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className="w-full px-3.5 py-2.5 rounded-lg border border-slate-300 text-sm focus:outline-none focus:ring-2 focus:ring-sky-500"
              />
            </div>

            <div>
              <label
                htmlFor="password"
                className="block text-xs font-semibold text-slate-700 mb-1"
              >
                Password
              </label>
              <input
                id="password"
                type="password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full px-3.5 py-2.5 rounded-lg border border-slate-300 text-sm focus:outline-none focus:ring-2 focus:ring-sky-500"
              />
            </div>

            {error && (
              <div
                role="alert"
                className="p-3 rounded-lg bg-rose-50 border border-rose-200 text-xs text-rose-800"
              >
                {error}
              </div>
            )}

            <button
              type="submit"
              disabled={loading}
              className="w-full py-2.5 px-4 rounded-lg bg-sky-600 hover:bg-sky-700 text-white text-sm font-semibold transition-colors disabled:opacity-60 cursor-pointer"
            >
              {loading ? "Verifying Credentials..." : "Sign In to Workspace"}
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
