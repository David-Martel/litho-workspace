param(
  [string]$TargetDir = "target-qmd-coverage",
  [string]$OutputDir = "coverage/qmd",
  [switch]$InstallTool
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
  Write-Host "[qmd-coverage] $Name" -ForegroundColor Cyan
  & $Action
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$outputPath = Join-Path $repoRoot $OutputDir
$htmlPath = Join-Path $outputPath "html"
$lcovPath = Join-Path $outputPath "lcov.info"
$summaryPath = Join-Path $outputPath "summary.txt"

Push-Location $repoRoot
try {
  if ($InstallTool) {
    Invoke-Step "Ensuring cargo-llvm-cov is installed" {
      & $script:CargoExe llvm-cov --version *> $null
      if ($LASTEXITCODE -ne 0) {
        & $script:CargoExe install cargo-llvm-cov --locked
        if ($LASTEXITCODE -ne 0) {
          throw "cargo install cargo-llvm-cov failed"
        }
      }
    }
  }

  Invoke-Step "Ensuring cargo-llvm-cov is available" {
    & $script:CargoExe llvm-cov --version
    if ($LASTEXITCODE -ne 0) {
      throw "cargo llvm-cov --version failed"
    }
  }

  $env:CARGO_TARGET_DIR = Join-Path $repoRoot $TargetDir

  Invoke-Step "Cleaning old coverage artifacts" {
    & $script:CargoExe llvm-cov clean --workspace
    if ($LASTEXITCODE -ne 0) {
      throw "cargo llvm-cov clean --workspace failed"
    }
  }

  Invoke-Step "Preparing coverage output directory" {
    if (Test-Path $outputPath) {
      Remove-Item -Path $outputPath -Recurse -Force
    }
    New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
  }

  Invoke-Step "Running coverage for qmd + integration packages" {
    & $script:CargoExe llvm-cov `
      --package litho-qmd-core `
      --package litho-qmd-storage `
      --package litho-qmd-cli `
      --package litho-qmd-mcp `
      --package litho-book `
      --package litho-codex `
      --tests `
      --no-report
    if ($LASTEXITCODE -ne 0) {
      throw "cargo llvm-cov coverage run failed"
    }
  }

  Invoke-Step "Writing HTML coverage report" {
    & $script:CargoExe llvm-cov report --html --output-dir $outputPath
    if ($LASTEXITCODE -ne 0) {
      throw "cargo llvm-cov HTML report failed"
    }
  }

  Invoke-Step "Writing LCOV coverage report" {
    & $script:CargoExe llvm-cov report --lcov --output-path $lcovPath
    if ($LASTEXITCODE -ne 0) {
      throw "cargo llvm-cov LCOV report failed"
    }
  }

  Invoke-Step "Writing summary coverage report" {
    $summary = & $script:CargoExe llvm-cov report --summary-only
    if ($LASTEXITCODE -ne 0) {
      throw "cargo llvm-cov summary report failed"
    }
    $summary | Set-Content -Path $summaryPath
    $summary | ForEach-Object { Write-Host $_ }
  }

  Write-Host "[qmd-coverage] Coverage generated:" -ForegroundColor Green
  Write-Host "  HTML: $(Join-Path $htmlPath 'index.html')"
  Write-Host "  LCOV: $lcovPath"
  Write-Host "  Summary: $summaryPath"
}
finally {
  Pop-Location
}
