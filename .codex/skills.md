# Repository Skills

## Required Today

- **rust-async-tokio**
  - Use for async orchestration, bounded concurrency, task lifecycle, and shutdown patterns in `litho-*` crates.
  - Relevant files: pipeline orchestration in `crates/litho-generator`, qmd background workers, service layers.

- **git-cluster-analyzer**
  - Use when batching and committing non-trivial working-tree changes.
  - Useful after implementation batches because this workspace accumulates many small edits across multiple crates.
  - Local copy: [git-cluster-analyzer.md](.codex/skills/git-cluster-analyzer.md)

## Relevant / Conditional

- **rust-wasm-ui**
  - Useful if you modify `litho-book` front-end surfaces or add WASM-based UI features.

- **security-best-practices**
  - Apply when reviewing request/response handling, credential flow, and external CLI/network boundaries.

- **rust-serial-networking**
  - Apply if expanding AST/network adapters or adding long-running networked services.

## Local Tools

- [Git commit cluster tools](.codex/tools/git-commit-cluster.md)
  - Tool workflows and MCP function map for `scan_repos`, `propose_clusters`, `validate_cluster`, `execute_cluster`, `scan_status`.
  - Includes CLI fallback scripts:
    - `.codex/tools/git-commit-cluster.ps1`
    - `.codex/tools/git-cluster-doctor.ps1`
  - Current rectification log:
    - `.codex/plans/2026-03-04-git-cluster-rectification.md`

## Not currently applicable

- **stm32-***, **usb-device**, **audio-dsp** and similar embedded skills are not relevant to this repo’s primary Rust documentation/workspace scope unless an embedded code path is added.
- **rust-dll-csharp-cli** is listed in the global inventory but not available under `C:/Users/david/.codex/skills/` in this environment.

## Use Discipline

- Use only skills that map directly to the current task.
- Prefer built-in repo instructions first (`AGENTS.md`, `CLAUDE.md`, `TODO.md`) and layer skills only when needed.
