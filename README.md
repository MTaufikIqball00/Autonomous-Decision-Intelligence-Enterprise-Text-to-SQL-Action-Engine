# DATA-TO-ACTION — Enterprise Text-to-SQL & Analytics Agent

> **English** | [Bahasa Indonesia (README.id.md)](README.id.md) | [Apache-2.0 License](LICENSE)

**DATA-TO-ACTION** is a standalone, security-first enterprise analytics platform built with a **Rust (`axum`, `tokio`, `sqlx`, `sqlparser`)** backend, **PostgreSQL 17.12.0** (strictly separated application metadata and read-only analytics databases), and a **Next.js + Tailwind CSS v4** analytical workspace.

## Deskripsi Singkat (Bahasa Indonesia)

**DATA-TO-ACTION** adalah platform analitik enterprise Text-to-SQL yang aman secara desain, bersifat **read-only**, dapat diaudit, dan berbasis bukti nyata. Pengguna bisnis dapat mengajukan pertanyaan dalam bahasa natural, melakukan penautan skema otomatis berbasis metadata PostgreSQL aktual, memvalidasi keamanan SQL secara deterministik melalui AST (`sqlparser`), mengeksekusi kueri pada peran read-only, serta menampilkan tabel, grafik interaktif, dan wawasan bisnis yang terverifikasi.

> **Suggested GitHub Repository Description:**
> `Enterprise Text-to-SQL & Data-to-Action analytics platform — secure-by-design Rust (Axum/SQLx/sqlparser AST), read-only PostgreSQL with RLS, schema linking, semantic catalog, and Next.js + Tailwind CSS v4.`

Dokumentasi lengkap berbahasa Indonesia tersedia di [README.id.md](README.id.md).

## Core Security & Architectural Guarantees

1. **Zero Unvalidated SQL Execution**: Every generated or manual SQL query passes through a deterministic PostgreSQL AST validator (`sqlparser` `PostgreSqlDialect`) before reaching PostgreSQL.
2. **Defense-in-Depth Read-Only Enforcement**:
   - **AST Gate**: Allows only a single `Statement::Query` (`SELECT` / `WITH` / set ops). Rejects DML/DDL/DCL/admin/transaction statements, comments (`--`, `/*`), multi-statements, `SELECT ... INTO`, locking clauses, `CROSS JOIN`, unbounded `WITH RECURSIVE`, unlisted/dangerous functions (`pg_read_file`, `pg_sleep`, `dblink`, `set_config`, etc.), unauthorized schemas/tables/columns, and RBAC-denied columns (including `SELECT *` leaks).
   - **Mandatory Row & Byte Caps**: Validated SQL is wrapped in `SELECT * FROM (<sql>) AS _dta_safe_result LIMIT <effective_limit + 1>`, paired with byte-size truncation tracking.
   - **PostgreSQL Read-Only Boundary**: Queries execute exclusively as role `dta_readonly` (`NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `default_transaction_read_only = on`) inside `BEGIN` -> `SET TRANSACTION READ ONLY` -> `SET LOCAL statement_timeout` -> `SET LOCAL app.current_tenant_region` -> `ROLLBACK`.
   - **PostgreSQL Row-Level Security (RLS)** on `analytics.orders` and zero grants on `restricted_hr` and `data_to_action_app` for `dta_readonly`.
3. **Database Separation**:
   - `data_to_action_analytics`: Source analytics warehouse (`analytics`, `finance`, plus restricted `restricted_hr`).
   - `data_to_action_app`: Application metadata (`users`, `rbac_policies`, encrypted `db_connections`, `schema_snapshots`, `manual_relationships`, versioned `semantic_metrics`, `query_executions`, `saved_queries`, `dashboards`, encrypted `llm_providers`, `audit_logs`).
4. **Evidence-Based Grounding**: Charts and business insights preserve `query_id`, source columns, aggregation methods, filters, time range, units, and verify numeric claims against executed rows.
5. **AI & Model Transparency**: Supports external `openai` and `gemini` HTTP adapters (configurable model names via environment/UI) and a transparent `local_deterministic` schema-and-semantic-grounded planner for verified offline execution.

## Isolated Port Allocation

| Service | Host | Port | Exposure | Config Env Var |
|---|---|---|---|---|
| PostgreSQL 17.12.0 (Project-Managed) | `127.0.0.1` | `5439` | Loopback internal | `DTA_PG_PORT` |
| Rust Axum Backend | `127.0.0.1` | `8420` | Loopback local | `DTA_BACKEND_PORT` |
| Next.js Tailwind Frontend | `127.0.0.1` | `3420` | Loopback local | `DTA_FRONTEND_PORT` |

## Quick Start (Windows PowerShell 5.1)

```powershell
# 1. Provision portable PostgreSQL 17.12.0, verify SHA-256, create .env, roles & seed data
powershell -ExecutionPolicy Bypass -File scripts/setup-postgres.ps1

# 2. Start PostgreSQL, Rust Backend, and Next.js Frontend
powershell -ExecutionPolicy Bypass -File scripts/start.ps1

# 3. Inspect live service status & URLs
powershell -ExecutionPolicy Bypass -File scripts/status.ps1

# 4. Stop all services cleanly
powershell -ExecutionPolicy Bypass -File scripts/stop.ps1
```

## Demo Local RBAC Accounts

Seeded in `data_to_action_app` for local evaluation (see `/login`):
- `admin@enterprise.local` / `AdminPass!2026` (`administrator`)
- `analyst@enterprise.local` / `AnalystPass!2026` (`analyst` — `tax_id` masked/denied, CSV export allowed)
- `business@enterprise.local` / `BusinessPass!2026` (`business_user` — `analytics` only, CSV export denied)
- `auditor@enterprise.local` / `AuditorPass!2026` (`auditor` — read-only audit & RBAC inspection)

## Automated Testing

```powershell
# Rust Security, Read-Only DB, Schema, Pipeline & RBAC Suites
cd backend
cargo test -- --test-threads=1

# Frontend Vitest & Production Build
cd ../frontend
npm test
npx tsc --noEmit
npm run build
```

## Implemented vs Unavailable Capabilities

- **Implemented & Verified**: Portable PostgreSQL 17.12.0 provisioning; `PostgresAdapter` read-only execution + RLS + timeout + truncation; schema discovery & versioning; semantic catalog versioning; `sqlparser` AST security validator; `local_deterministic` planner; self-correction with mandatory AST revalidation; OpenAI/Gemini HTTP adapters (tested against mock HTTP servers); AES-256-GCM connection/API-key encryption; Argon2id + JWT RBAC; Next.js Tailwind CSS v4 workspace; PowerShell lifecycle scripts.
- **Explicitly Unimplemented Stub**: `BigQueryAdapterStub` (returns `501 AdapterUnavailable` until real BigQuery credentials/integration are added and tested).
- **Omitted by Design**: Python/FastAPI worker (Rust `sqlparser` handles AST validation deterministically in-process) and Redis (single-node PostgreSQL + Tokio state suffices).
- **Requires Operator Keys**: Live vendor API calls to OpenAI or Google Gemini require setting `OPENAI_API_KEY` + `OPENAI_MODEL` or `GEMINI_API_KEY` + `GEMINI_MODEL` in `.env` or Admin UI.

## License

Licensed under the **Apache License, Version 2.0** — see [LICENSE](LICENSE) for the full text.
