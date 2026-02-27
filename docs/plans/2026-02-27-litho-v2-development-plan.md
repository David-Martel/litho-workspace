# Litho v2.0 Development Plan

**Date:** 2026-02-27
**Author:** claude-opus-4.6 + David T. Martel
**Branch:** main
**Status:** ACTIVE

## Vision

Build a high-performance, automated documentation machine with intelligence and
robustness to failure, even for large repositories (10K+ files, 50K+ LOC). The system
combines local LLM inference for incremental CI/CD tracking with cloud-scale LLM
(codex-rs) for initial heavy-lifting generation — all orchestrated through AST-aware
pattern detection that identifies *where documentation is needed*, not just metadata.

## Architecture Overview

```
                    ┌─────────────────────────────────────┐
                    │         litho-cli (entry point)      │
                    │   extract | generate | serve | search │
                    └──────────┬──────────────────────────┘
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                 ▼
    ┌─────────────┐  ┌─────────────────┐  ┌──────────┐
    │litho-extract│  │litho-generator  │  │litho-book│
    │ (tree-sitter│  │ (4-stage pipe)  │  │ (web UI) │
    │  AST parse) │  │                 │  └──────────┘
    └──────┬──────┘  │ Preprocess      │
           │         │   ↓ AST patterns│
           │         │ Research        │
           │         │   ↓ multi-agent │
           │         │ Composition     │
           │         │   ↓ doc gen     │
           │         │ Verification    │
           │         └────────┬────────┘
           │                  │
           │     ┌────────────┴────────────┐
           │     ▼                         ▼
           │  ┌──────────┐        ┌────────────┐
           │  │Local LLM │───X───▶│ codex-rs   │
           │  │(Ollama)  │fallback│ (OpenAI)   │
           │  └──────────┘        └────────────┘
           │
    ┌──────┴──────────────────────────────┐
    │       litho-qmd-* (5 crates)        │
    │  core | storage | llm | mcp | cli   │
    │     PostgreSQL + BM25 + vectors     │
    └─────────────────────────────────────┘
```

## Current State (2026-02-27)

| Crate | Lines | Files | Status |
|-------|------:|------:|--------|
| litho-generator | 22,034 | 90 | Functional — full 4-stage pipeline |
| litho-qmd-storage | 2,552 | 2 | Functional — PostgreSQL BM25 + vectors |
| litho-book | 2,005 | 6 | Functional — Axum web server |
| litho-extract | 1,467 | 12 | Functional — tree-sitter (4 languages) |
| litho-qmd-cli | 932 | 1 | Functional — full CLI |
| litho-qmd-mcp | 782 | 1 | Functional — MCP server |
| litho-qmd-llm | 732 | 1 | Functional — adaptive LLM reranking |
| litho-qmd-core | 606 | 5 | Functional — shared types/traits |
| litho-codex | 572 | 4 | Functional — Codex subprocess + in-process |
| litho-core | 481 | 4 | Functional — config, env, types |
| litho-cli | 262 | 1 | Minimal — only extract + generate |
| **Total** | **32,425** | **127** | |

**Binaries built (Feb 25):** litho.exe (11MB), litho-generator.exe (10MB),
litho-book.exe (3.5MB), litho-qmd-cli.exe (3.7MB), litho-qmd-mcp.exe (3.6MB)

**External:** codex-rs (52 crates, customized fork) integrated as workspace members.

**Tests:** 9 tests in litho-codex only. No other crates have tests.

## Phase 1: Foundation & Robustness (Week 1)

### 1.1 Codex-RS Fallback Architecture

**Goal:** Local LLM (Ollama) is primary for CI/CD speed; codex-rs is fallback for
initial generation and when local LLM fails.

**Implementation:**

Add `CodexRs` variant to `ProviderClient` in `src/llm/client/providers.rs`:
```rust
pub enum ProviderClient {
    // ... existing 8 providers ...
    CodexRs(CodexRsClient),
}
```

