# Litho Workspace TODOs

Last updated: 2026-03-05 (566-test nextest validation + benchmark smoke integration + native qmd wrappers + ollama/cache guardrails)

## P0: Build Scope & CI Reliability (Quick Wins, 2026-03-04)

- [x] **Remove codex-tui from default Litho build surfaces**
  - [x] Keep `codex-exec`/`codex-core` paths for `litho-codex`, but stop treating full vendored codex-rs workspace as part of `cargo * --workspace` quality gates (`default-members`, `build-tiered.ps1`, `.cargo/.codex` command presets)
  - [x] Update CI build/test/clippy steps to target Litho crates directly (or use `default-members`) instead of full workspace
  - [x] Update `check-workspace.ps1` to validate Litho crates only
- [x] **Track vendored codex-rs compatibility separately**
  - [x] Fix `external/codex-rs/codex-rs/tui/src/markdown_render.rs` for pulldown-cmark 0.13 enum variants (`Tag::BlockQuote(_)`, `TagEnd::BlockQuote(_)`)
  - [x] Add optional vendor-sync/vendor-verify job for codex-rs fork instead of blocking Litho release readiness (`.github/workflows/vendor-codex-verify.yml`)
- [x] **Stabilize CI expectations for database-dependent tests**
  - [x] Ensure `litho-qmd-storage` integration tests only use explicit `QMD_*` envs and fail fast when integration is explicitly requested but DB setup fails
  - [x] Split CI into always-on unit tests and opt-in Postgres integration tests (`workflow_dispatch` input + `postgres-integration` job)
- [x] **Remove remaining `dead_code` warnings from `litho-generator` all-target builds**
  - [x] Binary now consumes library modules (`litho_generator::...`) instead of redeclaring full module tree in `main.rs`
  - [x] `cargo clippy -p litho-generator --all-targets -- -D warnings` passes cleanly

## P0: Testing — Expanded (566 tests passing via nextest package set)

Validated this session:
- `cargo nextest run -p litho-generator -p litho-qmd-mcp -p litho-cli -p litho-extract --no-fail-fast`
- `pwsh -NoProfile -File scripts/benchmark-ollama-optimize.ps1 -ProjectPath . -OutputDir ./.litho/benchmark-smoke -DryRun`
- Result: 566 passed, 0 failed
- Test layers now covered:
  - Unit: parser/query/hint merging + existing extractor/LLM/unit suites
  - Integration: `litho-extract` backend-mode tests (`TreeSitter`, `AstGrep`, fallback when binary missing)
  - E2E: `litho-cli extract` command path with real binary invocation
  - Functional: missing ast-grep binary still yields successful extraction via graceful fallback
Remaining gaps:

- [ ] **Unit tests for Sprint 1 changes:**
  - [x] `original_document_extractor.rs`: CLAUDE.md/CONTRIBUTING.md ingestion, tech stack extraction, trim_markdown heading preservation
  - [x] `structure_extractor.rs`: `is_core` threshold fix (`>=` vs `>`), `tools/` path bonus
  - [x] `ollama_native.rs`: 5-strategy JSON parse cascade, context_window propagation
  - [x] `config.rs`: `context_window` field parsing from litho.toml
- [ ] **Unit tests for Sprint 2-4 changes:**
  - [x] `manifest.rs`: DocumentationManifest round-trip serialization, BLAKE3 hashing
  - [x] `change_detector.rs`: Git diff parsing, affected-agent mapping, >30% threshold
  - [x] `change_detector.rs`: temp-repo git commit integration tests for `manifest_commit..HEAD` behavior and rebuild thresholding
  - [x] `html_outlet.rs`: Markdown-to-HTML conversion, template wrapping
  - [x] `litho-cli`: Subcommand parsing (status, serve, validate, extract, generate)
  - [x] `litho-cli extract`: e2e/functional coverage
  - [x] `litho-cli`: add status/serve/validate/generate parsing and behavior tests
