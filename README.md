# litho-workspace

Rust monorepo for AI-driven code documentation generation. Combines tree-sitter
AST analysis, multi-agent LLM orchestration, and semantic search into an
automated documentation pipeline that works on repositories of any size.

**Version:** 2.0.0-alpha.1

## Binaries

| Binary | Crate | Purpose |
|--------|-------|---------|
| `litho` | litho-cli | Unified CLI: extract, generate, search, serve, validate |
| `litho-generator` | litho-generator | 4-stage doc generation pipeline with benchmark tuning |
| `litho-book` | litho-book | Axum web server for browsing generated docs |
| `litho-qmd-cli` | litho-qmd-cli | Local document index: ingest, search, embed, retrieve |
| `litho-qmd-mcp` | litho-qmd-mcp | MCP server exposing QMD tools to LLM agents |

## Quick Start

### Build

```bash
cargo build -p litho-cli -p litho-generator -p litho-book -p litho-qmd-cli -p litho-qmd-mcp
```

### Generate documentation for a project

```bash
# Using litho-generator with a local Ollama instance
litho-generator \
  -p /path/to/project \
  -o ./litho.docs \
  --llm-api-base-url http://localhost:11434/v1 \
  --llm-api-key ollama \
  --model-efficient qwen2.5-coder:7b \
  --model-powerful qwen2.5-coder:7b

# Using litho CLI with OpenAI Codex
litho generate /path/to/project --output ./litho-docs --provider codex-lib
```

### Browse generated docs

```bash
litho-book --docs-dir ./litho.docs --port 3333 --open
# or via litho CLI
litho serve ./litho.docs --port 3333
```

### Search indexed documents

```bash
# BM25 keyword search
litho-qmd-cli search "authentication flow" --limit 5

# Semantic vector search
litho-qmd-cli vsearch "how does caching work" --collection project-docs

# Hybrid (expansion + reranking)
litho-qmd-cli query "error handling strategy" --json
```

### Extract AST metadata

```bash
litho extract /path/to/project --format summary
litho extract /path/to/project --format json > codebase.json
```

---

## CLI Reference

### `litho` (unified CLI)

```
litho <COMMAND>

Commands:
  extract   Walk a project tree and extract AST metadata
  generate  Generate Markdown documentation via Codex-CLI
  status    Show generation status from the documentation manifest
  serve     Launch litho-book to serve generated documentation
  validate  Check generated docs for broken file path references
  search    Run a QMD BM25 search
  query     Run a QMD hybrid query (expansion + reranking)
  vsearch   Run a QMD vector search
  qmd       Forward all remaining args to litho-qmd-cli
```

#### `litho extract <PATH>`

| Flag | Default | Description |
|------|---------|-------------|
| `--format` | `json` | Output format: `json` (full AST) or `summary` (human-readable) |
| `--config <PATH>` | none | Path to `litho.toml` config file |
| `--extract-backend` | auto | Backend: `auto`, `tree-sitter`, `ast-grep` |
| `--ast-grep-bin <BIN>` | none | Override ast-grep binary path |

#### `litho generate [PATH]`

| Flag | Default | Description |
|------|---------|-------------|
| `--provider` | `codex-lib` | Provider: `codex-lib` (in-process) or `codex-exec` (subprocess) |
| `--output <DIR>` | `./litho-docs/` | Output directory for generated docs |
| `--model <MODEL>` | provider default | Model identifier (e.g. `o3`, `o4-mini`) |

#### `litho search|query|vsearch <QUERY>`

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `10` | Max results |
| `--min-score` | `0.0` | Minimum score threshold |
| `-c, --collection` | none | Filter by collection name |
| `--json` | false | Emit JSON output |
| `--index` | `index` | QMD index name |
| `--backend` | `auto` | Backend: `auto`, `sqlite`, `postgres` |

#### `litho serve <PATH>`

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | `3333` | Port to serve on |

#### `litho status [PATH]` / `litho validate <PATH>`

No additional flags. Shows manifest metadata or validates file references.

---

### `litho-generator`

The core documentation generation engine. When run without a subcommand, performs
full (or incremental) documentation generation.

```
litho-generator [OPTIONS] [COMMAND]

Commands:
  sync-knowledge      Sync external knowledge sources
  index-repo          Build/update repo index (no LLM generation)
  benchmark-optimize  Benchmark model/parameter candidates
```

#### Generation flags (default command)

