# QMD Repo-Local SQLite Proposal (2026-03-05)

## Current State

- `litho-qmd-core` defines a storage trait (`QmdStore`) with search, ingest, embed, context, and cleanup operations.
- `litho-qmd-storage` currently implements this trait only with `PostgresQmdStore`.
- `SqliteQmdStore` is only a type alias to `PostgresQmdStore` (not a real SQLite backend).
- `litho-qmd-cli` and `litho-qmd-mcp` instantiate `PostgresQmdStore::open_default(...)` directly.
- `litho-generator` integrates QMD by shelling out to the `qmd` CLI and ingesting JSON search hits into research memory.

## How PostgreSQL Is Used Today

QMD storage uses PostgreSQL for:

- Document/content persistence (`content`, `documents`).
- Lexical search (`documents.search` `tsvector` + GIN index + `websearch_to_tsquery` + `ts_rank_cd`).
- Native vector retrieval with quantized vectors (`content_vectors_native`) and LSH prefilter (`content_vectors_lsh`).
- Incremental ingest/upsert/deactivate workflow per collection.
- Optional DB bootstrap (create target DB if missing) and connection pooling (`r2d2_postgres`).

It also writes search telemetry to JSONL (file path from config/env), outside the database.

## Problem Statement

For repo-limited investigations, PostgreSQL setup adds friction:

- service lifecycle/dependency overhead,
- env/config complexity,
- less portable "clone repo and run qmd" experience.

## Proposal

Implement a real SQLite backend and support explicit backend selection.

### Backend Selection

- Add `StorageBackend` in `litho-qmd-storage`:
  - `Postgres` (existing),
  - `Sqlite` (new).
- Add backend selection via CLI/config/env:
  - `qmd --backend postgres|sqlite`,
  - `QMD_BACKEND=postgres|sqlite`,
  - optional `qmd.config.json` field (e.g. `database.backend`).

### Repo-Local SQLite Defaults

- Default SQLite path for repo-scoped mode:
  - `.litho/qmd/<index>.sqlite3`
- Keep existing collection config behavior, but ensure default collection/index artifacts stay under `.litho/` for portability.

### SQLite Feature Mapping

- Lexical: use SQLite FTS5 virtual table instead of Postgres `tsvector`.
- Vector: keep existing quantized embedding + LSH design in SQLite tables (`BLOB` + band/bucket index).
- Ingest/update/cleanup: same semantics as Postgres implementation.
- Telemetry: unchanged (JSONL file output).

## Tradeoffs

### SQLite Pros

- Zero external service dependency.
- Best developer UX for per-repo indexing.
- Fast local reads, simple deployment, easy CI portability.
- Good fit for `litho-generator` repo-scoped QMD seeding.

### SQLite Cons

- Weaker concurrent writer model than PostgreSQL.
- Less sophisticated ranking/query language than Postgres FTS stack.
- Requires new SQL/query tuning and parity testing to preserve relevance quality.

### PostgreSQL Pros

- Better multi-user/service deployment model.
- Mature FTS behavior and scaling headroom.
- Existing implementation is already productionized in this repo.

### PostgreSQL Cons

- Higher operational/setup burden for local, repo-only workflows.

## Recommendation

- Keep PostgreSQL as supported "service/shared" backend.
- Add SQLite as first-class "repo-local" backend and make it the default for developer-local flows.
- Preserve CLI/MCP/service contracts so `litho-generator` QMD integration remains unchanged (it calls CLI, not storage internals).

## Implementation Plan

1. Introduce backend abstraction in `litho-qmd-storage` (without behavior change).
2. Add `SqliteQmdStore` implementing `QmdStore` with:
   - schema bootstrap,
   - FTS5 lexical search,
   - quantized vector + LSH retrieval,
   - ingest/embed/cleanup parity.
3. Update `litho-qmd-cli` and `litho-qmd-mcp` to open selected backend.
4. Add parity tests that run against both backends for:
   - ingest/update/deactivate,
   - search/vsearch/query contracts,
   - get/multi-get/list/context operations.
5. Add migration utility (optional):
   - export/import from Postgres to SQLite for an index.
6. Update docs and examples (`qmd.config.json`, quickstart, CI notes).

## Acceptance Criteria

- `qmd` works with no Postgres dependency in repo-local mode.
- `litho-generator` QMD seeding works unchanged with SQLite backend.
- Relevance and latency are within agreed tolerances vs current Postgres baseline.
- Existing Postgres workflow remains functional and selectable.
