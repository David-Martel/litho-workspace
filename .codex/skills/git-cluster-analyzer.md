---
name: "git-cluster-analyzer"
description: "Use when the task involves committing, batching, or organizing uncommitted git changes across one or more repos. Provides semantic clustering via MCP tools: scan_repos, propose_clusters, validate_cluster, execute_cluster, scan_status."
---

# Git Cluster Analyzer Skill

## When to Use
- Committing uncommitted changes (single repo or multi-repo)
- Organizing dirty files into semantic commit groups
- Checking workspace status for dirty repos
- Validating proposed commit clusters before execution
- Batch committing across multiple repositories

## MCP Tools Available

This skill uses the `git_cluster_analyzer` MCP server (registered in config.toml).

| Tool | Purpose |
|------|---------|
| `scan_repos` | Scan repos for uncommitted changes (JSON with file-level numstat) |
| `propose_clusters` | Group files into semantically coherent commit clusters |
| `validate_cluster` | Check file existence and staged consistency before commit |
| `execute_cluster` | Stage, commit (GPG-signed), and optionally push |
| `scan_status` | Lightweight overview: repo name, branch, dirty count |
| `doctor` | Runtime diagnostics (config/tuning/git/repo/Ollama/MCP transport) |
| `health` | Soft MCP lifecycle control (`status`, `start`, `stop`, `reset`) |
| `ollama_memory` | Repo-scoped Ollama memory summary/clear operations |

## CWD-Aware

All tools auto-detect the git repo from the current working directory when `repoPath` is omitted. Works from any subdirectory within a git repo.

## Standard Workflow

### 1. Scan for changes
```
scan_repos({})
```
Returns: array of `{ path, name, branch, files[], recentCommits[], summary }`.

### 2. Propose clusters
```
propose_clusters({
  files: <files from scan>,
  recentCommits: <recentCommits from scan>,
  strategy: "auto"
})
```
Returns: `{ clusters[], stats }`. Each cluster has: `id, type, scope, message, files, confidence, reason`.

Optional controls for richer evaluation:
- `withOllama: true` to enable local LLM refinement of type/scope/message/confidence/reason.
- `tuningConfig: "<path>/config.json"` to load numeric tuning from file.
- `tune: ["key=value", ...]` for per-call overrides (for example `["ollama.enabled=true","ollama.merge_min_score=6","ollama.split_merge_penalty=0.10"]`).

When Ollama is enabled, refinement uses:
- a strict `response_template` (deterministic row/field envelope),
- stream-oriented JSON normalization for partial/non-ideal responses,
- optional second-stage quality gate model (`ollama.quality_control_*`) with retry feedback.
`action: "split"` and `action: "merge"` are executed into concrete cluster reshaping (not just metadata relabeling).

### 3. Review with user
Present clusters showing:
- Commit message (conventional format)
- Files in each cluster
- Confidence score and reason

### 4. Execute approved clusters
```
execute_cluster({
  files: <cluster.files>,
  message: <cluster.message>,
  push: false,
  dryRun: false
})
```
Returns: `{ sha, branch, pushed, filesCount }`.

### 5. Diagnose runtime issues (CLI)
```
git-cluster-analyzer doctor --repo <repo> --check-ollama
```
Returns JSON with config/tuning source resolution, git availability, repo status,
Ollama reachability/model visibility, and MCP transport capability flags.

### 5b. Run ollama-rs benchmarks
```
git-cluster-analyzer benchmark --model gemma3:12b-it-qat --num-ctx 8192,32768,131072 --num-predict 128,256 --runs 3 --warmup 1 --output-json .cache/bench.json
```
Use this for throughput/latency matrix runs via native `ollama-rs`.

E2E quality benchmark (qc on/off + regression log):
```
pwsh C:/codedev/git-cluster/scripts/benchmark-analyzer-e2e.ps1 -RepoPath C:/codedev/litho-workspace -AnalyzerExe C:/codedev/git-cluster/bin/git-cluster-analyzer.exe -Model gemma3:4b -Runs 1 -FixedCtxValues 8192
```

