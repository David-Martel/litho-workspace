# Repository Guidelines

## Project Structure & Module Organization
- This repository is a Rust workspace (`Cargo.toml`) with primary crates under `crates/`.
- Key crates: `litho-cli`, `litho-core`, `litho-generator`, `litho-extract`, `litho-codex`, `litho-book`, and `litho-qmd-*`.
- Source code lives in `crates/*/src`.
- Integration tests live in `crates/*/tests`; benchmarks in `crates/*/benches`.
- Supporting assets:
  - `docs/` for architecture, plans, and runbooks
  - `scripts/` for build/test automation
  - `external/` and `third_party/` for vendored dependencies

## Build, Test, and Development Commands
- `cargo build --workspace`: Build all crates.
- `cargo build --release -p litho-cli`: Build a specific binary in release mode.
- `cargo nextest run --workspace --no-fail-fast`: Primary test command.
- `cargo test --workspace`: Standard Rust test runner fallback.
- `cargo fmt --check`: Verify formatting.
- `cargo clippy --workspace --all-targets -- -D warnings`: Enforce lint quality.
- `pwsh scripts/build-tiered.ps1 -Release`: OOM-safe staged build flow.

## Coding Style & Naming Conventions
- Follow idiomatic Rust: `snake_case` for functions/modules, `PascalCase` for types.
- Keep crate/module names descriptive and consistent with existing workspace naming.
- Prefer small focused functions over broad utility abstractions.
- Run `cargo fmt` and clippy checks before opening a PR.

## Testing Guidelines
- Add unit tests near implementation (`src`), integration tests in `crates/<crate>/tests`.
- Use behavior-focused test names (for example `config_loads_defaults`, `provider_client_new_ollama_succeeds`).
- For touched crates, run targeted tests first, then workspace nextest.

## Commit & Pull Request Guidelines
- Use conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`, `ci:`.
- Keep commits atomic and scoped to one intent.
- PRs should include:
  - concise summary of behavior changes
  - affected crates/modules
  - exact validation commands executed

## Security & Configuration Tips
- Never commit secrets or local credentials.
- Start from `.env.example` where applicable.
- Validate local LLM and database settings (for generator/docs workflows) before running full pipelines.

## Local Commit Clustering Workflow
- Prefer MCP tools (`scan_repos`, `propose_clusters`, `validate_cluster`, `execute_cluster`, `scan_status`, `doctor`, `health`, `ollama_memory`) for commit batching.
- In this workspace, helper scripts live in `.codex/tools/`:
  - `pwsh ./.codex/tools/git-commit-cluster.ps1 -RepoPath . -PrettySummary`
  - `pwsh ./.codex/tools/git-cluster-doctor.ps1 -RepoPath . -CheckOllama`
- Use `health` for soft MCP lifecycle control (`status|start|stop|reset`) and `ollama_memory` to inspect or clear repo-scoped session recall state.
- If MCP transport is down, use CLI fallback from `C:/codedev/git-cluster`.

