# Git Commit Cluster Tools

## Purpose
Use these tools when packaging and committing large or mixed change sets into coherent commits.

## Local Wrapper Scripts

- `.\.codex\tools\git-commit-cluster.ps1`
  - CLI-based propose flow (works even when MCP host transport is unavailable).
  - Example:
    `pwsh .\.codex\tools\git-commit-cluster.ps1 -RepoPath . -WithOllama -PrettySummary -Tune ollama.merge_min_score=6`
- `.\.codex\tools\git-cluster-doctor.ps1`
  - Runs runtime diagnostics (config/tuning/git/repo/Ollama/MCP capabilities).
  - Example:
    `pwsh .\.codex\tools\git-cluster-doctor.ps1 -RepoPath . -CheckOllama`
- `C:/codedev/git-cluster/scripts/benchmark-ollama.ps1`
  - Runs native ollama-rs benchmark matrix via `git-cluster-analyzer benchmark`.
  - Example:
    `pwsh C:/codedev/git-cluster/scripts/benchmark-ollama.ps1 -AnalyzerExe C:/codedev/git-cluster/bin/git-cluster-analyzer.exe -Model gemma3:12b-it-qat -NumCtxValues "8192,32768,131072" -NumPredictValues "128,256" -Runs 3`
- `C:/codedev/git-cluster/scripts/benchmark-analyzer-e2e.ps1`
  - End-to-end `propose --with-ollama` benchmark (fixed vs adaptive context, with `qc_on`/`qc_off` matrix).
  - Example:
    `pwsh C:/codedev/git-cluster/scripts/benchmark-analyzer-e2e.ps1 -RepoPath C:/codedev/litho-workspace -AnalyzerExe C:/codedev/git-cluster/bin/git-cluster-analyzer.exe -Model gemma3:12b-it-qat -Runs 3`

## Available local tools

- `mcp__git_cluster_analyzer__scan_repos`
  - Discover dirty repos/files in the current workspace.
- `mcp__git_cluster_analyzer__propose_clusters`
  - Propose semantic clusters for commit grouping.
- `mcp__git_cluster_analyzer__validate_cluster`
  - Validate proposed cluster before execution.
- `mcp__git_cluster_analyzer__execute_cluster`
  - Stage + commit (and optionally push).
- `mcp__git_cluster_analyzer__scan_status`
  - Lightweight status across repos.
- `mcp__git_cluster_analyzer__doctor` (after MCP host refresh)
  - Runtime diagnostics for config/tuning/git/repo/Ollama/MCP transport.
- `mcp__git_cluster_analyzer__health`
  - MCP interface control (`action: status|start|stop|reset`).
- `mcp__git_cluster_analyzer__ollama_memory`
  - Repo-scoped Ollama session memory summary/clear (`action: summary|clear`).

## Typical flow

1. `scan_repos` to collect changed files and metadata.
2. `propose_clusters` with `strategy: "auto"` (or `"semantic"`, `"directory"`, `"single"`).
3. Review cluster messages and confidence.
4. `execute_cluster` for each approved cluster.
5. Use `validate_cluster` if cluster structure changes before execution.

## Practical notes

- Tool behavior is workspace-aware by default; run from a git repo subfolder.
- Discovery supports `.git` directories and `.git` files (worktrees) and deduplicates overlapping roots.
- Ollama review now stores repo-scoped session memory under `C:/codedev/git-cluster/state/ollama/repos/<repo-key>/` and recalls recent sessions for prompt context.
- Ollama refinement uses a template-constrained structured response + two-stage quality gate (`generation -> reviewer`) when `ollama.quality_control_enabled=true`.
- If MCP tools return `Transport closed`, use wrapper scripts above (CLI fallback path) while restarting the MCP host.
- Canonical protocol errors now use JSON-RPC/MCP `error.code` values (for example `-32602` Invalid Params) rather than only free-form tool text.
- Parser/transport/git command failures now emit explicit diagnostics on stderr; they are no longer silent.
- Prefer `dryRun: true` first when experimenting.
- `dryRun: true` still performs preflight path validation; bad file lists fail early.
- Prefer single-commit mode (`strategy: "single"`) for emergency hotfixes.
- `scan_status` accepts `max_depth` in MCP mode (CLI now supports `--max-depth` too).
- `propose_clusters` accepts tuning and AI controls:
  - `tuningConfig`: path to `config/config.json`
  - `tune`: list of `key=value` overrides (for example `ollama.enabled=true`, `ollama.merge_min_score=6`, `ollama.split_merge_penalty=0.10`)
  - `withOllama`: force local Ollama refinement for proposal metadata (including executable `split` and `merge` actions into concrete cluster reshaping)
- Adaptive context tuning keys:
  - `ollama.adaptive_ctx_enabled`, `ollama.adaptive_ctx_min`, `ollama.adaptive_ctx_max`, `ollama.adaptive_ctx_chars_per_token`, `ollama.adaptive_ctx_base_headroom`, `ollama.adaptive_ctx_per_cluster_tokens`, `ollama.adaptive_ctx_per_file_tokens`, `ollama.adaptive_ctx_step_tokens`
- Structured output + quality-gate keys:
  - `ollama.template_stream_chunk_chars`
  - `ollama.quality_control_enabled`, `ollama.quality_control_model`, `ollama.quality_control_max_rounds`, `ollama.quality_control_min_score`, `ollama.quality_control_num_ctx`, `ollama.quality_control_num_predict`, `ollama.quality_control_temperature`
- E2E benchmark script appends summary rows to `C:/codedev/git-cluster/docs/benchmark-history.md` for regression tracking.


