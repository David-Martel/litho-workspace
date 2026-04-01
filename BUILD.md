# Build Guide

Detailed build instructions for litho-workspace, covering toolchain setup,
compilation cache, memory optimization, and all build scripts.

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust (stable) | 1.94+ | Compiler + cargo |
| sccache | 0.13+ | Compilation cache (zstd-compressed) |
| cargo-nextest | 0.9+ | Parallel test runner |
| lefthook | 2.1+ | Git hooks (pre-commit, pre-push) |
| PowerShell | 7+ | Build scripts (pwsh) |
| MSVC Build Tools | VS 2022+ | Linker + C compiler (tree-sitter) |

Optional:
- `cargo-llvm-cov` — coverage reports
- `cargo-deny` / `cargo-audit` — dependency auditing
- `cargo-hack` — feature powerset testing
- Ollama — local LLM for doc generation (not needed for building)
- PostgreSQL 18 — for QMD postgres backend tests

## Quick Build

```bash
# Dev build (all 11 litho crates, excludes codex-rs)
cargo build

# Release build
cargo build --release

# Run tests
cargo nextest run --workspace --no-fail-fast

# Clippy (warnings = errors)
cargo clippy --workspace --all-targets -- -D warnings
```

## Compilation Cache (sccache)

This workspace uses a **project-local sccache** isolated from the global cache.

| Setting | Value |
|---------|-------|
| Cache directory | `.cache/sccache/` (gitignored) |
| Server port | 5100 (avoids conflict with global port 4400) |
| Max cache size | 5 GiB |
| Compression | zstd |
| Incremental compilation | Disabled (required for sccache) |

Configuration lives in `.cargo/config.toml` under `[env]`. All sccache env
vars use `{ value = "...", force = true }` table format to override the global
cargo config at `$CARGO_HOME/config.toml`.

### How it works

1. Cargo invokes `sccache` as `rustc-wrapper` for every compilation unit
2. sccache hashes the inputs (source, flags, deps) and checks the local cache
3. On cache hit: returns cached `.rlib`/`.rmeta` instantly (~0.003s)
4. On cache miss: compiles normally, stores result for next time
5. Second builds are typically 50-80% faster

### Cache management

```bash
# View cache stats (must specify port)
SCCACHE_SERVER_PORT=5100 sccache --show-stats

# Clear cache
SCCACHE_SERVER_PORT=5100 sccache --zero-stats
rm -rf .cache/sccache/

# Stop/restart server (auto-starts on next cargo build)
SCCACHE_SERVER_PORT=5100 sccache --stop-server

# Bypass sccache entirely (for debugging)
RUSTC_WRAPPER="" cargo build
```

### Why project-local?

The global sccache at `T:\RustCache\sccache` (port 4400) is shared across all
Rust projects. Heavy concurrent builds can cause server crashes or corrupted
cache entries. This project runs its own sccache instance on port 5100 with a
separate 5 GiB cache directory, preventing cross-project interference.

## Codegen Units and Memory Optimization

This workspace builds large crates (litho-generator pulls in 1000+ transitive
deps). Memory optimization is critical on Windows.

### Strategy: many small compilation units

| Profile | codegen-units | Purpose |
|---------|---------------|---------|
| dev | 512 | Minimize per-unit RSS during compilation |
| test | 512 | Same — tests compile large binaries |
| release | 16 | Balance optimization quality vs compile time |
| dev (deps) | 512 | Dependencies also use high codegen-units |

**How it helps:** Each codegen unit is compiled independently. With 512 units,
each unit is ~1/512th the size, requiring proportionally less memory. The
tradeoff is slightly slower runtime code in dev builds (compiler can't optimize
across unit boundaries), but compilation succeeds instead of OOMing.

### Linker: MSVC link.exe (not lld-link)

```toml
# .cargo/config.toml
[target.x86_64-pc-windows-msvc]
linker = "link.exe"  # MSVC link.exe, not lld-link
```

`lld-link` (LLVM linker) is faster but uses more memory during linking. For
large binaries like `litho-generator.exe` which link ~1000 crates, MSVC
`link.exe` handles memory pressure better. The global config sets `lld-link`
as default; this workspace overrides it.

### Debug info: line-tables-only for dependencies