| Flag | Default | Description |
|------|---------|-------------|
| `-p, --project-path` | `.` | Root directory of project |
| `-o, --output-path` | `./litho.docs` | Output directory |
| `-c, --config` | auto-discover | Path to `litho.toml` |
| `-n, --name` | auto-inferred | Project name |
| `-v, --verbose` | false | Enable verbose logging |
| `--incremental` | false | Only regenerate changed files |
| `--no-cache` | false | Disable cache |
| `--force-regenerate` | false | Clear cache and regenerate |
| `--format` | `md` | Output format: `md` or `html` |

#### LLM configuration flags

| Flag | Description |
|------|-------------|
| `--model-efficient <MODEL>` | Fast model for classification/analysis tasks |
| `--model-powerful <MODEL>` | Quality model for complex generation tasks |
| `--llm-api-base-url <URL>` | LLM API base URL (e.g. `http://localhost:11434/v1`) |
| `--llm-api-key <KEY>` | API key |
| `--llm-provider <PROVIDER>` | Provider: `openai`, `mistral`, `openrouter`, `anthropic`, `deepseek` |
| `--max-tokens <N>` | Max tokens per response |
| `--temperature <T>` | Sampling temperature |
| `--max-parallels <N>` | Max concurrent LLM requests |
| `--target-language <LANG>` | Output language: `en`, `zh`, `ja`, `ko`, `de`, `fr`, `ru`, `vi` |

#### `benchmark-optimize`

Benchmarks model/parameter combinations to find the optimal generation profile.

Key flags:
- `--models <LIST>` — comma-separated model names to evaluate
- `--output-dir <DIR>` — benchmark artifacts directory (default: `.litho/benchmark`)
- `--runs-per-candidate <N>` — measured runs per candidate (default: 3)
- `--run-timeout-seconds <S>` — hard timeout per run (default: 300)
- `--min-quality <F>` — minimum quality score for recommendation (default: 0.70)
- `--dry-run` — build matrix without executing runs
- `--weight-quality/latency/throughput/memory/stability` — composite score weights
- Promotion gates: `--gate-min-success-rate`, `--gate-max-p95-seconds`, `--gate-min-quality`

---

### `litho-book`

Web viewer for generated documentation.

```
litho-book --docs-dir <DIR> [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --docs-dir <DIR>` | required | Path to generated docs directory |
| `-p, --port` | `3000` | Port to serve on |
| `--host` | `127.0.0.1` | Host to bind to |
| `-o, --open` | false | Auto-open browser |
| `-v, --verbose` | false | DEBUG-level logging |

---

### `litho-qmd-cli`

Local document index with BM25, vector, and hybrid search.

```
litho-qmd-cli [--index <NAME>] [--backend auto|sqlite|postgres] <COMMAND>
```

| Command | Description |
|---------|-------------|
| `ingest` | Scan collections and upsert documents into index |
| `embed` | Build semantic vectors for indexed documents |
| `search <QUERY>` | BM25 keyword search |
| `vsearch <QUERY>` | Semantic vector search |
| `query <QUERY>` | Hybrid search with expansion + reranking |
| `get <FILE>` | Retrieve a document by path (supports `--from`, `--lines`, `--line-numbers`) |
| `multi-get <PATTERN>` | Retrieve documents by glob pattern |
| `ls [PREFIX]` | List indexed files |
| `collection list/add/remove/rename` | Manage collections |
| `context add/list/check/rm` | Manage collection context annotations |
| `status` | Show index health |
| `update` | Run collection update hooks + ingest |
| `cleanup` | Clean orphaned cache data |
| `mcp` | Forward to litho-qmd-mcp |

---

### `litho-qmd-mcp`

MCP (Model Context Protocol) server for LLM agent integration. Runs over
stdin/stdout JSON-RPC.

```
litho-qmd-mcp [--index <NAME>] [--backend auto|sqlite|postgres]
```

| Flag | Description |
|------|-------------|
| `--dump-capabilities` | Print supported tools as JSON and exit |
| `--healthcheck` | Run startup checks and exit |

**MCP tools exposed:** `search`, `vsearch`, `query`, `get`, `multi_get`,
`status`, `ingest`, `embed`

Register in Claude Code `mcp.json`:
```json
{
  "mcpServers": {
    "litho-qmd": {
      "command": "litho-qmd-mcp",
      "args": ["--backend", "sqlite"]
    }
  }
}
```

---

## Configuration

### `litho.toml`

Placed in the project root. Auto-discovered by `litho-generator` at
`<project-path>/litho.toml`.

