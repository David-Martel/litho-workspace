# QMD Rust Migration + Litho Integration Plan

Date: 2026-02-24  
Status: Planning only (no migration implementation in this document)

## 1) Goals

1. Copy the current QMD TypeScript/Bun codebase into `litho-workspace` as a frozen reference.
2. Rebuild QMD as a pure Rust-native implementation first (standalone parity target).
3. Merge and integrate QMD + Litho capabilities into one combined Rust workspace.
4. Preserve existing QMD behavior while improving performance, reliability, and maintainability.

## 2) Non-Goals (for first delivery)

1. No hard requirement to preserve QMD SQLite DB byte-for-byte compatibility on day one.
2. No mandatory semantic-embedding parity in phase 1 if lexical parity is not yet complete.
3. No full UI redesign in `litho-book`; only search/retrieval backend integration points.

## 3) Current Baseline (Measured)

From benchmark run on `C:\codedev\litho-workspace`:

- Default index (`**/*.md`): `66 docs` in `1.257s`.
- Code-focused index (`**/*.{rs,toml,md,yml,yaml}`): `290 docs` in `2.271s`.
- Search latency (`search --files -n 8`, 5 queries): mean `1.013s`, p95 `1.091s`.
- Repeated query (`workspace build`) x8: mean `1.144s`, stddev `0.073s`.
- Duplicate-content signal: `290 active docs`, `196 unique hashes`, `94 duplicate docs`.

Implications:
- Startup/invocation overhead is material for CLI.
- Duplicate document paths materially reduce result quality.
- Markdown-only defaults under-index code-heavy repos.

## 4) Source of Truth to Copy into Workspace

Copy into `third_party/qmd-ts` (or `vendor/qmd-ts`) as read-only migration reference:

- `src/qmd.ts`, `src/mcp.ts`, `src/store.ts`, `src/llm.ts`, `src/formatter.ts`, `src/collections.ts`
- tests under `src/*.test.ts`
- `package.json`, lockfile, example configs, and docs

Rules:
- Keep exact snapshot commit/hash metadata in `third_party/qmd-ts/MIGRATION_NOTES.md`.
- Do not modify this snapshot except for patch notes or provenance metadata.

## 5) Target Rust Architecture (Standalone First)

Create new crates in workspace:

1. `crates/litho-qmd-core`
- Index model, storage, hashing, chunking, collection/context metadata.
- Search engine traits:
  - lexical (FTS/BM25)
  - semantic (vector)
  - hybrid rerank
- Document retrieval APIs (`get`, `multi_get`, path/docid resolution).

2. `crates/litho-qmd-storage`
- SQLite schema, migrations, persistence, compaction/vacuum.
- Data-access layer with typed query boundaries.
- Optional feature flags:
  - `sqlite-vec` backend
  - pure-rust ANN fallback backend (if extension unavailable)

3. `crates/litho-qmd-llm`
- Provider abstraction:
  - query expansion
  - embedding
  - reranking
- Adapters for local providers (Ollama/OpenAI-compatible HTTP) behind traits.
- Deterministic “no-LLM” mode for offline/CI.

4. `crates/litho-qmd-cli`
- Command parity with QMD:
  - `collection add/list/remove/rename`
  - `context add/list/check/rm`
  - `ls`, `get`, `multi-get`, `status`, `update`, `embed`, `search`, `vsearch`, `query`, `cleanup`
- Implemented with `clap` + structured output modes (`json/csv/md/xml/files/cli`).

5. `crates/litho-qmd-mcp`
- Native MCP server for QMD tools/resources/prompts.
- Transport: stdio first; HTTP transport optional later.
- Tool/resource parity with current `mcp.ts`.

Cross-cutting best practices:
- `tokio` runtime (for async I/O + model requests).
- `tracing` + `tracing-subscriber` for structured telemetry.
- `thiserror` + `anyhow` layered error strategy.
- `serde` schema-first request/response types.
- Integration tests with fixture repositories and golden outputs.

## 6) Parity Strategy (TS -> Rust)

Build a parity matrix from TS features and test each item:

1. CLI behavior parity
- Command names, argument semantics, output shape, exit codes.

2. Retrieval parity
- Path/docid resolution, line slicing, context prefixes, multi-get truncation.

3. Search parity
- Lexical relevance ordering, snippet extraction, score thresholds.

4. MCP parity
- Tool names/input schemas/output structure, resource URI behavior.

5. Operational parity
- Index update semantics, cleanup, status reporting.

