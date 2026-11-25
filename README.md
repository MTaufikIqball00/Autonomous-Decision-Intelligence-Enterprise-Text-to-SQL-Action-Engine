# DATA-TO-ACTION — Agen Analitik Enterprise Text-to-SQL & Action Engine

> **Bahasa Indonesia** | [README.id.md](README.id.md) | [Lisensi Apache 2.0](LICENSE)

**DATA-TO-ACTION** adalah platform analitik dan *Autonomous Decision Intelligence* enterprise berbasis **Natural Language to SQL (Text-to-SQL) & Action Engine** yang aman secara desain (*secure-by-design*), bersifat **read-only** pada lapisan eksekusi data, dapat diaudit (*auditable*), dan berbasis bukti nyata (*evidence-based*).

Dibangun dengan backend **Rust (`axum`, `tokio`, `sqlx`, `sqlparser`)**, **PostgreSQL `17.12.0`** (pemisahan tegas antara basis data metadata aplikasi dan basis data analitik read-only), serta frontend **Next.js 16 + Tailwind CSS v4**.

> **Deskripsi Repositori GitHub:**
> `Platform analitik Enterprise Text-to-SQL & Autonomous Action Engine — aman secara desain dengan Rust (Axum/SQLx/sqlparser AST), PostgreSQL read-only + RLS, Live Dashboards, Human-in-the-Loop Action Center, dan Next.js + Tailwind CSS v4.`

---

## Fitur & Pembaruan Terbaru

1. **Keamanan Inti & Reliabilitas Backend (P0)**:
   - **Zero Unvalidated SQL Execution**: Setiap kueri SQL (hasil AI, SQL manual, *Refresh Live* dashboard, maupun monitor Action Engine) wajib lolos validator AST PostgreSQL deterministik (`sqlparser` `PostgreSqlDialect`) sebelum dieksekusi.
   - **Isolasi Scope AST & Ekspresi Analitik**: Isolasi `ScopeContext` untuk subquery/derived table agar bebas *false positive* pada `SELECT *`, validasi penuh cabang `UNION`/`INTERSECT`/`EXCEPT`, serta dukungan aman `AT TIME ZONE`, `IS DISTINCT FROM`, dan operator baca JSON (`->`/`->>`).
   - **Connection Pool Caching (`AppState`)**: Pool `PgPool` di-cache per `connection_id` dengan fingerprint parameter SHA-256 dan invalidasi otomatis saat koneksi dibuat atau diuji.
   - **Sesi Cookie `HttpOnly; SameSite=Lax` & Rate Limiting**: Autentikasi sesi browser menggunakan cookie `HttpOnly; SameSite=Lax` (`dta_session`) + endpoint `POST /api/v1/auth/logout` tanpa menyimpan JWT di `localStorage`, dilengkapi pembatasan laju (*sliding-window rate limiter* HTTP `429` + `Retry-After`) pada `/api/v1/auth/login` dan `/api/v1/queries/ask`.
   - **Proteksi CSV Formula Injection & Logging Terstruktur**: `csv_escape` menetralkan sel berawalan `=`, `+`, `-`, `@`, `\t`, `\r` sebelum ekspor CSV, dan seluruh kegagalan penulisan `audit_logs` / `query_executions` dicatat melalui `tracing::warn!`.
2. **Sinkronisasi Penuh Backend ↔ Frontend (P1)**:
   - **Live Interactive Dashboards (`/dashboards`)**: Tiap widget menampilkan `ChartRenderer` + `ResultTable`, mendukung penambahan widget ke dashboard yang sudah ada (`POST /api/v1/dashboards/{id}/widgets`) maupun dashboard baru, serta tombol **Refresh Live** (`POST /api/v1/dashboards/{id}/refresh`) yang memvalidasi ulang SQL sesuai RBAC pemanggil.
   - **Governance, RBAC & User Management (`/admin`)**: Form registrasi koneksi DB terenkripsi, editor kebijakan RBAC per peran (`PUT /api/v1/admin/rbac/{role}`), manajemen peran & status user (`GET/PUT /api/v1/admin/users`), serta pencarian dan filter `audit_logs`.
   - **One-Click Re-Open & Multi-Connection Switcher**: Tombol *Open in Workspace / Re-open* pada `/history` menuju `/query`, serta pemilih koneksi aktif (`connection_id`) di `/query` dan `/schema`.
3. **Autonomous Decision Intelligence & Action Engine (P2)**:
   - **Pemantauan Terjadwal Read-Only (`/actions`)**: Aturan monitor (`action_rules`) divalidasi oleh AST validator saat dibuat dan dievaluasi secara berkala oleh scheduler Tokio (`spawn_action_scheduler`) menggunakan peran `dta_readonly` sambil mencatat `query_executions` untuk bukti `source_query_id`.
   - **Alur Human-in-the-Loop (HITL)**: Proposal aksi (`action_proposals`) melewati status `pending` → `approved`/`rejected` → `executed`/`failed` dengan pemisahan wewenang RBAC (hanya `Administrator` atau `DataEngineer` yang dapat menyetujui/mengeksekusi).
   - **Konektor Non-Destruktif Terenkripsi**: Mendukung `webhook`, `slack_teams`, dan `email_ticket_payload` dengan secret terenkripsi AES-256-GCM, `idempotency_key`, retry terbatas, validasi URL anti-SSRF metadata, dan jejak audit lengkap.
