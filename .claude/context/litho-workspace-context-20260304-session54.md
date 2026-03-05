# Litho Workspace Context — Session 54 (2026-03-04)

## Context ID
`ctx-litho-workspace-20260304-session54`

## Project
- **Name:** litho-workspace
- **Root:** `C:\codedev\litho-workspace`
- **Type:** Rust (mixed — 11 litho crates + 52 codex-rs crates)
- **Branch:** main @ `3c0bb49`
- **Remote:** David-Martel/litho-workspace (private)

## Current State Summary

Session 54 focuses on **build scope rationalization**: decoupling the vendored
codex-rs workspace from Litho's default build, test, and CI surfaces. The
codex-tui binary was causing OOM and compatibility issues that blocked Litho
development. Changes cleanly separate "Litho development" from "codex-rs vendor
compatibility" across all build surfaces (Cargo.toml, CI, scripts, aliases).

Additional fixes: CLI contract corrections (litho serve invocation, dead
`--skip-*` flags), QualityConfig startup validation, review_agent doctest fix,
qmd-storage test cleanup (hardcoded repo paths removed), pulldown-cmark 0.13
compatibility in codex-rs TUI, and a new QMD repo-local SQLite proposal doc.

## Changes (Uncommitted)

### Build Scope Rationalization
| File | Change |
|------|--------|
| `Cargo.toml` | Added `default-members` listing all 11 litho crates |
| `.cargo/config.toml` | `build-safe` alias targets litho library crates explicitly |
| `.codex/config.toml` | Build/test/lint commands no longer use `--workspace` |
| `scripts/build-tiered.ps1` | Phase 1 targets litho libs explicitly; codex-tui opt-in via `-IncludeCodexTui` |

### CI Reliability
| File | Change |
|------|--------|
| `.github/workflows/ci.yml` | Build/test/clippy target litho crates; new `postgres-integration` opt-in job; `workflow_dispatch` input; qmd-quality skips integration by default |
| `.github/workflows/vendor-codex-verify.yml` | **NEW** — weekly `cargo check -p codex-cli -p codex-tui` |
| `scripts/qmd-quality.ps1` | `litho-qmd-storage` split into `--lib` (always) vs full (integration) |

### CLI Contract Fixes
| File | Change |
|------|--------|
| `crates/litho-cli/src/main.rs` | `litho serve` passes `--docs-dir` instead of bare `serve` to litho-book |
| `crates/litho-generator/src/cli.rs` | Removed deprecated `--skip-preprocessing/research/documentation` flags |
| `crates/litho-generator/src/main.rs` | Calls `QualityConfig::validate()` at startup |

### Test & Doc Fixes
| File | Change |
|------|--------|
| `crates/litho-generator/.../review_agent.rs` | Doctest uses `ignore` instead of `no_run` (private module path) |
| `crates/litho-qmd-storage/tests/pipeline.rs` | Removed hardcoded `qmd.config.json` fallback; cleaner `postgres_test_db_url()` |

### Vendored codex-rs Compatibility
| File | Change |
|------|--------|
| `external/codex-rs/.../markdown_render.rs` | pulldown-cmark 0.13 compat: `BlockQuote(_)`, `Superscript/Subscript`, `DefinitionList*`, `InlineMath/DisplayMath` |

### Documentation
| File | Change |
|------|--------|
| `TODO.md` | New P0 sections (build scope, CLI contract), P2 QMD backend strategy, incremental mode gaps |
| `docs/qmd-repo-local-sqlite-proposal-2026-03-05.md` | **NEW** — SQLite backend strategy for repo-local QMD |

## Decisions

1. **codex-rs out of default-members** — Litho development should not be gated by
   codex-rs compile/link/clippy issues. codex-rs is verified weekly via separate CI job.
2. **Postgres integration opt-in** — CI runs qmd-storage unit tests always, but
   full Postgres integration requires explicit `workflow_dispatch` trigger.
3. **Skip flags removed** — `--skip-preprocessing/research/documentation` were no-ops
   causing silent confusion. Removed rather than wiring dead code.
4. **QMD SQLite backend proposed** — repo-local `.litho/qmd/<index>.sqlite3` for
   friction-free setup, with Postgres preserved as optional shared backend.

## Patterns

- **Build surface = litho crates only** — All scripts, CI, aliases explicitly list
  litho crate names instead of `--workspace`. codex-rs is opt-in.
- **Integration test gating** — DB-dependent tests skip cleanly when env vars absent;
  CI has explicit opt-in lane.
- **Startup validation** — Config objects validate at load time, not deep in pipeline.

## Test Status
- 786 tests passing (litho-core + litho-extract + litho-generator)
- 0 errors, 0 clippy warnings

## Agent Work Registry

| Agent | Task | Files | Status |
|-------|------|-------|--------|
| (session work) | Build scope rationalization | Cargo.toml, .cargo/, CI, scripts | Complete |
| (session work) | CLI contract fixes | litho-cli, litho-generator | Complete |
| (session work) | codex-rs pulldown-cmark compat | markdown_render.rs | Complete |
| (session work) | qmd-storage test cleanup | pipeline.rs | Complete |

## Recommended Next Actions

1. **Commit & push** — 14 modified + 2 new files ready for semantic clustering
2. **Run tests** — Verify 786 tests still pass after changes
3. **Build release** — Confirm tiered build works with new scope
4. **SQLite backend** — Begin implementing `SqliteQmdStore` per proposal doc
5. **Incremental hardening** — Wire manifest population during normal runs