```toml
project_name = "my-project"

# Directories to exclude from scanning
excluded_dirs = [".git", "target", "node_modules", ".venv", "__pycache__"]

[llm]
provider = "ollama"                          # ollama | openai | anthropic | deepseek | mistral
api_base_url = "http://localhost:11434"      # LLM API endpoint
model_efficient = "qwen2.5-coder:7b"        # Fast model for regular tasks
model_powerful = "qwen2.5-coder:7b"          # Quality model for complex tasks
max_tokens = 4096                            # Max tokens per response
context_window = 131072                      # Model context window size
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `OLLAMA_URL` | Ollama API URL (default: `http://localhost:11434`) |
| `DATABASE_URL` | PostgreSQL connection string (for postgres QMD backend) |
| `CODEX_BINARY_PATH` / `CODEX_BIN` | Path to Codex CLI binary |
| `CODEX_MODEL` | Default model for Codex provider |
| `LITHO_BUILD_VERSION` | Build version override |

---

## Workspace Crates

| Crate | Purpose |
|-------|---------|
| `litho-core` | Shared config (`litho.toml`), env vars, types |
| `litho-extract` | Tree-sitter AST parsing (Rust, TypeScript, Python, C#), dependency graph |
| `litho-generator` | 4-stage doc generation: preprocess, research, compose, validate |
| `litho-codex` | Bridge to OpenAI Codex (in-process and subprocess modes) |
| `litho-cli` | Unified CLI combining extract, generate, search, serve |
| `litho-book` | Axum web server for documentation browsing |
| `litho-qmd-core` | QMD service trait and shared types |
| `litho-qmd-storage` | SQLite/PostgreSQL backends, BM25 search, vector storage |
| `litho-qmd-llm` | Adaptive LLM query expansion and reranking |
| `litho-qmd-mcp` | MCP server (JSON-RPC stdin/stdout) |
| `litho-qmd-cli` | QMD CLI for index management and search |

### External

`external/codex-rs/` contains a customized OpenAI Codex fork (52 crates).
Not a vanilla upstream -- tracked as regular files with local modifications.
Not part of the default build or test surface.

---

## Usage by LLM Agents

### Generating docs for a target repo

```bash
# Full generation with local Ollama
litho-generator \
  -p /path/to/target-repo \
  -o /path/to/target-repo/.litho-docs \
  --llm-provider openai \
  --llm-api-base-url http://localhost:11434/v1 \
  --llm-api-key ollama \
  --model-efficient qwen2.5-coder:7b \
  --model-powerful qwen2.5-coder:7b \
  --verbose

# Incremental update after code changes
litho-generator \
  -p /path/to/target-repo \
  -o /path/to/target-repo/.litho-docs \
  --incremental \
  -c /path/to/target-repo/litho.toml
```

### Building a searchable index for a repo

```bash
# 1. Add the repo as a collection
litho-qmd-cli collection add /path/to/target-repo \
  --mask "**/*.{rs,py,ts,md}" --name target-repo

# 2. Ingest documents
litho-qmd-cli ingest

# 3. Build semantic vectors
litho-qmd-cli embed

# 4. Search
litho-qmd-cli search "authentication" --collection target-repo --limit 5
litho-qmd-cli vsearch "how does error handling work" --collection target-repo
```

### Serving docs for human review

```bash
litho-book --docs-dir /path/to/target-repo/.litho-docs --port 3333 --open
```

### Validating generated docs

```bash
litho validate /path/to/target-repo/.litho-docs
```

---

## Build Commands

```bash
# Build all litho crates (excludes codex-rs)
cargo build --workspace

# Run tests
cargo nextest run --workspace --no-fail-fast

# Clippy
cargo clippy --workspace --all-targets -- -D warnings

# Release build
cargo build --workspace --release

# Memory-safe tiered build (serializes large binary links)
pwsh scripts/build-tiered.ps1 -Release
```

## State and Artifacts

Generated state lives under `.litho/` in the target project:

| Path | Purpose |
|------|---------|
| `.litho/manifest.json` | Generation metadata, module tracking |
| `.litho/repo-index.sqlite3` | Repo file snapshot for diff planning |
| `.litho/cache/` | Prompt-keyed LLM response cache |
| `.litho/ingestion-dag.json` | Preprocess-time file graph + RAG chunks |
| `.litho/qmd/*.sqlite3` | QMD SQLite index (when using sqlite backend) |

## License

Private repository. All rights reserved.
