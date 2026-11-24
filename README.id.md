# DATA-TO-ACTION — Enterprise Text-to-SQL & Analytics Agent (Bahasa Indonesia)

> [English README](README.md) | **Bahasa Indonesia (README.id.md)** | [Lisensi Apache 2.0](LICENSE)

## Deskripsi Singkat

**DATA-TO-ACTION** adalah platform analitik enterprise berbasis **Natural Language to SQL (Text-to-SQL)** yang aman secara desain (*secure-by-design*), bersifat **read-only**, dapat diaudit (*auditable*), dan berbasis bukti (*evidence-based*). Sistem ini memungkinkan pengguna bisnis mengajukan pertanyaan dalam bahasa natural, menautkan skema basis data yang relevan secara otomatis, menghasilkan kueri PostgreSQL, memvalidasi keamanan SQL secara deterministik melalui AST (*Abstract Syntax Tree*), mengeksekusi kueri pada peran read-only, memverifikasi hasil, serta menyajikan tabel, grafik interaktif, dan ringkasan wawasan bisnis yang terverifikasi.

**Deskripsi Repositori GitHub (Singkat):**
> Platform analitik Enterprise Text-to-SQL & Data-to-Action yang aman, read-only, dan berbasis bukti — menggunakan backend Rust (Axum, SQLx, sqlparser AST validator), PostgreSQL terpisah, serta antarmuka Next.js + Tailwind CSS v4.

---

## Jaminan Keamanan & Arsitektur Utama

1. **Zero Unvalidated SQL Execution**: Tidak ada kueri SQL hasil LLM atau input manual yang dieksekusi sebelum melewati validator AST deterministik (`sqlparser` `PostgreSqlDialect`).
2. **Pertahanan Berlapis (*Defense-in-Depth*) Read-Only**:
   - **Gerbang AST (`sql_validator`)**: Hanya mengizinkan satu pernyataan `Statement::Query` (`SELECT` / `WITH` / operasi himpunan). Menolak DML/DDL/DCL/transaksi (`INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `CREATE`, `TRUNCATE`, `GRANT`, `REVOKE`, `COPY`, `CALL`, `DO`, `SET`, `RESET`, `BEGIN`), komentar (`--`, `/* */`), multi-statement, `SELECT ... INTO`, klausa kunci (`FOR UPDATE`/`SHARE`), `CROSS JOIN`, `WITH RECURSIVE` tanpa `LIMIT`, fungsi berbahaya/tidak terdaftar (`pg_read_file`, `pg_sleep`, `dblink`, `set_config`), skema/tabel/kolom tidak berizin, serta kolom terlarang RBAC (termasuk kebocoran `SELECT *`).
   - **Pembatasan Baris & Ukuran Hasil**: Kueri tervalidasi dibungkus ke dalam `SELECT * FROM (<sql>) AS _dta_safe_result LIMIT <effective_limit + 1>` beserta pemeriksaan batas byte (`DTA_MAX_RESULT_BYTES`).
   - **Batas Read-Only PostgreSQL**: Dieksekusi eksklusif menggunakan peran `dta_readonly` (`NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `default_transaction_read_only = on`) dalam transaksi `BEGIN` → `SET TRANSACTION READ ONLY` → `SET LOCAL statement_timeout` → `SET LOCAL app.current_tenant_region` → `ROLLBACK`.
   - **Row-Level Security (RLS) PostgreSQL**: Diterapkan pada `analytics.orders` (`app.current_tenant_region`), serta pemblokiran total akses terhadap skema `restricted_hr` dan basis data `data_to_action_app`.
3. **Pemisahan Basis Data Fisik/Logis**:
   - `data_to_action_analytics`: Gudang data analitik sumber (`analytics`, `finance`, dan skema terlarang `restricted_hr` untuk pengujian).
   - `data_to_action_app`: Metadata aplikasi (`users`, `rbac_policies`, `db_connections` terenkripsi AES-256-GCM, `schema_snapshots`, `manual_relationships`, `semantic_metrics` berversi, `query_executions`, `saved_queries`, `dashboards`, `llm_providers`, `audit_logs`).