- [ ] **Unit tests for core crates (previously planned):**
  - [x] litho-core: Config parsing, env loading, TOML validation
  - [x] litho-extract: AST extraction per language (Rust, TypeScript, Python, C#)
- [ ] **Integration tests:**
  - [ ] litho-generator pipeline stages (preprocess → research → compose → output)
  - [x] litho-qmd-storage SQLite pipeline + auto-backend defaults
  - [ ] litho-qmd-storage with PostgreSQL 18
  - [ ] CodexRs fallback: Ollama failure triggers codex-rs generation
  - [ ] Incremental mode: full run → small change → verify only affected agents re-run
- [x] **Benchmark framework test integration**
  - [x] Dry-run benchmark optimization path is test-covered in `litho-generator` nextest suite
  - [x] Integration target `benchmark_report_regression` validates report schema fields, gate-failure artifact persistence, and deterministic dry-run ordering
  - [x] CI build lane runs benchmark smoke script to validate CLI optimize/report pipeline wiring
- [x] **MCP function schema validation tests (`litho-qmd-mcp`)**
  - [x] Tool/method suggestion paths (`did you mean ...`) covered
  - [x] Argument type/shape rejection covered
  - [x] Property-based fuzz tests added for parser/validator robustness
- [ ] Target: 600+ tests, 60% coverage (up from 500 tests)
- [ ] Test fixture: `tests/fixtures/david-t-martel-litho.toml` (Gemma3 128K config) available
- [x] Fix `litho-generator` doctest failure in `review_agent.rs` (private module path in example import)

## P0: CLI Contract Correctness (Quick Wins, 2026-03-04)

- [x] **Fix `litho serve` -> `litho-book` invocation mismatch**
  - [x] `litho-cli` now invokes `litho-book` using `--docs-dir <path> --port`
- [x] **Resolve skip-flag contract gap in `litho-generator`**
  - [x] Prevent silent no-op behavior: fail fast when `--skip-*` flags are passed until stage wiring is implemented
  - [x] Remove deprecated `--skip-preprocessing`, `--skip-research`, `--skip-documentation` flags from CLI surface to avoid unsupported/no-op stage contracts
- [x] **Validate quality config at startup**
  - [x] `QualityConfig::validate()` exists; call it in runtime config load path and fail fast on invalid thresholds/weights
- [x] **Add `litho qmd ...` passthrough command**
  - [x] `litho-cli` forwards trailing args to `litho-qmd-cli` and preserves stdio exit behavior
  - [x] Added parse coverage for passthrough argument capture

## P0: Simplification & Product Surface

- [ ] Collapse user-facing entrypoint to `litho` while retaining `litho-qmd-*` as internal/service binaries
- [ ] Add `litho index` thin wrapper that calls qmd functionality without requiring users to know qmd binary names
- [x] Add `litho search|query|vsearch` thin wrappers for common retrieval flows
- [ ] Add deprecation path + warning window for direct `litho-qmd-cli` usage (non-breaking transition)
- [ ] Publish one configuration surface for storage backend selection (`sqlite` repo-local default, optional postgres service mode)
- [ ] Remove duplicated CLI/docs text between litho and qmd crates after wrapper commands land

## P1: Performance — Preprocessing Bottleneck

Pipeline evaluation on david-t-martel (253 files, 44 source, 12 markdown):
- Total: 5892s (~98 min), **Preprocessing: 5181s (87.9%)**
- Research: 398s (6.8%), Documentation: 312s (5.3%)
- Cache hit rate (fresh): 8.9%

- [x] **Token-aware preprocessing** — `token_compress.rs` strips comments + collapses whitespace (~30-40% reduction, 22 tests)
- [x] **max_parallels raised 3→8** — immediate ~65% preprocessing speedup potential
- [x] **Smart file batching** — `code_analyze.rs` groups small files (<3KB source) into batched LLM calls (10 new tests, ~60-80% fewer LLM calls for small files)
- [ ] **Pre-computation cache warming** — hash source files before LLM calls, skip identical files from previous runs
- [ ] **Streaming preprocessing** — start research phase as soon as first files complete (don't wait for all)
- [x] **SIMD-backed text scan acceleration** — add `memchr`/`bytecount` fast paths in source compression + token estimation hot loops

## P1: Quality — Validation & Regression Prevention

Sprint 1 fixes (README trust, grounding constraints, tech stack extraction) dramatically
improved output, but there's no automated way to detect quality regressions.

- [x] **Content validator** — verify generated docs against source truth:
  - [x] Completeness: C1-C4 section presence and minimum length checks
  - [x] Accuracy: code references match actual file paths on disk
  - [x] Freshness: flag stale references to renamed/deleted files
  - [x] Anti-hallucination: tech stack mentions must appear in manifest files (Cargo.toml/package.json/requirements.txt)
- [ ] **Quality scoring** — inspired by david-t-martel's `cv_quality_scorer.py` (47 correction rules):
  - [ ] Terminology consistency across sections
  - [ ] Structural completeness (C1-C4 all present, non-empty)
  - [ ] Evidence grounding score (claims backed by code references)
- [x] **Representation coverage checks** — add file/symbol representation validation (core/source files and extracted symbols referenced in generated docs)
- [ ] **Regression test fixture** — snapshot david-t-martel output as golden reference, diff against future runs
- [ ] **`--min-quality` gate** — CLI flag to fail pipeline if quality score below threshold

## P2: Provider Improvements

### rig-core Removal — COMPLETE (Session 49)
rig-core 0.23 has been fully replaced with direct `reqwest` HTTP clients.
- [x] Replace rig-core 0.23 with direct `reqwest` + `serde` API clients
- [x] Re-enable thin LTO in `.cargo/config.toml`
- [x] Custom `AgentTool` trait replaces `rig::tool::Tool`
- [x] Custom `ChatMessage`/`AssistantContent` types replace `rig::completion::Message`
- [x] Multi-turn tool calling loop implemented directly in `ProviderAgent::multi_turn()`
- [x] OpenAI-compatible, Anthropic, and Gemini API formats all supported
- [x] All 489 tests pass, 0 regressions

### Codex-RS as Primary Provider
- [x] Draft phased codex/ollama/ast optimization plan (`docs/codex-rs-ollama-ast-optimization-plan-2026-03-05.md`)
- [x] Wire codex-rs as selectable primary provider (not just fallback)
- [ ] Enable frontier model usage (o3, Claude) for higher-quality generation
- [x] Add provider selection to litho.toml: `provider = "codex"` | `"ollama"` | `"openai"`
- [x] Fix prompt model routing for non-Ollama providers (`prompt_with_model` now honors requested model)
- [x] Extend codex-rs fallback from extract-only to prompt paths (compose/review resilience)
- [x] Pass explicit model/cwd/schema-json flags through `CodexRsClient` invocation
- [x] Normalize codex binary env vars across crates (`CODEX_BINARY_PATH` vs `CODEX_BIN`)

### ollama-rs Enhancements
- [ ] Coordinator/tool calling support for Gemma3 (function calling API)
- [x] Model auto-detection (query `/api/tags`, pick best available)
- [x] Warm model loading (startup prep with optional pull + warmup)
- [x] Add configurable hard timeout wrappers around native ollama-rs chat/warmup calls
- [x] Add bounded parse/quality-control retry rounds for extraction/generation responses
- [x] Add adaptive `num_ctx` sizing based on prompt/workload size
- [x] Add request-level Ollama tuning knobs (`num_gpu`, `num_thread`, `top_p`, `repeat_penalty`, `keep_alive_seconds`) via `LLMConfig` + env overrides
- [x] Harden JSON object extraction to handle braces inside quoted strings (reduces malformed parse retries)
- [x] Add native Ollama extraction failover chain (primary -> fallback model -> codex fallback when enabled)
- [x] Add optional strict runtime prep mode (`ollama_prepare_runtime_strict`) to fail fast instead of warning-and-continue
- [x] Add optional strict model selection (`ollama_strict_model_selection`) to prevent silent model substitutions
- [x] Add TTL-based `/api/tags` model cache (`ollama_local_models_cache_ttl_seconds`) to balance freshness vs request overhead
- [x] Add explicit Ollama decode budget override (`ollama_num_predict`) for latency/memory control
- [x] Add optional per-call Ollama perf telemetry (`ollama_log_perf_metrics`) using API usage counters for tuning feedback loops
- [x] Import `git-cluster` structured-output pattern: native extraction requests now use `format=StructuredJson` (`JsonStructure`) before parse-fallback cascade
- [x] Add native in-flight request limiter (`ollama_max_in_flight`) to cap concurrent requests and reduce overload-driven latency/memory spikes
- [x] Add extra Ollama decode controls (`top_k`, `repeat_last_n`, `tfs_z`, `seed`) via `LLMConfig` + env vars for tunable throughput/quality tradeoffs
- [x] Add benchmark/optimization framework command (`benchmark-optimize`) with candidate matrix generation, quality+latency+throughput+memory scoring, and JSON/Markdown recommendation reports
- [x] Add hard per-run benchmark timeout (`--run-timeout-seconds`) so stalled workflow runs fail fast and still produce actionable reports
- [x] Add benchmark promotion gates (`--gate-min-success-rate`, `--gate-max-p95-seconds`, `--gate-min-quality`) and fail-fast enforcement
- [x] Track candidate `p95` plus split `cold` vs `incremental` average run durations in benchmark reports
- [x] Add repo-local SQLite repo index primitives (`repo_state` + `file_index`) plus git-diff hints for invalidation planning
- [x] Wire repo-index refresh into full + incremental workflow startup and persist diff plan into memory (`repo_index:diff_plan`)
- [x] Add repo-index-only workflow mode (`litho-generator index-repo`) for first-run/no-LLM indexing
- [x] Add LRU + SQLite layered cache adapter in `CacheManager` (in-memory hot cache + sqlite fallback + file cache compatibility)
- [x] Extend benchmark execution to distinguish cold vs incremental mode (`keep-cache` + multi-run candidate behavior)
- [x] Add startup warmup smoke script (`scripts/startup-warmup-smoke.ps1`) for README readability + CLI/QMD startup probes
- [x] Add timing telemetry for preprocess sub-steps and first successful LLM response (`timing:*` memory keys + summary surfacing)
- [x] Add model-size-aware Qwen context defaults (`<=8b -> 32k`, `<=14b -> 65k`, larger -> 131k) to reduce unnecessary memory pressure
- [x] Harden default native Ollama concurrency (`ollama_max_in_flight` fallback now clamps to safe cap instead of matching max_parallels)
- [x] Add exponential backoff + bounded jitter for Ollama retry delays to reduce synchronized retry storms under load
- [x] Improve adaptive context sizing with token-estimator-based prompt accounting (instead of pure char length)
- [x] Make agent cache keys model/config-aware to prevent stale cross-model/cross-parameter cache reuse
- [x] Add in-flight prompt coalescing lock per cache key to avoid duplicate concurrent LLM calls
- [x] Set benchmark command defaults to stable sampling (`runs_per_candidate=3`, `warmup_runs=1`)
- [x] Wire benchmark smoke validation into CI (`benchmark-ollama-optimize.ps1 -DryRun`) and standard testing docs
- [x] Add opt-in live Ollama benchmark CI lane (`workflow_dispatch` + `run_live_benchmark`) for non-dry-run quality/latency gates
- [ ] Calibrate live benchmark gate defaults by model family/hardware tier to reduce false negatives on slower runners
- [ ] Add optional two-stage Ollama quality-control reviewer loop (primary model + reviewer model with bounded retries)
- [ ] Add template-constrained streaming JSON fitter for partial/malformed long responses

## P2: QMD Backend Strategy (Repo-Local vs Shared Service)

- [x] Add real `SqliteQmdStore` backend (no longer alias-only)
- [x] Add backend selection in qmd CLI/MCP via `AutoQmdStore` (env/config/autodetect)
- [x] Add explicit `--backend` CLI flag (`qmd`, `litho-qmd-mcp`) for deterministic override without env mutation
- [x] Default repo-local mode to `.litho/qmd/<index>.sqlite3` when running inside a git repo
- [x] Default to existing local `.litho/qmd/*.sqlite*` file when present (repo-limited/local workflows)
- [x] Preserve PostgreSQL as optional shared/service backend (`QMD_BACKEND=postgres` or config backend)
- [x] Add cross-backend parity tests for ingest/search/query/get/context/cleanup (SQLite + existing PostgreSQL integration tests)
- [x] Document rollout and migration path (`docs/qmd-repo-local-sqlite-proposal-2026-03-05.md`)

## P2: Incremental Mode — Hardening

Scaffolding exists (`--incremental`, `launch_incremental()`, `ChangeDetector`, `DocumentationManifest`)
but needs real-world hardening.

- [ ] **AST-level delta** — currently file-level git diff only. Add function-level change detection via litho-extract AST comparison
- [x] **Selective agent execution** — `execute_research_pipeline_selective()` and `execute_selective()` skip unaffected agents via `changeset.affected_agents`
- [ ] **Doc merging** — merge incrementally generated sections with existing output (currently overwrites)
- [ ] **Performance target** — verify <60s for <10% file changes on david-t-martel
- [x] **Manifest integrity** — handle corrupt/missing manifest gracefully (fall back to full run)
- [x] **Manifest population** — record `file_hashes` and per-agent `modules` during normal runs
- [x] **Change-ratio robustness** — avoid `full_rebuild_needed` inflation when `manifest.file_hashes` is empty/stale

## P3: AST-Driven Intelligence (Phase 2 Original Plan)

### Pattern-Based Documentation Detection
- [ ] New module: `crates/litho-extract/src/patterns.rs`
- [ ] Detect undocumented public APIs (pub fn/struct/trait without `///`)
- [ ] Detect complex async chains (nested `.await` > 3 deep)
- [ ] Detect state machines, builder patterns, FFI boundaries
- [ ] Wire `DocRequirements` into generator Memory for agent consumption
- [ ] Compute `documentation_debt_score` per module

### Language Expansion
- [ ] Add tree-sitter extractors: Go, Java, C, C++, Kotlin, Swift, Ruby, PHP
- [ ] Each language: extractor + complexity analyzer + interface parser
- [ ] Target: 12 languages total (up from 4)

### Incremental AST Cache
- [ ] BLAKE3 content hashing per file (manifest.rs already has BLAKE3 dep)
- [ ] Only re-parse files whose hash changed
- [ ] Store AST snapshots in `.litho/ast_cache/`

### AST/Walker Acceleration
- [x] Parallelize `litho-extract` file analysis pass with deterministic ordering
- [x] Add ast-grep batch extraction mode (grouped by language/pattern) with graceful degradation when `sg` missing
- [x] Wire walker chunking strategy for large repos (deterministic top-level/language grouping + size caps)
- [x] Build preprocess-time ingestion DAG/RAG artifacts using AST/tree-sitter extraction (`ingestion-dag.json`, memory seeding for research agents)
- [ ] Add tiny-group merge pass for walker batching to reduce underfilled chunks
- [ ] Use ingestion DAG provenance to replace coarse manifest `input_files` mapping for incremental agent targeting

## P3: CLI & Output Polish

### litho-cli Remaining Commands
- [x] `litho qmd` — passthrough bridge to `litho-qmd-cli`
- [x] `litho search` — delegates to qmd retrieval flow via native wrapper path
- [ ] `litho diff` — documentation diff between runs

### Additional Output Formats
- [ ] PDF output via pandoc subprocess
- [ ] DOCX output via pandoc subprocess

## Deferred (No Timeline)

- [ ] **PostgreSQL 18 Migration:** `scripts/postgres18-bootstrap.ps1` exists, needs testing
- [ ] **SIMD Acceleration:** Deferred to after AST cache
- [ ] **AOT Grammar Compilation:** tree-sitter runtime compile is fast enough
- [ ] **Redis Caching:** PostgreSQL is primary store for now
- [ ] **Multi-repo orchestration:** Generate docs across related repos (e.g., workspace + submodules)

---

## Completed

### Session 55 — Manifest integrity fallback + CLI blocker test closure (2026-03-05)
- [x] Updated `DocumentationManifest::load()` to return `Result<Option<_>>`:
  - [x] `Ok(None)` for missing manifest (first incremental run)
  - [x] `Err(...)` for corrupt/unreadable manifest
- [x] Updated `launch_incremental()` to gracefully fall back to full generation on missing/corrupt manifest with explicit diagnostics
- [x] Added `manifest.rs` tests for missing and corrupt manifest behavior (`Ok(None)` vs `Err`)
- [x] Added `litho-cli` parser unit tests for `status`/`serve`/`validate`/`generate`/`extract` enum parsing + invalid value rejection
- [x] Added `litho-cli` e2e/functional tests for:
  - [x] `status` without manifest (guidance path)
  - [x] `status` with manifest (count/report path)
  - [x] `serve` fallback behavior when `litho-book` is unavailable
  - [x] `validate` broken-reference and no-issue paths
  - [x] `generate` fail-fast missing project path + codex-exec readiness failure path
- [x] Validation:
  - [x] `cargo clippy -p litho-cli -p litho-generator --all-targets -- -D warnings`
  - [x] `cargo test -p litho-generator manifest_ -- --nocapture`
  - [x] `cargo test -p litho-cli --all-targets`
  - [x] `cargo nextest run -p litho-cli -p litho-generator --no-fail-fast` (470/470)

### Session 54 — AST-grep backend + dead_code blocker resolution + layered tests (2026-03-05)
- [x] Added `LithoConfig.extract_backend` (`auto|tree_sitter|ast_grep`) and optional `ast_grep_binary` override
- [x] Added CLI flags for extraction backend selection and ast-grep binary override (`litho extract --extract-backend --ast-grep-bin`)
- [x] Implemented ast-grep batch hint backend in `litho-extract` grouped by language/pattern
- [x] Added graceful per-batch fallback to tree-sitter extraction when ast-grep is missing/failing/malformed
- [x] Wired ast-grep hints into extraction output (interface/dependency enrichment with dedupe)
- [x] Resolved `litho-generator` all-target `dead_code` blocker by switching binary entrypoint to library module usage
- [x] Added layered test coverage:
  - [x] Unit: ast-grep stream parsing + hint aggregation/error behavior
  - [x] Integration: backend mode behavior + missing binary fallback + sg-available path
  - [x] E2E/Functional: `litho-cli extract` with backend flags and fallback behavior
- [x] Validation:
  - [x] `cargo clippy -p litho-generator --all-targets -- -D warnings`
  - [x] `cargo clippy -p litho-extract --all-targets -- -D warnings`
  - [x] `cargo clippy -p litho-cli --all-targets -- -D warnings`
  - [x] `cargo test -p litho-core -p litho-extract -p litho-cli -p litho-generator --all-targets`
  - [x] `cargo nextest run -p litho-core -p litho-extract -p litho-generator -p litho-cli --no-fail-fast` (503/503)

### Session 53 — Codex primary routing + Ollama hardening + tests (2026-03-05)
- [x] Codex provider path now preserves requested model through `prompt_with_model` and extractor routing
- [x] Codex prompt fallback added on non-Ollama prompt/reasoning paths when configured (`codex_as_fallback`)
- [x] Codex client now supports explicit default model + working dir + schema-file structured extraction flags
- [x] CODEX env normalization completed across crates (`CODEX_BINARY_PATH` primary, `CODEX_BIN` fallback)
- [x] Native Ollama path now uses bounded retries + timeout wrappers and adaptive context sizing
- [x] `litho-extract` walker batching added (top-level/language grouping with deterministic chunking)
- [x] Test expansion:
  - [x] New codex env precedence tests (`CODEX_BINARY_PATH` vs `CODEX_BIN`)
  - [x] New provider extractor model propagation tests (Codex + function-calling)
  - [x] Env-mutation tests serialized in `litho-core` to remove racey failures
  - [x] All targeted crates pass tests and clippy with `-D warnings`

### Session 52 — Ollama runtime + manifest hardening + extractor parallelism (2026-03-05)
- [x] Native Ollama runtime prep: `/api/tags` model discovery, optional `/api/pull`, optional startup warmup (`prepare_runtime()`)
- [x] `LLMClient` startup provider prep wired into both full and incremental workflow launch paths
- [x] Context window selection now model-aware for native Ollama calls (resolved model-based `num_ctx`)
- [x] Manifest population now records `file_hashes` and module metadata from preprocess + documentation memory outputs
- [x] Change detector ratio logic now handles empty/stale `file_hashes` by falling back to module input files (no forced ratio inflation)
- [x] Agent name normalization improved for incremental selective execution mapping
- [x] `litho-extract` analysis pass parallelized (`rayon`) with deterministic file ordering
- [x] Test expansion: new coverage for ollama model selection helpers, config bool parsing + ollama runtime fields, change-detector tracked-file fallback logic

### Session 51 — Content validator + Markdown fixer (2026-02-28)
- [x] **Content validator** (`validator/mod.rs`, ~300 LOC, 8 tests)
  - 4-dimensional quality checks: completeness, accuracy, freshness, grounding
  - Weighted quality score (completeness 0.30, accuracy 0.30, freshness 0.15, grounding 0.25)
  - Wired into both `launch()` and `launch_incremental()` workflows
  - Regex-based file path extraction, tech stack manifest parsing
- [x] **comrak-based markdown fixer** (`md_fixer.rs`, ~200 LOC, 8 tests)
  - AST-based structural markdown fixes using comrak 0.50
  - Enforce single H1 heading (demote duplicates to H2)
  - Fix empty links (replace `[]()` with `[](#)`)
  - Remove empty heading nodes (LLM artifacts)
  - Audit mermaid blocks and tables in fix report
  - Wired into DiskOutlet and HtmlOutlet (runs before MermaidFixer)
- [x] **pulldown-cmark 0.10→0.13** upgrade (workspace-wide, litho-book unified)
- [x] **Removed unused `markdown` crate** from litho-generator (replaced by comrak)
- [x] 500 litho-generator tests pass, 0 regressions

### Session 50 — Smart file batching (2026-02-27)
- [x] **Smart file batching** in `code_analyze.rs` — files <3KB source grouped into batched LLM calls
- [x] `BatchedCodeInsights` wrapper type for multi-file extraction
- [x] `group_into_batches()` with 50KB source byte budget per batch
- [x] Static analysis runs upfront for all files, then partitions batch vs individual
- [x] Fallback: if LLM returns fewer results than batch size, fills with static analysis
- [x] 10 new unit tests for batching logic (group_into_batches, threshold partitioning)
- [x] 466 litho-generator tests pass, 0 regressions

### Session 49 — rig-core removal + LTO (2026-02-27)
- [x] **rig-core 0.23 fully removed** — replaced with direct reqwest HTTP clients
- [x] New `chat_types.rs` (148 LOC): ChatMessage, AssistantContent, ToolCallInfo, ToolDefinition, ToolChoice, PromptError
- [x] New `AgentTool` trait (tools/mod.rs): async call_json() replacing rig::tool::Tool
- [x] Rewritten `providers.rs` (~500 LOC): ProviderClient (reqwest), ProviderAgent (prompt + multi_turn), ProviderExtractor
- [x] Supports OpenAI-compatible (6 providers), Anthropic, Gemini, CodexRs API formats
- [x] Manual multi-turn tool calling loop in ProviderAgent::multi_turn()
- [x] Function-calling extraction for non-Ollama providers
- [x] Updated: react.rs, react_executor.rs, summary_reasoner.rs, ollama_extractor.rs, agent_builder.rs
- [x] Updated tools: file_explorer.rs, file_reader.rs, time.rs (implement AgentTool)
- [x] **Thin LTO re-enabled** in .cargo/config.toml (no more rustc stack overflow)
- [x] codegen-units = 256 for dev profile (prevents rust-lld OOM on codex-tui)
- [x] Full workspace compiles, 489 tests pass, 0 regressions

### Session 48 — Compilation fixes + Feature work (2026-02-27)
- [x] tree-sitter 0.25 u32/usize type compatibility (4 extractor files)
- [x] litho-codex CodexLibGenerator: spawn_blocking + LocalSet for non-Send run_main
- [x] codex-exec Color re-export for litho-codex access
- [x] Token compression: compress_source_for_llm() (494 LOC, 22 tests)
- [x] OutletKind enum factory replacing hardcoded if/else
- [x] Incremental mode: selective agent execution (research + compose)
- [x] max_parallels 3→8, context_window default, ollama-rs u64 cast
- [x] Warning cleanup: unused imports, dead code, unnecessary mut
- [x] litho-qmd-storage: native-tls + postgres-native-tls dependencies
- [x] Orphaned target-* directories deleted (3.8 GB freed)
- [x] Full workspace compiles (only warnings), 489 tests pass

### Session 47 — TODO audit + rig-core evaluation (2026-02-27)
- [x] Comprehensive TODO.md rewrite with P0-P3 priorities
- [x] rig-core evaluation: KEEP (cost > benefit), already bypassed for Ollama
- [x] nextest infrastructure confirmed (configured in .config/nextest.toml)
- [x] Test count corrected: 489 (was incorrectly reported as 12)
- [x] Preprocessing bottleneck root cause identified (dual sequential LLM calls)

### Session 46 — ollama-rs + Gemma3 Pipeline (2026-02-27)
- [x] Native Ollama provider via ollama-rs 0.3 (`ollama_native.rs`, 339 LOC)
- [x] Configurable `context_window` in litho.toml (default 32768, Gemma3 uses 131072)
- [x] Full pipeline evaluation on david-t-martel (9 docs, ~98 min, Gemma3 12B-IT-QAT)
- [x] Pipeline results context copied (`david-t-martel-pipeline-results-20260227.md`)
- [x] Test fixture added (`tests/fixtures/david-t-martel-litho.toml`)

### Sprints 1-4 Implementation (2026-02-27, sessions 45-46)
- [x] **Sprint 1 — LLM Prompt Quality** (14 files, +403/-36):
  - README trust label fix (`step_forward_agent.rs`)
  - CLAUDE.md + CONTRIBUTING.md ingestion (`original_document_extractor.rs`, +306 LOC)
  - Tech stack extraction from Cargo.toml/pyproject.toml/package.json
  - Heading preservation in trim_markdown
  - Core file detection threshold fix (`>=` vs `>`, `tools/` bonus)
  - Grounding constraints on all research agents (anti-hallucination)
  - Structured analysis prompts for key_modules_insight
  - Anti-fabrication closing instructions on all 6 compose agents
- [x] **Sprint 2 — Documentation Manifest + Change Detection** (4 files, +443 LOC):
  - `manifest.rs` (151 LOC): DocumentationManifest with BLAKE3 hashing
  - `change_detector.rs` (289 LOC): Git diff-based change detection
  - Manifest save wired into workflow.rs after output stage
- [x] **Sprint 3 — Incremental Mode + CLI** (3 files, +328 LOC):
  - `--incremental` flag in cli.rs with `launch_incremental()` in workflow.rs
  - litho-cli: status, serve, validate subcommands
- [x] **Sprint 4 — Multi-Format Output + CI** (2 files, +268 LOC):
  - `html_outlet.rs` (191 LOC): HTML output via pulldown-cmark
  - `--format {md,html}` flag in cli.rs
  - `.github/workflows/docs.yml` (77 LOC): CI doc generation

### Phase 1 Foundation (sessions 43-44)
- [x] CodexRs fallback: provider variant + tiered fallback + config section
- [x] Serde hardening: `serde_helpers.rs` (490 LOC, 6 public deserializers)
- [x] OllamaExtractorWrapper: 5-strategy JSON parsing cascade
- [x] nextest infrastructure + 368 workspace tests passing
- [x] TOML config flexibility (serde defaults on all Config structs)
- [x] excluded_dirs fix (works with sub-paths)
- [x] Research pipeline serde resilience

### Build & Infrastructure (sessions 42-43)
- [x] LTO & binary size optimization (disabled due to rig-core stack overflow)
- [x] CI/CD reliability (linker hang fix, build profile)
- [x] Robustness: validate_readiness checks, binary discovery
- [x] Functional testing (litho-codex, 10 tests)
- [x] Build artifact cleanup (22 target-verify-* dirs removed)
- [x] Unified target directory (.cargo/config.toml target-dir = "target")
- [x] .rgignore for fast search (excludes target/, external/codex-rs/, coverage/)
- [x] .gitignore expanded (target-*, coverage/, logs, editor files)
- [x] Orphaned codex-rs .git pointer fixed (now tracked as regular files)
- [x] CLAUDE.md written for agent instructions
- [x] Development plan documented (docs/plans/2026-02-27-litho-v2-development-plan.md)
- [x] Rust-Native Search Parity: litho-qmd-* crates functional (2,552 LOC storage)

---

## Performance Baseline (david-t-martel, Gemma3 12B-IT-QAT, 2026-02-27)

| Stage | Time | % Total | Notes |
|-------|------|---------|-------|
| Preprocessing (127 files) | 5181s | 87.9% | **Bottleneck — sequential, no token compression** |
| Research (8 agents) | 398s | 6.8% | Parallel agents, grounding constraints applied |
| Documentation (6 agents) | 312s | 5.3% | Anti-fabrication instructions applied |
| **Total** | **5892s** | **100%** | ~98 min, 16.5 GB VRAM (100% GPU) |
| Cache hit rate (fresh) | 8.9% | — | Second run: ~55% hits |

## Architecture Quick Reference

| Item | Count | Location |
|------|-------|----------|
| Litho crates | 11 | `crates/litho-*/` |
| External codex-rs crates | 52 | `external/codex-rs/` |
| Total litho LOC | 32,425 | Across 127 source files |
| Shipping binaries | 5 | litho, litho-generator, litho-book, litho-qmd-cli, litho-qmd-mcp |
| Tests (litho crates) | 500 | litho-core + litho-extract + litho-generator |
| New Sprint 1-4 code | ~1,900 LOC | 5 new files + 22 modified files |
| Session 48 new code | ~1,000 LOC | token_compress, outlet factory, selective agents |
