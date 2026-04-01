# Runtime and Data Flows

This document describes how requests move through the system at runtime.

## 1) `litho-generator` Full Run

Entry: `cargo run -p litho-generator -- ...`

Flow:

1. Parse CLI/config, initialize provider and memory/cache context.
2. Refresh repo index (`repo-index.sqlite3`) to compute new/changed/removed files.
3. Preprocess:
- extract original docs and structure.
- identify core files.
- run code and relationship analysis.
- build ingestion DAG + RAG chunks using `litho-extract`.
4. Research:
- seed QMD context (when enabled).
- seed ingestion DAG RAG context.
- run research agents (system/domain/workflow/architecture/etc).
5. Compose:
- create documentation sections from research + preprocess memory.
6. Validate:
- quality dimensions + representation coverage checks.
7. Output:
- write markdown/html.
- write summary report.
- persist manifest for incremental mode.

Core code:
- [workflow.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/workflow.rs)
- [preprocess/mod.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/preprocess/mod.rs)
- [research/orchestrator.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/research/orchestrator.rs)
- [validator/mod.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/validator/mod.rs)

## 2) Incremental Run

Entry: `litho-generator --incremental`

Flow:

1. Load `manifest.json`.
2. Detect git deltas and derive affected agents.
3. Re-run preprocess (fresh source truth).
4. Run selective research and selective compose.
5. Re-validate and output.
6. Persist updated manifest.

Key files:
- [manifest.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/integrations/manifest.rs)
- [change_detector.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/integrations/change_detector.rs)

## 3) Repo Index Only Mode

Entry: `litho-generator index-repo`

Flow:

1. Compute snapshot diff for repository files.
2. Update sqlite state tables.
3. Print delta summary without running LLM stages.

Key file:
- [workflow.rs](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/workflow.rs)

## 4) QMD Ingest/Search Flow

CLI entry: `qmd ingest`, `qmd search`, `qmd query`, `qmd get`

Flow:

1. Resolve backend (auto/sqlite/postgres).
2. Open store and service.
3. Ingest collections or execute search/get operations.
4. Return JSON/text responses to CLI or MCP clients.

Key files:
- [main.rs](/C:/codedev/litho-workspace/crates/litho-qmd-cli/src/main.rs)
- [service.rs](/C:/codedev/litho-workspace/crates/litho-qmd-core/src/service.rs)
- [lib.rs](/C:/codedev/litho-workspace/crates/litho-qmd-storage/src/lib.rs)

## 5) Codex Generation Flow (`litho` CLI)

Entry: `litho generate <path>`

Flow:

1. Extract codebase via `litho-extract`.
2. Build prompts and optional QMD context snippets.
3. Generate docs via codex provider path.
4. Write docs to output directory.

Key files:
- [main.rs](/C:/codedev/litho-workspace/crates/litho-cli/src/main.rs)
- [exec.rs](/C:/codedev/litho-workspace/crates/litho-codex/src/exec.rs)
- [prompts.rs](/C:/codedev/litho-workspace/crates/litho-codex/src/prompts.rs)

## Runtime Stores and Memory Scopes

- In-memory scopes: preprocess/research/compose/timing.
- Persistent internal state: `.litho/*`.
- QMD store state: sqlite/postgres backend.
- Ingestion DAG artifact: `.litho/ingestion-dag.json`.

## Related Docs

- [README.md](/C:/codedev/litho-workspace/README.md)
- [ARCHITECTURE.md](/C:/codedev/litho-workspace/ARCHITECTURE.md)
- [IMPLEMENTATION_LAYOUT.md](/C:/codedev/litho-workspace/docs/IMPLEMENTATION_LAYOUT.md)
