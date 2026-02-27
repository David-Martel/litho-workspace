param(
  [switch]$InstallTools,
  [switch]$SkipCoverage,
  [switch]$SkipBench,
  [switch]$SkipIntegration,
  [switch]$BenchNoPlot,
  [string]$TargetDir = "target-qmd-quality"
)

$ErrorActionPreference = "Stop"
function Resolve-RealCargoExe {
  $rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
  if (Test-Path $rustup) {
    $resolved = & $rustup which cargo 2>$null
    if ($LASTEXITCODE -eq 0 -and $resolved) {
      return ($resolved | Select-Object -First 1).Trim()
    }
  }

  $cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
  if ($cargo) { return $cargo }
  return "cargo"
}

$script:CargoExe = Resolve-RealCargoExe

function Invoke-Step {
  param(
    [string]$Name,
    [scriptblock]$Action
  )
  Write-Host "[qmd-quality] $Name" -ForegroundColor Cyan
  & $Action
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$coverageScript = Join-Path $PSScriptRoot "qmd-coverage.ps1"
$benchScript = Join-Path $PSScriptRoot "qmd-bench.ps1"

Push-Location $repoRoot
try {
  $env:CARGO_TARGET_DIR = Join-Path $repoRoot $TargetDir

  Invoke-Step "Running qmd crate tests" {
    & $script:CargoExe test -p litho-qmd-core -p litho-qmd-storage -p litho-qmd-cli -p litho-qmd-mcp
    if ($LASTEXITCODE -ne 0) {
      throw "qmd crate tests failed"
    }
  }

  if (-not $SkipIntegration) {
    Invoke-Step "Running qmd integration tests in litho-book and litho-codex" {
      & $script:CargoExe test -p litho-book qmd_backend
      if ($LASTEXITCODE -ne 0) {
        throw "litho-book qmd integration tests failed"
      }
      & $script:CargoExe test -p litho-codex --test prompt_test
      if ($LASTEXITCODE -ne 0) {
        throw "litho-codex prompt_test failed"
      }
    }
  }

  Invoke-Step "Running MCP runtime healthcheck" {
    & $script:CargoExe run -p litho-qmd-mcp -- --healthcheck
    if ($LASTEXITCODE -ne 0) {
      throw "litho-qmd-mcp healthcheck failed"
    }
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