Validation method:
- Golden test corpus with recorded TS outputs.
- Rust outputs compared by normalization rules (line endings, ordering, score epsilon).

## 7) Integration Plan with Existing Litho Crates

### A) `litho-book` search integration
- Current: in-memory markdown line scan.
- Plan:
  1. Introduce `SearchBackend` trait in `litho-book`.
  2. Keep existing in-memory backend as default fallback.
  3. Add QMD backend using `litho-qmd-core` APIs.
  4. Enable query routing and relevance-ranked responses.

### B) `litho-generator` research memory enrichment
- Current: scope/key JSON memory store.
- Plan:
  1. Add `QmdRetriever` adapter used by research agents.
  2. Enrich system/domain/workflow analysis prompts with retrieved citations.
  3. Store retrieval artifacts back into memory scopes for traceability.

### C) `litho-codex` prompt augmentation
- Current: prompt uses extracted snapshot + interfaces.
- Plan:
  1. Add optional retrieval augmentation mode.
  2. Pull targeted evidence snippets per section.
  3. Use bounded, deduped, citation-bearing inserts to control token cost.

## 8) Performance + Quality Workstreams

1. Performance
- Warm persistent service path for repeated queries (MCP/daemon mode).
- Batch indexing transactions and prepared statement reuse.
- Minimize per-command startup overhead in CLI.
- Result-cache for repeated lexical queries + snippet windows.

2. Quality
- Hash-level dedup by default in ranking pipeline.
- Path/domain boosting (prefer `crates/` over mirrored/generated trees).
- Deterministic non-interactive mode (no terminal progress sequences in automation).
- Better score calibration and cutoff defaults.

3. Reliability
- Crash-safe migration/versioning for DB schema.
- Graceful shutdown and cancellation handling.
- Strict input validation and bounded resource usage.

## 9) Execution Phases and Gates

### Phase 0: Snapshot + scaffolding
- Copy TS reference into workspace.
- Create Rust crate skeletons and CI wiring.
- Gate: project builds and tests run with placeholders.

### Phase 1: Rust standalone lexical parity
- Implement storage, indexing, lexical search, get/multi-get, status.
- Gate: parity tests pass for lexical commands and retrieval behavior.

### Phase 2: Rust MCP parity
- Implement MCP tools/resources/prompts in Rust.
- Gate: inspector-based MCP contract tests pass.

### Phase 3: Vector/hybrid parity
- Implement embedding/vector/rerank providers and hybrid query.
- Gate: semantic/hybrid regression tests pass on fixture corpora.

### Phase 4: Litho integration
- Integrate with `litho-book`, `litho-generator`, `litho-codex` behind feature flags.
- Gate: existing behavior unchanged when feature flag disabled; integration tests pass when enabled.

### Phase 5: Performance hardening
- Benchmark, profile hotspots, apply optimizations.
- Gate: measurable improvement targets met (defined below).

## 10) Acceptance Criteria

Functional:
- Rust CLI + MCP cover all existing QMD commands/tools used in current workflows.
- No regressions in core retrieval semantics (document path/docid/line handling).

Performance targets (initial):
- Indexing throughput >= current baseline on code-focused corpus.
- Repeated lexical query p95 <= `0.6s` in persistent mode.
- Memory usage stable under repeated search/load tests.

Quality:
- Duplicate result rate reduced by >= 80% on mirrored-path corpus.
- Deterministic JSON output for automation scenarios.

## 11) Risks and Mitigations

1. Vector backend portability
- Risk: sqlite-vec platform friction.
- Mitigation: backend trait + fallback ANN implementation.

2. Behavioral drift from TS
- Risk: subtle output/ordering mismatch.
- Mitigation: golden corpus + compatibility tests before switching defaults.

3. Scope creep in integration
- Risk: touching multiple crates introduces regressions.
- Mitigation: feature flags, staged merges, and crate-local integration tests.

## 12) Proposed Repository Layout Changes

Planned additions:

- `third_party/qmd-ts/` (frozen TS/Bun snapshot)
- `crates/litho-qmd-core/`
- `crates/litho-qmd-storage/`
- `crates/litho-qmd-llm/`
- `crates/litho-qmd-cli/`
- `crates/litho-qmd-mcp/`
- `docs/qmd-rust-migration-integration-plan.md` (this plan)
- `docs/qmd-parity-matrix.md` (generated during phase 0/1)

## 13) Implementation Start Condition

Proceed to implementation only after approval of:

1. crate topology (`litho-qmd-*` naming and boundaries),
2. backend choices for vector/embedding providers,
3. phased gates and acceptance thresholds.