```toml
[profile.dev.package."*"]
debug = "line-tables-only"  # Cuts ~40% of debug section size
opt-level = 2               # Optimize deps for faster proc-macro execution
```

Dependencies get minimal debug info (line numbers only, no variable names) and
`opt-level = 2` for faster proc-macro expansion. This reduces both compile
memory and link time.

### Other flags

| Flag | Effect |
|------|--------|
| `target-cpu=native` | Use AVX2/SSE4.2 — faster tree-sitter parsing |
| `link-arg=/DEBUG:FASTLINK` | Partial PDB — faster link, smaller PDB |
| `split-debuginfo = "packed"` | Fewer .pdb files |
| `RUST_MIN_STACK = 8388608` | 8 MB thread stack — prevents overflow on deep type recursion |

## Cargo Aliases

Defined in `.cargo/config.toml`:

```bash
cargo build-safe   # Build library crates only (parallel, low memory)
cargo build-tui    # Build codex-tui only (jobs=1, opt-in)
cargo build-gen    # Build litho-generator only (jobs=1)
cargo test-safe    # Run tests for core + extract + generator
```

## Tiered Build Script

For OOM-prone environments, `scripts/build-tiered.ps1` splits the build into
two phases:

```powershell
# Phase 1: Libraries (parallel, jobs=2)
# Phase 2: Binaries (sequential, jobs=1 — prevents concurrent linker OOM)

# Standard dev build
pwsh scripts/build-tiered.ps1

# Release build
pwsh scripts/build-tiered.ps1 -Release

# Cranelift backend (nightly, 30-60% less rustc memory)
pwsh scripts/build-tiered.ps1 -Cranelift

# Skip binaries (library check only)
pwsh scripts/build-tiered.ps1 -SkipBinaries

# Dynamic std linking (smaller per-binary, needs Rust DLLs at runtime)
pwsh scripts/build-tiered.ps1 -PreferDynamic

# Include codex-tui (excluded by default)
pwsh scripts/build-tiered.ps1 -IncludeCodexTui
```

## CargoTools PowerShell Module

