-- ==============================================================================
-- DATA-TO-ACTION: Database Separation & Least-Privilege Read-Only Role Setup
-- Executed by scripts/setup-postgres.ps1 with psql -v parameters (no secrets hardcoded).
-- Variables expected:
--   app_db_name, app_db_user, app_db_password
--   analytics_db_name, readonly_db_user, readonly_db_password
-- ==============================================================================

\set ON_ERROR_STOP on

-- 1. Application metadata role (separate from source analytics execution role)
SELECT format(
    'CREATE ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L',
    :'app_db_user',
    :'app_db_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'app_db_user')
\gexec

SELECT format(
    'ALTER ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L',
    :'app_db_user',
    :'app_db_password'
)
\gexec

-- 2. Strict read-only analytics execution role
SELECT format(
    'CREATE ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION PASSWORD %L',
    :'readonly_db_user',
    :'readonly_db_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'readonly_db_user')
\gexec

SELECT format(
    'ALTER ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION PASSWORD %L',
    :'readonly_db_user',
    :'readonly_db_password'
)
\gexec

-- Enforce read-only transactions and timeouts at the PostgreSQL role level
SELECT format('ALTER ROLE %I SET default_transaction_read_only = on', :'readonly_db_user') \gexec
SELECT format('ALTER ROLE %I SET statement_timeout = ''15000''', :'readonly_db_user') \gexec
SELECT format('ALTER ROLE %I SET lock_timeout = ''3000''', :'readonly_db_user') \gexec
SELECT format('ALTER ROLE %I SET idle_in_transaction_session_timeout = ''10000''', :'readonly_db_user') \gexec

-- 3. Create separated Application Metadata DB
SELECT format(
    'CREATE DATABASE %I OWNER %I ENCODING ''UTF8''',
    :'app_db_name',
    :'app_db_user'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'app_db_name')
\gexec

-- 4. Create separated Source Analytics DB (owned by bootstrap admin, NOT readonly or app role)
SELECT format(
    'CREATE DATABASE %I ENCODING ''UTF8''',
    :'analytics_db_name'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'analytics_db_name')
\gexec

-- 5. Lock down cross-database access strictly
SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', :'app_db_name') \gexec
SELECT format('REVOKE ALL ON DATABASE %I FROM %I', :'app_db_name', :'readonly_db_user') \gexec
SELECT format('GRANT ALL ON DATABASE %I TO %I', :'app_db_name', :'app_db_user') \gexec

SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', :'analytics_db_name') \gexec
SELECT format('REVOKE ALL ON DATABASE %I FROM %I', :'analytics_db_name', :'app_db_user') \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO %I', :'analytics_db_name', :'readonly_db_user') \gexec
