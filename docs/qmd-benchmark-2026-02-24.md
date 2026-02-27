# QMD Benchmark on litho-workspace (2026-02-24)

## Target repository
- `C:\codedev\litho-workspace`
- Rationale: active multi-crate Rust workspace with code + docs, and direct relevance to Litho pipeline integration.

## Corpus profile
- Total files on disk: `338`
- Code/docs-focused set (`*.rs,*.toml,*.md,*.yml,*.yaml`): `295`
- Top file types by count:
  - `.rs`: `214`
  - `.md`: `66`
  - `.toml`: `10`
  - `.yml`: `4`
- Top file types by size:
  - `.rs`: `1776.1 KB`
  - `.md`: `1099.8 KB`
  - `.webp`: `610.0 KB`
  - `.png`: `518.7 KB`

## Benchmark environment
- QMD runtime: `C:\Users\david\.codex\tools\qmd-mcp\runtime`
- Benchmark artifact: `C:\Users\david\qmd_litho_benchmark_20260224_144736.json`
- Indexes:
  - `litho_perf_default_20260224_144736`
  - `litho_perf_code_20260224_144736`

## Measured results

### Index build
- Default pattern (`**/*.md`)
  - Files indexed: `66`
  - Time: `1.257s`
  - Throughput: `52.5 docs/s`
  - Index size: `2.7 MB`
- Code-focused pattern (`**/*.{rs,toml,md,yml,yaml}`)
  - Files indexed: `290`
  - Time: `2.271s`
  - Throughput: `127.7 docs/s`
  - Index size: `6.8 MB`

### Search/get latency (code-focused index)
- `search --files -n 8 ...` (5 queries)
  - Mean: `1.013s`
  - P95: `1.091s`
  - Min/Max: `0.878s / 1.120s`
- `get Cargo.toml -l 80`
  - Time: `0.817s`

### Repeatability checks
- Repeated query (`workspace build`) x8:
  - Mean: `1.144s`
  - Std dev: `0.073s`
- Repeated `status` x8:
  - Mean: `1.568s`
  - Std dev: `0.314s`

## Index quality and duplication signals
- Active docs: `290`
- Unique active hashes: `196`
- Duplicate docs: `94` (all duplicate groups size `2`)
- Duplicate hash groups: `94`
- Query result duplication observed in `3/5` sampled queries (same docid appearing for mirrored paths)
- Average result score in sampled searches: `0.182` (max `0.24`)

## Functional findings
- Default collection-add pattern is markdown-only, which excludes the majority of `litho-workspace` source files.
- Vector index is absent by default (`0 embedded`), so semantic retrieval quality is unavailable until embedding is executed.
- CLI latency is dominated by per-invocation startup/open costs; repeated short searches cost ~1s each.

## Optimization candidates for QMD
- Add hash-level dedup on result output by default (or optional `--dedup hash`) to reduce mirrored-path noise.
- Add path/domain boosts (e.g., prefer `crates/` over generated mirror trees) to improve ranking relevance.
- Add non-interactive mode that suppresses progress/OSC output cleanly for automation/CI (`--quiet --no-progress`).
- Add daemon/persistent mode for CLI workflows (or enforce MCP usage for repeated calls) to amortize startup cost.
- Expose lightweight benchmark command (`qmd bench`) with structured latency output.

## Integration opportunities for litho-workspace

### 1) litho-book search backend (`crates/litho-book`)
- Current search is in-memory line scan over markdown-only content.
- Replace or augment `/api/search` with QMD-backed retrieval to support:
  - ranked scoring
  - multi-extension search
  - deduplication and richer snippets

### 2) litho-generator research memory (`crates/litho-generator`)
- Current memory API stores and retrieves JSON blobs by scope/key.
- Add a `QmdRetriever` adapter for research phases:
  - `query`/`search` for domain evidence collection
  - `get`/`multi_get` for cited context extraction
  - store selected retrieval artifacts back into memory scopes

### 3) litho-codex prompt construction (`crates/litho-codex`)
- Prompt builder currently includes top files + interfaces from extraction snapshot.
- Add optional QMD augmentation:
  - section-specific retrieval snippets (architecture/workflow/boundary)
  - citation paths from `qmd://...`
  - deduped, bounded-context inserts to reduce token waste

## Recommended next steps
1. Standardize a project profile for indexing:
   - `--mask "**/*.{rs,toml,md,yml,yaml}"` for code/docs analysis
2. Add a pre-filter to exclude mirrored generated trees where appropriate (`deepwiki-rs`, generated docs copies).
3. Run embeddings once for the benchmark index and compare:
   - retrieval relevance
   - latency delta
   - token efficiency in downstream prompts
4. Implement a thin Rust integration layer in `litho-generator` that shells out to QMD MCP tools with JSON output contracts.