4. **Generalisasi Planner, Multi-Turn Context & CI/CD (P3)**:
   - **Konteks Percakapan Multi-Turn**: `/api/v1/queries/ask` dan workspace `/query` mendukung `session_id` + `conversation_history` untuk pertanyaan lanjutan seperti *"by region"*.
   - **Generalisasi Skema & Rentang Waktu**: Menghilangkan asumsi tahun statis pada `analytics.rs`, `linker.rs`, dan `local_deterministic.rs`, serta mendukung agregasi dinamis berbasis metadata tabel/kolom terizinkan.
   - **CI/CD GitHub Actions**: Workflow otomatis pada [.github/workflows/ci.yml](.github/workflows/ci.yml) untuk `cargo clippy --all-targets -- -D warnings`, `cargo test`, `tsc --noEmit`, `vitest`, dan `next build`.

---

## Alokasi Port Terisolasi

| Layanan | Host | Port | Eksposur | Variabel Konfigurasi |
|---|---|---|---|---|
| PostgreSQL `17.12.0` (Portabel Proyek) | `127.0.0.1` | `5439` | Loopback internal | `DTA_PG_PORT` |
| Backend Rust Axum | `127.0.0.1` | `8420` | Loopback lokal | `DTA_BACKEND_PORT` |
| Frontend Next.js + Tailwind CSS v4 | `127.0.0.1` | `3420` | Loopback lokal | `DTA_FRONTEND_PORT` |

---

## Panduan Cepat (Windows PowerShell 5.1)

```powershell
# 1. Unduh & verifikasi SHA-256 PostgreSQL 17.12.0 portabel, buat .env, peran read-only, dan data awal
powershell -ExecutionPolicy Bypass -File scripts/setup-postgres.ps1

# 2. Jalankan PostgreSQL, Backend Rust, dan Frontend Next.js
powershell -ExecutionPolicy Bypass -File scripts/start.ps1

# 3. Periksa status layanan dan URL aktif
powershell -ExecutionPolicy Bypass -File scripts/status.ps1

# 4. Hentikan seluruh layanan secara bersih
powershell -ExecutionPolicy Bypass -File scripts/stop.ps1
```

---

## Akun Demo RBAC Lokal

Tersedia otomatis di `data_to_action_app` untuk evaluasi lokal (lihat `/login`):
- `admin@enterprise.local` / `AdminPass!2026` (`administrator` — akses penuh & approval Action Engine)
- `analyst@enterprise.local` / `AnalystPass!2026` (`analyst` — kolom `tax_id` disembunyikan/ditolak, ekspor CSV diizinkan)
- `business@enterprise.local` / `BusinessPass!2026` (`business_user` — hanya skema `analytics`, ekspor CSV ditolak)
- `auditor@enterprise.local` / `AuditorPass!2026` (`auditor` — inspeksi log audit & kebijakan RBAC)

---

## Pengujian Otomatis & Linter

```powershell
# Backend Rust: Clippy & seluruh integration test suite (15 tests)
cd backend
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1

# Frontend Next.js: TypeCheck, Vitest (9 tests / 4 files) & Production Build
cd ../frontend
npx tsc --noEmit
npm test
npm run build
```

---

## Status Kemampuan (Terimplementasi vs Belum Tersedia)

- **Terimplementasi & Terverifikasi**: Provisioning PostgreSQL `17.12.0` portabel; `PostgresAdapter` read-only + RLS + timeout + pemotongan hasil + pool caching; penemuan skema & snapshot berversi; katalog semantik berversi; validator AST `sqlparser` dengan isolasi subquery & operasi himpunan; planner `local_deterministic` + konteks multi-turn; koreksi mandiri dengan validasi ulang AST; adaptor HTTP OpenAI/Gemini (teruji dengan mock HTTP server); enkripsi AES-256-GCM; RBAC Argon2id + JWT `HttpOnly` cookie + rate limiting; Live Dashboards; Action Center dengan scheduler & approval HITL; CI/CD GitHub Actions; skrip launcher PowerShell.
- **Stub Eksplisit**: `BigQueryAdapterStub` (mengembalikan `501 AdapterUnavailable` hingga integrasi BigQuery nyata ditambahkan dan diuji).
- **Memerlukan Konfigurasi Operator**: Pemanggilan API eksternal OpenAI/Gemini membutuhkan `OPENAI_API_KEY` + `OPENAI_MODEL` atau `GEMINI_API_KEY` + `GEMINI_MODEL` di `.env` atau halaman `/admin`; konektor `email_ticket_payload` berjalan dalam mode payload teraudit lokal bila `endpoint_url` webhook/SMTP belum diisi.

---

## Lisensi

Proyek ini dilisensikan di bawah **Apache License, Version 2.0** — lihat berkas [LICENSE](LICENSE) untuk ketentuan lengkapnya.
