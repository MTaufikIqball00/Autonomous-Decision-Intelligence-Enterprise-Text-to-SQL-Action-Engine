"use client";

import React, { useEffect, useState } from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import {
  Activity,
  BookOpen,
  Database,
  History,
  LayoutDashboard,
  LogOut,
  Menu,
  MessageSquareText,
  ShieldCheck,
  ShieldAlert,
  X,
} from "lucide-react";
import { api, clearAuthSession, getStoredUser } from "@/lib/api";
import type { UserProfile } from "@/lib/types";

const NAV_ITEMS = [
  { href: "/", label: "Executive Overview", icon: Activity },
  { href: "/query", label: "NL-to-SQL Workspace", icon: MessageSquareText },
  { href: "/schema", label: "Schema Explorer", icon: Database },
  { href: "/semantics", label: "Semantic Catalog", icon: BookOpen },
  { href: "/history", label: "History & Saved", icon: History },
  { href: "/dashboards", label: "Dashboard Builder", icon: LayoutDashboard },
  { href: "/actions", label: "Action Center", icon: ShieldAlert },
  { href: "/admin", label: "Governance & RBAC", icon: ShieldCheck },
];

export function AppShell({
  children,
  title,
  subtitle,
}: {
  children: React.ReactNode;
  title: string;
  subtitle?: string;
}) {
  const pathname = usePathname();
  const router = useRouter();
  const [user, setUser] = useState<UserProfile | null>(null);
  const [mobileOpen, setMobileOpen] = useState(false);

  useEffect(() => {
    const stored = getStoredUser();
    if (!stored && pathname !== "/login") {
      router.replace("/login");
      return;
    }
    setUser(stored);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathname]);

  function handleLogout() {
    void api.logout().catch(() => {});
    clearAuthSession();
    router.push("/login");
  }

  return (
    <div className="min-h-screen flex bg-slate-50 text-slate-900">
      {/* Desktop Left Sidebar */}
      <aside className="hidden lg:flex lg:w-64 lg:flex-col lg:fixed lg:inset-y-0 bg-slate-900 text-slate-100 border-r border-slate-800">
        <div className="px-5 py-5 border-b border-slate-800 flex items-center gap-3">
          <div className="h-9 w-9 rounded-lg bg-sky-500/20 border border-sky-400/30 flex items-center justify-center text-sky-400">
            <ShieldCheck className="h-5 w-5" />
          </div>
          <div>
            <div className="text-sm font-semibold tracking-tight text-white">
              DATA-TO-ACTION
            </div>
            <div className="text-[11px] text-slate-400">
              Enterprise Text-to-SQL
            </div>
          </div>
        </div>

        <nav aria-label="Primary Navigation" className="flex-1 px-3 py-4 space-y-1 overflow-y-auto">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            const active = pathname === item.href;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400 ${
                  active
                    ? "bg-sky-500/15 text-sky-300 border border-sky-500/30"
                    : "text-slate-300 hover:bg-slate-800 hover:text-white"
                }`}
              >
                <Icon className="h-4 w-4 shrink-0" />
                <span>{item.label}</span>
              </Link>
            );
          })}
        </nav>

        <div className="p-4 border-t border-slate-800 space-y-3">
          <div className="rounded-lg bg-slate-800/70 p-3 text-xs space-y-1">
            <div className="flex items-center justify-between">
              <span className="text-slate-400">Execution Mode</span>
              <span className="inline-flex items-center gap-1 text-emerald-400 font-medium">
                <ShieldCheck className="h-3.5 w-3.5" />
                READ-ONLY
              </span>
            </div>
            {user && (
              <>
                <div className="text-slate-200 font-medium truncate pt-1">
                  {user.display_name}
                </div>
                <div className="text-slate-400 truncate">{user.email}</div>
                <div className="inline-block mt-1 px-2 py-0.5 rounded bg-slate-700 text-sky-300 uppercase tracking-wider text-[10px] font-semibold">
                  {user.role.replace("_", " ")}
                </div>
              </>
            )}
          </div>
          {user && (
            <button
              type="button"
              onClick={handleLogout}
              className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-slate-300 hover:bg-slate-800 hover:text-white border border-slate-700 transition-colors cursor-pointer"
            >
              <LogOut className="h-3.5 w-3.5" />
              Sign Out
            </button>
          )}
        </div>
      </aside>

      {/* Mobile Navigation Drawer */}
      {mobileOpen && (
        <div className="fixed inset-0 z-50 lg:hidden flex">
          <div
            className="fixed inset-0 bg-slate-900/60"
            onClick={() => setMobileOpen(false)}
          />
          <div className="relative w-64 max-w-full bg-slate-900 text-slate-100 flex flex-col h-full p-4 z-10">
            <div className="flex items-center justify-between pb-4 border-b border-slate-800">
              <span className="font-semibold text-sm">DATA-TO-ACTION</span>
              <button
                type="button"
                aria-label="Close navigation menu"
                onClick={() => setMobileOpen(false)}
                className="p-1 text-slate-300 hover:text-white"
              >
                <X className="h-5 w-5" />
              </button>
            </div>
            <nav className="mt-4 space-y-1 flex-1">
              {NAV_ITEMS.map((item) => {
                const Icon = item.icon;
                const active = pathname === item.href;
                return (
                  <Link
                    key={item.href}
                    href={item.href}
                    onClick={() => setMobileOpen(false)}
                    className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium ${
                      active ? "bg-sky-500/20 text-sky-300" : "text-slate-300"
                    }`}
                  >
                    <Icon className="h-4 w-4" />
                    <span>{item.label}</span>
                  </Link>
                );
              })}
            </nav>
            {user && (
              <button
                type="button"
                onClick={handleLogout}
                className="mt-4 w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-slate-300 border border-slate-700"
              >
                <LogOut className="h-3.5 w-3.5" />
                Sign Out
              </button>
            )}
          </div>
        </div>
      )}

      {/* Main Content Area */}
      <div className="flex-1 lg:pl-64 flex flex-col min-w-0">
        <header className="bg-white border-b border-slate-200 px-4 sm:px-6 py-4 flex items-center justify-between gap-4 sticky top-0 z-20">
          <div className="flex items-center gap-3 min-w-0">
            <button
              type="button"
              aria-label="Open navigation menu"
              onClick={() => setMobileOpen(true)}
              className="lg:hidden p-2 rounded-lg border border-slate-200 text-slate-700 hover:bg-slate-100"
            >
              <Menu className="h-5 w-5" />
            </button>
            <div className="min-w-0">
              <h1 className="text-lg sm:text-xl font-semibold text-slate-900 truncate">
                {title}
              </h1>
              {subtitle && (
                <p className="text-xs sm:text-sm text-slate-500 truncate">
                  {subtitle}
                </p>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <span className="hidden sm:inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium bg-emerald-50 text-emerald-800 border border-emerald-200">
              <ShieldCheck className="h-3.5 w-3.5" />
              AST + Read-Only PG
            </span>
            {user?.policy.region_filter && user.policy.region_filter !== "ALL" && (
              <span className="inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-xs font-medium bg-amber-50 text-amber-800 border border-amber-200">
                <ShieldAlert className="h-3.5 w-3.5" />
                RLS: {user.policy.region_filter}
              </span>
            )}
          </div>
        </header>

        <main className="flex-1 p-4 sm:p-6 max-w-7xl w-full mx-auto">
          {children}
        </main>
      </div>
    </div>
  );
}