### 6. Control MCP health
```
health({ action: "status" })
```
Use `start`, `stop`, or `reset` actions to manage process-level access state.

### 7. Inspect repo-scoped Ollama memory
```
ollama_memory({ repoPath: "<repo>", action: "summary", maxSessions: 8 })
```
Clear persisted memory with `action: "clear"` when needed.

## Strategy Guide

| Scenario | Strategy | Notes |
|----------|----------|-------|
| General use | `auto` | Best default, uses all semantic layers |
| Quick commits | `directory` | Fastest, groups by directory only |
| Python/Rust/TS | `semantic` | Import graph links cross-directory files |
| Single commit | `single` | Everything in one cluster |

## Multi-Repo Workflow

```
1. scan_repos({ roots: ["/c/codedev", "/t/projects"] })
2. For each dirty repo → propose_clusters({ files, recentCommits })
3. Present all clusters grouped by repo
4. Execute approved clusters per repo
```

## Discovery & Error Guarantees

- Repo discovery now supports both `.git` directories and `.git` files (git worktrees).
- Overlapping `roots` are deduplicated by canonical path to avoid duplicate repo entries.
- Transport parsing and git command failures emit explicit diagnostics to stderr (`[gca][warn]...`) instead of silently failing.
- `scan_status` supports `max_depth` via MCP parameters (`{ roots: [...], max_depth: 2 }`).
- Ollama refinement stores per-repo session logs and recall memory under `C:/codedev/git-cluster/state/ollama/repos/<repo-key>/`.
- Adaptive context window controls are tunable:
  - `ollama.adaptive_ctx_enabled`, `ollama.adaptive_ctx_min`, `ollama.adaptive_ctx_max`
  - `ollama.adaptive_ctx_chars_per_token`, `ollama.adaptive_ctx_base_headroom`
  - `ollama.adaptive_ctx_per_cluster_tokens`, `ollama.adaptive_ctx_per_file_tokens`, `ollama.adaptive_ctx_step_tokens`
- Structured-output + quality-gate controls are tunable:
  - `ollama.template_stream_chunk_chars`
  - `ollama.quality_control_enabled`, `ollama.quality_control_model`
  - `ollama.quality_control_max_rounds`, `ollama.quality_control_min_score`
  - `ollama.quality_control_num_ctx`, `ollama.quality_control_num_predict`, `ollama.quality_control_temperature`

## Autofix & Formatting Tools (Analyzer Repo)

Use these in `C:/codedev/git-cluster`:

- `pwsh ./scripts/format-and-fix.ps1`
  - Runs `cargo fmt`, clippy autofix (`cargo clippy-fix` alias), strict lint (`cargo lint`), and full tests.
- `pwsh ./scripts/check-style.ps1`
  - Fast style gate (`cargo fmt-check` + strict lint).

## Confidence Scores

| Range | Meaning | Action |
|-------|---------|--------|
| 0.8-1.0 | High coherence | Commit as-is |
| 0.5-0.7 | Moderate | Review grouping |
| 0.3-0.5 | Low | Consider splitting |
| <0.3 | Very low | Manual review |

## Dry Run

`execute_cluster({ ..., dryRun: true })` validates file paths and repo state, then returns
`{ sha: "dry-run" }` without changing git state. Invalid/missing file paths now fail fast.

## Troubleshooting

- **"repoPath not provided and current directory is not inside a git repository"**: Run from inside a git repo, or pass `repoPath` explicitly.
- **GPG signing fails**: The user may need to run `gpgconf --kill gpg-agent`.
- **Tool timeout**: Increase `tool_timeout_sec` in `~/.codex/config.toml` for large repos.
- **`Transport closed` from MCP calls**: verify the deployed binary and restart the MCP host. The server supports both newline JSON-RPC and `Content-Length` framed stdio. Use CLI fallback (`git-cluster-analyzer propose ...`) while reconnecting.
- **Canonical MCP errors**: invalid arguments/parameter issues are returned with JSON-RPC `error.code` (for example `-32602` Invalid Params).


