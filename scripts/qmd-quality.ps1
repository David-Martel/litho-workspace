param(
  [switch]$InstallTools,
  [switch]$SkipCoverage,
  [switch]$SkipBench,
  [switch]$SkipIntegration,
  [switch]$BenchNoPlot,
  [string]$TargetDir = "target-qmd-quality"
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
. (Join-Path $repoRoot "scripts\litho-build-helpers.ps1")

function Invoke-Step {
  param(
    [string]$Name,
    [scriptblock]$Action
  )
  Write-Host "[qmd-quality] $Name" -ForegroundColor Cyan
  & $Action
}

$coverageScript = Join-Path $PSScriptRoot "qmd-coverage.ps1"
$benchScript = Join-Path $PSScriptRoot "qmd-bench.ps1"

Push-Location $repoRoot
try {
  $stamp = Set-LithoBuildStamp
  Write-Host "[qmd-quality] Build token: $($stamp.BuildToken)" -ForegroundColor DarkGray
  Enable-LithoSccache -AllowFallback

  $env:CARGO_TARGET_DIR = Join-Path $repoRoot $TargetDir

  Invoke-Step "Running qmd crate tests" {
    Invoke-LithoCargo -Arguments @(
      "test",
      "-p", "litho-qmd-core",
      "-p", "litho-qmd-storage",
      "-p", "litho-qmd-cli",
      "-p", "litho-qmd-mcp"
    )
  }

  if (-not $SkipIntegration) {
    Invoke-Step "Running qmd integration tests in litho-book and litho-codex" {
      Invoke-LithoCargo -Arguments @("test", "-p", "litho-book", "qmd_backend")
      Invoke-LithoCargo -Arguments @("test", "-p", "litho-codex", "--test", "prompt_test")
    }
  }

  Invoke-Step "Running MCP runtime healthcheck" {
    Invoke-LithoCargo -Arguments @("run", "-p", "litho-qmd-mcp", "--", "--healthcheck")
  }

  if (-not $SkipCoverage) {
    Invoke-Step "Generating code coverage (qmd + integration packages)" {
      & $coverageScript -TargetDir "target-qmd-coverage" -OutputDir "coverage/qmd" -InstallTool:$InstallTools
    }
  }

  if (-not $SkipBench) {
    Invoke-Step "Running benchmarks (criterion)" {
      & $benchScript -TargetDir "target-qmd-bench" -CriterionHome "coverage/qmd/bench" -NoPlot:$BenchNoPlot
    }
  }

  Write-Host "[qmd-quality] Complete." -ForegroundColor Green
}
finally {
  Pop-Location
}
