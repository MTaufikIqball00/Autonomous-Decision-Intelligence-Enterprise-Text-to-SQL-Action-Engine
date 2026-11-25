# DATA-TO-ACTION — Agen Analitik Enterprise Text-to-SQL & Action Engine (Bahasa Indonesia)

> [README Utama (Bahasa Indonesia)](README.md) | **README.id.md** | [Lisensi Apache 2.0](LICENSE)

## Deskripsi Singkat

**DATA-TO-ACTION** adalah platform analitik enterprise berbasis **Natural Language to SQL (Text-to-SQL) & Action Engine** yang aman secara desain (*secure-by-design*), bersifat **read-only**, dapat diaudit (*auditable*), dan berbasis bukti (*evidence-based*). Sistem ini memungkinkan pengguna bisnis mengajukan pertanyaan dalam bahasa natural, menautkan skema basis data yang relevan secara otomatis, menghasilkan kueri PostgreSQL, memvalidasi keamanan SQL secara deterministik melalui AST (*Abstract Syntax Tree*), mengeksekusi kueri pada peran read-only, memverifikasi hasil, menyajikan tabel serta grafik interaktif, hingga mengoordinasikan tindak lanjut melalui **Action Center (Human-in-the-Loop)**.

---

## Jaminan Keamanan & Arsitektur Utama

1. **Zero Unvalidated SQL Execution**: Tidak ada kueri SQL hasil LLM, input manual, refresh dashboard, maupun monitor terjadwal yang dieksekusi sebelum lolos validator AST deterministik (`sqlparser` `PostgreSqlDialect`).
2. **Pertahanan Berlapis (*Defense-in-Depth*) Read-Only**:
   - **Gerbang AST (`sql_validator`)**: Hanya mengizinkan satu pernyataan `Statement::Query` (`SELECT` / `WITH` / `UNION` / `INTERSECT` / `EXCEPT`). Mengisolasi `ScopeContext` pada subquery agar pengecekan `SELECT *` akurat tanpa *false positive*, mendukung `AT TIME ZONE`, `IS DISTINCT FROM`, dan akses baca JSON (`->`/`->>`), serta menolak DML/DDL/DCL/transaksi, komentar (`--`, `/* */`), multi-statement, `SELECT ... INTO`, `CROSS JOIN`, fungsi berbahaya/tidak terdaftar, serta kolom terlarang RBAC.
   - **Pembatasan Baris & Ukuran Hasil**: Kueri tervalidasi dibungkus dengan `LIMIT <effective_limit + 1>` dan pemeriksaan batas byte (`DTA_MAX_RESULT_BYTES`).
   - **Batas Read-Only & RLS PostgreSQL**: Eksekusi menggunakan peran `dta_readonly` di dalam transaksi `BEGIN` → `SET TRANSACTION READ ONLY` → `SET LOCAL statement_timeout` → `SET LOCAL app.current_tenant_region` → `ROLLBACK`.
   - **Connection Pool Caching & Keamanan Sesi**: Pool `PgPool` di-cache per `connection_id` di `AppState`; autentikasi browser menggunakan cookie `HttpOnly; SameSite=Lax` (`dta_session`); endpoint `/auth/login` dan `/queries/ask` dilindungi rate limiter (`429 Too Many Requests` + `Retry-After`); serta `csv_escape` melindungi ekspor CSV dari formula injection.
3. **Pemisahan Basis Data**:
   - `data_to_action_analytics`: Sumber data analitik (`analytics`, `finance`, plus `restricted_hr`).
   - `data_to_action_app`: Metadata aplikasi (`users`, `rbac_policies`, `db_connections`, `schema_snapshots`, `manual_relationships`, `semantic_metrics`, `query_executions`, `saved_queries`, `dashboards`, `llm_providers`, `audit_logs`, `action_connectors`, `action_rules`, `action_proposals`, `action_executions`).
4. **Autonomous Action Engine (Human-in-the-Loop)**:
   - Scheduler Tokio (`spawn_action_scheduler`) mengevaluasi rule secara read-only dan membuka proposal `pending` dengan bukti `source_query_id`.
   - Persetujuan (`approve`/`reject`) dan eksekusi (`execute`) dibatasi hanya untuk peran `Administrator` dan `DataEngineer`, dengan secret konektor terenkripsi AES-256-GCM, `idempotency_key`, dan retry terbatas.

---

## Alokasi Port Terisolasi

| Layanan | Host | Port | Eksposur | Variabel Konfigurasi |
|---|---|---|---|---|
| PostgreSQL `17.12.0` (Portabel Proyek) | `127.0.0.1` | `5439` | Loopback internal | `DTA_PG_PORT` |
| Backend Rust Axum | `127.0.0.1` | `8420` | Loopback lokal | `DTA_BACKEND_PORT` |
| Frontend Next.js + Tailwind CSS v4 | `127.0.0.1` | `3420` | Loopback lokal | `DTA_FRONTEND_PORT` |

---

## Panduan Cepat & Pengujian

```powershell
# Jalankan PostgreSQL portabel, Backend Rust, dan Frontend Next.js
powershell -ExecutionPolicy Bypass -File scripts/setup-postgres.ps1
powershell -ExecutionPolicy Bypass -File scripts/start.ps1
powershell -ExecutionPolicy Bypass -File scripts/status.ps1
powershell -ExecutionPolicy Bypass -File scripts/stop.ps1

# Pengujian Backend (15 integration tests) & Clippy
cd backend
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1

# Pengujian Frontend (9 Vitest tests, TypeScript & Next.js build)
cd ../frontend
npx tsc --noEmit
npm test
npm run build
```

---

## Lisensi

Proyek ini dilisensikan di bawah **Apache License, Version 2.0** — lihat berkas [LICENSE](LICENSE) untuk ketentuan lengkapnya.
