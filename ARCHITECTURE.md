# Architecture

This document describes the current high-level architecture of `litho-workspace`.

## System View

The workspace is organized into four logical layers:

1. Interface layer
- `litho-cli` (end-user commands)
- `litho-generator` binary (advanced pipeline commands)
- `litho-qmd-cli` and `litho-qmd-mcp` (retrieval services)

2. Orchestration layer
- `litho-generator` workflow orchestrates preprocessing, research, composition, validation, and output.
- `litho-codex` orchestrates codex-centric generation flows.

3. Analysis and retrieval layer
- `litho-extract` provides AST/tree-sitter analysis with optional ast-grep hints.
- QMD (`litho-qmd-core` + `litho-qmd-storage` + `litho-qmd-llm`) provides ingest/search/retrieval context.

4. Foundation layer
- `litho-core` provides shared config, environment, and common types used by higher layers.

## Primary Pipelines

### A) `litho-generator` Documentation Pipeline

Entry point: [main.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/main.rs)

Flow:

1. Config + provider runtime initialization
2. Repo index refresh (`repo-index.sqlite3`)
3. Preprocess phase
- project structure extraction
- core file identification
- code insight + relationship analysis
- ingestion DAG/RAG build via `litho-extract`, persisted as `.litho/ingestion-dag.json`
4. Research phase
- research agents execute
- QMD and ingestion RAG context is seeded into research memory
5. Compose phase
- doc agents generate section outputs
6. Validation phase
- completeness, file-reference accuracy, freshness, grounding, coherence, helpfulness
- representation coverage checks for missing core files/symbols
7. Output phase
- markdown/html writeout + summary report
- manifest update for incremental mode

Core orchestration: [workflow.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/workflow.rs)

### B) QMD Retrieval Pipeline

Entry surfaces:
- [qmd CLI](/C:/codedev/litho-workspace/crates/litho-qmd-cli/src/main.rs)
- [qmd MCP](/C:/codedev/litho-workspace/crates/litho-qmd-mcp/src/main.rs)

Flow:

1. `QmdService` receives query/search/get requests.
2. `AutoQmdStore` resolves backend (SQLite-first in repo-local scenarios, PostgreSQL optional).
3. Storage backend executes ingest/search/vector/search-context operations.
4. Results return to CLI/MCP callers and can be consumed by `litho-generator` research.

Service contract: [service.rs](/C:/codedev/litho-workspace/crates/litho-qmd-core/src/service.rs)

### C) `litho` CLI Extract/Generate Pipeline

Entry point: [litho CLI](/C:/codedev/litho-workspace/crates/litho-cli/src/main.rs)

Flow:

1. `extract`: runs `litho-extract` and emits JSON/summary.
2. `generate`: extracts codebase, then delegates generation through `litho-codex`.

## Data and State Artifacts

Typical state under `.litho/`:

- `manifest.json`: generation metadata and module/file tracking.
- `repo-index.sqlite3`: repo snapshot + diff planning.
- `cache/*`: layered cache artifacts.
- `ingestion-dag.json`: preprocess-time file graph + RAG chunks.
- benchmark reports under `.litho/benchmark*`.

QMD state is backend-dependent:

- SQLite: repo-local `.litho/qmd/*.sqlite3` by default.
- PostgreSQL: shared/service mode when explicitly configured.

## Provider and Fallback Model

LLM/provider behavior (generator):

- Primary provider can be Ollama/Codex/OpenAI/etc based on config.
- codex-rs fallback paths exist for resilience when configured.
- benchmark command evaluates model/parameter candidates with quality + latency + memory signals and optional promotion gates.

## Incremental and Reconciliation Strategy

Current mechanism:

- `manifest.json` + git diff map changed files to affected agents.
- selective research/compose execution in incremental mode.

Recent improvements:

- preprocess ingestion DAG/RAG for richer grounding.
- validator representation coverage flags files/symbols not represented in docs.

Remaining architectural gap:

- module `input_files` provenance is still coarse for most sections and should be replaced with DAG-derived provenance for tighter incremental targeting.

## Related Docs

- Root overview: [README.md](/C:/codedev/litho-workspace/README.md)
- Implementation map: [docs/IMPLEMENTATION_LAYOUT.md](/C:/codedev/litho-workspace/docs/IMPLEMENTATION_LAYOUT.md)
- Runtime/data flow details: [docs/RUNTIME_AND_DATA_FLOWS.md](/C:/codedev/litho-workspace/docs/RUNTIME_AND_DATA_FLOWS.md)