Add tiered fallback in `LLMClient::extract_inner()` (`src/llm/client/mod.rs`):
```
Primary model (Ollama/local) → Fallback model (config) → Codex-RS (emergency)
```

Config addition (`litho.toml`):
```toml
[llm]
provider = "ollama"
model_efficient = "qwen2.5-coder:7b"
model_powerful = "qwen2.5-coder:7b"
fallover_model = "gpt-4o-mini"

[llm.codex_fallback]
enabled = true
timeout_ms = 120000
sandbox = "read-only"
model = "o4-mini"
```

**Files to modify:**
- `crates/litho-generator/src/llm/client/providers.rs` — add CodexRs variant
- `crates/litho-generator/src/llm/client/mod.rs` — add codex fallback path
- `crates/litho-core/src/config.rs` — add CodexFallbackConfig
- `crates/litho-codex/src/lib.rs` — expose client for generator consumption

**Effort:** 2 days

### 1.2 LLM Failure Recovery (Patches 7-9 Port)

**Goal:** When LLM returns malformed JSON, fall back to regex/heuristic extraction
instead of crashing.

The standalone deepwiki-rs had 9 patches for serde hardening. litho-generator
lacks these. Port the key patterns:

1. **Serde defaults for all deserialized types** — `#[serde(default)]` on every
   struct field that can be absent
2. **String-or-object coercion** — `#[serde(deserialize_with = "string_or_struct")]`
   for fields that LLMs sometimes return as plain strings
3. **Regex fallback extraction** — When JSON parse fails, extract key content
   via regex patterns (`r#"## .*"#`, `r#"\*\*.*\*\*"#`, etc.)

**Files to modify:**
- `crates/litho-generator/src/types/*.rs` — add serde defaults
- `crates/litho-generator/src/llm/client/mod.rs` — add regex fallback
- New: `crates/litho-generator/src/llm/recovery.rs` — recovery utilities

**Effort:** 1.5 days

### 1.3 Testing Infrastructure

**Goal:** Establish test framework with cargo-nextest across all crates.

```powershell
# Run all tests with nextest (parallel, better output)
cargo nextest run --workspace --no-fail-fast

# Coverage with llvm-cov
cargo llvm-cov nextest --workspace --html --output-dir coverage/
```

Test targets per crate:

| Crate | Test Type | Target |
|-------|-----------|--------|
| litho-core | Unit | Config parsing, env loading |
| litho-extract | Unit + Integration | AST extraction per language, dependency graph |
| litho-codex | Unit + Integration | Prompt building, readiness checks, generator trait |
| litho-generator | Unit + Integration | Pipeline stages, agent execution, recovery |
| litho-qmd-core | Unit | Service trait, error handling |
| litho-qmd-storage | Integration | PostgreSQL operations, BM25 search |
| litho-qmd-llm | Unit | Query expansion, reranking |
| litho-qmd-mcp | Integration | MCP protocol, tool dispatch |
| litho-qmd-cli | Integration | CLI commands, output formatting |
| litho-book | Integration | Web routes, markdown rendering |

**Effort:** 3 days

## Phase 2: AST-Driven Intelligence (Week 2)

### 2.1 Pattern-Based Documentation Detection

**Goal:** Use tree-sitter to identify *functional patterns where documentation is
needed* — not just metadata extraction but semantic analysis of code structure.

**New module:** `crates/litho-extract/src/patterns.rs`

Detect these patterns across all supported languages:

| Pattern | Detection Method | Documentation Need |
|---------|-----------------|-------------------|
| Undocumented public APIs | AST: pub fn/struct/trait without `///` comments | API reference |
| Complex async chains | AST: nested `.await` calls > 3 deep | Flow documentation |
| Error handling paths | AST: `?` operator chains, match on Result/Option | Error catalog |
| Trait implementations | AST: `impl Trait for Type` blocks | Interface docs |
| State machines | AST: enum with transition methods | State diagram |
| Builder patterns | AST: method chains returning `Self` | Usage examples |
| Critical thresholds | AST: numeric constants, config values | Configuration guide |
| FFI boundaries | AST: `extern "C"`, `#[no_mangle]`, PyO3 | Interop docs |
| Database queries | AST: SQL string literals, sqlx macros | Schema docs |
| Test patterns | AST: `#[test]`, `#[tokio::test]` | Test coverage map |

