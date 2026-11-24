# Architecture & Technical Decision Record

## 1. Environment Inspection Summary

Inspected on Windows 11 (`Win32NT 10.0.26300.0`, Windows PowerShell `5.1.26100.7877`):
- **Verified Installed**: `rustc 1.99.1`, `cargo 1.99.1`, `rustup 1.29.3`, `node v24.15.0`, `npm 12.0.1`, `npx 12.0.1`, `python 3.10.0`, `py 3.11.9`, `git 2.54.0.windows.1`.
- **Verified Missing**: `psql`, `pg_ctl`, `initdb`, `postgres`, `docker`, `redis-server`, `redis-cli`.
- **Occupied Ports Observed**: `135, 139, 445, 1001, 5000 (python), 5040, 7680, 8000 (svchost), 9090 (svchost), 22346, 24830, 27015, 42050, 49664-49676, 54973-54975, 65344-65361`.

## 2. Isolated Port Allocation

| Service | Host | Port | Exposure | Env Var | Health Endpoint |
|---|---|---|---|---|---|
| PostgreSQL 17.12.0 (Project-Managed) | `127.0.0.1` | `5439` | Loopback internal | `DTA_PG_PORT` | TCP / `pg_isready` |
| Rust Axum API Backend | `127.0.0.1` | `8420` | Loopback local | `DTA_BACKEND_PORT` | `GET /api/v1/health` |
| Next.js Tailwind Frontend | `127.0.0.1` | `3420` | Loopback local | `DTA_FRONTEND_PORT` | `GET /` |

## 3. Core Technology Decisions

1. **Primary Backend — Rust (`axum`, `tokio`, `sqlx`, `sqlparser`)**:
   - Chosen per architecture requirements for memory safety, strict typing, async concurrency, and deterministic AST validation without runtime GC pauses.
2. **No Python / FastAPI Service**:
   - Evaluated whether Python (`SQLGlot` / `LlamaIndex` / `FastAPI`) provided material advantage. `sqlparser` with `PostgreSqlDialect` executes deterministically in-process inside the Rust backend, eliminating inter-process latency, JSON serialization mismatches, extra attack surface, and an unrequested FastAPI service.
3. **No Redis**:
   - Not installed on host and not justified for single-node project scope; in-memory `tokio::sync::RwLock` structures backed by `data_to_action_app` PostgreSQL tables handle schema catalog snapshots, RBAC lookup, and audit persistence cleanly.
4. **Project-Managed Portable PostgreSQL 17.12.0**:
   - Because PostgreSQL was absent locally, `scripts/setup-postgres.ps1` downloads the official signed `theseus-rs/postgresql-binaries` `17.12.0` x86_64 Windows archive, verifies its SHA-256 checksum, initializes `.local/pgdata` with `scram-sha-256`, and runs strictly on `127.0.0.1:5439`.
5. **Strict Database Separation**:
   - `data_to_action_analytics`: Source analytics warehouse queried **exclusively** through `dta_readonly`.
   - `data_to_action_app`: Application metadata database (`users`, `rbac_policies`, `db_connections`, `schema_snapshots`, `manual_relationships`, `semantic_metrics`, `query_executions`, `saved_queries`, `dashboards`, `llm_providers`, `audit_logs`) accessed through `dta_app_user`. Neither role has cross-database privileges.
6. **Provider-Independent LLM Abstraction + Transparent `local_deterministic` Planner**:
   - Supports OpenAI (`/v1/chat/completions`) and Google Gemini (`generateContent`) with operator-configured model names (never hardcoded).
   - Because neither `OPENAI_API_KEY` nor `GEMINI_API_KEY` is configured in the host environment, a `local_deterministic` schema-and-semantic-grounded planner is included and explicitly labeled as `local_deterministic` in responses and UI so full end-to-end testing runs deterministically offline without fabricating external LLM responses.
7. **Database Adapter Extensibility**:
   - `DatabaseAdapter` trait defines `health_check`, `discover_schema`, and `execute_readonly_query`. `PostgresAdapter` is fully implemented; `BigQueryAdapterStub` exists explicitly as `AdapterUnavailable` for future extension without untested stubbing.
