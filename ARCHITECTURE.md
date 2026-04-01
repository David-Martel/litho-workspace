# Architecture

High-level architecture of `litho-workspace`. This document is designed to give
LLM agents and developers enough context to understand, extend, and operate the
system.

## System Layers

```
Layer 4 - Interface
  litho (CLI)               litho-generator (CLI)     litho-qmd-cli (CLI)
  litho-book (web)          litho-qmd-mcp (MCP)

Layer 3 - Orchestration
  litho-generator::workflow     4-stage pipeline: preprocess -> research -> compose -> validate
  litho-codex                   Codex-CLI integration (in-process + subprocess)
  litho-qmd-llm                Query expansion + reranking

Layer 2 - Analysis & Storage
  litho-extract                 Tree-sitter AST (Rust, TS, Python, C#) + ast-grep hints
  litho-qmd-storage             SQLite/PostgreSQL backends, BM25 + vector search

Layer 1 - Foundation
  litho-core                    Config (litho.toml), env vars, shared types
  litho-qmd-core                QMD service trait, shared models
```

## Primary Pipelines

### A) `litho-generator` Documentation Pipeline

Entry point: `crates/litho-generator/src/main.rs`

Core orchestration: `crates/litho-generator/src/generator/workflow.rs`

```
  litho.toml + CLI flags
        |
        v
  Config + Provider init
        |
        v
  Repo index refresh (repo-index.sqlite3)
        |
        v
  +----- PREPROCESS -----+
  |  project structure    |
  |  core file detection  |     <-- litho-extract (tree-sitter AST)
  |  code insight LLM     |     <-- 2 LLM calls per file (purpose + analysis)
  |  ingestion DAG build  |     <-- .litho/ingestion-dag.json
  +----------------------+
        |
        v
  +----- RESEARCH --------+
  |  8 parallel agents     |     <-- system context, module analysis, architecture,
  |  QMD + DAG context     |        workflows, patterns, dependencies, security, API
  +----------------------+
        |
        v
  +----- COMPOSE ---------+
  |  6 agents generate     |     <-- C1-C4 architecture docs (context, containers,
  |  section outputs       |        components, code), guides, API reference
  +----------------------+
        |
        v
  +----- VALIDATE --------+
  |  completeness          |     <-- file-reference accuracy, freshness, grounding,
  |  representation check  |        coherence, helpfulness (6-dimension scoring)
  |  quality gate          |     <-- regression detection, enforcement threshold
  +----------------------+
        |
        v
  +----- OUTPUT ----------+
  |  markdown/html write   |
  |  summary report        |
  |  manifest update       |     <-- .litho/manifest.json (incremental tracking)
  +----------------------+
```

**Key patterns:**
- `StepForwardAgent` trait: declarative `data_config()` + `prompt_template()`
- `Memory` DAG: shared `Arc<RwLock<Memory>>` for inter-agent data flow
- Provider abstraction: direct reqwest HTTP client (OpenAI-compat, Anthropic, Gemini, CodexRs)
- Fallback chain: primary model -> model_powerful -> codex-rs
- Cache: `CacheManager` with MD5 prompt keying, ~55% hit rate on 2nd run
- BLAKE3 content-hash for cache warming (skip unchanged files)

**Performance characteristics:**
- Preprocessing dominates (~88% of runtime): 2 sequential LLM calls per file
- Research + compose: 8+6 agents, parallelism capped by `max_parallels` (default: 8)
- Incremental mode: `manifest.json` + git diff -> selective agent re-execution

### B) QMD Retrieval Pipeline

Entry surfaces:
- `crates/litho-qmd-cli/src/main.rs` (CLI)
- `crates/litho-qmd-mcp/src/main.rs` (MCP server for LLM agents)

```
  Collection config
        |
        v
  +----- INGEST ----------+
  |  scan directories      |
  |  parse markdown/code   |
  |  upsert to backend     |
  +----------------------+
        |
        v
  +----- EMBED -----------+
  |  chunk documents       |
  |  generate vectors      |     <-- GPU-accelerated embedding
  |  store in backend      |
  +----------------------+
        |
        v
  +----- SEARCH ----------+
  |  BM25 keyword (fast)   |     <-- search command
  |  vector similarity     |     <-- vsearch command
  |  hybrid + rerank       |     <-- query command (expansion + reranking)
  +----------------------+
```

**Backend resolution:** `AutoQmdStore` resolves backend at startup:
1. Check `--backend` flag -> `auto` | `sqlite` | `postgres`
2. `auto`: try SQLite in `.litho/qmd/` first, fall back to PostgreSQL via `DATABASE_URL`
3. SQLite is repo-local, zero-config; PostgreSQL for shared/service mode

**MCP integration:** `litho-qmd-mcp` exposes 8 tools via JSON-RPC stdin/stdout:
`search`, `vsearch`, `query`, `get`, `multi_get`, `status`, `ingest`, `embed`

### C) `litho` CLI Extract/Generate Pipeline

Entry point: `crates/litho-cli/src/main.rs`

```
  litho extract <path>
        |
        v
  litho-extract        -> JSON or summary output (no LLM required)

  litho generate <path>
        |
        v
  litho-extract        -> codebase analysis
        |
        v
  litho-codex          -> Codex-CLI generation (requires OpenAI API key)
```

The `litho` CLI is a thin wrapper. For full pipeline generation (preprocess ->
research -> compose -> validate), use `litho-generator` directly.

## Crate Dependency Graph