**Implementation:**
```rust
pub struct PatternAnalyzer {
    parsers: HashMap<Language, tree_sitter::Parser>,
}

impl PatternAnalyzer {
    /// Analyze a codebase and return documentation requirements
    pub fn analyze(&self, project: &Path) -> Result<DocRequirements> {
        // Walk files, parse ASTs, detect patterns
        // Return prioritized list of documentation needs
    }
}

pub struct DocRequirements {
    pub undocumented_apis: Vec<ApiSignature>,
    pub complex_flows: Vec<FlowPath>,
    pub error_catalogs: Vec<ErrorPath>,
    pub state_machines: Vec<StateMachine>,
    pub documentation_debt_score: f64,  // 0.0 (well-documented) to 1.0 (needs work)
}
```

**Wire into generator:** Store `DocRequirements` to Memory after preprocessing.
Research agents reference it to focus LLM attention on areas needing documentation.

**Effort:** 3 days

### 2.2 Expand Tree-Sitter Language Support

Current: Rust, TypeScript, Python, C#
Add: Go, Java, C, C++, Kotlin, Swift, Ruby, PHP

Use `tree-sitter-*` crates (already available via crates.io). Each language gets
an extractor implementing the `Extractor` trait in litho-extract.

**Effort:** 2 days

### 2.3 Incremental AST Cache

Store parsed AST snapshots per-file with content hash:
```rust
pub struct AstCache {
    entries: HashMap<PathBuf, AstCacheEntry>,
}

pub struct AstCacheEntry {
    content_hash: [u8; 32],  // BLAKE3
    parsed_at: DateTime<Utc>,
    interfaces: Vec<Interface>,
    patterns: Vec<DetectedPattern>,
}
```

On subsequent runs, only re-parse files whose content hash changed. This makes
AST analysis O(changed files) instead of O(all files).

**Effort:** 1 day

## Phase 3: CI/CD Integration (Week 3)

### 3.1 Change Detection Module

**New:** `crates/litho-generator/src/integrations/change_detector.rs`

```rust
pub struct ChangeDetector;

impl ChangeDetector {
    /// Detect changes since last documentation run
    pub async fn detect_changes(
        config: &Config,
        manifest: &DocumentationManifest,
    ) -> Result<ChangeSet> {
        // 1. git diff --name-only <last-manifest-commit>..HEAD
        // 2. Parse AST delta (current vs cached) for changed files
        // 3. Identify affected documentation modules
    }
}

pub struct ChangeSet {
    pub changed_files: Vec<PathBuf>,
    pub ast_delta: AstDelta,
    pub affected_doc_sections: Vec<String>,
}
```

### 3.2 Incremental Documentation Mode

Add `--incremental` flag to litho-generator:
- Load previous `DocumentationManifest` from `.litho/manifest.json`
- Run change detection
- Only execute research agents for affected modules
- Merge new docs with previous output
- Update manifest

**Performance target:** PR documentation feedback in <60 seconds (vs 10-30 min full).

### 3.3 Documentation Manifest

```rust
pub struct DocumentationManifest {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub git_commit: String,
    pub modules: HashMap<String, ModuleDoc>,
    pub ast_cache_hash: String,
}
```

Saved after every generation run. Enables:
- Delta computation on next run
- Staleness detection in CI
- Documentation coverage tracking over time

### 3.4 GitHub Actions Workflow

**New:** `.github/workflows/docs.yml`

