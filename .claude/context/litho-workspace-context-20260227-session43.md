# Litho-Workspace Context — Session 43 (2026-02-27)

## Context ID: ctx-litho-workspace-20260227-session43
**Branch:** main @ 09c93f6
**Created:** 2026-02-27T22:00:00Z
**Created By:** claude-opus-4.6

## State Summary

Session 43 completed a comprehensive evaluation and cleanup of the litho-workspace,
followed by a 10-commit push covering: customized codex-rs fork (2,707 files),
build config optimization, development plans, and all 11 litho crates including
the new QMD Rust-native search stack (5 crates, 5,604 LOC).

## Recent Changes (10 commits)

| Commit | Type | Scope | Files |
|--------|------|-------|-------|
| 9f2d2bf | feat | external | Customized codex-rs fork (52 crates, 531K LOC) |
| 8d4a5bb | chore | config | Unified target dir, .rgignore, expanded .gitignore, build opts |
| f6d40b0 | docs | plans | v2 development plan, CLAUDE.md, TODO.md, GEMINI.md |
| 6e0292b | feat | litho-core | Expanded config types, env handling |
| e764ef9 | feat | litho-extract | Tree-sitter extraction improvements, Rust integration test |
| c6f4462 | feat | litho-codex | Codex bridge, prompt tests (9 tests) |
| 47ae880 | feat | litho-generator | Multi-agent pipeline: 63 files, research/compose/LLM improvements |
| 60245bc | feat | litho-book,cli | Web server routes, CLI entry point |
| c54bea0 | feat | qmd | 5 Rust-native QMD crates (core/storage/llm/mcp/cli), 5,604 LOC |
| 09c93f6 | chore | scripts | PostgreSQL bootstrap, quality/coverage/bench pipelines |

## Workspace Metrics

| Metric | Value |
|--------|-------|
| Total litho crates | 11 |
| Total Rust source (litho) | 32,425 lines / 127 files |
| External crates (codex-rs) | 52 |
| Working binaries | 5 (litho, litho-generator, litho-book, litho-qmd-cli, litho-qmd-mcp) |
| Tests | 9 (litho-codex only) |
| Build artifacts cleaned | 22 target-verify-* dirs (~15 GB freed) |

## Build System

- sccache + rust-lld + native CPU flags (`.cargo/config.toml`)
- CargoTools PowerShell module: `Invoke-CargoBuild -Release -UseLld -FixSqlite`
- cargo-nextest, cargo-binstall, cargo-deny, cargo-audit available
- Thin LTO + codegen-units=4 for release profile
- Unified target directory: `target/` (no more target-verify-*)

## Decisions Made

1. **codex-rs as regular files**: Not a submodule — contains custom modifications
2. **Unified target directory**: Eliminated 22 separate target-verify-* dirs
3. **4-phase development plan**: Foundation → AST Intelligence → CI/CD → Polish
4. **AST for functional patterns**: Tree-sitter detects where docs are needed, not just metadata
5. **Tiered LLM fallback**: Local Ollama → fallover model → codex-rs emergency

## Development Plan Summary

| Phase | Focus | Timeline | Key Deliverables |
|-------|-------|----------|-----------------|
| 1 | Foundation & Robustness | Week 1 | Codex-RS fallback, LLM recovery, test infra |
| 2 | AST-Driven Intelligence | Week 2 | Pattern detection, 12 languages, AST cache |
| 3 | CI/CD Integration | Week 3 | Change detection, incremental mode, GitHub Actions |
| 4 | Quality & Polish | Week 4 | Content validation, multi-format, rig-core migration |

## Agent Work Registry

| Agent | Task | Files | Status |
|-------|------|-------|--------|
| claude-opus-4.6 | Workspace evaluation + cleanup | All | Complete |
| Explore agent | CargoTools + build infra analysis | N/A | Complete |
| Explore agent | litho-generator architecture analysis | N/A | Complete |
| claude-opus-4.6 | Commit cluster (10 groups) | 2,832 files | Complete |

## Recommended Next Agents

1. **rust-pro**: Start Phase 1.1 — CodexRs provider variant in litho-generator
2. **test-automator**: Start Phase 1.3 — nextest infrastructure across all crates
3. **architect-reviewer**: Review Phase 2 AST pattern detection design
4. **deployment-engineer**: CI/CD docs.yml workflow for Phase 3

## Roadmap

### Immediate
- [ ] Push 10 commits to GitHub
- [ ] Start Phase 1.1: CodexRs fallback provider
- [ ] Start Phase 1.3: Test infrastructure with nextest

### This Week
- [ ] Phase 1.2: Port deepwiki-rs serde patches for LLM failure recovery
- [ ] Phase 2.1: Pattern-based documentation detection module
- [ ] PostgreSQL 18 setup for qmd-storage integration testing

### Tech Debt
- [ ] rig-core 0.23 is legacy — migration planned for Phase 4
- [ ] deepwiki-rs and litho-book are embedded git repos (warning on commit)
- [ ] 9 tests total — target 300+ by Phase 4
