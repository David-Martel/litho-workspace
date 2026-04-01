# Implementation Layout

This file maps the codebase layout to concrete responsibilities and entrypoints.

## Top-Level Structure

- `crates/`: primary Rust crates.
- `docs/`: plans, runbooks, architecture notes.
- `scripts/`: build/test/benchmark helper scripts.
- `external/`: vendored dependencies (including codex-rs workspace).
- `tests/`: workspace-level fixtures and integration support.

## Crate Responsibilities

### `litho-core`

- Shared configuration and environment handling.
- Common models used by extract/generate paths.

Primary files:
- [config.rs](/C:/codedev/litho-workspace/crates/litho-core/src/config.rs)
- [env.rs](/C:/codedev/litho-workspace/crates/litho-core/src/env.rs)

### `litho-extract`

- Source discovery and parsing by language.
- tree-sitter extraction and ast-grep hint integration.
- dependency graph construction.

Primary files:
- [lib.rs](/C:/codedev/litho-workspace/crates/litho-extract/src/lib.rs)
- [parser.rs](/C:/codedev/litho-workspace/crates/litho-extract/src/parser.rs)
- [graph.rs](/C:/codedev/litho-workspace/crates/litho-extract/src/graph.rs)

### `litho-generator`

- Main deep pipeline implementation and orchestrators.
- Cache/index/benchmark subsystems.
- validation and quality gating.

Primary files:
- [main.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/main.rs)
- [workflow.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/workflow.rs)
- [preprocess/mod.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/preprocess/mod.rs)
- [research/orchestrator.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/research/orchestrator.rs)
- [validator/mod.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/validator/mod.rs)
- [ingestion.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/ingestion.rs)

### `litho-codex`

- Codex-oriented prompt construction and generator backend wrappers.

Primary files:
- [exec.rs](/C:/codedev/litho-workspace/crates/litho-codex/src/exec.rs)
- [prompts.rs](/C:/codedev/litho-workspace/crates/litho-codex/src/prompts.rs)

### `litho-cli`

- User-facing command dispatcher for extract/generate/status/serve/validate.

Primary file:
- [main.rs](/C:/codedev/litho-workspace/crates/litho-cli/src/main.rs)

### QMD stack (`litho-qmd-core`, `litho-qmd-storage`, `litho-qmd-llm`, `litho-qmd-cli`, `litho-qmd-mcp`)

- API traits and service logic.
- backend storage implementation (SQLite/Postgres).
- retrieval/reranking model helpers.
- CLI and MCP interface surfaces.

Primary files:
- [service.rs](/C:/codedev/litho-workspace/crates/litho-qmd-core/src/service.rs)
- [lib.rs](/C:/codedev/litho-workspace/crates/litho-qmd-storage/src/lib.rs)
- [sqlite_impl.rs](/C:/codedev/litho-workspace/crates/litho-qmd-storage/src/sqlite_impl.rs)
- [main.rs](/C:/codedev/litho-workspace/crates/litho-qmd-cli/src/main.rs)
- [main.rs](/C:/codedev/litho-workspace/crates/litho-qmd-mcp/src/main.rs)

### `litho-book`

- documentation serving/rendering support.

## State and Cache Layout

Default runtime artifacts under `.litho/`:

- `manifest.json`
- `repo-index.sqlite3`
- `cache/`
- `ingestion-dag.json`
- benchmark outputs

QMD repo-local SQLite defaults:

- `.litho/qmd/<index>.sqlite3`

## Tests

- crate-local unit/integration tests in `crates/*/tests` and module-level `#[cfg(test)]`.
- workspace fixtures under `tests/fixtures`.

## Related Docs

- [README.md](/C:/codedev/litho-workspace/README.md)
- [ARCHITECTURE.md](/C:/codedev/litho-workspace/ARCHITECTURE.md)
- [RUNTIME_AND_DATA_FLOWS.md](/C:/codedev/litho-workspace/docs/RUNTIME_AND_DATA_FLOWS.md)
