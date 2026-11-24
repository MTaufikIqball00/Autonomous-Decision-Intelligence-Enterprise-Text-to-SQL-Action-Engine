# Enterprise Tailwind CSS UI Audit & Design System Specification

## 1. Pre-Implementation Audit Summary

- **Existing Workspace**: Clean standalone project; no pre-existing frontend framework, stylesheet, or conflicting component libraries present.
- **Selected UI Stack**: Next.js 16 (App Router, TypeScript), Tailwind CSS v4 (`@import "tailwindcss";` with CSS-first design tokens), `lucide-react` iconography, `recharts` data visualization, and `vitest` + `@testing-library/react` for automated UI tests.
- **Enterprise UX Objectives**:
  - High-trust analytical precision over decorative clutter.
  - Explicit visual truth for security & execution states (never display a "Completed" or "Verified" state prior to backend confirmation).
  - Transparent AI provenance (clearly indicate `local_deterministic` offline planner vs external `openai` / `gemini` LLM providers, schema catalog version, AST validation checks, and source `query_id`).
  - Zero exposure of database credentials, secret keys, or internal connection strings.

## 2. Design System Tokens

### Typography
- **UI Sans**: `Inter`, system-ui, sans-serif.
- **Technical / SQL / Metric Monospace**: `JetBrains Mono`, ui-monospace, monospace with `tabular-nums` on all numeric table columns, durations, row counts, and KPI metrics so decimal columns align vertically.

### Color System (High-Contrast Enterprise Slate + Semantic Signals)
- **Surfaces**:
  - App Canvas: `#F8FAFC` (`bg-slate-50`)
  - Elevated Card / Panel: `#FFFFFF` (`bg-white`) with `1px solid #E2E8F0` (`border-slate-200`)
  - Primary Navigation Rail: `#0F172A` (`bg-slate-900`) with `#E2E8F0` text and `#38BDF8` active accent
  - Code / SQL Surface: `#0B1220` dark slate monospace pane for high legibility
- **Semantic Status Palette** (WCAG AA contrast compliant):
  - **Validation Passed / Read-Only Safe / Completed**: Emerald (`bg-emerald-50 text-emerald-800 border-emerald-200`)
  - **Validation Pending / Running**: Sky (`bg-sky-50 text-sky-800 border-sky-200`)
  - **Empty Result / Clarification Needed / Truncated**: Amber (`bg-amber-50 text-amber-800 border-amber-200`)
  - **Validation Rejected / Authorization Denied / Execution Failed**: Rose (`bg-rose-50 text-rose-800 border-rose-200`)

## 3. Navigation & Screen Coverage

| Route | Screen | Core Capabilities |
|---|---|---|
| `/` | Executive Analytics Overview | Live source DB read-only health, active provider badge, schema version, real execution/rejection counts, recent queries |
| `/login` | Enterprise Authentication | Email/password login + local demo role selector buttons (Administrator, Analyst, Business User, Auditor) |
| `/query` | NL-to-SQL & Analytics Workspace | NL prompt, schema-grounded examples, clarification disambiguation card, security state pipeline, SQL preview + AST checks, self-correction trace, Result Table, Recharts builder, Grounded Insights, Data-to-Action save/dashboard/export |
| `/schema` | Schema Explorer & Relationships | Searchable tables/views, PK/FK/index badges, descriptions, schema refresh (RBAC-gated), manual relationship editor |
| `/semantics` | Semantic Catalog | Versioned business metric definitions, formulas, dimensions, filters, currency/timezone conventions |
| `/history` | Query History & Saved Queries | Auditable query ledger + saved/shared query library with one-click re-open |
| `/dashboards` | Dashboard Builder | Compose multi-widget analytical dashboards backed by saved query outputs |
| `/admin` | Governance & Operations | Encrypted DB Connections (`test connection`), LLM Provider management, User & RBAC Policies, Security Audit Logs |

## 4. Accessibility, Responsive Layout & States Audit

- **Responsive Layout**: Collapsible mobile drawer navigation (`lg:pl-64`) with clean single-column fallback below `768px` and split grid (`xl:grid-cols-12`) on desktop workstations.
- **Accessibility**: Semantic `<nav>`, `<main>`, `<header>`, `<button>` labels, `aria-live="polite"` on query execution status updates, `aria-label` on icon-only controls, visible `focus-visible:ring-2 focus-visible:ring-sky-500` outlines, and non-color-only status indicators (icon + text label).
- **Empty, Loading, Error & Truncated States**:
  - Loading: Explicit `"Executing against read-only PostgreSQL..."` status banner (never premature success).
  - Empty Result (`0` rows): Distinct amber informational state explaining filters matched 0 rows (not treated as an error).
  - Truncated Result: Explicit truncation warning badge + reason banner (`row_limit` or `max_result_bytes`).
  - Rejected / Denied: Rose security rejection panel showing safe deterministic AST or RBAC policy reason without leaking secrets.
