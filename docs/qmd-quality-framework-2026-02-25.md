# QMD Quality Framework (2026-02-25)

This workspace now includes a Rust-first quality framework for QMD rewrite validation:

- code coverage (LLVM source-based coverage via `cargo-llvm-cov`)
- reproducible microbenchmarks (criterion)
- focused integration checks for `litho-book` and `litho-codex`
- MCP runtime healthcheck gate

## Scripts

- `scripts/qmd-quality.ps1`
  - Runs qmd crate tests, integration tests, MCP healthcheck, coverage, and benchmarks.
- `scripts/qmd-coverage.ps1`
  - Generates summary + LCOV + HTML coverage reports for qmd and integration packages.
- `scripts/qmd-bench.ps1`
  - Runs criterion benchmarks for:
    - `litho-qmd-core/benches/service_query.rs`
    - `litho-qmd-storage/benches/fast_table.rs`

## Cargo aliases

Defined in `.cargo/config.toml`:

- `cargo qmd-test`
- `cargo qmd-bench-core`
- `cargo qmd-bench-storage`
- `cargo qmd-cov -- --html --output-dir coverage/qmd/html`

## Coverage scope

Coverage run includes:

- `litho-qmd-core`
- `litho-qmd-storage`
- `litho-qmd-cli`
- `litho-qmd-mcp`
- integration packages:
  - `litho-book`
  - `litho-codex`

Artifacts:

- `coverage/qmd/lcov.info`
- `coverage/qmd/html/index.html`
- `coverage/qmd/summary.txt`

## Benchmark scope

- `service_query` benchmark stresses hybrid query path:
  - expansion
  - lexical + semantic merge
  - dedup and rerank
- `fast_table` benchmark compares native Rust QMD table against:
  - `std::collections::HashMap`
  - `std::collections::BTreeMap`

Artifacts:

- `coverage/qmd/bench`

## CI integration

`.github/workflows/ci.yml` includes `qmd-quality` job:

- installs `llvm-tools-preview` + `cargo-llvm-cov`
- runs `scripts/qmd-quality.ps1`
- publishes `coverage/qmd` artifacts
