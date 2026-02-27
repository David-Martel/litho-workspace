# QMD Rust Architecture Optimizations (2026-02-24)

## Implemented in this iteration

1. PostgreSQL-native storage backend with pooled connections (`r2d2_postgres`).
2. Automatic PostgreSQL bootstrap path:
   - detects missing target DB and creates it automatically via admin DB connection.
3. Full-text retrieval on PostgreSQL (`tsvector` + GIN + `websearch_to_tsquery`).
4. Native quantized semantic vectors retained (`i16` in `BYTEA`) with LSH candidate prefilter.
5. Hybrid retrieval and rerank path retained and tuned.
6. Adaptive LLM strategy uses heuristic-first + selective ollama augmentation.
7. Config architecture migrated to repo-file-first behavior:
   - `qmd.config.json` and `.env` are primary
   - env vars are fallback only
8. Rust MCP is now the default launcher/runtime path.

## Search and quality impact

- Reduced false negatives with lexical variant fusion + semantic fallback.
- Reduced false positives with lexical overlap blending and reranking.
- Preserved recall via vector full-scan fallback when LSH prefilter misses.

## Performance and reliability controls

- Tunable pool limits/timeouts via repo config.
- Transactional ingest/embed updates.
- Default local model discovery from repo config (`model_dirs`) and `.ollama`.
- Local Postgres server utility script added for operational bootstrap/repair.

## Remaining opportunities

1. Add persistent embedding worker process to remove per-query model startup overhead.
2. Add ingest chunk-level parallelism with bounded worker pools.
3. Add query telemetry loop for automatic per-collection score tuning.
