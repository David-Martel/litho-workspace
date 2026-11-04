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

Litho's root CI uses stable Rust. The nested Codex standalone toolchain pin does
not select the compiler for a build launched from this workspace root. The root
[`async-trait` dependency](Cargo.toml) starts at 0.1.92, which no longer generates
redundant `must_use` attributes on boxed-future methods such as the vendored
[`HttpTransport`](external/codex-rs/codex-rs/codex-client/src/transport.rs).
This preserves unused-future diagnostics without a lint exception, including on
older supported compilers. The standalone Codex manifest and its toolchain pin
remain separate inputs to its Bazel dependency graph.
The root also starts `thiserror` at 2.0.21 so its generated named-field `From`
implementations remain compatible with current Clippy while preserving automatic
error conversion and source chaining.

On Windows, the pinned Bazel GNU-LLVM Cargo also needs `libunwind.dll` from its
matching Rust compiler distribution. Make that runtime directory available on
the invoking shell's `PATH` for the Bazel lock commands. Verify the archive
against the Rust distribution manifest; do not substitute a different Cargo or
change the standalone compiler pin to work around a loader failure.

The private MCP startup sender keeps its concrete channel error and unsent event
inside a box so the async result no longer carries a large error inline.
Its [real-channel controls](external/codex-rs/codex-rs/core/src/mcp_connection_manager/emit_update_tests.rs)
check exact delivery and payload custody when the receiver has closed.
The vendored test setup retains an unsafe block around each actual environment
mutation; its redundant outer block is removed. The
[compaction control](external/codex-rs/codex-rs/core/tests/suite/compact.rs)
keeps its request and rollout assertions after a guarded-match lint cleanup.
The [review register](TEST_REVIEW.csv) records that selector independently.
Skill metadata fixtures select configuration roots without discovering the
operator's home agents directory; a separate owned-home control exercises that
root explicitly. The full-path PowerShell fixture discovers the installed
executable and checks both allowed and refused candidate commands through its
real AST parser. These are fixture corrections, with their original metadata
and rollout assertions retained.

Insta snapshots use the Cargo invocation's workspace root. The root and nested
standalone Cargo configurations each provide a project-relative
`INSTA_WORKSPACE_ROOT` default so vendored `file!()` paths reach the existing
goldens. An explicit operator environment value still wins. Invoke Cargo from
the intended workspace directory; an outer working directory with a nested
`--manifest-path` does not select the standalone environment defaults. See
[Insta's root override](https://insta.rs/docs/advanced/#workspace-root) and
[Cargo's relative environment configuration](https://doc.rust-lang.org/cargo/reference/config.html#env).

The [CLI dispatcher](external/codex-rs/codex-rs/cli/src/main.rs) pins its root and
heavy subcommand futures on the heap. The pre-thread argument-zero bootstrap and
command routing stay at their existing boundaries. Deep debug execution also
needs the standalone workspace's existing 8 MiB Windows stack reserve. The
[CLI build script](external/codex-rs/codex-rs/cli/build.rs) and
[exec build script](external/codex-rs/codex-rs/exec/build.rs) apply that policy
only to their named binaries, including when built from this outer workspace.
They select the target's MSVC or GNU linker syntax without changing host or
workspace-wide flags. See Cargo's
[binary-specific linker directive](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-arg-bin).

The [status fixtures](external/codex-rs/codex-rs/tui/src/status/tests.rs) assert
the current package-version header before normalizing only that header's version
and equivalent padding for the existing full-card goldens. MCP initialization
and app-server user-agent fixtures retain full response equality while deriving
the package version from their manifests. The
[MCP approval fixture](external/codex-rs/codex-rs/mcp-server/tests/suite/codex_tool.rs)
uses an untrusted native file-creation cmdlet on Windows and Python elsewhere;
the approval payload, owned-file assertion and five-second deadline remain.

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
