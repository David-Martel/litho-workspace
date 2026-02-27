# Litho Workspace TODOs

Last updated: 2026-02-27 (post-Session 48 compilation fixes + feature work)

## P0: Testing — Expanded (489 tests passing)

The codebase now has **489 tests across litho-core, litho-extract, litho-generator**.
Previous count of "12" was incorrect — litho-generator alone has 456+ inline tests.
Remaining gaps:

- [ ] **Unit tests for Sprint 1 changes:**
  - [ ] `original_document_extractor.rs`: CLAUDE.md/CONTRIBUTING.md ingestion, tech stack extraction, trim_markdown heading preservation
  - [ ] `structure_extractor.rs`: `is_core` threshold fix (`>=` vs `>`), `tools/` path bonus
  - [ ] `ollama_native.rs`: 5-strategy JSON parse cascade, context_window propagation
  - [ ] `config.rs`: `context_window` field parsing from litho.toml
- [ ] **Unit tests for Sprint 2-4 changes:**
  - [ ] `manifest.rs`: DocumentationManifest round-trip serialization, BLAKE3 hashing
  - [ ] `change_detector.rs`: Git diff parsing, affected-agent mapping, >30% threshold
  - [ ] `html_outlet.rs`: Markdown-to-HTML conversion, template wrapping
  - [ ] `litho-cli`: Subcommand parsing (status, serve, validate, extract, generate)
- [ ] **Unit tests for core crates (previously planned):**
  - [ ] litho-core: Config parsing, env loading, TOML validation
  - [ ] litho-extract: AST extraction per language (Rust, TypeScript, Python, C#)
- [ ] **Integration tests:**
  - [ ] litho-generator pipeline stages (preprocess → research → compose → output)
  - [ ] litho-qmd-storage with PostgreSQL 18
  - [ ] CodexRs fallback: Ollama failure triggers codex-rs generation
  - [ ] Incremental mode: full run → small change → verify only affected agents re-run
- [ ] Target: 600+ tests, 60% coverage (up from 489 tests)
- [ ] Test fixture: `tests/fixtures/david-t-martel-litho.toml` (Gemma3 128K config) available
- [ ] litho-cli and litho-codex test binaries fail to link (codex-rs rlib format issue)

## P1: Performance — Preprocessing Bottleneck

Pipeline evaluation on david-t-martel (253 files, 44 source, 12 markdown):
- Total: 5892s (~98 min), **Preprocessing: 5181s (87.9%)**
- Research: 398s (6.8%), Documentation: 312s (5.3%)
- Cache hit rate (fresh): 8.9%

- [x] **Token-aware preprocessing** — `token_compress.rs` strips comments + collapses whitespace (~30-40% reduction, 22 tests)
- [x] **max_parallels raised 3→8** — immediate ~65% preprocessing speedup potential
- [ ] **Smart file batching** — group small files into single LLM calls (currently 1 call per file regardless of size)
- [ ] **Smart file batching** — group small files into single LLM calls (currently 1 call per file regardless of size)
- [ ] **Pre-computation cache warming** — hash source files before LLM calls, skip identical files from previous runs
- [ ] **Streaming preprocessing** — start research phase as soon as first files complete (don't wait for all)

## P1: Quality — Validation & Regression Prevention

Sprint 1 fixes (README trust, grounding constraints, tech stack extraction) dramatically
improved output, but there's no automated way to detect quality regressions.

- [ ] **Content validator** — verify generated docs against source truth:
  - [ ] Completeness: all public APIs mentioned (cross-reference litho-extract AST)
  - [ ] Accuracy: code references match actual file paths on disk
  - [ ] Freshness: flag stale references to renamed/deleted files
  - [ ] Anti-hallucination: tech stack mentions must appear in manifest files
- [ ] **Quality scoring** — inspired by david-t-martel's `cv_quality_scorer.py` (47 correction rules):
  - [ ] Terminology consistency across sections
  - [ ] Structural completeness (C1-C4 all present, non-empty)
  - [ ] Evidence grounding score (claims backed by code references)
- [ ] **Regression test fixture** — snapshot david-t-martel output as golden reference, diff against future runs
- [ ] **`--min-quality` gate** — CLI flag to fail pipeline if quality score below threshold

## P2: Provider Improvements

### rig-core Migration (DEFERRED — cost > benefit for now)
rig-core 0.23 is used in 8 files (~1,850 LOC, 6% of litho-generator). Already bypassed
for Ollama via `ollama_native.rs`. Removal would require 600-800 LOC replacement, 3-5 days.
LTO is disabled due to rustc stack overflow in rig-core.
- [ ] Replace rig-core 0.23 with direct `reqwest` + `serde` API clients (when capacity allows)
- [ ] Re-enable LTO in `.cargo/config.toml` after rig-core removal

### Codex-RS as Primary Provider
- [ ] Wire codex-rs as selectable primary provider (not just fallback)
- [ ] Enable frontier model usage (o3, Claude) for higher-quality generation
- [ ] Add provider selection to litho.toml: `provider = "codex"` | `"ollama"` | `"openai"`

### ollama-rs Enhancements
- [ ] Coordinator/tool calling support for Gemma3 (function calling API)
- [ ] Model auto-detection (query /api/tags, pick best available)
- [ ] Warm model loading (pre-pull before pipeline start)

## P2: Incremental Mode — Hardening

Scaffolding exists (`--incremental`, `launch_incremental()`, `ChangeDetector`, `DocumentationManifest`)
but needs real-world hardening.

- [ ] **AST-level delta** — currently file-level git diff only. Add function-level change detection via litho-extract AST comparison
- [x] **Selective agent execution** — `execute_research_pipeline_selective()` and `execute_selective()` skip unaffected agents via `changeset.affected_agents`
- [ ] **Doc merging** — merge incrementally generated sections with existing output (currently overwrites)
- [ ] **Performance target** — verify <60s for <10% file changes on david-t-martel
- [ ] **Manifest integrity** — handle corrupt/missing manifest gracefully (fall back to full run)

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

## P3: CLI & Output Polish

### litho-cli Remaining Commands
- [ ] `litho search` — delegates to litho-qmd-cli
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
| Tests (litho crates) | 489 | litho-core + litho-extract + litho-generator |
| New Sprint 1-4 code | ~1,900 LOC | 5 new files + 22 modified files |
| Session 48 new code | ~1,000 LOC | token_compress, outlet factory, selective agents |
