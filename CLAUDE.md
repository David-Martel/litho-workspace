# CLAUDE.md — litho-workspace

## Project Purpose

Unified Rust workspace for AI-driven code documentation generation. Combines
tree-sitter AST analysis, multi-agent LLM orchestration, and semantic search
into an automated documentation pipeline that works on repositories of any size.

**Repo:** https://github.com/David-Martel/litho-workspace (PUBLIC)
**Status:** 540 tests, 11 crates, 5 binaries, Rust stable 1.94

## Quick Start

```bash
# Build all litho crates (excludes vendored codex-rs)
cargo build

# Run tests
cargo nextest run --workspace --no-fail-fast

# Clippy (warnings = errors, enforced by lefthook pre-commit)
cargo clippy --workspace --all-targets -- -D warnings

# Generate docs for a project (local Ollama)
litho-generator -p /path/to/project -o ./litho.docs \
  --llm-api-base-url http://localhost:11434/v1 --llm-api-key ollama \
  --model-efficient qwen2.5-coder:7b --model-powerful qwen2.5-coder:7b

# Serve generated docs
litho-book --docs-dir ./litho.docs --port 3333 --open

# Search indexed documents (SQLite backend, no PostgreSQL needed)
litho search "query" --limit 5
```

## Development Tracking

**Target state:** `GOALS.md` — the requirement-level vision for the finished
workspace.

**Active roadmap:** `TODO.md` — prioritized work items with crate candidates
that should lead toward `GOALS.md`.

Always check both `GOALS.md` and `TODO.md` before starting new work to stay
aligned with the end state and the current priorities.

| Priority | Focus |
|----------|-------|
| P0 | Public CLI surface, deterministic regression harness, cache wiring |
| P1 | QMD/config consolidation, dependency trimming, deprecation path |
| P2 | Async pipeline, QMD async I/O, incremental regeneration hardening |
| P3 | Search/embedding upgrades, AST expansion, output polish |

**Key docs:**
- `GOALS.md` — target-state requirements and definition of done
- `README.md` — CLI reference for all 5 binaries
- `ARCHITECTURE.md` — pipeline diagrams, crate graph, provider config
- `BUILD.md` — sccache, codegen-units, CargoTools, tiered build, troubleshooting
- `TODO.md` — prioritized roadmap with crate candidates table

`BUILD.md` is the authoritative build-policy reference. If you change build
scripts, artifact paths, sccache handling, or release workflow behavior, update
`BUILD.md` in the same change and keep the scripts consistent with it.

## Architecture

11 Rust crates + 52 codex-rs crates in a unified Cargo workspace.
`default-members` scopes `cargo build/test/clippy` to litho crates only.

### Core Pipeline (litho-generator)

4-stage sequential pipeline with multi-agent orchestration:

1. **Preprocessing** — Tree-sitter AST extraction, file classification, ingestion DAG
2. **Research** — 8 parallel agents (system context, modules, architecture, workflows)
3. **Composition** — 6 agents generate C1-C4 architecture documentation
4. **Verification** — 6-dimension quality scoring, mermaid validation, quality gate

### Crate Map

