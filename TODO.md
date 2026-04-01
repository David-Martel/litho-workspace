# Litho Workspace TODOs

Last updated: 2026-04-01 (Session 55: full re-evaluation, crate research, async audit, simplification pass)

Current state: 540 tests, 11 crates, 5 binaries, repo PUBLIC, Rust 1.94

---

## P0: Low-Hanging Fruit (immediate impact, minimal risk)

### CLI Unification (group naturally with simplification)
- [ ] Add `litho index <path>` — thin wrapper over `litho-qmd-cli collection add + ingest + embed`
- [ ] Add `litho init` — create default `litho.toml` in target project
- [ ] Expose `--min-quality <F>` and `--enforce-gate` as CLI flags on `litho-generator`
  (config fields exist in `QualityConfig`, just need clap wiring)
- [ ] Remove duplicated search/query/vsearch help text between litho-cli and litho-qmd-cli

### Testing Quick Wins (path to 600+)
- [ ] Add `insta` snapshot tests for litho-extract JSON output (deterministic, catches regressions)
- [ ] Add `wiremock` HTTP mock tests for LLM client (test retry/fallback without real Ollama)
- [ ] Add `rstest` parameterized tests for multi-language AST extraction
- [ ] Target: 600+ tests, 60% coverage (currently 540)

### Crate Upgrades (drop-in improvements)
- [ ] Replace custom in-memory LRU cache with `moka` 0.12 (`moka::future::Cache`)
  — concurrent, async-native, TTL/size-bounded, zero custom code needed
- [ ] Add `insta` 1.x for snapshot testing (test extract output stability)
- [ ] Add `wiremock` 0.6 for HTTP mocking (test LLM client paths without network)

---

## P1: Async Pipeline & Performance

### Streaming Pipeline (biggest performance win)
The preprocessing stage is 87.9% of runtime. The pipeline is strictly sequential:
preprocess ALL → research ALL → compose ALL → validate. Current async: `Semaphore`
+ `join_all` for agent fan-out, `rayon` for file extraction, `buffer_unordered`
in structure_extractor. No `JoinSet`, no inter-stage streaming.