```
litho-cli -----> litho-extract
             |-> litho-codex -----> codex-rs (external/)
             |-> litho-core
             |-> litho-qmd-cli

litho-generator -> litho-extract
                |-> litho-core
                |-> litho-codex

litho-qmd-cli -> litho-qmd-core
              |-> litho-qmd-storage
              |-> litho-qmd-llm

litho-qmd-mcp -> litho-qmd-core
              |-> litho-qmd-storage
              |-> litho-qmd-llm

litho-book (standalone, no litho deps)
```

## Data and State Artifacts

### Per-project state (`.litho/`)

| Path | Written by | Purpose |
|------|-----------|---------|
| `manifest.json` | litho-generator | Generation metadata, file/module tracking, git commit |
| `repo-index.sqlite3` | litho-generator | Repo file snapshot for change detection |
| `cache/` | litho-generator | Prompt-keyed LLM response cache (MD5 keys) |
| `cache/cache-index.sqlite3` | litho-generator | Cache index for fast lookup |
| `ingestion-dag.json` | litho-generator | Preprocess-time file graph + RAG chunks |
| `qmd/*.sqlite3` | litho-qmd-storage | QMD SQLite index (collections, vectors, BM25) |

### Generated output

Default output at `./litho.docs/` (configurable via `-o`):
- Markdown files organized by C4 architecture level
- Summary report with quality scores
- HTML files if `--format html`

## Provider and Model Configuration

### LLM provider support

| Provider | API Format | Config value |
|----------|-----------|-------------|
| Ollama | OpenAI-compatible | `ollama` or via `--llm-api-base-url http://localhost:11434/v1` |
| OpenAI | Native | `openai` |
| Anthropic | Native | `anthropic` |
| DeepSeek | OpenAI-compatible | `deepseek` |
| Mistral | OpenAI-compatible | `mistral` |
| OpenRouter | OpenAI-compatible | `openrouter` |
| Codex-RS | Custom | via `litho-codex` bridge |

### Two-model strategy

`litho-generator` uses two model slots:
- **model_efficient**: fast model for classification, code analysis, preprocessing
- **model_powerful**: quality model for composition, research summaries, complex tasks

Both can be the same model. Fallback: efficient -> powerful -> codex-rs.

### Recommended configurations

**Local (Ollama):**
```toml
[llm]
provider = "ollama"
api_base_url = "http://localhost:11434"
model_efficient = "qwen2.5-coder:7b"
model_powerful = "qwen2.5-coder:7b"
context_window = 131072
```

**Cloud (OpenAI):**
```toml
[llm]
provider = "openai"
model_efficient = "gpt-4o-mini"
model_powerful = "gpt-4o"
```

## Incremental Generation

Mechanism:
1. `manifest.json` stores per-file metadata from last generation
2. `repo-index.sqlite3` snapshots file tree
3. On `--incremental`, git diff maps changed files to affected agents
4. Only affected research/compose agents re-execute
5. Changed sections are re-validated

The `ingestion-dag.json` provides file-level provenance for tighter targeting.

## Quality Framework

6-dimension scoring applied during validation:
1. **Completeness** — all core files/symbols represented
2. **Accuracy** — file references resolve, code citations valid
3. **Freshness** — docs reflect current codebase state
4. **Grounding** — claims backed by actual code evidence
5. **Coherence** — consistent terminology, logical flow
6. **Helpfulness** — LLM-as-judge evaluation (G-Eval rubric)

Quality gate: `quality_gate.rs` can enforce minimum scores, detect regressions
against a baseline report, and trigger re-generation of failing sections.

## Benchmark System

`litho-generator benchmark-optimize` evaluates model/parameter combinations:
- Generates a candidate matrix from model x temperature x context_window x ...
- Executes N measured runs per candidate (configurable)
- Scores each candidate on quality, latency, throughput, memory, stability
- Applies promotion gates (min success rate, max p95, min quality)
- Produces ranked recommendations with composite scores

## Key Source Files

| File | Purpose |
|------|---------|
| `crates/litho-generator/src/generator/workflow.rs` | Main pipeline orchestration |
| `crates/litho-generator/src/generator/agent_executor.rs` | Agent execution + caching |
| `crates/litho-generator/src/llm/client/providers.rs` | LLM provider abstraction |
| `crates/litho-generator/src/llm/client/ollama_native.rs` | Ollama HTTP client |
| `crates/litho-generator/src/config.rs` | Full config model |
| `crates/litho-generator/src/cache/mod.rs` | Cache manager |
| `crates/litho-generator/src/generator/validator/mod.rs` | Quality validation |
| `crates/litho-generator/src/benchmark.rs` | Benchmark framework |
| `crates/litho-extract/src/parser.rs` | Tree-sitter AST extraction |
| `crates/litho-extract/src/lib.rs` | Codebase analysis + classification |
| `crates/litho-core/src/config.rs` | litho.toml parsing |
| `crates/litho-core/src/env.rs` | Environment variable accessors |
| `crates/litho-qmd-storage/src/lib.rs` | QMD storage backends |
| `crates/litho-qmd-mcp/src/main.rs` | MCP server implementation |

## Related Docs

- [README.md](README.md) — CLI reference and quick start
- [docs/IMPLEMENTATION_LAYOUT.md](docs/IMPLEMENTATION_LAYOUT.md) — Module-level file map
- [docs/RUNTIME_AND_DATA_FLOWS.md](docs/RUNTIME_AND_DATA_FLOWS.md) — Runtime data flow details
- [CLAUDE.md](CLAUDE.md) — Development conventions and build system details