| Crate | Purpose | Binary |
|-------|---------|--------|
| litho-core | Config (TOML), env vars, shared types | — |
| litho-extract | Tree-sitter AST parsing (Rust/TS/Python/C#) | — |
| litho-codex | Bridge to OpenAI Codex (subprocess + in-process) | — |
| litho-generator | 4-stage doc generation pipeline | litho-generator.exe |
| litho-book | Axum web server for doc browsing | litho-book.exe |
| litho-cli | Unified CLI (extract, generate, init, index, search, serve, validate) | litho.exe |
| litho-qmd-core | QMD shared types, service trait | — |
| litho-qmd-storage | SQLite/PostgreSQL storage, BM25 search, vectors | — |
| litho-qmd-llm | Adaptive LLM query expansion + reranking | — |
| litho-qmd-mcp | MCP server (JSON-RPC stdin/stdout) | litho-qmd-mcp.exe |
| litho-qmd-cli | QMD CLI (search, ingest, embed, status) | litho-qmd-cli.exe |

### External Dependencies

- `external/codex-rs/` — Customized OpenAI Codex fork (52 crates). Not a
  vanilla upstream — contains local modifications. Tracked as regular files.
  NOT part of default build/test/CI surfaces.

## Build System

### Prerequisites

- Rust stable 1.94+ (edition 2024, resolver 3)
- sccache 0.13+, cargo-nextest, lefthook (pre-commit/pre-push hooks)
- MSVC Build Tools (linker + tree-sitter C compilation)
- Optional: PostgreSQL 18 (for QMD postgres backend), Ollama (for doc generation)

See `BUILD.md` for detailed build documentation including sccache isolation,
codegen-units strategy, CargoTools module, and OOM troubleshooting.

### Build Config Summary

| Setting | Value | Location |
|---------|-------|----------|
| sccache | Project-local `.cache/sccache/`, port 5100 | `.cargo/config.toml` |
| Linker | MSVC link.exe (not lld-link) | `.cargo/config.toml` |
| codegen-units | 512 (dev/test), 16 (release) | `.cargo/config.toml` |
| Debug info (deps) | line-tables-only | `.cargo/config.toml` |
| Target dir | `T:\RustCache\cargo-target` (global) | global cargo config |
| Published release dir | `.\release\` (repo-local, authoritative) | `scripts/litho-build-helpers.ps1` |
| Jobs | 2 (caps peak memory) | `.cargo/config.toml` |

### Commands

```bash
# Standard build
cargo build --workspace

# Memory-safe tiered build (serializes large binary links, publishes to .\release\)
pwsh scripts/build-tiered.ps1              # dev build, OOM-safe
pwsh scripts/build-tiered.ps1 -Release     # release build

# Cargo aliases (defined in .cargo/config.toml)
cargo build-safe   # parallel libs, excludes large binaries
cargo build-gen    # litho-generator only, jobs=1
cargo test-safe    # litho-core + litho-extract + litho-generator

# Test
cargo nextest run --workspace --no-fail-fast

# Clippy
cargo clippy --workspace --all-targets -- -D warnings

# CargoTools wrapper (optional, provides JSON output for agents)
pwsh -NoLogo -NoProfile -c "Invoke-CargoWrapper build --quick-check --llm-output"
```

## Skills & MCP Tools

### Relevant Skills (invoke via Skill tool)

| Skill | When to use |
|-------|-------------|
| `cargo-build` | ANY cargo operation — wraps with sccache, preflight, JSON output |
| `superpowers:test-driven-development` | Before implementing features |
| `superpowers:systematic-debugging` | When encountering bugs or test failures |
| `superpowers:brainstorming` | Before creative work or new features |
| `superpowers:writing-plans` | For multi-step implementation planning |
| `codex:codex-review` | Code review with Codex |
| `codex:codex-debug` | Complex debugging with Codex |

### Relevant MCP Tools

| MCP Server | Use case in this project |
|------------|--------------------------|
| `ast-grep` | Structural code search across litho crates |
| `serena` | Rust LSP analysis for litho-extract, litho-generator |
| `context7` | Framework docs lookup (tokio, axum, clap, tree-sitter) |
| `git-cluster-analyzer` | Semantic commit clustering for commit-cluster |
| `agent-bus` | Multi-agent coordination for parallel work |

### Relevant Agents

| Agent | Task |
|-------|------|
| `rust-pro` | All Rust development in litho crates |
| `code-reviewer` / `architect-reviewer` | Review changes against TODO.md priorities |
| `security-auditor` | Check for credential leaks (repo is PUBLIC) |
| `performance-engineer` | Profile preprocessing bottleneck |
| `test-automator` | Generate tests toward 600+ target |

## Key Patterns

- **Agent trait**: `StepForwardAgent` — declarative `data_config()` + `prompt_template()`
- **Memory DAG**: Shared `Arc<RwLock<Memory>>` for inter-agent data flow
- **Provider abstraction**: Direct reqwest HTTP in `providers.rs` (OpenAI-compat, Anthropic, Gemini, CodexRs)
- **Fallback chain**: model_efficient → model_powerful → codex-rs
- **Cache**: `CacheManager` with MD5 prompt keying + BLAKE3 content hash, ~55% hit rate
- **Async**: `Semaphore` + `join_all` for agent fan-out, `rayon` for file extraction
- **Quality gate**: 6-dimension scoring (completeness, accuracy, freshness, grounding, coherence, helpfulness)

## Configuration

Primary config: `litho.toml` in the target project root.

```toml
project_name = "my-project"
excluded_dirs = [".git", "target", "node_modules"]

[llm]
provider = "ollama"
api_base_url = "http://localhost:11434"
model_efficient = "qwen2.5-coder:7b"
model_powerful = "qwen2.5-coder:7b"
context_window = 131072
```

Environment variables: `OLLAMA_URL`, `DATABASE_URL`, `CODEX_BINARY_PATH`.
See `litho-core/src/env.rs` for the full list.

## Testing

```bash
# All tests (540 passing)
cargo nextest run --workspace --no-fail-fast

# Specific crate
cargo nextest run -p litho-generator

# With output
cargo nextest run --workspace --no-capture
```

Lefthook hooks:
- **pre-commit**: `cargo fmt --check` + `cargo clippy -D warnings` (all 11 crates)
- **pre-push**: `cargo nextest run` (core + extract + generator)

## Gotchas

- `external/codex-rs/` is NOT a git submodule — tracked as regular files with local mods
- sccache uses **port 5100** (project-local, isolated from global port 4400)
- Prefer the repo-local `.cache/sccache/` for cache cleanup and lock regulation
- Treat `.\release\` as the runnable release artifact folder; do not rely on `T:\RustCache\cargo-target\release\...` for handoff or smoke execution
- If sccache fails, bypass with `RUSTC_WRAPPER=""` (loses cache)
- Tree-sitter C compilation requires MSVC clang-cl on Windows
- rig-core was removed in Session 49 — all LLM calls go through direct reqwest HTTP
- QMD defaults to SQLite (`.litho/qmd/`), PostgreSQL is optional (`--backend postgres`)
- `.rgignore` excludes `external/codex-rs/` from ripgrep for speed
- Repo is **PUBLIC** — never commit credentials, PII, or secrets
- `qmd.config.json` is gitignored — use `qmd.config.example.json` as template
- codegen-units=512 in dev builds — intentionally high to minimize per-unit memory
