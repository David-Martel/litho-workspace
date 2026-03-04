# Git Cluster Analyzer Rectification Plan (2026-03-04)

## Completed in this cycle

1. Error handling and observability
- Added explicit diagnostics channel (`[gca][warn]` / `[gca][error]`) in analyzer runtime.
- Removed major silent-failure paths in MCP stdio transport, git command wrappers, config loading, tuning loading, and AST extraction.
- Canonical JSON-RPC error behavior remains intact (`-32602` for invalid params, etc.).
- Fixed `git log` co-change extraction format (`--format=format:COMMIT_SEP`), eliminating recurring scan-time warnings in normal CLI runs.
- Added MCP service-unavailable control handling (`error.code = -32001`) when interface is stopped via runtime health control.

2. Discovery + clustering infrastructure quality
- Replaced recursive repo discovery with `ignore::WalkBuilder` traversal.
- Added support for detecting repos via `.git` directory and `.git` file (worktrees).
- Added canonical-path deduplication for overlapping roots.
- Normalized root-level file directory metadata (`dir="."` instead of empty string).

3. Strictness and build hygiene
- Enforced Rust warnings as errors via lints (`warnings = deny`, `unused_must_use = deny`).
- Enforced no `unwrap/expect/panic` in non-test runtime paths.
- Updated build script to use direct cargo executable and deterministic lint rules.

4. Test framework expansion
- Added dedicated MCP tests:
  - canonical JSON-RPC invalid-params error on invalid `execute_cluster` call.
  - `Content-Length` framing acceptance with extra headers.
- Added discovery tests:
  - overlapping roots deduplication.
  - worktree detection through `.git` file.
  - root untracked file `dir="."` behavior.

5. LLM output reliability
- Added deterministic metadata enforcement that repairs invalid LLM output into valid conventional commit metadata.
- Added unit test to verify malformed metadata is repaired.
- Added repo-scoped Ollama session memory with persisted logs + prompt recall context to improve commit message continuity across runs.

6. MCP health/control surface
- Added `health` MCP tool with `status|start|stop|reset` actions for soft lifecycle control in the active MCP process.
- Added `ollama_memory` MCP tool for repo-scoped memory summary/clear operations.

7. Autofix/format tooling
- Added cargo aliases (`fmt-check`, `fmt-fix`, `lint`, `lint-all`, `clippy-fix`, `check-all`).
- Added scripts:
  - `C:/codedev/git-cluster/scripts/format-and-fix.ps1`
  - `C:/codedev/git-cluster/scripts/check-style.ps1`
- Executed autofix/lint/test pipeline successfully.

8. Structured-output quality control pipeline
- Added template-constrained response shaping (`response_template`) and stream-oriented JSON fitter to normalize model output into deterministic cluster rows.
- Added two-stage Ollama flow:
  - stage 1 generation model (cluster metadata proposal),
  - stage 2 quality-control model (`valid/score/retry/issues/rewrite_hint`) with bounded retry rounds.
- Added tuning keys and doctor visibility for:
  - `ollama.template_stream_chunk_chars`
  - `ollama.quality_control_*` controls (model, rounds, score, ctx, predict, temperature)
- Added benchmark matrix support for `qc_on`/`qc_off` in e2e script and persistent benchmark history logging (`docs/benchmark-history.md`).
- Fixed benchmark wrapper argument handling by expanding integer matrices and emitting repeated `--num-ctx`/`--num-predict` flags.

## Remaining high-priority TODOs (Analyzer)

1. MCP output typing
- Return structured JSON content from MCP tools instead of serialized JSON-in-text payloads.

2. Config intent fidelity
- Wire currently loaded config sections (`parallel_limit`, reviewers, metadata policy) into runtime behavior.

3. Performance and scalability
- Add explicit directory pruning strategy for large roots (`node_modules`, build trees, etc.) configurable via tuning/config.
- Add benchmark tests for discovery and scan throughput.
- Add separate benchmark profiles for 12B generation + 3B quality model (`qc_on`) with fixed seed diff sets.

4. Protocol robustness
- Expand framing tests to include malformed frame recovery behavior and canonical error payload contracts.

5. Lint policy cleanup
- Clippy `-D warnings` currently flags many test-only `unwrap/expect/panic` usages in existing integration/e2e suites.
- Decide policy:
  - allow these lints in test targets, or
  - incrementally refactor tests to fully lint-clean style.

## Litho-workspace alignment TODOs

1. Keep `.codex` skill/tool docs synchronized with analyzer behavior after each analyzer release.
2. Add a periodic verification job that runs analyzer style/lint/tests and captures results in plan snapshots.
3. Keep commit-clustering runbooks aligned with canonical MCP error semantics and CLI fallback behavior.

## Documentation synchronization completed

- Updated workspace docs: `AGENTS.md`, `.codex/tools/git-commit-cluster.md`, `.codex/skills/git-cluster-analyzer.md`.
- Updated `~/.claude` docs: `commands/commit-cluster.md`, `shared-utils/claude-commands/commit-cluster.md`, `tools/git-cluster-analyzer/README.md`, `tools/git-cluster-analyzer/SKILLS.md`.
- Added explicit guidance for 6-tool MCP surface, canonical error semantics, worktree/dedup discovery behavior, and autofix scripts.

