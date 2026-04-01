# litho-workspace

`litho-workspace` is a Rust monorepo for repository documentation generation, code extraction, and local retrieval-augmented context (QMD/RAG).

## What This Repo Provides

- `litho` CLI for extraction and codex-based generation.
- `litho-generator` pipeline for deep project analysis and multi-stage doc generation.
- `litho-qmd-*` stack for ingest/search/context retrieval over local docs/code.
- Shared core config/types via `litho-core`.

## Start Here

- High-level architecture: [ARCHITECTURE.md](/C:/codedev/litho-workspace/ARCHITECTURE.md)
- Workspace implementation map: [docs/IMPLEMENTATION_LAYOUT.md](/C:/codedev/litho-workspace/docs/IMPLEMENTATION_LAYOUT.md)
- Runtime/data flows: [docs/RUNTIME_AND_DATA_FLOWS.md](/C:/codedev/litho-workspace/docs/RUNTIME_AND_DATA_FLOWS.md)

## Workspace Crates

- `crates/litho-core`: shared config, env, core types.
- `crates/litho-extract`: AST/tree-sitter extraction + ast-grep hint integration + dependency graph.
- `crates/litho-generator`: deepwiki-style generation pipeline (preprocess/research/compose/validate/output), cache/index/benchmarking.
- `crates/litho-codex`: codex-facing generation provider and prompt shaping.
- `crates/litho-cli`: top-level user CLI (`extract`, `generate`, `status`, `serve`, `validate`).
- `crates/litho-book`: docs rendering/serving support.
- `crates/litho-qmd-core`: QMD service traits + models.
- `crates/litho-qmd-storage`: SQLite/Postgres backends with auto backend detection.
- `crates/litho-qmd-llm`: retrieval/reranking helpers.
- `crates/litho-qmd-cli`: local QMD CLI.
- `crates/litho-qmd-mcp`: MCP server surface for QMD features.

## Common Commands

```powershell
cargo build --workspace
cargo nextest run --workspace --no-fail-fast
cargo nextest run -p litho-generator --test benchmark_report_regression --no-fail-fast
pwsh -NoProfile -File scripts/benchmark-ollama-optimize.ps1 -ProjectPath . -OutputDir ./.litho/benchmark-smoke -DryRun
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Notes

- The vendored `external/codex-rs` workspace exists for integration/support but is not the primary release gate for litho crates.
- Current architecture includes preprocess-time ingestion DAG/RAG artifacts in `litho-generator` (`.litho/ingestion-dag.json`) to improve grounding and reconciliation.
