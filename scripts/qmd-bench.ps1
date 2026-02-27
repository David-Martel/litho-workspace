param(
  [string]$TargetDir = "target-qmd-bench",
  [string]$CriterionHome = "coverage/qmd/bench",
  [switch]$NoPlot,
  [int]$SampleSize = 20,
  [int]$MeasurementSeconds = 5
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
  Write-Host "[qmd-bench] $Name" -ForegroundColor Cyan
  & $Action
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$criterionPath = Join-Path $repoRoot $CriterionHome

Push-Location $repoRoot
try {
  $env:CARGO_TARGET_DIR = Join-Path $repoRoot $TargetDir
  $env:CRITERION_HOME = $criterionPath
  New-Item -ItemType Directory -Path $criterionPath -Force | Out-Null

  Invoke-Step "Running qmd-core benchmark: service_query" {
    if ($SampleSize -lt 10) { throw "SampleSize must be >= 10 for criterion stability" }
    if ($MeasurementSeconds -lt 1) { throw "MeasurementSeconds must be >= 1" }
    if ($NoPlot) {
      & $script:CargoExe bench -p litho-qmd-core --bench service_query -- --noplot --sample-size $SampleSize --measurement-time $MeasurementSeconds
    } else {
      & $script:CargoExe bench -p litho-qmd-core --bench service_query -- --sample-size $SampleSize --measurement-time $MeasurementSeconds
    }
    if ($LASTEXITCODE -ne 0) {
      throw "qmd-core benchmark failed"
    }
  }

  Invoke-Step "Running qmd-storage benchmark: fast_table" {
    if ($NoPlot) {
      & $script:CargoExe bench -p litho-qmd-storage --bench fast_table -- --noplot --sample-size $SampleSize --measurement-time $MeasurementSeconds
    } else {
      & $script:CargoExe bench -p litho-qmd-storage --bench fast_table -- --sample-size $SampleSize --measurement-time $MeasurementSeconds
    }
    if ($LASTEXITCODE -ne 0) {
      throw "qmd-storage benchmark failed"
    }
  }

  Write-Host "[qmd-bench] Criterion artifacts:" -ForegroundColor Green
  Write-Host "  $criterionPath"
}
finally {
  Pop-Location
}
