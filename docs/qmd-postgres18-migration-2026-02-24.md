# QMD PostgreSQL 18 Migration (2026-02-24)

## Completed

1. Storage backend is now PostgreSQL-native (`r2d2_postgres`), not SQLite.
2. QMD CLI/MCP use the Rust/Postgres store by default.
3. Automatic DB bootstrap is implemented:
   - if target DB is missing, QMD auto-creates it via admin DB connection.
4. Config is now repo-file-first:
   - `qmd.config.json`
   - `.env` (or `config/.env`)
   - environment variables are fallback only.
5. Rust MCP is default runtime path in launcher (Bun fallback only when Rust binary is unavailable).

## Key files

- `crates/litho-qmd-storage/src/lib.rs`
- `crates/litho-qmd-storage/Cargo.toml`
- `crates/litho-qmd-cli/src/main.rs`
- `crates/litho-qmd-mcp/src/main.rs`
- `crates/litho-qmd-llm/src/lib.rs`
- `qmd.config.json`
- `.env` / `.env.example`
- `scripts/postgres18-bootstrap.ps1`
- `C:\Users\david\.codex\tools\qmd-mcp\run-qmd-mcp.ps1`

## PostgreSQL tools discovered

- `C:\Program Files\PostgreSQL\18\bin\psql.exe`
- `C:\Program Files\PostgreSQL\18\bin\pg_ctl.exe`
- `C:\Program Files\PostgreSQL\18\bin\pg_isready.exe`
- `C:\Program Files\PostgreSQL\18\bin\createdb.exe`
- `C:\Program Files\PostgreSQL\18\bin\createuser.exe`

## Verified

- `cargo check` passed for qmd core/storage/llm/cli/mcp packages.
- `cargo build` passed for qmd cli/mcp.
- `litho-qmd-mcp --healthcheck` passes with repo config defaults.
- `scripts/postgres18-bootstrap.ps1` runs successfully and ensures role+DB.

## Default local auth

Current local setup works with:

- user: `postgres`
- password: `password`
- database: `qmd_index`
