# CLAUDE.md — litho-workspace

## Project Purpose

Unified Rust workspace for AI-driven code documentation generation. Combines
tree-sitter AST analysis, multi-agent LLM orchestration, and semantic search
into an automated documentation pipeline that works on repositories of any size.

## Quick Start

```powershell
Import-Module CargoTools

# Build all crates
Invoke-CargoBuild -Path . -Release -UseLld -FixSqlite

# Or plain cargo
cargo build --workspace --release

# Run tests
cargo nextest run --workspace --no-fail-fast

# Generate docs for a project
./target/release/litho-generator.exe -p /path/to/project -o ./litho.docs \
  --llm-api-base-url http://localhost:11434/v1 --llm-api-key ollama \
  --model-efficient qwen2.5-coder:7b --model-powerful qwen2.5-coder:7b

# Iterative bootstrap loop (full + incremental reruns + captured artifacts)
pwsh -NoProfile -File scripts/litho-doc-bootstrap.ps1 \
  -ProjectPath . -OutputPath .\litho.docs -ConfigPath .\litho.toml \
  -Iterations 3 -IncrementalAfterFirst

# Serve generated docs
./target/release/litho-book.exe --docs-dir ./litho.docs --port 3333

# Search (requires PostgreSQL)
./target/release/litho-qmd-cli.exe search "query" --index index
```

## Architecture

11 Rust crates + 52 codex-rs crates in a unified Cargo workspace.

### Core Pipeline (litho-generator)

4-stage sequential pipeline with multi-agent orchestration:

1. **Preprocessing** — Tree-sitter AST extraction, file classification, dependency graph
2. **Research** — 8 parallel agents analyze system context, modules, architecture, workflows
3. **Composition** — 6 agents generate C1-C4 architecture documentation
4. **Verification** — Mermaid diagram validation, summary generation

### Crate Map

| Crate | Purpose | Binary |
|-------|---------|--------|
| litho-core | Config (TOML), env vars, shared types | — |
| litho-extract | Tree-sitter AST parsing (Rust/TS/Python/C#) | — |
| litho-codex | Bridge to OpenAI Codex (subprocess + in-process) | — |
| litho-generator | 4-stage doc generation pipeline | litho-generator.exe |
| litho-book | Axum web server for doc browsing | litho-book.exe |
| litho-cli | Unified CLI (extract, generate) | litho.exe |
| litho-qmd-core | QMD shared types, service trait | — |
| litho-qmd-storage | PostgreSQL storage, BM25 search, vectors | — |
| litho-qmd-llm | Adaptive LLM query expansion + reranking | — |
| litho-qmd-mcp | MCP server (JSON-RPC stdin/stdout) | litho-qmd-mcp.exe |
| litho-qmd-cli | QMD CLI (search, ingest, embed, status) | litho-qmd-cli.exe |

### External Dependencies

- `external/codex-rs/` — Customized OpenAI Codex fork (52 crates). Not a
  vanilla upstream — contains local modifications. Tracked as regular files.

## Build System

### Prerequisites

- Rust nightly (edition 2024, resolver 3)
- sccache, rust-lld, cargo-nextest, cargo-binstall (all in PATH)
- PostgreSQL 18 (for qmd-storage)
- Ollama with qwen2.5-coder:7b (for doc generation)
- CargoTools PowerShell module

### Build Accelerators

| Tool | Purpose | Config |
|------|---------|--------|
| sccache | Compilation cache | `.cargo/config.toml` |
| rust-lld | Fast linker | `.cargo/config.toml` |
| cargo-nextest | Parallel test runner | `cargo nextest run` |
| thin LTO | Optimized release | `.cargo/config.toml` |
| native CPU flags | Architecture-specific | `.cargo/config.toml` |

### Commands

```bash
# Build
cargo build --workspace --release

# Test (use nextest for speed)
cargo nextest run --workspace --no-fail-fast

# Clippy
cargo clippy --workspace --all-targets -- -D warnings

# Coverage
cargo llvm-cov nextest --workspace --html --output-dir coverage/

# Security
cargo deny check && cargo audit

# Quality pipeline (PowerShell)
pwsh scripts/qmd-quality.ps1
```

### Unified Target Directory

All builds output to `target/`. Do NOT create `target-*` directories.
The `.cargo/config.toml` enforces `target-dir = "target"`.

## Key Patterns

- **Agent trait**: `StepForwardAgent` — declarative data_config() + prompt_template()
- **Memory DAG**: Shared `Arc<RwLock<Memory>>` for inter-agent data flow
- **Provider abstraction**: Direct reqwest HTTP client supporting OpenAI-compatible, Anthropic, Gemini, CodexRs
- **Fallback chain**: Primary model → fallover_model → codex-rs (Phase 1)
- **Cache**: `CacheManager` with MD5 prompt keying, ~55% hit rate on 2nd run

## Development Plans

Active plan: `docs/plans/2026-02-27-litho-v2-development-plan.md`

Readiness and iterative bootstrap references:
- `docs/plans/2026-02-28-litho-readiness-matrix.md`
- `docs/plans/2026-02-28-litho-generator-iterative-bootstrap.md`

Phase 1: Foundation & Robustness (codex-rs fallback, failure recovery, tests)
Phase 2: AST-Driven Intelligence (pattern detection, language expansion)
Phase 3: CI/CD Integration (incremental mode, change detection)
Phase 4: Quality & Polish (validation, multi-format, rig-core migration)

## Testing

```bash
# All tests
cargo nextest run --workspace

# Specific crate
cargo nextest run -p litho-codex

# With output
cargo nextest run --workspace --no-capture
```

## Gotchas

- `external/codex-rs/` is NOT a git submodule — it's tracked as regular files
  with custom modifications
- sccache may conflict with parallel builds — use `RUSTC_WRAPPER=""` to bypass
- Tree-sitter C compilation requires MSVC clang-cl on Windows
- rig-core removed in Session 49 — all LLM calls go through direct reqwest HTTP in providers.rs
- PostgreSQL 18 required for qmd-storage (bootstrap: `scripts/postgres18-bootstrap.ps1`)
- `.rgignore` excludes `external/codex-rs/` from ripgrep searches for speed
