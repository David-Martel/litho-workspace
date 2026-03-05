<#
.SYNOPSIS
    Memory-safe tiered build for litho-workspace.
    Serializes large binary links to prevent OOM.

.DESCRIPTION
    Splits the workspace build into two phases:
    1. Libraries (parallel, jobs=2) — low linker memory
    2. Binaries (sequential, jobs=1) — prevents concurrent link.exe OOM

    For extreme OOM cases, use -Cranelift to enable the Cranelift backend
    (nightly-only, 30-60% less rustc memory, slower output code).

.PARAMETER Release
    Build in release profile instead of dev.

.PARAMETER Cranelift
    Use Cranelift codegen backend (nightly toolchain required).
    Produces slower code but uses 30-60% less memory during compilation.

.PARAMETER IncludeCodexTui
    Include codex-tui in the serialized binary tier.
    By default codex-tui is excluded from the Litho development build loop.

.PARAMETER LibJobs
    Parallelism for library tier (default: 2).

.PARAMETER BinJobs
    Parallelism for binary tier (default: 1).

.PARAMETER SkipBinaries
    Only build libraries (useful for fast compile-check).

.PARAMETER PreferDynamic
    Link Rust std dynamically to reduce linker RSS per binary.
    Output .exe requires Rust runtime DLLs at runtime.
#>
param(
    [switch]$Release,
    [switch]$Cranelift,
    [switch]$IncludeCodexTui,
    [switch]$SkipBinaries,
    [switch]$PreferDynamic,
    [int]$LibJobs = 2,
    [int]$BinJobs = 1
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $repoRoot

# Source shared helpers for sccache management
. (Join-Path $repoRoot "scripts\litho-build-helpers.ps1")
Enable-LithoSccache -AllowFallback

try {
    $profileFlag = if ($Release) { "--release" } else { $null }
    $toolchain = if ($Cranelift) { "+nightly" } else { $null }

    # Build RUSTFLAGS for OOM mitigation
    $extraFlags = @("-C", "target-cpu=native")
    if ($Cranelift) {
        $extraFlags += @("-Z", "codegen-backend=cranelift")
        Write-Host "[tiered-build] Cranelift backend enabled (nightly)" -ForegroundColor Yellow
    }
    if ($PreferDynamic) {
        $extraFlags += @("-C", "prefer-dynamic")
        Write-Host "[tiered-build] Dynamic std linking enabled" -ForegroundColor Yellow
    }
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = ($extraFlags -join " ")

    function Invoke-CargoTier {
        param(
            [string]$Label,
            [string[]]$CrateFlags,
            [int]$Jobs
        )
        Write-Host "`n=== Tier: $Label (jobs=$Jobs) ===" -ForegroundColor Cyan
        $args = @("build") + @($CrateFlags) + @("--jobs", $Jobs)
        if ($profileFlag) { $args += $profileFlag }
        if ($toolchain) { $args = @($toolchain) + $args }

        Write-Host "  cargo $($args -join ' ')" -ForegroundColor DarkGray
        & cargo @args
        if ($LASTEXITCODE -ne 0) {
            throw "Build failed at tier: $Label (exit=$LASTEXITCODE)"
        }
    }

    # Phase 1: Litho libraries in parallel — safe, low memory
    Invoke-CargoTier "Libraries" @(
        "-p", "litho-core",
        "-p", "litho-extract",
        "-p", "litho-codex",
        "-p", "litho-qmd-core",
        "-p", "litho-qmd-storage",
        "-p", "litho-qmd-llm"
    ) -Jobs $LibJobs

    if (-not $SkipBinaries) {
        # Phase 2: Large binaries one at a time — prevents linker OOM
        $binaries = @(
            "litho-generator",
            "litho-book",
            "litho-cli",
            "litho-qmd-cli",
            "litho-qmd-mcp"
        )
        if ($IncludeCodexTui) {
            $binaries = @("codex-tui") + $binaries
        }
        foreach ($crate in $binaries) {
            Invoke-CargoTier "Binary: $crate" @("-p", $crate) -Jobs $BinJobs
        }
    }

    Write-Host "`nBuild complete." -ForegroundColor Green

} finally {
    # Restore RUSTFLAGS
    Remove-Item env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS -ErrorAction SilentlyContinue
    Pop-Location
}