4. **Katalog Semantik & *Schema Linking* Berbasis Bukti**:
   - Penautan skema menggunakan pencocokan kata kunci, deskripsi tabel/kolom, metrik semantik aktif, dan penelusuran *foreign key* terverifikasi (tidak menebak relasi hanya karena kemiripan nama kolom).
   - Mendeteksi ambiguitas definisi bisnis (mis. `Gross Revenue` vs `Recognized Net Revenue`) dan metrik yang tidak tersedia (mis. *churn rate*, *NPS*) tanpa mengarang rumus.
5. **Koreksi Mandiri (*Self-Correction*) Terkendali**:
   - Hanya berjalan pada galat `syntax`, `schema`, atau `type` dengan batas percobaan (`DTA_SELF_CORRECTION_MAX_RETRIES`), mencegah pengulangan SQL identik, dan wajib melewati validasi ulang AST penuh sebelum eksekusi ulang.
6. **Transparansi AI & Penyedia LLM**:
   - Mendukung adaptor HTTP **OpenAI** dan **Google Gemini** (model dikonfigurasi melalui environment/UI, tidak di-hardcode) serta mode `local_deterministic` (`schema-semantic-planner-v1`) untuk eksekusi offline yang dapat diverifikasi.

---

## Alokasi Port Terisolasi

| Layanan | Host | Port | Eksposur | Variabel Konfigurasi |
|---|---|---|---|---|
| PostgreSQL `17.12.0` (Portabel Proyek) | `127.0.0.1` | `5439` | Loopback internal | `DTA_PG_PORT` |
| Backend Rust Axum | `127.0.0.1` | `8420` | Loopback lokal | `DTA_BACKEND_PORT` |
| Frontend Next.js + Tailwind CSS | `127.0.0.1` | `3420` | Loopback lokal | `DTA_FRONTEND_PORT` |

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
- `admin@enterprise.local` / `AdminPass!2026` (`administrator`)
- `analyst@enterprise.local` / `AnalystPass!2026` (`analyst` — kolom `tax_id` disembunyikan/ditolak, ekspor CSV diizinkan)
- `business@enterprise.local` / `BusinessPass!2026` (`business_user` — hanya skema `analytics`, ekspor CSV ditolak)
- `auditor@enterprise.local` / `AuditorPass!2026` (`auditor` — inspeksi log audit & kebijakan RBAC)

---

## Pengujian Otomatis

```powershell
# Pengujian Backend Rust (Keamanan SQL AST, Read-Only DB, Pipeline, RBAC)
cd backend
cargo test -- --test-threads=1

# Pengujian Frontend Vitest, TypeScript, dan Build Produksi
cd ../frontend
npm test
npm run typecheck
npm run build
```

---

## Status Kemampuan (Terimplementasi vs Belum Tersedia)

- **Terimplementasi & Terverifikasi**: Provisioning PostgreSQL `17.12.0` portabel; `PostgresAdapter` read-only + RLS + timeout + pemotongan hasil; penemuan skema & snapshot berversi; katalog semantik berversi; validator AST `sqlparser`; planner `local_deterministic`; koreksi mandiri dengan validasi ulang AST; adaptor HTTP OpenAI/Gemini (teruji terhadap mock HTTP server); enkripsi AES-256-GCM; RBAC Argon2id + JWT; UI Next.js Tailwind CSS v4; skrip launcher PowerShell.
- **Stub Eksplisit**: `BigQueryAdapterStub` (mengembalikan `501 AdapterUnavailable` hingga integrasi BigQuery nyata ditambahkan dan diuji).
- **Tidak Digunakan Secara Sengaja**: Layanan Python/FastAPI dan Redis (pengecekan AST deterministik berjalan langsung di dalam proses Rust).
- **Memerlukan Kunci API Operator**: Pemanggilan API eksternal OpenAI atau Google Gemini membutuhkan `OPENAI_API_KEY` + `OPENAI_MODEL` atau `GEMINI_API_KEY` + `GEMINI_MODEL` pada `.env` atau halaman Admin UI.

---

## Lisensi

Proyek ini dilisensikan di bawah **Apache License, Version 2.0** — lihat berkas [LICENSE](LICENSE) untuk ketentuan lengkapnya.
