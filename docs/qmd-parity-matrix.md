# QMD TS -> Rust Parity Matrix

Date: 2026-02-24

## Scope

This matrix tracks migration status from `third_party/qmd-ts` into the Rust-native crates:
- `crates/litho-qmd-core`
- `crates/litho-qmd-storage`
- `crates/litho-qmd-llm`
- `crates/litho-qmd-cli`
- `crates/litho-qmd-mcp`

## CLI Commands

| TS command | Rust command | Status | Notes |
|---|---|---|---|
| `context add/list/check/rm` | `qmd context add/list/check/rm` | Implemented | Supports global + virtual-path + filesystem-path targets, current-dir auto-targeting, context previews, and top-level gap suggestions. |
| `get` | `qmd get` | Implemented | Supports `:line`, `--from`, `-l/--lines`, `--line-numbers`, JSON output. |
| `multi-get` | `qmd multi-get` | Implemented | Supports glob and CSV path list; `max-bytes` and line slicing retained. |
| `ls` | `qmd ls` | Implemented | Prefix filtering available. |
| `collection list/add/remove/rename` | `qmd collection ...` | Implemented | YAML-backed config parity with path/pattern/name support. |
| `status` | `qmd status` | Implemented | Reports document counts, vector index presence, per-collection metadata. |
| `update` | `qmd update` | Implemented | Executes configured collection `update` commands cross-platform shell. |
| `embed` | `qmd embed` | Implemented | Native quantized semantic embedding (`native-hash-v1`, 256-d int16 vectors). |
| `pull` | `qmd pull` | Implemented | Local model inventory + optional Ollama-backed pull verification. |
| `search` | `qmd search` | Implemented | BM25-backed search via PostgreSQL `tsvector` ranking. |
| `vsearch` | `qmd vsearch` | Implemented | Native vector search over quantized embeddings with lexical overlap blending. |
| `query` | `qmd query` | Implemented | Hybrid BM25 + vector merge with heuristic expansion/rerank. |
| `mcp` | `qmd mcp` | Implemented | Delegates to `litho-qmd-mcp` binary. |
| `cleanup` | `qmd cleanup` | Implemented | LLM cache/inactive/orphan cleanup + vacuum. |

## MCP Surface

| TS MCP registration | Rust MCP method/tool | Status | Notes |
|---|---|---|---|
| `registerResource(qmd://{+path})` | `resources/list` + `resources/read` | Implemented | `qmd://` URI contract retained via resources endpoint and `get` tool payloads. |
| `registerPrompt(query)` | `prompts/list` + `prompts/get` | Implemented | Query guide prompt exposed for MCP clients. |
| `registerTool(search)` | `tools/call` name=`search` | Implemented | Returns text + structured JSON. |
| `registerTool(vsearch)` | `tools/call` name=`vsearch` | Implemented | Native semantic scoring path is active. |
| `registerTool(query)` | `tools/call` name=`query` | Implemented | Hybrid scoring active with deterministic reranking. |
| `registerTool(get)` | `tools/call` name=`get` | Implemented | Returns resource-like payload. |
| `registerTool(multi_get)` | `tools/call` name=`multi_get` | Implemented | Batch fetch preserved with warnings/errors. |
| `registerTool(status)` | `tools/call` name=`status` | Implemented | Status structured payload retained. |
| n/a (Rust extension) | `tools/call` name=`ingest` | Implemented | Explicit ingestion trigger for MCP workflows/automation. |
| n/a (Rust extension) | `tools/call` name=`embed` | Implemented | Explicit embedding trigger for MCP workflows/automation. |

## Architectural Deficiencies Addressed

1. Removed Bun runtime coupling (`Bun.argv`, `Bun.spawn`, `bun:sqlite`) by introducing Rust-native CLI/process paths.
2. Eliminated hidden mutable singleton store by using explicit `PostgresQmdStore` instances.
3. Introduced shared typed contracts in `litho-qmd-core` for CLI/MCP consistency.
4. Replaced unstructured exits with typed errors (`QmdError`) and contextual `anyhow` handling in CLI binaries.
5. Upgraded Rust MCP transport to content-length framed stdio for MCP client interoperability.

## Remaining High-Impact Work

1. Expand integration tests from synthetic fixtures to multi-repo corpora benchmarks.
2. Add configurable relevance-feedback loop for automatic threshold calibration.
3. Add optional advanced vector backend for million-document scale.