- [ ] **Async channel pipeline**: use `tokio::sync::mpsc` to stream preprocessed
  files into research as they complete (don't wait for all)
- [ ] **Replace `join_all` with `JoinSet`**: modernize from `futures::join_all` +
  `Semaphore` (in `threads.rs`, `knowledge_sync.rs`) to `tokio::task::JoinSet`
  for structured concurrency with cancellation
- [ ] **Add `tower` rate-limiting layer** for LLM calls: replace manual `Semaphore`
  in `ollama_native.rs:55` with `tower::limit::RateLimit` + `tower::retry::Retry`
- [ ] **Full cache warming pass**: BLAKE3 hash pre-scan of all files before LLM
  calls, skip unchanged (BLAKE3 exists in `cache/mod.rs:491` but no pre-scan)
- [ ] **Add `governor` rate limiter** per LLM provider (e.g., 60 req/min for Ollama)
  to prevent 429 storms instead of relying on server-side rejection

### QMD Async Migration
QMD storage is entirely synchronous (`pub fn`, no async). This blocks the MCP
server and CLI on I/O.

- [ ] Migrate `QmdStore` trait to async (`async fn search/ingest/embed`)
- [ ] Use `rusqlite` with `spawn_blocking` or switch to `tokio-rusqlite` for
  non-blocking SQLite access
- [ ] Consider `deadpool-postgres` for async PostgreSQL (replace `r2d2` sync pool)

---

## P1: Quality & Reliability

### Regression Infrastructure
- [ ] **Snapshot golden reference**: run `litho-generator` on a small fixture project,
  commit output as `tests/fixtures/golden-output/`, diff against future runs
- [ ] **`insta` integration**: use `insta::assert_snapshot!` for extract output stability
- [ ] Terminology consistency checker (beyond current 6-dimension validator)

### Integration Tests (4 remaining gaps)
- [ ] Pipeline end-to-end: preprocess → research → compose → output (use mock LLM)
- [ ] CodexRs fallback: simulate Ollama failure, verify codex-rs takeover
- [ ] Incremental mode: full run → edit one file → verify selective re-run
- [ ] QMD PostgreSQL 18 integration (CI opt-in lane exists, needs test)

---

## P2: Simplification & Consolidation

### Crate Merges (11 → 9 crates)
Audit confirmed three crates are too thin for independent compilation units:

- [ ] **Merge `litho-qmd-core` (606 LOC) into `litho-qmd-storage`** — 19 of 22 public
  methods are single-line pass-through delegation. Move `model.rs`, `traits.rs`,
  `error.rs` into `litho-qmd-storage` as submodules.
- [ ] **Merge `litho-qmd-llm` (732 LOC, 0 tests) into `litho-qmd-storage`** — 4
  utility functions duplicated verbatim between the two crates (`discover_repo_file`,
  `repo_qmd_config`, `repo_dotenv_values`, `get_dotenv_or_env`). Two incompatible
  `RepoQmdConfig` struct definitions. Merging fixes both duplication and the config
  divergence. Add tests (currently 0).
- [ ] **Keep `litho-codex` separate** — it firewalls the 52-crate codex-rs dependency
  tree. BUT reconcile the two parallel Codex paths: `litho-codex` (used by litho-cli)
  vs `litho-generator/codex_provider.rs` (subprocess, used by litho-generator).
  Pick one integration pattern.

Result: QMD subsystem goes from 3 lib + 2 bin → 1 lib (`litho-qmd`) + 2 bin.

### Code Quality Quick Wins
- [ ] **Split `litho-qmd-storage/src/lib.rs` (2,644 LOC)** — extract `postgres_impl.rs`,
  `auto_store.rs`, `config.rs` modules (currently a god module)
- [ ] **Feature-gate PostgreSQL** in litho-qmd-storage — `default = ["sqlite"]`,
  `postgres` feature for PostgreSQL backend. Saves compile time for local use.
- [ ] **Feature-gate `pdf-extract`** in litho-generator — used at exactly 1 call site
  (`integrations/local_docs.rs:382`), pulls in heavy deps (`lopdf`, `encoding_rs`)
- [ ] **Remove duplicate search commands** from litho-cli — `search`/`query`/`vsearch`
  duplicate litho-qmd-cli identically (same format string, same `QmdService` setup).
  Either delegate to subprocess or eliminate litho-qmd-cli entirely.
- [ ] **Standardize markdown parser** — litho-generator depends on both `comrak` AND
  `pulldown-cmark`. Pick one to reduce dependency surface.

### LLM Client Simplification
The project maintains ~2,600 LOC of hand-rolled HTTP client code across `providers.rs`
(1,009 LOC), `ollama_native.rs` (1,097 LOC), `codex_provider.rs` (579 LOC).

- [ ] **Evaluate `async-openai` v0.34** — covers Chat, Responses API, Embeddings with
  built-in exponential backoff retry on 429s. Works with Ollama via `OPENAI_BASE_URL`.
  Would replace the OpenAI arm of `providers.rs` (~150 LOC). Does NOT cover Anthropic
  or Gemini — those still need thin wrappers.
- [ ] **Evaluate `genai` v0.5** — 14 providers out of the box (Ollama, OpenAI,
  Anthropic, Gemini, DeepSeek, Groq, Cohere). Would replace most of `providers.rs`.
  Tradeoff: larger dependency but removes ~800 LOC of per-provider format code.
- [ ] **Add `reqwest-middleware` + `reqwest-retry`** — composable retry middleware
  for any custom providers that remain. Replaces ad-hoc backoff logic.
- [ ] **Add `futures-concurrency` v7.7** — replace `futures::join_all` + `Semaphore`
  with `FutureGroup` for structured concurrency. `try_join` cancels siblings on
  first error. `ConcurrentStream` for batch file processing.
- [ ] **Add `minijinja` v2.7** — replace `format!()` prompt construction with Jinja2
  templates. Separates prompt text from Rust code, enables iteration without recompile.

### Configuration Unification
Audit found **6 distinct config mechanisms** (2 TOML schemas both called `litho.toml`,
2 JSON readers for `qmd.config.json` with incompatible structs, 2 `.env` parsers,
plus CLI flags and raw env vars). Consolidate to 3:

- [ ] Unify `litho-core::LithoConfig` + `litho-generator::Config` into single hierarchy
  (core defines base fields, generator extends with pipeline-specific fields)
- [ ] Merge QMD database config into `litho.toml` under `[qmd]` section
- [ ] Deprecate standalone `qmd.config.json` (provide migration path)
- [ ] One `.env` parser in `litho-core` for `LITHO_*` vars, one in QMD for `QMD_*`

### Deprecation Path
- [ ] Add deprecation warnings when `litho-qmd-cli` is invoked directly
  (point users to `litho search/query/vsearch/index`)
- [ ] Collapse user-facing entrypoint to single `litho` binary over 2 releases

---

## P2: Incremental Mode Hardening

- [ ] **AST-level delta**: use `litho-extract` AST diff (not just file-level git diff)
  to detect function-level changes. Only re-run agents whose input symbols changed.
- [ ] **Doc section merging**: merge incrementally generated sections with existing
  output instead of overwriting. Requires section-level provenance tracking.
- [ ] **DAG-driven targeting**: use `ingestion-dag.json` provenance to replace coarse
  `manifest.input_files` mapping for precise agent re-execution.
- [ ] **Performance validation**: verify <60s for <10% file changes on david-t-martel

---

## P2: Embedding & Search Improvements

### Replace Custom Embedding with `fastembed-rs`
The QMD pipeline currently shells out to external embedding models. `fastembed-rs`
(v5.12, actively maintained) provides:
- Native Rust ONNX inference — no Python/subprocess needed
- GPU acceleration via CUDA/CoreML
- Quantized models (Q8) for low memory
- Text, sparse, and image embeddings

- [ ] Add `fastembed` as optional dependency to `litho-qmd-storage`
- [ ] Implement `EmbeddingProvider` trait backed by fastembed
- [ ] Support `nomic-embed-text` and `all-MiniLM-L6-v2` models out of the box

### Vector Search Acceleration
- [ ] Evaluate `usearch` crate for HNSW approximate nearest neighbor search
  (replaces brute-force cosine similarity in current implementation)

---

## P3: AST Intelligence & Language Expansion

### Pattern Detection (new module)
- [ ] `crates/litho-extract/src/patterns.rs` — detect:
  - Undocumented public APIs (`pub fn/struct/trait` without `///`)
  - Complex async chains (nested `.await` > 3 deep)
  - Builder patterns, state machines, FFI boundaries
- [ ] Wire `DocRequirements` into generator Memory
- [ ] Compute `documentation_debt_score` per module

### Language Expansion
- [ ] Add tree-sitter extractors: Go, Java, C, C++ (4 → 8 languages)
  - Leverage `rust-code-analysis` (Mozilla) for metrics on C/C++/Go/Java
- [ ] Each language: extractor + complexity analyzer + interface parser
- [ ] Longer term: Kotlin, Swift, Ruby, PHP (8 → 12)

### Incremental AST Cache
- [ ] BLAKE3 content hash per file → skip re-parse on unchanged files
- [ ] Store AST snapshots in `.litho/ast_cache/` (avoid re-running tree-sitter)
- [ ] Integrate with `salsa`-style incremental computation for dependency tracking

---

## P3: Output & Polish

- [ ] `litho diff` — compare documentation between two generation runs
- [ ] PDF output via `pandoc` subprocess
- [ ] `--watch` mode — monitor project for changes and auto-regenerate
- [ ] Frontier model support (o3, Claude) for highest-quality generation

---

## Deferred (No Timeline)

- [ ] PostgreSQL 18 migration: `scripts/postgres18-bootstrap.ps1` exists, needs testing
- [ ] Multi-repo orchestration: generate docs across workspace + submodules
- [ ] Gemma3 function calling / coordinator support (blocked on ollama-rs upstream)
- [ ] Two-stage LLM reviewer loop (primary model + reviewer with bounded retries)
- [ ] Template-constrained streaming JSON fitter for partial/malformed responses
- [ ] Redis caching layer (PostgreSQL is primary store)

---

## Crate Candidates (researched 2026-04-01, agent-verified)

### Priority Adoption (P0-P1)

| Crate | Version | Purpose | Impact |
|-------|---------|---------|--------|
| `async-openai` | 0.34 | OpenAI-compat client with retry, Responses API | Replaces ~150 LOC, built-in 429 backoff |
| `wiremock` | 0.6 | HTTP mock server for deterministic LLM tests | Eliminates live-Ollama test dependency |
| `insta` | 1.47 | Snapshot testing with JSON redactions | Catches LLM prompt/output regressions |
| `moka` | 0.12 | Concurrent async cache, stampede prevention | `get_with()` deduplicates parallel LLM calls |
| `fastembed` | 5.13 | Native ONNX embeddings (GPU, no subprocess) | Eliminates 12-45s CLI cold-start latency |
| `futures-concurrency` | 7.7 | `FutureGroup`, `try_join`, `ConcurrentStream` | Structured cancellation, backpressure |
| `rstest` | 0.25 | Parameterized tests, async fixtures | Reduces quality-scoring test boilerplate |

### Medium-Term (P2)

| Crate | Version | Purpose | Impact |
|-------|---------|---------|--------|
| `genai` | 0.5 | 14-provider LLM client | Replaces ~800 LOC provider code |
| `governor` | 0.10 | GCRA rate limiter for LLM APIs | Prevents 429 storms under parallel agents |
| `reqwest-middleware` | latest | Composable retry/timeout middleware | Replaces ad-hoc backoff in providers |
| `minijinja` | 2.7 | Jinja2 prompt templates | Separates prompt text from Rust code |
| `usearch` | 2.24 | HNSW vector index | No-PostgreSQL embedded search |
| `tantivy` | 0.25 | Full-text search engine | Replaces `bm25` crate with phrase/facet support |
| `syn` | 2.0 | Deep Rust AST analysis | Typed AST vs raw tree-sitter for `.rs` files |

### Long-Term (P3)

| Crate | Version | Purpose | Impact |
|-------|---------|---------|--------|
| `salsa` | 0.26 | Dependency-tracked incremental computation | Only recompute changed files (salsa-style) |
| `oxc_parser` | 0.75 | 2-3x faster JS/TS parsing vs tree-sitter | Richer AST for TypeScript extraction |
| `ast-grep-core` | 0.42 | Structural pattern matching on tree-sitter | Replaces manual cursor walks in discovery |
| `rig-core` | 0.33 | Full LLM orchestration (revisit after v0.23 removal) | Test if stack overflow fixed in v0.33 |

## Reference Projects

| Project | URL | Relevance |
|---------|-----|-----------|
| CodePrism | [rustic-ai/codeprism](https://rustic-ai.github.io/codeprism/) | MCP-based code knowledge graph |
| rust-code-analysis | [mozilla/rust-code-analysis](https://github.com/mozilla/rust-code-analysis) | Tree-sitter metrics for C/C++/Go/Java |
| graniet/llm | [graniet/llm](https://github.com/graniet/llm) | Rust multi-backend LLM orchestration |
| rs-graph-llm | [a-agmon/rs-graph-llm](https://github.com/a-agmon/rs-graph-llm) | Graph-based multi-agent workflows |

---

## Completed (Sessions 42-55)

<details>
<summary>Click to expand completed items (Sessions 42-55)</summary>

### Session 55 (2026-04-01)
- [x] Repo sync: 48 modified files + 69 untracked → 9 semantic commits pushed
- [x] Local sccache isolation (.cache/sccache/, port 5100)
- [x] Global cargo config → table-format env vars for per-project overrides
- [x] Build flags: codegen-units=512, MSVC link.exe, DEBUG:FASTLINK
- [x] Gitignore: .litho/benchmark-*, .tmp-*, SQLite caches, proptest regressions
- [x] 3 clippy fixes: manual_clamp, field_reassign_with_default, let-else→?
- [x] Stash dropped (superseded from Session 47)
- [x] README.md + ARCHITECTURE.md rewritten with full CLI reference
- [x] Security audit: qmd.config.json gitignored, credentials scrubbed
- [x] Repo made PUBLIC
- [x] BUILD.md with sccache, codegen-units, CargoTools documentation
- [x] TODO.md reconciled against repo state

### Sessions 42-54
- [x] Build scope rationalization, codex-rs decoupled from default surfaces
- [x] Quality gate with regression detection and enforcement
- [x] 6-dimension quality scoring framework
- [x] Content validator (completeness, accuracy, freshness, grounding)
- [x] OOM-safe tiered build system
- [x] rig-core 0.23 removed → direct reqwest HTTP clients
- [x] Codex-RS wired as primary provider with fallback chain
- [x] QMD SQLite backend + AutoQmdStore backend selection
- [x] Smart file batching for preprocessing
- [x] Token compression (494 LOC, 22 tests)
- [x] ast-grep batch extraction with graceful fallback
- [x] Parallel litho-extract analysis (rayon)
- [x] Incremental mode scaffolding (manifest, change detection, selective agents)
- [x] Benchmark optimization framework
- [x] MCP server with fuzzy name matching
- [x] All Sprint 1-4 implementations (LLM quality, manifest, incremental, HTML output)
- [x] lefthook pre-commit/pre-push hooks
- [x] 540 tests, 0 clippy warnings, full CI pipeline

</details>

---

## Architecture Quick Reference

| Item | Count | Location |
|------|-------|----------|
| Litho crates | 11 | `crates/litho-*/` |
| External codex-rs crates | 52 | `external/codex-rs/` |
| Shipping binaries | 5 | litho, litho-generator, litho-book, litho-qmd-cli, litho-qmd-mcp |
| Tests (litho crates) | 540 | litho-core + litho-extract + litho-generator + litho-cli |
| Repo visibility | PUBLIC | https://github.com/David-Martel/litho-workspace |

## Performance Baseline (david-t-martel, Gemma3 12B-IT-QAT, 2026-02-27)

| Stage | Time | % Total |
|-------|------|---------|
| Preprocessing (127 files) | 5181s | 87.9% |
| Research (8 agents) | 398s | 6.8% |
| Documentation (6 agents) | 312s | 5.3% |
| **Total** | **5892s** | **100%** |