```yaml
on:
  push:
    branches: [main]
  pull_request:

jobs:
  docs:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Generate docs
        run: |
          if [ "${{ github.event_name }}" == "pull_request" ]; then
            litho-generator --incremental --project-path .
          else
            litho-generator --project-path .
          fi
      - name: Validate docs
        run: litho-generator validate --min-quality 0.7
      - name: Upload docs artifact
        uses: actions/upload-artifact@v4
        with:
          name: litho-docs
          path: litho.docs/
```

**Effort:** 3 days total (3.1 + 3.2 + 3.3 + 3.4)

## Phase 4: Quality & Polish (Week 4)

### 4.1 Content Validation

Extend verification phase beyond Mermaid diagrams:
- **Completeness:** All public APIs mentioned in docs
- **Consistency:** Terminology alignment across sections
- **Accuracy:** Code references match actual file paths
- **Freshness:** Flag stale references (renamed/deleted files)

### 4.2 Multi-Format Output

Add output formats via Outlet trait implementations:
- Markdown (existing)
- HTML (via pulldown-cmark rendering)
- PDF (via pandoc subprocess)
- DOCX (via pandoc subprocess)

### 4.3 LLM Provider Migration

Replace rig-core 0.23 (legacy) with direct API clients:
- `reqwest` + `serde_json` for OpenAI-compatible endpoints
- `anthropic` crate for Claude
- Remove rig dependency entirely

This reduces compile time and binary size while improving provider support.

### 4.4 litho-cli Expansion

Add commands:
- `litho serve <docs-dir>` — delegates to litho-book
- `litho search <query>` — delegates to litho-qmd-cli
- `litho validate <docs-dir>` — content quality checks
- `litho diff <old-docs> <new-docs>` — documentation diff
- `litho status` — show documentation freshness and coverage

**Effort:** 4 days total

## Build System

### CargoTools Integration

All builds use the CargoTools PowerShell module:

```powershell
Import-Module CargoTools

# Development build
Invoke-CargoBuild -Path "C:\codedev\litho-workspace" -UseLld -FixSqlite

# Release build
Invoke-CargoBuild -Path "C:\codedev\litho-workspace" -Release -UseLld -FixSqlite

# Environment check
Get-CargoEnvironment
Test-RustToolchain
```

### Build Accelerators

| Tool | Config Location | Purpose |
|------|-----------------|---------|
| sccache | `.cargo/config.toml` `[build].rustc-wrapper` | Compilation cache |
| rust-lld | `.cargo/config.toml` `[target.*.linker]` | 10-20% faster linking |
| cargo-nextest | `scripts/qmd-quality.ps1` | Parallel test execution |
| cargo-binstall | manual | Binary caching for tool installs |
| cargo-deny | CI workflow | License + vulnerability audit |
| cargo-audit | CI workflow | CVE scanning |
| cargo-watch | development | Auto-rebuild on save |
| thin LTO | `.cargo/config.toml` `[profile.release]` | Optimized release builds |
| native CPU | `.cargo/config.toml` `[target.*.rustflags]` | Architecture-specific opts |

### Unified Target Directory

All builds output to `target/` (configured in `.cargo/config.toml`).
Scripts MUST NOT create `target-*` directories. The `qmd-quality.ps1`,
`qmd-bench.ps1`, and `qmd-coverage.ps1` scripts need updating to use
`target/` with cargo profiles instead of separate target directories.

### Recommended Build Commands

```powershell
# Full workspace build
cargo build --workspace --release

# Test with nextest
cargo nextest run --workspace --no-fail-fast

# Clippy (warnings as errors)
cargo clippy --workspace --all-targets -- -D warnings

# Coverage
cargo llvm-cov nextest --workspace --html --output-dir coverage/

# Benchmarks
cargo bench -p litho-qmd-core -p litho-qmd-storage

# Security audit
cargo deny check && cargo audit
```

## File Structure (Target State)

