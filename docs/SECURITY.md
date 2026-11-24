# Security Architecture & Defense-in-Depth Specification

## 1. Non-Negotiable Security Invariants

1. **Zero Unvalidated SQL Execution**: Every generated or corrected SQL query passes through the deterministic `sqlparser` PostgreSQL AST validator before reaching PostgreSQL.
2. **AI Never Decides Permissions**: Role permissions, allowed schemas, allowed tables, denied columns, row limits, and RLS tenant filters are fetched from `data_to_action_app` via server-verified JWT `AuthUser` identity — never from client headers or LLM output.
3. **Defense-in-Depth Read-Only Boundary**:
   - **Layer 1 (AST)**: Only a single `Statement::Query` (`SELECT` / `WITH` / set ops) is accepted. Comments (`--`, `/*`), multi-statements, `SELECT INTO`, locking clauses (`FOR UPDATE`/`SHARE`), DML, DDL, DCL, transaction statements, unlisted functions, unauthorized schemas/tables/columns, and RBAC-denied columns (including `SELECT *` when denied columns exist) are rejected deterministically.
   - **Layer 2 (Row Cap Wrapping)**: Every validated query is wrapped in `SELECT * FROM (<validated>) AS _dta_safe_result LIMIT <effective_limit + 1>` so row caps and truncation detection are guaranteed.
   - **Layer 3 (PostgreSQL Role & Transaction)**: Connections use `dta_readonly` (`NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `NOINHERIT`, `NOREPLICATION`, `default_transaction_read_only = on`, `statement_timeout = '15000'`), wrapped in an explicit `BEGIN` -> `SET TRANSACTION READ ONLY` -> `SET LOCAL statement_timeout` -> `SET LOCAL app.current_tenant_region` -> `ROLLBACK` lifecycle.
   - **Layer 4 (PostgreSQL Schema Grants & Row-Level Security)**: `dta_readonly` has zero privileges on `restricted_hr` and `data_to_action_app`, and `analytics.orders` enforces PostgreSQL RLS on `app.current_tenant_region`.
4. **Credential & Secret Protection**:
   - Database passwords stored via connection management are encrypted at rest with AES-256-GCM (`DTA_MASTER_KEY`, random 12-byte nonces).
   - User passwords use Argon2id (`argon2`).
   - API error sanitizers strip URLs/credentials before returning errors or recording audit events.
   - Browser clients never receive DB credentials, encrypted blobs, or LLM API keys.
5. **Self-Correction Guardrails**:
   - Retries occur ONLY on `syntax`, `schema`, or `type` errors — never on `permission` or `security_validation` rejections.
   - Maximum retries enforced via `DTA_SELF_CORRECTION_MAX_RETRIES`, duplicate candidate SQL rejected, and every corrected candidate undergoes full AST revalidation.

## 2. Explicit Scope Boundaries & Honest Limitations

- No external penetration test or high-concurrency load benchmark has been executed; production deployment requires TLS termination at an ingress reverse proxy, external KMS/secret manager integration for `DTA_MASTER_KEY` and `DTA_JWT_SECRET`, and connection rate limiting.
- External OpenAI / Gemini API calls are implemented and verified against mock HTTP endpoints; live vendor API calls cannot be verified without operator-supplied API keys.
- BigQuery adapter is intentionally an explicit unimplemented stub (`AdapterUnavailable`).
