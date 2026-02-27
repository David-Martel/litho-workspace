# Litho Workspace TODOs

## Phase 1: Foundation & Robustness (Active)

### Codex-RS Fallback
- [ ] Add `CodexRs` variant to `ProviderClient` in litho-generator
- [ ] Wire tiered fallback: local LLM → fallover model → codex-rs emergency
- [ ] Add `[llm.codex_fallback]` config section to litho-core
- [ ] Integration test: Ollama failure triggers codex-rs generation

### LLM Failure Recovery
- [ ] Port deepwiki-rs patches 7-9: serde defaults on all deserialized types
- [ ] Add string-or-object coercion deserializer
- [ ] Implement regex fallback extraction when JSON parse fails
- [ ] New module: `crates/litho-generator/src/llm/recovery.rs`

### Testing Infrastructure
- [ ] Set up cargo-nextest as primary test runner
- [ ] Unit tests for litho-core (config parsing, env loading)
- [ ] Unit tests for litho-extract (AST extraction per language)
- [ ] Integration tests for litho-generator pipeline stages
- [ ] Integration tests for litho-qmd-storage (PostgreSQL)
- [ ] Target: 100+ tests, 40% coverage

## Phase 2: AST-Driven Intelligence

### Pattern-Based Documentation Detection
- [ ] New module: `crates/litho-extract/src/patterns.rs`
- [ ] Detect undocumented public APIs (pub fn/struct/trait without `///`)
- [ ] Detect complex async chains (nested `.await` > 3 deep)
- [ ] Detect state machines, builder patterns, FFI boundaries
- [ ] Wire `DocRequirements` into generator Memory for agent consumption
- [ ] Compute documentation_debt_score per module

### Language Expansion
- [ ] Add tree-sitter extractors: Go, Java, C, C++, Kotlin, Swift, Ruby, PHP
- [ ] Each language: extractor + complexity analyzer + interface parser
- [ ] Target: 12 languages total (up from 4)

### Incremental AST Cache
- [ ] BLAKE3 content hashing per file
- [ ] Only re-parse files whose hash changed
- [ ] Store AST snapshots in `.litho/ast_cache/`

## Phase 3: CI/CD Integration

### Change Detection
- [ ] New module: `crates/litho-generator/src/integrations/change_detector.rs`
- [ ] Git diff-based file change detection
- [ ] AST delta computation (new/modified/removed functions)
- [ ] Identify affected documentation modules

### Incremental Mode
- [ ] `--incremental` flag for litho-generator
- [ ] Load previous `DocumentationManifest` from `.litho/manifest.json`
- [ ] Selective agent execution (skip unaffected modules)
- [ ] Merge new docs with previous output
- [ ] Target: PR feedback in <60 seconds

### GitHub Actions
- [ ] New workflow: `.github/workflows/docs.yml`
- [ ] Full generation on main, incremental on PRs
- [ ] Upload docs as build artifact
- [ ] Quality gate: `--min-quality 0.7`

## Phase 4: Quality & Polish

### Content Validation
- [ ] Completeness: all public APIs mentioned in docs
- [ ] Consistency: terminology alignment across sections
- [ ] Accuracy: code references match actual file paths
- [ ] Freshness: flag stale references to renamed/deleted files

### Multi-Format Output
- [ ] HTML output via pulldown-cmark rendering
- [ ] PDF output via pandoc subprocess
- [ ] DOCX output via pandoc subprocess

### rig-core Migration
- [ ] Replace rig-core 0.23 with direct reqwest + serde API clients
- [ ] Native anthropic crate for Claude support
- [ ] Remove rig dependency entirely
- [ ] Reduce compile time and binary size

### litho-cli Expansion
- [ ] `litho serve` — delegates to litho-book
- [ ] `litho search` — delegates to litho-qmd-cli
- [ ] `litho validate` — content quality checks
- [ ] `litho diff` — documentation diff
- [ ] `litho status` — documentation freshness and coverage

## Completed

- [x] LTO & binary size optimization
- [x] CI/CD reliability (linker hang fix, build profile)
- [x] Robustness: validate_readiness checks, binary discovery
- [x] Functional testing (litho-codex)
- [x] Build artifact cleanup (22 target-verify-* dirs removed, 2026-02-27)
- [x] Unified target directory (.cargo/config.toml target-dir = "target")
- [x] .rgignore for fast search (excludes target/, external/codex-rs/, coverage/)
- [x] .gitignore expanded (target-*, coverage/, logs, editor files)
- [x] Orphaned codex-rs .git pointer fixed (now tracked as regular files)
- [x] CLAUDE.md written for agent instructions
- [x] Development plan documented (docs/plans/2026-02-27-litho-v2-development-plan.md)

## In-Progress Transitions (Legacy — Being Replaced by Phase Plan)

- [x] **Rust-Native Search Parity:** litho-qmd-* crates functional (2,552 LOC storage)
- [ ] **PostgreSQL 18 Migration:** `scripts/postgres18-bootstrap.ps1` exists, needs testing
- [ ] **Model Alignment:** Addressed by Phase 1 codex-rs fallback architecture
- [ ] **SIMD Acceleration:** Deferred to Phase 2 (after AST cache)
- [ ] **Incremental Indexing:** Addressed by Phase 3 change detection
- [ ] **AOT Grammar Compilation:** Deferred (tree-sitter runtime compile is fast enough)
- [ ] **Redis Caching:** Deferred (PostgreSQL is primary store for now)
