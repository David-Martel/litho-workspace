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
The preprocessing stage is 87.9% of runtime (5181s/5892s). The pipeline is
strictly sequential: preprocess ALL → research ALL → compose ALL → validate.

- [ ] **Async channel pipeline**: use `tokio::sync::mpsc` to stream preprocessed
  files into research as they complete (don't wait for all files)
- [ ] **Replace `join_all` with `JoinSet`**: modernize from `futures::join_all` +
  `Semaphore` to `tokio::task::JoinSet` for structured concurrency with
  cancellation support
- [ ] **Add `tower` rate-limiting layer** for LLM calls: replace manual `Semaphore`
  in `ollama_native.rs` with `tower::limit::RateLimit` + `tower::retry::Retry`
  for composable middleware (retry, timeout, rate limit in one stack)
- [ ] **Full cache warming pass**: pre-scan all files with BLAKE3 hashing before
  LLM calls, skip those unchanged from prior run (BLAKE3 hash exists in
  `cache/mod.rs` but no pre-scan pass)

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

### Crate Merges (reduce surface area)
Evaluate whether thin crates justify separate compilation units:

- [ ] **Merge `litho-qmd-core` into `litho-qmd-storage`** — `litho-qmd-core` is just
  trait definitions + types. Merging eliminates one crate and simplifies deps.
- [ ] **Merge `litho-qmd-llm` into `litho-qmd-storage`** — query expansion + reranking
  is tightly coupled to storage; separating adds indirection without benefit.
- [ ] **Evaluate `litho-codex` → `litho-generator`** — codex bridge may fold into
  generator's provider abstraction. Keep separate only if used standalone.

Result: 11 crates → 8-9 crates, fewer inter-crate boundaries.

### LLM Client Simplification
The project maintains ~1500 LOC of hand-rolled HTTP client code in `providers.rs`,
`ollama_native.rs`, `codex_provider.rs`. Consider:

- [ ] **Evaluate `genai` crate** (v0.5) — multi-provider library supporting Ollama,
  OpenAI, Anthropic, Gemini, DeepSeek, Groq, Cohere out of the box. Could replace
  most of `providers.rs` and eliminate per-provider format handling.
  Tradeoff: adds a dependency but removes ~800 LOC of API format code.
- [ ] **Evaluate `async-openai`** — mature OpenAI-compatible client. Works with Ollama
  via base URL override. Typed request/response structs, streaming support.
- [ ] If neither fits: at minimum extract `tower::Service` middleware for retry +
  rate limit + timeout (currently hand-rolled in each provider).

### Configuration Unification
Currently 4 config surfaces: `litho.toml`, `.env`, CLI flags, `qmd.config.json`.

- [ ] Merge QMD database config into `litho.toml` under `[qmd]` section
- [ ] Deprecate standalone `qmd.config.json` (provide migration path)
- [ ] Document single config surface in README.md

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

## Crate Candidates (researched 2026-04-01)

| Crate | Version | Purpose | Replaces |
|-------|---------|---------|----------|
| `moka` | 0.12 | Concurrent async cache with TTL/size eviction | Custom LRU in `CacheManager` |
| `fastembed` | 5.12 | Native ONNX embedding generation (GPU) | External embedding subprocess |
| `genai` | 0.5 | Multi-provider LLM client (14 providers) | ~800 LOC in `providers.rs` |
| `tower` | 0.5 | Rate limit + retry + timeout middleware | Manual `Semaphore` + backoff |
| `insta` | 1.x | Snapshot testing for extract output | Manual output comparison |
| `wiremock` | 0.6 | HTTP mock server for LLM client tests | No LLM client tests today |
| `salsa` | 0.3 | Incremental computation framework | Manual cache invalidation |
| `usearch` | latest | HNSW vector index for fast ANN search | Brute-force cosine similarity |
| `tokio-rusqlite` | latest | Async SQLite access | `spawn_blocking` + `rusqlite` |

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