```
litho-workspace/
├── .cargo/config.toml          # Build config (sccache, lld, native CPU)
├── .github/workflows/
│   ├── ci.yml                  # Build + test + clippy
│   ├── docs.yml                # Documentation generation (NEW)
│   └── release.yml             # Binary release on tag
├── .gitignore
├── .rgignore                   # Ripgrep/fd search exclusions
├── Cargo.toml                  # Workspace manifest (63 members)
├── Cargo.lock
├── CLAUDE.md                   # Agent instructions (NEW)
├── TODO.md                     # Active task tracking
├── GEMINI.md                   # Architecture reference
├── crates/
│   ├── litho-core/             # Config, env, shared types
│   ├── litho-extract/          # Tree-sitter AST + pattern detection
│   ├── litho-codex/            # Codex-RS bridge (subprocess + library)
│   ├── litho-generator/        # 4-stage documentation pipeline
│   ├── litho-book/             # Web documentation viewer
│   ├── litho-cli/              # Unified CLI entry point
│   ├── litho-qmd-core/         # QMD shared types + service trait
│   ├── litho-qmd-storage/      # PostgreSQL storage + BM25
│   ├── litho-qmd-llm/          # LLM query expansion + reranking
│   ├── litho-qmd-mcp/          # MCP server for agent access
│   └── litho-qmd-cli/          # QMD CLI
├── external/
│   └── codex-rs/               # Customized OpenAI Codex fork (52 crates)
├── scripts/
│   ├── postgres18-bootstrap.ps1
│   ├── qmd-quality.ps1
│   ├── qmd-coverage.ps1
│   └── qmd-bench.ps1
├── docs/
│   ├── plans/                  # Development plans
│   └── litho-internal-docs/    # Generated self-documentation
├── target/                     # Single unified build directory
└── .litho/
    ├── cache/                  # LLM response cache
    └── manifest.json           # Documentation generation manifest
```

## Dependency Graph (Crate Level)

```
litho-cli
├── litho-extract
│   └── litho-core
├── litho-codex
│   ├── litho-core
│   └── litho-extract
├── litho-generator
│   ├── litho-core
│   ├── litho-extract (Phase 2: AST patterns)
│   └── litho-codex (Phase 1: fallback)
└── litho-book
    └── litho-core

litho-qmd-cli
├── litho-qmd-core
├── litho-qmd-storage
│   └── litho-qmd-core
└── litho-qmd-llm
    └── litho-qmd-core

litho-qmd-mcp
├── litho-qmd-core
├── litho-qmd-storage
└── litho-qmd-llm
```

## Success Criteria

| Metric | Current | Phase 1 | Phase 2 | Phase 4 |
|--------|---------|---------|---------|---------|
| Test count | 9 | 100+ | 200+ | 300+ |
| Test coverage | ~0% | 40% | 60% | 75% |
| Languages supported | 4 | 4 | 12 | 12 |
| LLM failure recovery | None | Codex fallback + serde | + regex | + heuristic |
| Incremental mode | No | No | No | Yes (<60s) |
| Output formats | 1 (MD) | 1 | 1 | 4 (MD/HTML/PDF/DOCX) |
| Content validation | Mermaid only | + JSON schema | + completeness | + accuracy |
| Doc generation time (large repo) | Timeout | 5-15 min | 3-10 min | <60s incremental |

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| rig-core deprecation blocks provider support | HIGH | Phase 4.3 replaces with direct clients |
| Ollama 7B model insufficient for large repos | HIGH | Phase 1.1 codex-rs fallback |
| codex-rs API changes break integration | MEDIUM | Pin codex-rs version, integration tests |
| PostgreSQL 18 not available in CI | MEDIUM | SQLite fallback for qmd-storage tests |
| Tree-sitter grammar updates break parsing | LOW | Pin grammar versions in Cargo.toml |

## Immediate Next Actions

1. [ ] Commit this plan + cleanup changes
2. [ ] Write CLAUDE.md for litho-workspace
3. [ ] Fix scripts to use unified target directory
4. [ ] Start Phase 1.1: CodexRs provider variant
5. [ ] Start Phase 1.3: Test infrastructure with nextest
