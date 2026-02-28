# Litho Workspace Context — Session 51 (2026-02-28)

## Project State

- **Branch:** main @ 018e188
- **Tests:** 645 passing (litho-core + litho-extract + litho-generator)
- **Clippy:** Clean with `-D warnings` enforced via pre-commit hook
- **Commits this session:** 5 pushed to David-Martel/litho-workspace (private)

## Recent Changes (Session 51)

### 1. Content Validator (`generator/validator/mod.rs`)
- 4-dimensional quality checks: completeness, accuracy, freshness, grounding
- Weighted scoring with configurable thresholds
- Wired into both `launch()` and `launch_incremental()` in workflow.rs
- 8 unit tests

### 2. Comrak Markdown Fixer (`generator/outlet/md_fixer.rs`)
- AST-based structural markdown fixes using comrak 0.50
- Enforces single H1, fixes empty links, removes empty headings
- Audits mermaid blocks and tables
- Runs before both disk write and HTML conversion
- Replaced unused `markdown` crate dependency
- pulldown-cmark unified to 0.13 across workspace (kept for HTML rendering only)
- 8 unit tests

### 3. BLAKE3 Content-Hash Cache Warming (`cache/mod.rs`)
- Source file hash → cached CodeInsight lookup
- Bypasses prompt-keyed cache entirely (resilient to template/config changes)
- Integrated into CodeAnalyze::execute() Phase 0 (check) + Phase 6 (store)
- 3 unit tests

### 4. Sprint 1-4 Unit Tests (93 tests)
- manifest.rs: 9 tests (round-trip serialization, BLAKE3 hashing)
- change_detector.rs: 8 tests (ChangeSet helpers, affected-agent mapping)
- original_document_extractor.rs: 18 tests (trim_markdown, dep parsing)
- structure_extractor.rs: 17 tests (is_core threshold, file scoring)
- Found and fixed real bug: pyproject dep parser trailing quote

### 5. Pre-commit Hooks & Clippy Clean
- lefthook.yml: pre-commit (fmt-check + clippy -D warnings), pre-push (nextest)
- Fixed sccache conflict: removed CARGO_INCREMENTAL=1 from local config
- Fixed 25+ categories of clippy warnings across 82 files
- All 11 litho crates pass clippy with warnings-as-errors

## Architecture Summary

```
litho-generator 4-stage pipeline:
  Preprocessing → Research → Composition → Verification

LLM Providers (providers.rs ~500 LOC):
  OpenAI-compatible (6) | Anthropic | Gemini | CodexRs

Memory: Arc<RwLock<Memory>> DAG with scoped keys
Cache: CacheManager (prompt-keyed MD5 + content-hash BLAKE3)
Outlets: Markdown | HTML | Summary (via OutletKind enum dispatch)
```

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| comrak over markdown-rs | AST-based parse→transform→render; markdown-rs has no serializer |
| Keep pulldown-cmark for HTML | Event-stream parser ideal for HTML rendering; comrak for AST transforms |
| BLAKE3 for content hashing | Already in deps, fast, deterministic |
| lefthook over husky | Polyglot support, cargo-native, no Node dependency |
| Remove CARGO_INCREMENTAL=1 | Must be 0 for sccache compatibility |

## Agent Registry (Session 51)

| Agent | Task | Files | Status |
|-------|------|-------|--------|
| rust-pro | Fix all clippy -D warnings | 82 files across 6 crates | Complete |
| rust-pro | Write 93 Sprint 1-4 unit tests | 4 test files | Complete |
| Explore | Research markdown crates | — | Complete |
| Explore | Survey existing markdown processing | — | Complete |

## codex-rs Integration Notes

- **Location:** `external/codex-rs/` — tracked as regular files (NOT a submodule)
- **Upstream:** openai/codex (forked with local modifications)
- **Key modifications:** Custom patches for litho integration
- **2707 tracked files** in the main codex-rs checkout
- **Nested duplicate:** `external/codex-rs/codex-rs/external/codex-rs/` exists with
  174 untracked `.snap.new` files — these are test artifacts, now in .gitignore
- **Strategy:** Selectively pull upstream changes that don't conflict with local customizations

## Remaining Work

### Immediate
- Quality scoring system (Task #12): terminology consistency, structural completeness, --min-quality gate
- Streaming preprocessing optimization
- Provider improvements (retry logic, rate limiting)

### Tech Debt
- `imports_granularity = Item` in rustfmt.toml requires nightly — consider removing
- litho-cli and litho-codex test binaries fail to link (codex-rs rlib format issue)
- codex-tui OOMs during linking — codegen-units=256 workaround in place

## Build Commands

```bash
# Standard workflow
cargo build --workspace --release
cargo nextest run -p litho-core -p litho-extract -p litho-generator --no-fail-fast
cargo clippy -p litho-core -p litho-extract -p litho-generator -p litho-book \
  -p litho-cli -p litho-codex -p litho-qmd-core -p litho-qmd-storage \
  -p litho-qmd-llm -p litho-qmd-mcp -p litho-qmd-cli --all-targets -- -D warnings

# Pre-commit hooks auto-run via lefthook
# Skip hooks: LEFTHOOK=0 git commit ...
```