The [CargoTools](file:///C:/Users/david/Documents/PowerShell/Modules/CargoTools/)
module (v0.8.0) wraps cargo with sccache management, preflight checks, and
LLM-friendly JSON output.

```powershell
# Quick check (rewrites build -> check)
pwsh -NoLogo -NoProfile -c "Invoke-CargoWrapper build --quick-check --llm-output"

# Build with JSON status output
pwsh -NoLogo -NoProfile -c "Invoke-CargoWrapper build --release --llm-output"

# Test with nextest
pwsh -NoLogo -NoProfile -c "Invoke-CargoWrapper test --nextest --llm-output"

# Clippy + autofix
pwsh -NoLogo -NoProfile -c "Invoke-CargoWrapper clippy --fix --llm-output"

# Full preflight (format + clippy + check)
pwsh -NoLogo -NoProfile -c "Invoke-CargoWrapper build --preflight --preflight-mode all --llm-output"

# Project context for LLM analysis
pwsh -NoLogo -NoProfile -c "Get-RustProjectContext | ConvertTo-Json -Depth 5"

# Environment diagnostics
pwsh -NoLogo -NoProfile -c "Test-BuildEnvironment"
```

CargoTools is optional — plain `cargo` commands work fine for all operations.

## Build Helper Functions

`scripts/litho-build-helpers.ps1` provides shared PowerShell functions:

| Function | Purpose |
|----------|---------|
| `Enable-LithoSccache` | Start sccache with fallback to direct rustc |
| `Set-LithoBuildStamp` | Set build metadata env vars (git tag, SHA, timestamp) |
| `Invoke-LithoCargo` | Run cargo with automatic sccache fallback on failure |
| `Resolve-RealCargoExe` | Find actual cargo binary (through rustup shim) |
| `Get-LithoFreeTcpPort` | Find available TCP port for sccache server |

Usage from other scripts:

```powershell
. scripts/litho-build-helpers.ps1
Enable-LithoSccache -AllowFallback
Set-LithoBuildStamp
Invoke-LithoCargo -Arguments @("build", "--release")
```

## Git Hooks (lefthook)

Configured in `lefthook.yml`:

| Hook | What runs | Scope |
|------|-----------|-------|
| pre-commit | `cargo fmt --check` | Only when `crates/**/*.rs` files staged |
| pre-commit | `cargo clippy -D warnings` | All 11 litho crates, only when `.rs` staged |
| pre-push | `cargo nextest run` | litho-core + litho-extract + litho-generator |

```bash
# Install hooks
lefthook install

# Skip hooks for a single commit
LEFTHOOK=0 git commit -m "..."
```

## Workspace Structure

The workspace uses `default-members` to scope `cargo build/test/clippy` to
litho crates only. The vendored `external/codex-rs/` (52 crates) is in the
workspace but excluded from default builds:

```toml
# Cargo.toml
[workspace]
members = ["crates/*", "external/codex-rs/..."]
default-members = [
    "crates/litho-core",
    "crates/litho-extract",
    "crates/litho-codex",
    "crates/litho-generator",
    "crates/litho-cli",
    "crates/litho-book",
    "crates/litho-qmd-core",
    "crates/litho-qmd-storage",
    "crates/litho-qmd-llm",
    "crates/litho-qmd-cli",
    "crates/litho-qmd-mcp",
]
```

To build codex-rs crates explicitly: `cargo build -p codex-tui --jobs 1`

## Available Scripts

| Script | Purpose |
|--------|---------|
| `scripts/build-tiered.ps1` | OOM-safe phased build |
| `scripts/litho-build-helpers.ps1` | Shared build functions |
| `scripts/litho-doc-bootstrap.ps1` | Iterative doc generation loop |
| `scripts/benchmark-ollama-optimize.ps1` | Benchmark model/parameter candidates |
| `scripts/startup-warmup-smoke.ps1` | CLI/QMD startup probes |
| `scripts/check-pagefile.ps1` | Windows pagefile sizing check |
| `scripts/postgres18-bootstrap.ps1` | PostgreSQL 18 setup for QMD |
| `scripts/qmd-quality.ps1` | QMD quality pipeline |
| `scripts/qmd-bench.ps1` | QMD benchmark suite |
| `scripts/qmd-coverage.ps1` | QMD coverage report |
| `scripts/verify-build-tokens.ps1` | CI build token verification |

## Release Build

```bash
# Full release build (thin LTO, codegen-units=16, stripped)
cargo build --workspace --release

# Binaries are at:
# T:\RustCache\cargo-target\release\litho.exe
# T:\RustCache\cargo-target\release\litho-generator.exe
# T:\RustCache\cargo-target\release\litho-book.exe
# T:\RustCache\cargo-target\release\litho-qmd-cli.exe
# T:\RustCache\cargo-target\release\litho-qmd-mcp.exe

# Or with tiered build for memory safety
pwsh scripts/build-tiered.ps1 -Release
```

## CI/CD

GitHub Actions (`.github/workflows/ci.yml`):
- **Always-on:** format check, clippy, nextest (litho crates only)
- **Opt-in:** PostgreSQL integration tests (`workflow_dispatch`)
- **Separate:** `vendor-codex-verify.yml` for codex-rs fork compatibility

## Troubleshooting

### OOM during compilation

1. Try `scripts/build-tiered.ps1` (serializes binary links)
2. Reduce codegen-units won't help — they're already at 512
3. Try `RUSTC_WRAPPER="" cargo build -p <crate> --jobs 1`
4. Check pagefile: `pwsh scripts/check-pagefile.ps1` (recommend 8GB min / 24GB max)
5. Last resort: `-Cranelift` flag (nightly only)

### sccache errors

```bash
# "Server startup failed" or port conflict
SCCACHE_SERVER_PORT=5100 sccache --stop-server
# Then retry cargo build (will auto-start new server)

# Nuclear option: bypass sccache
RUSTC_WRAPPER="" cargo build
```

### "could not compile proc-macro2" with 446 errors

sccache server crashed mid-compilation, producing corrupted artifacts.
Stop sccache, clear cache, rebuild:

```bash
SCCACHE_SERVER_PORT=5100 sccache --stop-server
rm -rf .cache/sccache/
cargo build
```

### Linker OOM on codex-tui

codex-tui is excluded from default builds for this reason. If you need it:

```bash
cargo build -p codex-tui --jobs 1
# or
pwsh scripts/build-tiered.ps1 -IncludeCodexTui
```
