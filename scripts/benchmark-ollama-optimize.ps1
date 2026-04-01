param(
    [string]$ConfigPath = "",
    [string]$ProjectPath = ".",
    [string]$OutputDir = "./.litho/benchmark",
    [string]$Models = "",
    [string]$ContextWindows = "",
    [string]$NumPredict = "",
    [int]$RunsPerCandidate = 1,
    [int]$WarmupRuns = 0,
    [int]$MaxCandidates = 24,
    [int]$RunTimeoutSeconds = 300,
    [double]$MinQuality = 0.70,
    [Nullable[double]]$GateMinSuccessRate = $null,
    [Nullable[double]]$GateMaxP95Seconds = $null,
    [Nullable[double]]$GateMinQuality = $null,
    [switch]$KeepCache,
    [switch]$RetainArtifacts,
    [switch]$DryRun
)

$cmd = @(
    "run", "-p", "litho-generator", "--", "benchmark-optimize",
    "--project-path", $ProjectPath,
    "--output-dir", $OutputDir,
    "--runs-per-candidate", $RunsPerCandidate,
    "--warmup-runs", $WarmupRuns,
    "--max-candidates", $MaxCandidates,
    "--run-timeout-seconds", $RunTimeoutSeconds,
    "--min-quality", $MinQuality
)

if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $defaultConfig = Join-Path (Get-Location) "litho.toml"
    if (Test-Path $defaultConfig) {
        $ConfigPath = $defaultConfig
    }
}

if (-not [string]::IsNullOrWhiteSpace($ConfigPath)) {
    if (-not (Test-Path $ConfigPath)) {
        throw "Config path not found: $ConfigPath"
    }
    $cmd += @("--config", $ConfigPath)
}

if ($Models -ne "") { $cmd += @("--models", $Models) }
if ($ContextWindows -ne "") { $cmd += @("--context-windows", $ContextWindows) }
if ($NumPredict -ne "") { $cmd += @("--num-predict", $NumPredict) }
if ($GateMinSuccessRate -ne $null) { $cmd += @("--gate-min-success-rate", $GateMinSuccessRate) }
if ($GateMaxP95Seconds -ne $null) { $cmd += @("--gate-max-p95-seconds", $GateMaxP95Seconds) }
if ($GateMinQuality -ne $null) { $cmd += @("--gate-min-quality", $GateMinQuality) }
if ($KeepCache) { $cmd += "--keep-cache" }
if ($RetainArtifacts) { $cmd += "--retain-artifacts" }
if ($DryRun) { $cmd += "--dry-run" }

Write-Host "Running: cargo $($cmd -join ' ')"
cargo @cmd
if ($LASTEXITCODE -ne 0) {
    throw "benchmark-optimize failed with exit code $LASTEXITCODE"
}
